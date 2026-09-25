//! A streaming Motion-JPEG AVI writer. Every frame is a JPEG, so recording
//! needs no video encoder, and the files play in VLC, browsers' media
//! players, QuickTime with common codecs, and Windows' Movies & TV.

use anyhow::{Context as _, Result};
use std::fs::File;
use std::io::{BufWriter, Seek, SeekFrom, Write};
use std::path::Path;
use std::time::Duration;

const AVIF_HASINDEX: u32 = 0x10;
const AVIIF_KEYFRAME: u32 = 0x10;

pub struct AviWriter {
    file: BufWriter<File>,
    width: u32,
    height: u32,
    /// Offset and length of each frame chunk, for the index.
    frames: Vec<(u32, u32)>,
    largest_frame: u32,
    /// Where the `movi` list's fourcc sits; index offsets count from here.
    movi_start: u64,
}

// Byte positions of the header fields that are only known once recording
// ends. They follow from the fixed layout written in `create`.
const RIFF_SIZE_AT: u64 = 4;
const AVIH_MICROSECONDS_AT: u64 = 32;
const AVIH_MAX_BYTES_PER_SECOND_AT: u64 = 36;
const AVIH_TOTAL_FRAMES_AT: u64 = 48;
const AVIH_BUFFER_SIZE_AT: u64 = 60;
const STRH_SCALE_AT: u64 = 128;
const STRH_RATE_AT: u64 = 132;
const STRH_LENGTH_AT: u64 = 140;
const STRH_BUFFER_SIZE_AT: u64 = 144;
const MOVI_SIZE_AT: u64 = 216;

impl AviWriter {
    pub fn create(path: &Path, width: u32, height: u32) -> Result<Self> {
        let file = File::create(path).with_context(|| format!("couldn't create {}", path.display()))?;
        let mut file = BufWriter::new(file);
        let mut header = Vec::with_capacity(224);
        header.extend_from_slice(b"RIFF");
        header.extend_from_slice(&0u32.to_le_bytes());
        header.extend_from_slice(b"AVI ");

        header.extend_from_slice(b"LIST");
        header.extend_from_slice(&192u32.to_le_bytes());
        header.extend_from_slice(b"hdrl");

        header.extend_from_slice(b"avih");
        header.extend_from_slice(&56u32.to_le_bytes());
        for value in [
            100_000u32, // microseconds per frame, patched at the end
            0,          // max bytes per second, patched
            0,          // padding granularity
            AVIF_HASINDEX,
            0, // total frames, patched
            0, // initial frames
            1, // streams
            0, // suggested buffer size, patched
            width,
            height,
            0,
            0,
            0,
            0,
        ] {
            header.extend_from_slice(&value.to_le_bytes());
        }

        header.extend_from_slice(b"LIST");
        header.extend_from_slice(&116u32.to_le_bytes());
        header.extend_from_slice(b"strl");

        header.extend_from_slice(b"strh");
        header.extend_from_slice(&56u32.to_le_bytes());
        header.extend_from_slice(b"vids");
        header.extend_from_slice(b"MJPG");
        header.extend_from_slice(&0u32.to_le_bytes()); // flags
        header.extend_from_slice(&0u16.to_le_bytes()); // priority
        header.extend_from_slice(&0u16.to_le_bytes()); // language
        for value in [
            0u32, // initial frames
            1,    // scale, patched
            10,   // rate, patched
            0,    // start
            0,    // length, patched
            0,    // suggested buffer size, patched
            u32::MAX, // quality: default
            0,    // sample size
        ] {
            header.extend_from_slice(&value.to_le_bytes());
        }
        for value in [0u16, 0, width as u16, height as u16] {
            header.extend_from_slice(&value.to_le_bytes());
        }

        header.extend_from_slice(b"strf");
        header.extend_from_slice(&40u32.to_le_bytes());
        header.extend_from_slice(&40u32.to_le_bytes());
        header.extend_from_slice(&(width as i32).to_le_bytes());
        header.extend_from_slice(&(height as i32).to_le_bytes());
        header.extend_from_slice(&1u16.to_le_bytes());
        header.extend_from_slice(&24u16.to_le_bytes());
        header.extend_from_slice(b"MJPG");
        header.extend_from_slice(&(width * height * 3).to_le_bytes());
        for _ in 0..4 {
            header.extend_from_slice(&0u32.to_le_bytes());
        }

        header.extend_from_slice(b"LIST");
        header.extend_from_slice(&4u32.to_le_bytes()); // movi size, patched
        let movi_start = header.len() as u64;
        header.extend_from_slice(b"movi");
        debug_assert_eq!(header.len(), 224);
        file.write_all(&header)?;
        Ok(Self {
            file,
            width,
            height,
            frames: Vec::new(),
            largest_frame: 0,
            movi_start,
        })
    }

    pub fn dimensions(&self) -> (u32, u32) {
        (self.width, self.height)
    }

