//! English to Russian, offline, with the public OPUS-MT model.
//!
//! The weights are not in the repository. The first launch downloads the quantised encoder,
//! decoder and tokenizer into the user's local data folder, then keeps them. Nothing is sent
//! except that one download.

use std::fs::{self, File};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::time::Duration;

use ort::session::Session;
use ort::value::Tensor;
use tokenizers::Tokenizer;

const ENCODER_URL: &str = "https://huggingface.co/Xenova/opus-mt-en-ru/resolve/main/onnx/encoder_model_quantized.onnx";
const DECODER_URL: &str = "https://huggingface.co/Xenova/opus-mt-en-ru/resolve/main/onnx/decoder_model_quantized.onnx";
const TOKENIZER_URL: &str = "https://huggingface.co/Xenova/opus-mt-en-ru/resolve/main/tokenizer.json";

const DECODER_START: i64 = 62517;
const EOS: i64 = 0;
const MAX_NEW_TOKENS: usize = 48;

pub struct Translator {
    encoder: Session,
    decoder: Session,
    tokenizer: Tokenizer,
}

impl Translator {
    pub fn load(
        bundled: Option<&Path>,
        log: &mut dyn Write,
        mut report: impl FnMut(&str),
    ) -> Result<Self, String> {
        let dir = model_dir(bundled)?;
        let encoder_path = ensure(&dir, "encoder.onnx", ENCODER_URL, 8_000_000, log, &mut report)?;
        let decoder_path = ensure(&dir, "decoder.onnx", DECODER_URL, 8_000_000, log, &mut report)?;
        let tokenizer_path = ensure(&dir, "tokenizer.json", TOKENIZER_URL, 50_000, log, &mut report)?;
        report("sessions");

        let _ = writeln!(log, "loading sessions");
        let encoder = Session::builder()
            .map_err(|error| error.to_string())?
            .with_intra_threads(2)
            .map_err(|error| error.to_string())?
            .commit_from_file(&encoder_path)
            .map_err(|error| format!("encoder: {error}"))?;
        let decoder = Session::builder()
            .map_err(|error| error.to_string())?
            .with_intra_threads(2)
            .map_err(|error| error.to_string())?
            .commit_from_file(&decoder_path)
            .map_err(|error| format!("decoder: {error}"))?;
        let _ = writeln!(log, "encoder inputs: {}", input_names(&encoder));
        let _ = writeln!(log, "decoder inputs: {}", input_names(&decoder));
        let tokenizer = load_tokenizer(&tokenizer_path)?;
        Ok(Self {
            encoder,
            decoder,
            tokenizer,
        })
    }

