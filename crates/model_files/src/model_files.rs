//! Reading and editing large model files (GGUF and safetensors) without
//! loading their weights. Only headers are parsed; edits write a new header and
//! stream the tensor data across unchanged, so files of any size work.

use anyhow::{Context as _, Result, bail, ensure};
use std::fs::File;
use std::io::{BufReader, BufWriter, Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};

const GGUF_MAGIC: &[u8; 4] = b"GGUF";
const DEFAULT_ALIGNMENT: u64 = 32;
/// Headers larger than this are almost certainly corrupt; refusing them keeps
/// a bad file from exhausting memory.
const MAX_HEADER_BYTES: u64 = 1024 * 1024 * 1024;
const COPY_BUFFER_BYTES: usize = 8 * 1024 * 1024;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Format {
    Gguf,
    Safetensors,
}

pub fn detect_format(path: &Path) -> Result<Format> {
    let mut file = File::open(path).with_context(|| format!("couldn't open {}", path.display()))?;
    let mut magic = [0u8; 8];
    let read = file.read(&mut magic)?;
    if read >= 4 && &magic[..4] == GGUF_MAGIC {
        return Ok(Format::Gguf);
    }
    if read == 8 {
        let header_length = u64::from_le_bytes(magic);
        let mut first = [0u8; 1];
        if header_length > 1
            && header_length < MAX_HEADER_BYTES
            && file.read(&mut first)? == 1
            && first[0] == b'{'
        {
            return Ok(Format::Safetensors);
        }
    }
    bail!(
        "{} is not a GGUF or safetensors file. Other formats (PyTorch .bin/.pt, ONNX) \
         can be converted with llama.cpp's convert_hf_to_gguf.py.",
        path.display()
    )
}

/// A GGUF metadata value, kept in its original type so a rewrite is exact.
#[derive(Debug, Clone, PartialEq)]
pub enum GgufValue {
    U8(u8),
    I8(i8),
    U16(u16),
    I16(i16),
    U32(u32),
    I32(i32),
    F32(f32),
    Bool(bool),
    String(String),
    Array(u32, Vec<GgufValue>),
    U64(u64),
    I64(i64),
    F64(f64),
}

impl GgufValue {
    fn type_id(&self) -> u32 {
        match self {
            GgufValue::U8(_) => 0,
            GgufValue::I8(_) => 1,
            GgufValue::U16(_) => 2,
            GgufValue::I16(_) => 3,
            GgufValue::U32(_) => 4,
            GgufValue::I32(_) => 5,
            GgufValue::F32(_) => 6,
            GgufValue::Bool(_) => 7,
            GgufValue::String(_) => 8,
            GgufValue::Array(..) => 9,
            GgufValue::U64(_) => 10,
            GgufValue::I64(_) => 11,
            GgufValue::F64(_) => 12,
        }
    }

    /// A short, human-readable form; long strings and arrays are abbreviated.
    pub fn display(&self, max_chars: usize) -> String {
        let text = match self {
            GgufValue::U8(value) => value.to_string(),
            GgufValue::I8(value) => value.to_string(),
            GgufValue::U16(value) => value.to_string(),
            GgufValue::I16(value) => value.to_string(),
            GgufValue::U32(value) => value.to_string(),
            GgufValue::I32(value) => value.to_string(),
            GgufValue::F32(value) => value.to_string(),
            GgufValue::Bool(value) => value.to_string(),
            GgufValue::String(value) => format!("{value:?}"),
            GgufValue::Array(_, items) => {
                let shown: Vec<String> =
                    items.iter().take(8).map(|item| item.display(40)).collect();
                if items.len() > 8 {
                    format!("[{}, … {} items]", shown.join(", "), items.len())
                } else {
                    format!("[{}]", shown.join(", "))
                }
            }
            GgufValue::U64(value) => value.to_string(),
            GgufValue::I64(value) => value.to_string(),
            GgufValue::F64(value) => value.to_string(),
        };
        truncate_chars(text, max_chars)
    }

    /// Parses `text` as a value of the same type as `self`, so an edit keeps
    /// the key's type.
    fn parse_like(&self, text: &str) -> Result<GgufValue> {
        let text = text.trim_end_matches('\0');
        Ok(match self {
            GgufValue::U8(_) => GgufValue::U8(text.trim().parse()?),
            GgufValue::I8(_) => GgufValue::I8(text.trim().parse()?),
            GgufValue::U16(_) => GgufValue::U16(text.trim().parse()?),
            GgufValue::I16(_) => GgufValue::I16(text.trim().parse()?),
            GgufValue::U32(_) => GgufValue::U32(text.trim().parse()?),
            GgufValue::I32(_) => GgufValue::I32(text.trim().parse()?),
            GgufValue::F32(_) => GgufValue::F32(text.trim().parse()?),
            GgufValue::Bool(_) => GgufValue::Bool(text.trim().parse()?),
            GgufValue::String(_) => GgufValue::String(text.to_string()),
            GgufValue::U64(_) => GgufValue::U64(text.trim().parse()?),
            GgufValue::I64(_) => GgufValue::I64(text.trim().parse()?),
            GgufValue::F64(_) => GgufValue::F64(text.trim().parse()?),
            GgufValue::Array(..) => bail!("array values can't be edited as text"),
        })
    }
}

