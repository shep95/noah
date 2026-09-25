//! Talking with shepherd: speech is recorded from the microphone and turned
//! into text by the person's own model provider, and shepherd's replies can
//! be read aloud by the same provider. Nothing goes through noah's servers;
//! the audio goes only to the provider whose API key is used, and only when
//! the person presses the microphone or turns on read-aloud.

use std::io::Cursor;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use anyhow::{Context as _, Result, bail};
use futures::AsyncReadExt as _;
use gpui::{App, AppContext as _, Task};
use http_client::{AsyncBody, HttpClient, Method, Request as HttpRequest};
use language_model::{LanguageModelRegistry, SpeechApi};
use rodio::Source as _;
use settings::Settings as _;

/// Recordings stop on their own after this long.
const MAX_RECORDING_SECONDS: usize = 300;
/// Replies longer than this are read up to this point.
const MAX_SPOKEN_CHARS: usize = 4_000;

/// The provider to talk through: the one behind the default model when it
/// offers speech, otherwise any connected provider that does.
pub fn speech_api(cx: &App) -> Option<SpeechApi> {
    let registry = LanguageModelRegistry::read_global(cx);
    let providers = registry.providers();
    let preferred = registry
        .default_model()
        .map(|configured| configured.provider.id())
        .and_then(|id| providers.iter().find(|provider| provider.id() == id))
        .and_then(|provider| provider.speech_api(cx));
    preferred.or_else(|| providers.iter().find_map(|provider| provider.speech_api(cx)))
}

pub const NO_SPEECH_PROVIDER: &str =
    "voice needs a provider with speech: add a Venice or OpenAI API key in Settings > AI";

pub struct Recording {
    stop: Arc<AtomicBool>,
    audio: Task<Result<Vec<u8>>>,
}

impl Recording {
    /// Stops listening and returns the recording as WAV.
    pub fn finish(self) -> Task<Result<Vec<u8>>> {
        self.stop.store(true, Ordering::Relaxed);
        self.audio
    }
}

pub fn start_recording(cx: &mut App) -> Result<Recording> {
    let device = audio::AudioSettings::get_global(cx).input_audio_device.clone();
    let stop = Arc::new(AtomicBool::new(false));
    let (sender, receiver) = futures::channel::oneshot::channel();
    std::thread::Builder::new()
        .name("noah-voice-input".into())
        .spawn({
            let stop = stop.clone();
            move || {
                sender.send(record(device, &stop)).ok();
            }
        })
        .context("couldn't start recording")?;
    let audio = cx.background_spawn(async move {
        receiver
            .await
            .context("the recording ended unexpectedly")?
    });
    Ok(Recording { stop, audio })
}

// The microphone source blocks until samples arrive, so recording runs on
// its own thread rather than on the async executor.
fn record(device: Option<rodio::cpal::DeviceId>, stop: &AtomicBool) -> Result<Vec<u8>> {
    let microphone = audio::open_input_stream(device).context("couldn't open the microphone")?;
    let channels = microphone.channels().get() as usize;
    let sample_rate = microphone.sample_rate().get();
    let limit = sample_rate as usize * MAX_RECORDING_SECONDS;
    let mut mono: Vec<f32> = Vec::with_capacity(sample_rate as usize * 10);
    let mut frame_sum = 0.0;
    let mut frame_len = 0;
    for sample in microphone {
        frame_sum += sample;
        frame_len += 1;
        if frame_len == channels {
            mono.push(frame_sum / channels as f32);
            frame_sum = 0.0;
            frame_len = 0;
            if stop.load(Ordering::Relaxed) || mono.len() >= limit {
                break;
            }
        }
    }
    if mono.len() < sample_rate as usize / 4 {
        bail!("the recording was too short to hear anything");
    }
    Ok(wav_bytes(&mono, sample_rate))
}

/// 16-bit mono PCM WAV, the format every transcription API accepts.
pub fn wav_bytes(samples: &[f32], sample_rate: u32) -> Vec<u8> {
    let data_len = (samples.len() * 2) as u32;
    let mut out = Vec::with_capacity(44 + data_len as usize);
    out.extend_from_slice(b"RIFF");
    out.extend_from_slice(&(36 + data_len).to_le_bytes());
    out.extend_from_slice(b"WAVEfmt ");
    out.extend_from_slice(&16u32.to_le_bytes());
    out.extend_from_slice(&1u16.to_le_bytes());
    out.extend_from_slice(&1u16.to_le_bytes());
    out.extend_from_slice(&sample_rate.to_le_bytes());
    out.extend_from_slice(&(sample_rate * 2).to_le_bytes());
    out.extend_from_slice(&2u16.to_le_bytes());
    out.extend_from_slice(&16u16.to_le_bytes());
    out.extend_from_slice(b"data");
    out.extend_from_slice(&data_len.to_le_bytes());
    for sample in samples {
        let value = (sample.clamp(-1.0, 1.0) * i16::MAX as f32) as i16;
        out.extend_from_slice(&value.to_le_bytes());
    }
    out
}

fn multipart_body(boundary: &str, fields: &[(&str, &str)], audio: &[u8]) -> Vec<u8> {
    let mut body = Vec::with_capacity(audio.len() + 512);
    for (name, value) in fields {
        body.extend_from_slice(
            format!("--{boundary}\r\nContent-Disposition: form-data; name=\"{name}\"\r\n\r\n{value}\r\n")
                .as_bytes(),
        );
    }
    body.extend_from_slice(
        format!(
            "--{boundary}\r\nContent-Disposition: form-data; name=\"file\"; filename=\"speech.wav\"\r\nContent-Type: audio/wav\r\n\r\n"
        )
        .as_bytes(),
    );
    body.extend_from_slice(audio);
    body.extend_from_slice(format!("\r\n--{boundary}--\r\n").as_bytes());
    body
}