    pub fn translate(&mut self, text: &str) -> Result<String, String> {
        let text = text.trim();
        if text.is_empty() {
            return Ok(String::new());
        }
        let encoding = self.tokenizer.encode(text, true).map_err(|error| error.to_string())?;
        let mut ids: Vec<i64> = encoding.get_ids().iter().map(|id| i64::from(*id)).collect();
        if ids.is_empty() {
            return Ok(text.to_owned());
        }
        if *ids.last().unwrap_or(&EOS) != EOS {
            ids.push(EOS);
        }
        let length = ids.len() as i64;
        let mask = vec![1i64; length as usize];
        let ids_tensor = tensor_i64(vec![1i64, length], ids)?;
        let mask_tensor = tensor_i64(vec![1i64, length], mask.clone())?;

        let encoded = self
            .encoder
            .run(ort::inputs![
                "input_ids" => ids_tensor,
                "attention_mask" => mask_tensor,
            ])
            .map_err(|error| format!("encoder run: {error}"))?;
        let (hidden_shape, hidden) = output_f32(&encoded, "hidden")?;
        if hidden_shape.len() != 3 {
            return Err(format!("encoder rank {}", hidden_shape.len()));
        }
        drop(encoded);

        let mut generated = vec![DECODER_START];
        for _ in 0..MAX_NEW_TOKENS {
            let step_len = generated.len() as i64;
            let step_tensor = tensor_i64(vec![1, step_len], generated.clone())?;
            let hidden_tensor = tensor_f32(hidden_shape.clone(), hidden.clone())?;
            let mask_tensor = tensor_i64(vec![1, length], mask.clone())?;
            let decoded = self
                .decoder
                .run(ort::inputs![
                    "input_ids" => step_tensor,
                    "encoder_hidden_states" => hidden_tensor,
                    "encoder_attention_mask" => mask_tensor,
                ])
                .map_err(|error| format!("decoder run: {error}"))?;
            let (shape, logits) = output_f32(&decoded, "logits")?;
            if shape.len() != 3 || shape[2] == 0 || shape[1] == 0 {
                return Err("decoder logits have an unexpected shape".to_owned());
            }
            let vocab = shape[2] as usize;
            let last = (shape[1] as usize - 1) * vocab;
            let mut best = 0usize;
            let mut best_score = f32::NEG_INFINITY;
            for (index, score) in logits.iter().skip(last).take(vocab).enumerate() {
                if index as i64 == DECODER_START {
                    continue;
                }
                if *score > best_score {
                    best_score = *score;
                    best = index;
                }
            }
            let token = best as i64;
            if token == EOS {
                break;
            }
            generated.push(token);
        }

        let output_ids: Vec<u32> = generated
            .into_iter()
            .filter(|id| *id != DECODER_START && *id != EOS && *id >= 0)
            .map(|id| id as u32)
            .collect();
        self.tokenizer
            .decode(&output_ids, true)
            .map_err(|error| error.to_string())
    }
}

fn input_names(session: &Session) -> String {
    session
        .inputs()
        .iter()
        .map(|input| input.name())
        .collect::<Vec<_>>()
        .join(", ")
}

fn tensor_i64(shape: Vec<i64>, data: Vec<i64>) -> Result<Tensor<i64>, String> {
    Tensor::from_array((shape, data)).map_err(|error| error.to_string())
}

fn tensor_f32(shape: Vec<i64>, data: Vec<f32>) -> Result<Tensor<f32>, String> {
    Tensor::from_array((shape, data)).map_err(|error| error.to_string())
}

fn output_f32(outputs: &ort::session::SessionOutputs<'_>, needle: &str) -> Result<(Vec<i64>, Vec<f32>), String> {
    let names: Vec<String> = outputs.iter().map(|(name, _)| name.to_owned()).collect();
    let name = names
        .iter()
        .find(|name| name.contains(needle))
        .or_else(|| names.first())
        .ok_or_else(|| "model returned no outputs".to_owned())?;
    let value = outputs
        .get(name.as_str())
        .ok_or_else(|| format!("missing output {name}"))?;
    let (shape, data) = value
        .try_extract_tensor::<f32>()
        .map_err(|error| format!("{name}: {error}"))?;
    Ok((shape.to_vec(), data.to_vec()))
}

fn model_dir(bundled: Option<&Path>) -> Result<PathBuf, String> {
    if let Some(dir) = bundled {
        let candidate = dir.join("opus");
        if candidate.join("encoder.onnx").is_file() {
            return Ok(candidate);
        }
    }
    let base = std::env::var_os("LOCALAPPDATA")
        .map(PathBuf::from)
        .unwrap_or_else(std::env::temp_dir);
    let dir = base.join("DynoTranslate").join("opus-en-ru");
    fs::create_dir_all(&dir).map_err(|error| error.to_string())?;
    Ok(dir)
}

fn ensure(
    dir: &Path,
    name: &str,
    url: &str,
    minimum: u64,
    log: &mut dyn Write,
    report: &mut dyn FnMut(&str),
) -> Result<PathBuf, String> {
    let path = dir.join(name);
    if file_ok(&path, minimum) {
        return Ok(path);
    }
    let _ = writeln!(log, "downloading {name}");
    report(name);
    let partial = dir.join(format!("{name}.partial"));
    if let Err(error) = download(url, &partial, &mut |bytes| {
        let mb = bytes / (1024 * 1024);
        if mb > 0 {
            report(&format!("{name}:{mb}"));
        }
    }) {
        let _ = fs::remove_file(&partial);
        return Err(format!("download {name}: {error}"));
    }
    if !file_ok(&partial, minimum) {
        let _ = fs::remove_file(&partial);
        return Err(format!("{name} download was too small to be the model"));
    }
    fs::rename(&partial, &path).map_err(|error| error.to_string())?;
    Ok(path)
}