#[derive(Debug, Clone)]
pub struct GgufTensor {
    pub name: String,
    pub dimensions: Vec<u64>,
    pub tensor_type: u32,
    pub offset: u64,
}

impl GgufTensor {
    pub fn element_count(&self) -> u64 {
        self.dimensions.iter().product()
    }
}

#[derive(Debug, Clone)]
pub struct GgufHeader {
    pub version: u32,
    pub metadata: Vec<(String, GgufValue)>,
    pub tensors: Vec<GgufTensor>,
    /// Where the tensor data begins in the file.
    pub data_offset: u64,
    pub alignment: u64,
}

impl GgufHeader {
    pub fn get(&self, key: &str) -> Option<&GgufValue> {
        self.metadata
            .iter()
            .find(|(existing, _)| existing == key)
            .map(|(_, value)| value)
    }
}

struct Reader<R: Read> {
    inner: R,
    position: u64,
}

impl<R: Read> Reader<R> {
    fn bytes<const N: usize>(&mut self) -> Result<[u8; N]> {
        let mut buffer = [0u8; N];
        self.inner
            .read_exact(&mut buffer)
            .context("the file ended inside its header")?;
        self.position += N as u64;
        Ok(buffer)
    }

    fn u32(&mut self) -> Result<u32> {
        Ok(u32::from_le_bytes(self.bytes()?))
    }

    fn u64(&mut self) -> Result<u64> {
        Ok(u64::from_le_bytes(self.bytes()?))
    }

    fn string(&mut self) -> Result<String> {
        let length = self.u64()?;
        ensure!(
            length < MAX_HEADER_BYTES,
            "a header string is impossibly long"
        );
        let mut buffer = vec![0u8; length as usize];
        self.inner
            .read_exact(&mut buffer)
            .context("the file ended inside its header")?;
        self.position += length;
        Ok(String::from_utf8_lossy(&buffer).into_owned())
    }

    fn value(&mut self, type_id: u32, depth: usize) -> Result<GgufValue> {
        Ok(match type_id {
            0 => GgufValue::U8(u8::from_le_bytes(self.bytes()?)),
            1 => GgufValue::I8(i8::from_le_bytes(self.bytes()?)),
            2 => GgufValue::U16(u16::from_le_bytes(self.bytes()?)),
            3 => GgufValue::I16(i16::from_le_bytes(self.bytes()?)),
            4 => GgufValue::U32(self.u32()?),
            5 => GgufValue::I32(i32::from_le_bytes(self.bytes()?)),
            6 => GgufValue::F32(f32::from_le_bytes(self.bytes()?)),
            7 => GgufValue::Bool(u8::from_le_bytes(self.bytes()?) != 0),
            8 => GgufValue::String(self.string()?),
            9 => {
                ensure!(depth < 4, "GGUF arrays are nested too deeply");
                let item_type = self.u32()?;
                let length = self.u64()?;
                ensure!(length < MAX_HEADER_BYTES, "a GGUF array is impossibly long");
                let mut items = Vec::with_capacity(length.min(1 << 20) as usize);
                for _ in 0..length {
                    items.push(self.value(item_type, depth + 1)?);
                }
                GgufValue::Array(item_type, items)
            }
            10 => GgufValue::U64(self.u64()?),
            11 => GgufValue::I64(i64::from_le_bytes(self.bytes()?)),
            12 => GgufValue::F64(f64::from_le_bytes(self.bytes()?)),
            other => bail!("unknown GGUF value type {other}"),
        })
    }
}

