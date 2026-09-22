//! English to Russian, offline, with the public OPUS-MT model.
//!
//! The weights are not in the repository. The first launch downloads the quantised encoder,
//! decoder and tokenizer into the user's local data folder, then keeps them. Nothing is sent
//! except that one download.

use std::fs::{self, File};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::time::Duration;

use ndarray::{Array2, Array3};
use ort::session::Session;
use tokenizers::Tokenizer;

const ENCODER_URL: &str =
    "https://huggingface.co/Xenova/opus-mt-en-ru/resolve/main/onnx/encoder_model_quantized.onnx";
const DECODER_URL: &str =
    "https://huggingface.co/Xenova/opus-mt-en-ru/resolve/main/onnx/decoder_model_quantized.onnx";
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
    pub fn load(bundled: Option<&Path>, log: &mut dyn Write) -> Result<Self, String> {
        let dir = model_dir(bundled)?;
        let encoder_path = ensure(&dir, "encoder.onnx", ENCODER_URL, 8_000_000, log)?;
        let decoder_path = ensure(&dir, "decoder.onnx", DECODER_URL, 8_000_000, log)?;
        let tokenizer_path = ensure(&dir, "tokenizer.json", TOKENIZER_URL, 50_000, log)?;

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
        let tokenizer = Tokenizer::from_file(&tokenizer_path).map_err(|error| error.to_string())?;
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
        let length = ids.len();
        let input_ids = Array2::from_shape_vec((1, length), ids).map_err(|error| error.to_string())?;
        let mask = Array2::from_elem((1, length), 1i64);

        let encoded = self
            .encoder
            .run(ort::inputs![
                "input_ids" => input_ids,
                "attention_mask" => mask.clone(),
            ])
            .map_err(|error| format!("encoder run: {error}"))?;
        let hidden = encoded["last_hidden_state"]
            .try_extract_array::<f32>()
            .map_err(|error| format!("encoder output: {error}"))?;
        let hidden_shape = hidden.shape().to_vec();
        if hidden_shape.len() != 3 {
            return Err(format!("encoder rank {}", hidden_shape.len()));
        }
        let hidden = Array3::from_shape_vec(
            (hidden_shape[0], hidden_shape[1], hidden_shape[2]),
            hidden.iter().copied().collect(),
        )
        .map_err(|error| error.to_string())?;

        let mut generated = vec![DECODER_START];
        for _ in 0..MAX_NEW_TOKENS {
            let step = Array2::from_shape_vec((1, generated.len()), generated.clone())
                .map_err(|error| error.to_string())?;
            let decoded = self
                .decoder
                .run(ort::inputs![
                    "input_ids" => step,
                    "encoder_hidden_states" => hidden.clone(),
                    "encoder_attention_mask" => mask.clone(),
                ])
                .map_err(|error| format!("decoder run: {error}"))?;
            let logits = decoded["logits"]
                .try_extract_array::<f32>()
                .map_err(|error| format!("decoder output: {error}"))?;
            let shape = logits.shape();
            if shape.len() != 3 || shape[2] == 0 {
                return Err("decoder logits have an unexpected shape".to_owned());
            }
            let vocab = shape[2];
            let last = (shape[1] - 1) * vocab;
            let row = logits.iter().skip(last).take(vocab);
            let mut best = 0usize;
            let mut best_score = f32::NEG_INFINITY;
            for (index, score) in row.enumerate() {
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

trait JoinComma {
    fn join_comma(&self) -> String;
}

impl JoinComma for std::iter::Map<std::slice::Iter<'_, ort::session::Input>, fn(&ort::session::Input) -> &str> {
    fn join_comma(&self) -> String {
        String::new()
    }
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

fn ensure(dir: &Path, name: &str, url: &str, minimum: u64, log: &mut dyn Write) -> Result<PathBuf, String> {
    let path = dir.join(name);
    if file_ok(&path, minimum) {
        return Ok(path);
    }
    let _ = writeln!(log, "downloading {name}");
    let partial = dir.join(format!("{name}.partial"));
    let agent = ureq::AgentBuilder::new()
        .timeout_connect(Duration::from_secs(20))
        .timeout_read(Duration::from_secs(180))
        .build();
    let response = agent
        .get(url)
        .set("User-Agent", "DynoTranslate")
        .call()
        .map_err(|error| format!("download {name}: {error}"))?;
    let mut reader = response.into_reader();
    let mut file = File::create(&partial).map_err(|error| error.to_string())?;
    let mut buffer = [0u8; 64 * 1024];
    loop {
        let read = reader.read(&mut buffer).map_err(|error| error.to_string())?;
        if read == 0 {
            break;
        }
        file.write_all(&buffer[..read]).map_err(|error| error.to_string())?;
    }
    file.flush().map_err(|error| error.to_string())?;
    drop(file);
    if !file_ok(&partial, minimum) {
        let _ = fs::remove_file(&partial);
        return Err(format!("{name} download was too small to be the model"));
    }
    fs::rename(&partial, &path).map_err(|error| error.to_string())?;
    Ok(path)
}

fn file_ok(path: &Path, minimum: u64) -> bool {
    fs::metadata(path).map(|meta| meta.len() >= minimum).unwrap_or(false)
}