fn download(url: &str, path: &Path, progress: &mut dyn FnMut(u64)) -> Result<(), String> {
    let agent = ureq::AgentBuilder::new()
        .timeout_connect(Duration::from_secs(20))
        .timeout_read(Duration::from_secs(180))
        .redirects(10)
        .build();
    let response = agent
        .get(url)
        .set("User-Agent", "DynoTranslate")
        .call()
        .map_err(|error| error.to_string())?;
    let mut reader = response.into_reader();
    let mut file = File::create(path).map_err(|error| error.to_string())?;
    let mut buffer = [0u8; 64 * 1024];
    let mut total = 0u64;
    let mut reported = 0u64;
    loop {
        let read = reader.read(&mut buffer).map_err(|error| error.to_string())?;
        if read == 0 {
            break;
        }
        file.write_all(&buffer[..read]).map_err(|error| error.to_string())?;
        total += read as u64;
        if total - reported >= 1024 * 1024 {
            reported = total;
            progress(total);
        }
    }
    file.flush().map_err(|error| error.to_string())
}

fn file_ok(path: &Path, minimum: u64) -> bool {
    fs::metadata(path).map(|meta| meta.len() >= minimum).unwrap_or(false)
}

/// The published OPUS dictionary marks its normaliser as present but leaves the table empty.
/// The tokenizer library panics on that instead of skipping it. English text does not need the
/// table, so the empty marker is removed before the dictionary is opened.
fn load_tokenizer(path: &Path) -> Result<Tokenizer, String> {
    let prepared = prepare_tokenizer(path)?;
    let loaded = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        Tokenizer::from_file(&prepared)
    }));
    match loaded {
        Ok(Ok(tokenizer)) => Ok(tokenizer),
        Ok(Err(error)) => Err(format!("tokenizer: {error}")),
        Err(error) => Err(format!("tokenizer: {}", panic_payload(error.as_ref()))),
    }
}

fn prepare_tokenizer(path: &Path) -> Result<PathBuf, String> {
    let text = fs::read_to_string(path).map_err(|error| error.to_string())?;
    let mut value: serde_json::Value =
        serde_json::from_str(&text).map_err(|error| error.to_string())?;
    let Some(normalizer) = value.get_mut("normalizer") else {
        return Ok(path.to_owned());
    };
    if !clear_empty_normalizer(normalizer) {
        return Ok(path.to_owned());
    }
    let patched = path.with_extension("patched.json");
    let rendered = serde_json::to_vec(&value).map_err(|error| error.to_string())?;
    fs::write(&patched, rendered).map_err(|error| error.to_string())?;
    Ok(patched)
}

fn clear_empty_normalizer(value: &mut serde_json::Value) -> bool {
    if is_empty_precompiled(value) {
        *value = serde_json::Value::Null;
        return true;
    }
    if value.get("type").and_then(|item| item.as_str()) != Some("Sequence") {
        return false;
    }
    let Some(items) = value.get_mut("normalizers").and_then(|item| item.as_array_mut()) else {
        return false;
    };
    let before = items.len();
    items.retain(|item| !is_empty_precompiled(item));
    items.len() != before
}

fn is_empty_precompiled(value: &serde_json::Value) -> bool {
    if value.get("type").and_then(|item| item.as_str()) != Some("Precompiled") {
        return false;
    }
    match value.get("precompiled_charsmap") {
        Some(serde_json::Value::String(text)) if !text.is_empty() => false,
        _ => true,
    }
}

fn panic_payload(error: &(dyn std::any::Any + Send)) -> String {
    if let Some(text) = error.downcast_ref::<&str>() {
        return (*text).to_owned();
    }
    if let Some(text) = error.downcast_ref::<String>() {
        return text.clone();
    }
    "словарь перевода не открылся".to_owned()
}