pub fn read_gguf_header(path: &Path) -> Result<GgufHeader> {
    let file = File::open(path).with_context(|| format!("couldn't open {}", path.display()))?;
    let mut reader = Reader {
        inner: BufReader::new(file),
        position: 0,
    };
    let magic: [u8; 4] = reader.bytes()?;
    ensure!(
        &magic == GGUF_MAGIC,
        "{} is not a GGUF file",
        path.display()
    );
    let version = reader.u32()?;
    ensure!(
        version == 2 || version == 3,
        "GGUF version {version} isn't supported (only 2 and 3)"
    );
    let tensor_count = reader.u64()?;
    let metadata_count = reader.u64()?;
    ensure!(
        tensor_count < 10_000_000 && metadata_count < 10_000_000,
        "the GGUF header counts are impossibly large"
    );

    let mut metadata = Vec::with_capacity(metadata_count as usize);
    for _ in 0..metadata_count {
        let key = reader.string()?;
        let type_id = reader.u32()?;
        let value = reader.value(type_id, 0)?;
        metadata.push((key, value));
    }

    let mut tensors = Vec::with_capacity(tensor_count as usize);
    for _ in 0..tensor_count {
        let name = reader.string()?;
        let dimension_count = reader.u32()?;
        ensure!(
            dimension_count <= 8,
            "tensor {name} has too many dimensions"
        );
        let mut dimensions = Vec::with_capacity(dimension_count as usize);
        for _ in 0..dimension_count {
            dimensions.push(reader.u64()?);
        }
        let tensor_type = reader.u32()?;
        let offset = reader.u64()?;
        tensors.push(GgufTensor {
            name,
            dimensions,
            tensor_type,
            offset,
        });
    }

    let alignment = metadata
        .iter()
        .find(|(key, _)| key == "general.alignment")
        .and_then(|(_, value)| match value {
            GgufValue::U32(alignment) => Some(u64::from(*alignment)),
            _ => None,
        })
        .filter(|alignment| *alignment > 0)
        .unwrap_or(DEFAULT_ALIGNMENT);
    let data_offset = reader.position.div_ceil(alignment) * alignment;
    Ok(GgufHeader {
        version,
        metadata,
        tensors,
        data_offset,
        alignment,
    })
}

fn write_string(out: &mut impl Write, text: &str) -> Result<u64> {
    out.write_all(&(text.len() as u64).to_le_bytes())?;
    out.write_all(text.as_bytes())?;
    Ok(8 + text.len() as u64)
}

fn write_value(out: &mut impl Write, value: &GgufValue) -> Result<u64> {
    Ok(match value {
        GgufValue::U8(value) => {
            out.write_all(&value.to_le_bytes())?;
            1
        }
        GgufValue::I8(value) => {
            out.write_all(&value.to_le_bytes())?;
            1
        }
        GgufValue::U16(value) => {
            out.write_all(&value.to_le_bytes())?;
            2
        }
        GgufValue::I16(value) => {
            out.write_all(&value.to_le_bytes())?;
            2
        }
        GgufValue::U32(value) => {
            out.write_all(&value.to_le_bytes())?;
            4
        }
        GgufValue::I32(value) => {
            out.write_all(&value.to_le_bytes())?;
            4
        }
        GgufValue::F32(value) => {
            out.write_all(&value.to_le_bytes())?;
            4
        }
        GgufValue::Bool(value) => {
            out.write_all(&[u8::from(*value)])?;
            1
        }
        GgufValue::String(value) => write_string(out, value)?,
        GgufValue::Array(item_type, items) => {
            out.write_all(&item_type.to_le_bytes())?;
            out.write_all(&(items.len() as u64).to_le_bytes())?;
            let mut written = 12;
            for item in items {
                written += write_value(out, item)?;
            }
            written
        }
        GgufValue::U64(value) => {
            out.write_all(&value.to_le_bytes())?;
            8
        }
        GgufValue::I64(value) => {
            out.write_all(&value.to_le_bytes())?;
            8
        }
        GgufValue::F64(value) => {
            out.write_all(&value.to_le_bytes())?;
            8
        }
    })
}

/// A metadata change to apply to a model file.
#[derive(Debug, Clone)]
pub enum MetadataEdit {
    /// Sets `key`. An existing key keeps its type; a new key is a string.
    Set {
        key: String,
        value: String,
    },
    Remove {
        key: String,
    },
}

/// Writes a copy of `source` with its metadata changed to `destination`,
/// reporting progress as a fraction of the bytes copied. Tensor data is
/// copied byte for byte.
pub fn edit_metadata(
    source: &Path,
    destination: &Path,
    edits: &[MetadataEdit],
    progress: &mut dyn FnMut(f64),
) -> Result<()> {
    ensure!(
        source != destination,
        "write edits to a new file; the original is read while copying"
    );
    match detect_format(source)? {
        Format::Gguf => edit_gguf(source, destination, edits, progress),
        Format::Safetensors => edit_safetensors(source, destination, edits, progress),
    }
}