    pub fn frame_count(&self) -> usize {
        self.frames.len()
    }

    pub fn write_frame(&mut self, jpeg: &[u8]) -> Result<()> {
        let position = self.file.stream_position()?;
        let offset = (position - self.movi_start) as u32;
        let length = jpeg.len() as u32;
        self.file.write_all(b"00dc")?;
        self.file.write_all(&length.to_le_bytes())?;
        self.file.write_all(jpeg)?;
        if length % 2 == 1 {
            self.file.write_all(&[0])?;
        }
        self.frames.push((offset, length));
        self.largest_frame = self.largest_frame.max(length);
        Ok(())
    }

    /// Writes the index and the header fields that depend on the whole
    /// recording. `duration` sets the playback rate to real time.
    pub fn finish(mut self, duration: Duration) -> Result<()> {
        let movi_end = self.file.stream_position()?;
        self.file.write_all(b"idx1")?;
        self.file
            .write_all(&((self.frames.len() * 16) as u32).to_le_bytes())?;
        for (offset, length) in &self.frames {
            self.file.write_all(b"00dc")?;
            self.file.write_all(&AVIIF_KEYFRAME.to_le_bytes())?;
            self.file.write_all(&offset.to_le_bytes())?;
            self.file.write_all(&length.to_le_bytes())?;
        }
        let end = self.file.stream_position()?;

        let frames = self.frames.len().max(1) as f64;
        let seconds = duration.as_secs_f64().max(0.001);
        let fps = (frames / seconds).clamp(0.5, 60.0);
        let microseconds = (1_000_000.0 / fps).round() as u32;
        let rate = (fps * 1000.0).round() as u32;
        let bytes_per_second = (self.largest_frame as f64 * fps) as u32;
        let patches: [(u64, u32); 10] = [
            (RIFF_SIZE_AT, (end - 8) as u32),
            (AVIH_MICROSECONDS_AT, microseconds),
            (AVIH_MAX_BYTES_PER_SECOND_AT, bytes_per_second),
            (AVIH_TOTAL_FRAMES_AT, self.frames.len() as u32),
            (AVIH_BUFFER_SIZE_AT, self.largest_frame + 8),
            (STRH_SCALE_AT, 1000),
            (STRH_RATE_AT, rate),
            (STRH_LENGTH_AT, self.frames.len() as u32),
            (STRH_BUFFER_SIZE_AT, self.largest_frame + 8),
            (MOVI_SIZE_AT, (movi_end - self.movi_start) as u32),
        ];
        for (at, value) in patches {
            self.file.seek(SeekFrom::Start(at))?;
            self.file.write_all(&value.to_le_bytes())?;
        }
        self.file.flush()?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn u32_at(bytes: &[u8], at: usize) -> u32 {
        u32::from_le_bytes([bytes[at], bytes[at + 1], bytes[at + 2], bytes[at + 3]])
    }

    #[test]
    fn writes_a_well_formed_file() {
        let directory = tempfile::tempdir().expect("tempdir");
        let path = directory.path().join("clip.avi");
        let mut writer = AviWriter::create(&path, 64, 48).expect("create");
        writer.write_frame(&[0xFF, 0xD8, 1, 0xFF, 0xD9]).expect("frame");
        writer.write_frame(&[0xFF, 0xD8, 2, 2, 0xFF, 0xD9]).expect("frame");
        writer.finish(Duration::from_millis(200)).expect("finish");

        let bytes = std::fs::read(&path).expect("read");
        assert_eq!(&bytes[0..4], b"RIFF");
        assert_eq!(u32_at(&bytes, 4) as usize, bytes.len() - 8);
        assert_eq!(&bytes[8..12], b"AVI ");
        assert_eq!(&bytes[24..28], b"avih");
        assert_eq!(u32_at(&bytes, 48), 2, "total frames");
        assert_eq!(u32_at(&bytes, 32), 100_000, "10 frames per second");
        assert_eq!(&bytes[88..92], b"LIST");
        assert_eq!(&bytes[100..104], b"strh");
        assert_eq!(&bytes[108..112], b"vids");
        assert_eq!(u32_at(&bytes, 140), 2, "stream length");
        assert_eq!(&bytes[164..168], b"strf");
        assert_eq!(&bytes[212..216], b"LIST");
        assert_eq!(&bytes[220..224], b"movi");
        let movi_size = u32_at(&bytes, 216) as usize;
        let index_at = 220 + movi_size;
        assert_eq!(&bytes[index_at..index_at + 4], b"idx1");
        assert_eq!(u32_at(&bytes, index_at + 4), 32);
        let first_offset = u32_at(&bytes, index_at + 16) as usize;
        assert_eq!(&bytes[220 + first_offset..220 + first_offset + 4], b"00dc");
        let second_offset = u32_at(&bytes, index_at + 32) as usize;
        assert_eq!(second_offset, first_offset + 8 + 6, "odd frames are padded");
    }
}
