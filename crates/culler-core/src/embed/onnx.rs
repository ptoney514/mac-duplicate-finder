//! The real CLIP embedder: ONNX Runtime sessions for the vision and text
//! encoders (with projection heads, 512-dim outputs) plus the model's own
//! tokenizer. Registers the CoreML execution provider with silent CPU
//! fallback (PRD §14 open question; revisit with measurements).

use std::path::Path;
use std::sync::Mutex;

use ort::session::Session;
use ort::value::Tensor;

use crate::embed::{normalize, preprocess, Embedder, EMBED_DIM};
use crate::{CoreError, Result};

fn model_err(e: impl std::fmt::Display) -> CoreError {
    CoreError::Model {
        message: e.to_string(),
    }
}

/// CLIP's maximum text context length.
const MAX_TOKENS: usize = 77;

pub struct OnnxEmbedder {
    // ort's Session::run takes &mut self; the Embedder trait is &self.
    vision: Mutex<Session>,
    text: Mutex<Session>,
    tokenizer: tokenizers::Tokenizer,
    vision_input: String,
    vision_output: String,
    text_output: String,
    text_wants_mask: bool,
}

/// Prefer the projected embedding output ("image_embeds"/"text_embeds");
/// fall back to the first output for other CLIP exports.
fn embeds_output(session: &Session) -> Result<String> {
    let names: Vec<&str> = session.outputs().iter().map(|o| o.name()).collect();
    names
        .iter()
        .find(|n| n.contains("embeds"))
        .or(names.first())
        .map(|n| (*n).to_string())
        .ok_or_else(|| model_err("model has no outputs"))
}

impl OnnxEmbedder {
    /// Loads `vision_model.onnx`, `text_model.onnx`, and `tokenizer.json`
    /// from `models_dir`.
    pub fn load(models_dir: &Path) -> Result<Self> {
        let session = |file: &str| -> Result<Session> {
            let err = |e: &dyn std::fmt::Display| model_err(format!("{file}: {e}"));
            let builder = Session::builder().map_err(|e| err(&e))?;
            let mut builder = builder
                .with_execution_providers([ort::ep::CoreML::default().build()])
                .map_err(|e| err(&e))?;
            builder
                .commit_from_file(models_dir.join(file))
                .map_err(|e| err(&e))
        };
        let vision = session("vision_model.onnx")?;
        let text = session("text_model.onnx")?;
        let tokenizer = tokenizers::Tokenizer::from_file(models_dir.join("tokenizer.json"))
            .map_err(|e| model_err(format!("tokenizer.json: {e}")))?;

        let vision_input = vision
            .inputs()
            .first()
            .map(|i| i.name().to_string())
            .ok_or_else(|| model_err("vision model has no inputs"))?;
        let vision_output = embeds_output(&vision)?;
        let text_output = embeds_output(&text)?;
        let text_wants_mask = text.inputs().iter().any(|i| i.name() == "attention_mask");

        Ok(Self {
            vision: Mutex::new(vision),
            text: Mutex::new(text),
            tokenizer,
            vision_input,
            vision_output,
            text_output,
            text_wants_mask,
        })
    }
}

fn extract_vector(outputs: &ort::session::SessionOutputs<'_>, name: &str) -> Result<Vec<f32>> {
    let (_shape, data) = outputs[name]
        .try_extract_tensor::<f32>()
        .map_err(model_err)?;
    if data.len() < EMBED_DIM {
        return Err(model_err(format!(
            "output {name} has {} values, expected {EMBED_DIM}",
            data.len()
        )));
    }
    let mut v = data[..EMBED_DIM].to_vec();
    normalize(&mut v);
    Ok(v)
}

impl Embedder for OnnxEmbedder {
    fn embed_image(&self, img: &image::DynamicImage) -> Result<Vec<f32>> {
        let pixels = preprocess::clip_pixels(img);
        let side = preprocess::CLIP_SIDE as usize;
        let tensor = Tensor::from_array(([1usize, 3, side, side], pixels)).map_err(model_err)?;
        let mut session = self.vision.lock().unwrap_or_else(|p| p.into_inner());
        let outputs = session
            .run(ort::inputs![self.vision_input.as_str() => tensor])
            .map_err(model_err)?;
        extract_vector(&outputs, &self.vision_output)
    }

    fn embed_text(&self, text: &str) -> Result<Vec<f32>> {
        let encoding = self
            .tokenizer
            .encode(text, true)
            .map_err(|e| model_err(format!("tokenize: {e}")))?;
        let mut ids: Vec<i64> = encoding.get_ids().iter().map(|&id| i64::from(id)).collect();
        if ids.len() > MAX_TOKENS {
            // Keep the end-of-text token when truncating.
            let eot = *ids.last().expect("non-empty encoding");
            ids.truncate(MAX_TOKENS - 1);
            ids.push(eot);
        }
        let len = ids.len();
        let ids_tensor = Tensor::from_array(([1usize, len], ids)).map_err(model_err)?;

        let mut session = self.text.lock().unwrap_or_else(|p| p.into_inner());
        let outputs = if self.text_wants_mask {
            let mask = Tensor::from_array(([1usize, len], vec![1i64; len])).map_err(model_err)?;
            session
                .run(ort::inputs!["input_ids" => ids_tensor, "attention_mask" => mask])
                .map_err(model_err)?
        } else {
            session
                .run(ort::inputs!["input_ids" => ids_tensor])
                .map_err(model_err)?
        };
        extract_vector(&outputs, &self.text_output)
    }
}