fn edit_gguf(
    source: &Path,
    destination: &Path,
    edits: &[MetadataEdit],
    progress: &mut dyn FnMut(f64),
) -> Result<()> {
    let mut header = read_gguf_header(source)?;
    for edit in edits {
        match edit {
            MetadataEdit::Set { key, value } => {
                ensure!(
                    key != "general.alignment",
                    "general.alignment can't be changed; it fixes where the tensor data sits"
                );
                match header
                    .metadata
                    .iter_mut()
                    .find(|(existing, _)| existing == key)
                {
                    Some((_, existing)) => {
                        *existing = existing
                            .parse_like(value)
                            .with_context(|| format!("{value:?} doesn't fit the type of {key}"))?;
                    }
                    None => header
                        .metadata
                        .push((key.clone(), GgufValue::String(value.clone()))),
                }
            }
            MetadataEdit::Remove { key } => {
                ensure!(
                    key != "general.alignment",
                    "general.alignment can't be removed"
                );
                let before = header.metadata.len();
                header.metadata.retain(|(existing, _)| existing != key);
                ensure!(header.metadata.len() < before, "{key} isn't in this file");
            }
        }
    }

    let mut output = BufWriter::new(
        File::create(destination)
            .with_context(|| format!("couldn't create {}", destination.display()))?,
    );
    let mut written: u64 = 0;
    output.write_all(GGUF_MAGIC)?;
    output.write_all(&header.version.to_le_bytes())?;
    output.write_all(&(header.tensors.len() as u64).to_le_bytes())?;
    output.write_all(&(header.metadata.len() as u64).to_le_bytes())?;
    written += 24;
    for (key, value) in &header.metadata {
        written += write_string(&mut output, key)?;
        output.write_all(&value.type_id().to_le_bytes())?;
        written += 4 + write_value(&mut output, value)?;
    }
    for tensor in &header.tensors {
        written += write_string(&mut output, &tensor.name)?;
        output.write_all(&(tensor.dimensions.len() as u32).to_le_bytes())?;
        written += 4;
        for dimension in &tensor.dimensions {
            output.write_all(&dimension.to_le_bytes())?;
            written += 8;
        }
        output.write_all(&tensor.tensor_type.to_le_bytes())?;
        output.write_all(&tensor.offset.to_le_bytes())?;
        written += 12;
    }
    let padded = written.div_ceil(header.alignment) * header.alignment;
    output.write_all(&vec![0u8; (padded - written) as usize])?;

    copy_tail(source, header.data_offset, &mut output, progress)?;
    output.flush()?;
    Ok(())
}

fn edit_safetensors(
    source: &Path,
    destination: &Path,
    edits: &[MetadataEdit],
    progress: &mut dyn FnMut(f64),
) -> Result<()> {
    let (header_length, mut header) = read_safetensors_header(source)?;
    let object = header
        .as_object_mut()
        .context("the safetensors header isn't a JSON object")?;
    let metadata = object
        .entry("__metadata__")
        .or_insert_with(|| serde_json::Value::Object(Default::default()))
        .as_object_mut()
        .context("__metadata__ isn't a JSON object")?;
    for edit in edits {
        match edit {
            MetadataEdit::Set { key, value } => {
                // The format only allows string metadata.
                metadata.insert(key.clone(), serde_json::Value::String(value.clone()));
            }
            MetadataEdit::Remove { key } => {
                ensure!(metadata.remove(key).is_some(), "{key} isn't in this file");
            }
        }
    }

    let mut json = serde_json::to_vec(&header)?;
    // Tensor data stays 8-byte aligned when the header length is.
    while json.len() % 8 != 0 {
        json.push(b' ');
    }
    let mut output = BufWriter::new(
        File::create(destination)
            .with_context(|| format!("couldn't create {}", destination.display()))?,
    );
    output.write_all(&(json.len() as u64).to_le_bytes())?;
    output.write_all(&json)?;
    copy_tail(source, 8 + header_length, &mut output, progress)?;
    output.flush()?;
    Ok(())
}

fn copy_tail(
    source: &Path,
    from: u64,
    output: &mut impl Write,
    progress: &mut dyn FnMut(f64),
) -> Result<()> {
    let mut input = File::open(source)?;
    let total = input.metadata()?.len().saturating_sub(from).max(1);
    input.seek(SeekFrom::Start(from))?;
    let mut buffer = vec![0u8; COPY_BUFFER_BYTES];
    let mut copied: u64 = 0;
    loop {
        let read = input.read(&mut buffer)?;
        if read == 0 {
            break;
        }
        output.write_all(&buffer[..read])?;
        copied += read as u64;
        progress(copied as f64 / total as f64);
    }
    Ok(())
}