fn explain_status(api: &SpeechApi, status: u16, body: &str, what: &str) -> anyhow::Error {
    match status {
        401 | 403 => anyhow::anyhow!("{} didn't accept the API key for {what}", api.provider),
        404 | 405 => anyhow::anyhow!(
            "{} doesn't offer {what} at {}, so voice isn't available with this key",
            api.provider,
            api.api_url
        ),
        _ => anyhow::anyhow!(
            "{what} failed ({status}): {}",
            body.chars().take(300).collect::<String>()
        ),
    }
}

/// Turns recorded speech into text. `language` is an ISO 639-1 hint.
pub async fn transcribe(
    http_client: Arc<dyn HttpClient>,
    api: SpeechApi,
    audio: Vec<u8>,
    language: Option<String>,
) -> Result<String> {
    let boundary = format!("noah{:x}", rand_boundary());
    let mut fields = vec![
        ("model", api.transcription_model.as_str()),
        ("response_format", "json"),
    ];
    if let Some(language) = language.as_deref() {
        fields.push(("language", language));
    }
    let body = multipart_body(&boundary, &fields, &audio);
    let request = HttpRequest::builder()
        .method(Method::POST)
        .uri(format!("{}/audio/transcriptions", api.api_url))
        .header("Authorization", format!("Bearer {}", api.api_key))
        .header(
            "Content-Type",
            format!("multipart/form-data; boundary={boundary}"),
        )
        .body(AsyncBody::from(body))?;
    let mut response = http_client
        .send(request)
        .await
        .with_context(|| format!("couldn't reach {}", api.provider))?;
    let mut text = String::new();
    response.body_mut().read_to_string(&mut text).await?;
    if !response.status().is_success() {
        return Err(explain_status(&api, response.status().as_u16(), &text, "speech to text"));
    }
    let json: serde_json::Value =
        serde_json::from_str(&text).context("the transcription came back unreadable")?;
    Ok(json["text"].as_str().unwrap_or_default().trim().to_string())
}

fn rand_boundary() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|elapsed| elapsed.as_nanos() as u64)
        .unwrap_or(0x5eed)
}

/// Text worth speaking from a markdown reply: code blocks, links and
/// markdown symbols read badly aloud, so they're left out.
pub fn speakable(markdown: &str) -> String {
    let mut out = String::new();
    let mut in_code = false;
    for line in markdown.lines() {
        if line.trim_start().starts_with("```") {
            in_code = !in_code;
            if in_code {
                out.push_str("(code omitted.) ");
            }
            continue;
        }
        if in_code {
            continue;
        }
        let cleaned: String = line
            .trim_start_matches(['#', '>', '-', '*', ' '])
            .replace(['*', '_', '`'], "");
        if !cleaned.trim().is_empty() {
            out.push_str(cleaned.trim());
            out.push(' ');
        }
    }
    let mut out = out.trim().to_string();
    if out.len() > MAX_SPOKEN_CHARS {
        let mut end = MAX_SPOKEN_CHARS;
        while !out.is_char_boundary(end) {
            end -= 1;
        }
        out.truncate(end);
    }
    out
}

pub async fn synthesize(http_client: Arc<dyn HttpClient>, api: SpeechApi, text: String) -> Result<Vec<u8>> {
    let body = serde_json::json!({
        "model": api.speech_model,
        "input": text,
        "voice": api.voice,
        "response_format": "wav",
    });
    let request = HttpRequest::builder()
        .method(Method::POST)
        .uri(format!("{}/audio/speech", api.api_url))
        .header("Authorization", format!("Bearer {}", api.api_key))
        .header("Content-Type", "application/json")
        .body(AsyncBody::from(serde_json::to_vec(&body)?))?;
    let mut response = http_client
        .send(request)
        .await
        .with_context(|| format!("couldn't reach {}", api.provider))?;
    let mut bytes = Vec::new();
    response.body_mut().read_to_end(&mut bytes).await?;
    if !response.status().is_success() {
        return Err(explain_status(
            &api,
            response.status().as_u16(),
            &String::from_utf8_lossy(&bytes),
            "text to speech",
        ));
    }
    Ok(bytes)
}

/// Audio being played. Dropping it stops playback.
pub struct Playback {
    _sink: rodio::MixerDeviceSink,
    player: rodio::Player,
}

impl Playback {
    pub fn is_done(&self) -> bool {
        self.player.empty()
    }
}

pub fn play(audio: Vec<u8>, cx: &App) -> Result<Playback> {
    let device = audio::AudioSettings::get_global(cx).output_audio_device.clone();
    let sink = audio::open_test_output(device).context("couldn't open the speakers")?;
    let player = rodio::play(sink.mixer(), Cursor::new(audio)).context("couldn't play the reply")?;
    Ok(Playback { _sink: sink, player })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn writes_valid_wav() {
        let wav = wav_bytes(&[0.0, 0.5, -0.5, 1.0], 16_000);
        assert_eq!(&wav[0..4], b"RIFF");
        assert_eq!(&wav[8..16], b"WAVEfmt ");
        assert_eq!(wav.len(), 44 + 8);
        assert_eq!(u32::from_le_bytes([wav[24], wav[25], wav[26], wav[27]]), 16_000);
    }

    #[test]
    fn speaks_prose_not_code() {
        let reply = "## Done\n\nI fixed **the bug** in `auth.rs`.\n\n```rust\nfn x() {}\n```\n- tests pass";
        assert_eq!(speakable(reply), "Done I fixed the bug in auth.rs. (code omitted.) tests pass");
    }
}