pub fn read_safetensors_header(path: &Path) -> Result<(u64, serde_json::Value)> {
    let mut file = File::open(path).with_context(|| format!("couldn't open {}", path.display()))?;
    let mut length = [0u8; 8];
    file.read_exact(&mut length)
        .context("the file is too short to be safetensors")?;
    let length = u64::from_le_bytes(length);
    ensure!(
        length > 1 && length < MAX_HEADER_BYTES,
        "the safetensors header length is invalid"
    );
    let mut json = vec![0u8; length as usize];
    file.read_exact(&mut json)
        .context("the file ended inside its header")?;
    let header =
        serde_json::from_slice(&json).context("the safetensors header isn't valid JSON")?;
    Ok((length, header))
}

/// ggml's names for GGUF tensor types, which are also its quantization names.
pub fn gguf_type_name(tensor_type: u32) -> String {
    let name = match tensor_type {
        0 => "F32",
        1 => "F16",
        2 => "Q4_0",
        3 => "Q4_1",
        6 => "Q5_0",
        7 => "Q5_1",
        8 => "Q8_0",
        9 => "Q8_1",
        10 => "Q2_K",
        11 => "Q3_K",
        12 => "Q4_K",
        13 => "Q5_K",
        14 => "Q6_K",
        15 => "Q8_K",
        16 => "IQ2_XXS",
        17 => "IQ2_XS",
        18 => "IQ3_XXS",
        19 => "IQ1_S",
        20 => "IQ4_NL",
        21 => "IQ3_S",
        22 => "IQ2_S",
        23 => "IQ4_XS",
        24 => "I8",
        25 => "I16",
        26 => "I32",
        27 => "I64",
        28 => "F64",
        29 => "IQ1_M",
        30 => "BF16",
        34 => "TQ1_0",
        35 => "TQ2_0",
        39 => "MXFP4",
        other => return format!("type {other}"),
    };
    name.to_string()
}

/// A readable report of a model file, built from its header alone.
pub fn describe(path: &Path, max_value_chars: usize) -> Result<String> {
    let size = std::fs::metadata(path)?.len();
    let mut report = format!("{}\nsize: {}\n", path.display(), human_bytes(size));
    match detect_format(path)? {
        Format::Gguf => {
            let header = read_gguf_header(path)?;
            let parameters: u64 = header.tensors.iter().map(GgufTensor::element_count).sum();
            report.push_str(&format!(
                "format: GGUF v{}\nparameters: {}\ntensors: {}\n",
                header.version,
                human_count(parameters),
                header.tensors.len()
            ));
            let mut types: Vec<(String, u64)> = Vec::new();
            for tensor in &header.tensors {
                let name = gguf_type_name(tensor.tensor_type);
                match types.iter_mut().find(|(existing, _)| *existing == name) {
                    Some((_, count)) => *count += 1,
                    None => types.push((name, 1)),
                }
            }
            types.sort_by(|a, b| b.1.cmp(&a.1));
            let types: Vec<String> = types
                .iter()
                .map(|(name, count)| format!("{name} ×{count}"))
                .collect();
            report.push_str(&format!(
                "tensor types: {}\n\nmetadata:\n",
                types.join(", ")
            ));
            for (key, value) in &header.metadata {
                report.push_str(&format!("  {key} = {}\n", value.display(max_value_chars)));
            }
        }
        Format::Safetensors => {
            let (_, header) = read_safetensors_header(path)?;
            let object = header
                .as_object()
                .context("the header isn't a JSON object")?;
            let mut parameters: u64 = 0;
            let mut dtypes: Vec<(String, u64)> = Vec::new();
            let mut tensors = 0;
            for (name, tensor) in object {
                if name == "__metadata__" {
                    continue;
                }
                tensors += 1;
                let elements: u64 = tensor["shape"]
                    .as_array()
                    .map(|shape| shape.iter().filter_map(|value| value.as_u64()).product())
                    .unwrap_or_default();
                parameters += elements;
                let dtype = tensor["dtype"].as_str().unwrap_or("unknown").to_string();
                match dtypes.iter_mut().find(|(existing, _)| *existing == dtype) {
                    Some((_, count)) => *count += 1,
                    None => dtypes.push((dtype, 1)),
                }
            }
            let dtypes: Vec<String> = dtypes
                .iter()
                .map(|(name, count)| format!("{name} ×{count}"))
                .collect();
            report.push_str(&format!(
                "format: safetensors\nparameters: {}\ntensors: {tensors}\ndtypes: {}\n\nmetadata:\n",
                human_count(parameters),
                dtypes.join(", ")
            ));
            if let Some(metadata) = object
                .get("__metadata__")
                .and_then(|value| value.as_object())
            {
                for (key, value) in metadata {
                    report.push_str(&format!(
                        "  {key} = {}\n",
                        truncate_chars(value.to_string(), max_value_chars)
                    ));
                }
            }
        }
    }
    Ok(report)
}

/// The file name an edit writes to by default: next to the original, which
/// is left untouched.
pub fn default_edited_path(source: &Path) -> PathBuf {
    let stem = source
        .file_stem()
        .map(|stem| stem.to_string_lossy().into_owned())
        .unwrap_or_else(|| "model".into());
    let extension = source
        .extension()
        .map(|extension| format!(".{}", extension.to_string_lossy()))
        .unwrap_or_default();
    source.with_file_name(format!("{stem}.edited{extension}"))
}

/// A local model runner a file can be imported into.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Runner {
    Ollama,
    LmStudio,
}

/// Whether `name` is usable as a model name in both runners.
pub fn valid_model_name(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= 128
        && !name.starts_with(['.', '-', '/'])
        && !name.contains("..")
        && name
            .chars()
            .all(|character| character.is_ascii_alphanumeric() || "._-:/".contains(character))
}

/// Imports a model file so the runner lists it, which also makes it appear
/// in noah's model picker. Blocking: call it off the main thread.
pub fn import(
    source: &Path,
    runner: Runner,
    name: &str,
    progress: &mut dyn FnMut(f64),
) -> Result<String> {
    ensure!(
        valid_model_name(name),
        "{name:?} isn't a usable model name; use letters, digits, '.', '_', '-' or ':'"
    );
    let source = source
        .canonicalize()
        .with_context(|| format!("couldn't find {}", source.display()))?;
    detect_format(&source)?;
    match runner {
        Runner::Ollama => import_into_ollama(&source, name),
        Runner::LmStudio => {
            let home = home_directory().context("couldn't find the home folder")?;
            let destination = import_into_lm_studio(&source, name, &home, progress)?;
            Ok(format!(
                "imported into LM Studio at {}. load it in LM Studio, then choose it in noah's model picker under LM Studio.",
                destination.display()
            ))
        }
    }
}

fn import_into_ollama(source: &Path, name: &str) -> Result<String> {
    let ollama = find_program("ollama").context(
        "Ollama isn't installed or isn't on the PATH. Install it from ollama.com, \
         or import into LM Studio instead.",
    )?;
    // Ollama reads the Modelfile's FROM path relative to the Modelfile, and
    // paths with spaces aren't accepted, so the file is linked in under a
    // plain name.
    let workspace = std::env::temp_dir().join(format!("noah-import-{}", std::process::id()));
    std::fs::create_dir_all(&workspace)?;
    let file_name = match source.extension().and_then(|extension| extension.to_str()) {
        Some("safetensors") => "model.safetensors",
        _ => "model.gguf",
    };
    let linked = workspace.join(file_name);
    std::fs::remove_file(&linked).ok();
    link_or_copy(source, &linked, &mut |_| {})?;
    std::fs::write(workspace.join("Modelfile"), format!("FROM ./{file_name}\n"))?;
    let output = background_command(&ollama)
        .arg("create")
        .arg(name)
        .arg("-f")
        .arg("Modelfile")
        .current_dir(&workspace)
        .output()
        .context("couldn't run ollama")?;
    std::fs::remove_dir_all(&workspace).ok();
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    ensure!(
        output.status.success(),
        "ollama create failed: {}",
        if stderr.trim().is_empty() {
            stdout.trim()
        } else {
            stderr.trim()
        }
    );
    Ok(format!(
        "imported into Ollama as {name}. choose it in the model picker under Ollama."
    ))
}

fn import_into_lm_studio(
    source: &Path,
    name: &str,
    home: &Path,
    progress: &mut dyn FnMut(f64),
) -> Result<PathBuf> {
    let models = [
        home.join(".lmstudio").join("models"),
        home.join(".cache").join("lm-studio").join("models"),
    ]
    .into_iter()
    .find(|directory| directory.is_dir())
    .unwrap_or_else(|| home.join(".lmstudio").join("models"));
    let destination_directory = models.join("noah").join(name.replace([':', '/'], "-"));
    std::fs::create_dir_all(&destination_directory)?;
    let file_name = source
        .file_name()
        .context("the model path has no file name")?;
    let destination = destination_directory.join(file_name);
    ensure!(
        !destination.exists(),
        "{} already exists; choose another name",
        destination.display()
    );
    link_or_copy(source, &destination, progress)?;
    Ok(destination)
}

/// Hard-links when source and destination share a disk, so a 70 GB model
/// costs no space; otherwise copies.
fn link_or_copy(source: &Path, destination: &Path, progress: &mut dyn FnMut(f64)) -> Result<()> {
    if std::fs::hard_link(source, destination).is_ok() {
        progress(1.0);
        return Ok(());
    }
    let mut input = File::open(source)?;
    let total = input.metadata()?.len().max(1);
    let mut output = BufWriter::new(File::create(destination)?);
    let mut buffer = vec![0u8; COPY_BUFFER_BYTES];
    let mut copied: u64 = 0;
    loop {
        let read = input.read(&mut buffer)?;
        if read == 0 {
            break;
        }
        output.write_all(&buffer[..read])?;
        copied += read as u64;
        progress(copied as f64 / total as f64);
    }
    output.flush()?;
    Ok(())
}

fn home_directory() -> Option<PathBuf> {
    std::env::var_os(if cfg!(windows) { "USERPROFILE" } else { "HOME" }).map(PathBuf::from)
}

fn find_program(name: &str) -> Option<PathBuf> {
    let executable = if cfg!(windows) {
        format!("{name}.exe")
    } else {
        name.to_string()
    };
    let path = std::env::var_os("PATH")?;
    std::env::split_paths(&path)
        .map(|directory| directory.join(&executable))
        .chain(
            // Ollama's Windows installer adds itself to the PATH only for new
            // shells, so its default location is checked too.
            std::env::var_os("LOCALAPPDATA").map(|local| {
                PathBuf::from(local)
                    .join("Programs")
                    .join("Ollama")
                    .join(&executable)
            }),
        )
        .find(|candidate| candidate.is_file())
}

fn background_command(program: &Path) -> std::process::Command {
    #[allow(unused_mut)]
    let mut command = std::process::Command::new(program);
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt as _;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        command.creation_flags(CREATE_NO_WINDOW);
    }
    command
}

pub fn human_bytes(bytes: u64) -> String {
    const UNITS: [&str; 5] = ["B", "KB", "MB", "GB", "TB"];
    let mut value = bytes as f64;
    let mut unit = 0;
    while value >= 1024.0 && unit < UNITS.len() - 1 {
        value /= 1024.0;
        unit += 1;
    }
    if unit == 0 {
        format!("{bytes} B")
    } else {
        format!("{value:.1} {}", UNITS[unit])
    }
}

fn human_count(count: u64) -> String {
    match count {
        count if count >= 1_000_000_000 => format!("{:.2}B", count as f64 / 1e9),
        count if count >= 1_000_000 => format!("{:.1}M", count as f64 / 1e6),
        count => count.to_string(),
    }
}

fn truncate_chars(text: String, max_chars: usize) -> String {
    if text.chars().count() <= max_chars {
        return text;
    }
    let mut cut: String = text.chars().take(max_chars).collect();
    cut.push('…');
    cut
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Builds a small but real GGUF file: two metadata keys, two tensors and
    /// recognizable tensor bytes.
    fn write_test_gguf(path: &Path) -> Vec<u8> {
        let mut header = Vec::new();
        header.extend_from_slice(GGUF_MAGIC);
        header.extend_from_slice(&3u32.to_le_bytes());
        header.extend_from_slice(&2u64.to_le_bytes());
        header.extend_from_slice(&2u64.to_le_bytes());
        write_string(&mut header, "general.name").ok();
        header.extend_from_slice(&8u32.to_le_bytes());
        write_string(&mut header, "tiny").ok();
        write_string(&mut header, "llama.context_length").ok();
        header.extend_from_slice(&4u32.to_le_bytes());
        header.extend_from_slice(&2048u32.to_le_bytes());
        for (name, offset) in [("token_embd.weight", 0u64), ("output.weight", 32u64)] {
            write_string(&mut header, name).ok();
            header.extend_from_slice(&2u32.to_le_bytes());
            header.extend_from_slice(&4u64.to_le_bytes());
            header.extend_from_slice(&2u64.to_le_bytes());
            header.extend_from_slice(&0u32.to_le_bytes());
            header.extend_from_slice(&offset.to_le_bytes());
        }
        while header.len() % 32 != 0 {
            header.push(0);
        }
        let data: Vec<u8> = (0..64u8).collect();
        let mut file = header;
        file.extend_from_slice(&data);
        std::fs::write(path, &file).expect("write");
        data
    }

    #[test]
    fn reads_gguf_header() {
        let directory = tempfile::tempdir().expect("tempdir");
        let path = directory.path().join("tiny.gguf");
        write_test_gguf(&path);
        let header = read_gguf_header(&path).expect("reads");
        assert_eq!(
            header.get("general.name"),
            Some(&GgufValue::String("tiny".into()))
        );
        assert_eq!(header.tensors.len(), 2);
        assert_eq!(header.tensors[1].element_count(), 8);
        let report = describe(&path, 200).expect("describes");
        assert!(report.contains("format: GGUF v3"), "{report}");
        assert!(report.contains("F32 ×2"), "{report}");
        assert!(report.contains("llama.context_length = 2048"), "{report}");
    }

    #[test]
    fn edits_gguf_metadata_and_keeps_tensor_data() {
        let directory = tempfile::tempdir().expect("tempdir");
        let source = directory.path().join("tiny.gguf");
        let data = write_test_gguf(&source);
        let destination = default_edited_path(&source);
        assert_eq!(
            destination.file_name().and_then(|name| name.to_str()),
            Some("tiny.edited.gguf")
        );
        let long_template = "{% for message in messages %}".repeat(40);
        let mut last_progress = 0.0;
        edit_metadata(
            &source,
            &destination,
            &[
                MetadataEdit::Set {
                    key: "general.name".into(),
                    value: "renamed by shepherd".into(),
                },
                MetadataEdit::Set {
                    key: "llama.context_length".into(),
                    value: "8192".into(),
                },
                MetadataEdit::Set {
                    key: "tokenizer.chat_template".into(),
                    value: long_template.clone(),
                },
            ],
            &mut |fraction| last_progress = fraction,
        )
        .expect("edits");
        assert_eq!(last_progress, 1.0);

        let edited = read_gguf_header(&destination).expect("reads edited");
        assert_eq!(
            edited.get("general.name"),
            Some(&GgufValue::String("renamed by shepherd".into()))
        );
        assert_eq!(
            edited.get("llama.context_length"),
            Some(&GgufValue::U32(8192))
        );
        assert_eq!(
            edited.get("tokenizer.chat_template"),
            Some(&GgufValue::String(long_template))
        );
        assert_eq!(edited.data_offset % 32, 0);
        let bytes = std::fs::read(&destination).expect("read");
        assert_eq!(&bytes[edited.data_offset as usize..], &data[..]);
        assert!(
            edit_metadata(
                &source,
                &destination,
                &[MetadataEdit::Set {
                    key: "llama.context_length".into(),
                    value: "lots".into()
                }],
                &mut |_| {}
            )
            .is_err(),
            "a number key must reject text"
        );
    }

    #[test]
    fn edits_safetensors_metadata() {
        let directory = tempfile::tempdir().expect("tempdir");
        let source = directory.path().join("model.safetensors");
        let header = br#"{"__metadata__":{"format":"pt"},"w":{"dtype":"F16","shape":[2,2],"data_offsets":[0,8]}}"#;
        let mut file = (header.len() as u64).to_le_bytes().to_vec();
        file.extend_from_slice(header);
        file.extend_from_slice(&[1, 2, 3, 4, 5, 6, 7, 8]);
        std::fs::write(&source, &file).expect("write");
        assert!(describe(&source, 80).expect("describes").contains("F16 ×1"));

        let destination = directory.path().join("out.safetensors");
        edit_metadata(
            &source,
            &destination,
            &[MetadataEdit::Set {
                key: "author".into(),
                value: "asher".into(),
            }],
            &mut |_| {},
        )
        .expect("edits");
        let (length, header) = read_safetensors_header(&destination).expect("reads");
        assert_eq!(header["__metadata__"]["author"], "asher");
        assert_eq!(length % 8, 0);
        let bytes = std::fs::read(&destination).expect("read");
        assert_eq!(&bytes[8 + length as usize..], &[1, 2, 3, 4, 5, 6, 7, 8]);
    }

    #[test]
    fn imports_into_lm_studio_by_linking() {
        let directory = tempfile::tempdir().expect("tempdir");
        let source = directory.path().join("tiny.gguf");
        write_test_gguf(&source);
        let home = directory.path().join("home");
        std::fs::create_dir_all(home.join(".lmstudio").join("models")).expect("mkdir");
        let destination =
            import_into_lm_studio(&source, "tiny-test", &home, &mut |_| {}).expect("imports");
        assert!(
            destination.ends_with("noah/tiny-test/tiny.gguf"),
            "{}",
            destination.display()
        );
        assert_eq!(
            std::fs::read(&destination).ok(),
            std::fs::read(&source).ok()
        );
        assert!(import_into_lm_studio(&source, "tiny-test", &home, &mut |_| {}).is_err());
        assert!(!valid_model_name("../escape"));
        assert!(valid_model_name("qwen3-coder:30b"));
    }

    #[test]
    fn rejects_other_files() {
        let directory = tempfile::tempdir().expect("tempdir");
        let path = directory.path().join("notes.txt");
        std::fs::write(&path, "hello there, not a model").expect("write");
        assert!(detect_format(&path).is_err());
    }
}
