//! Local OCR runtime adapter for liteparse.
//!
//! The first supported engine is PaddleOCR because it returns text boxes and
//! polygons that map directly to liteparse's OCR merge contract.

use std::future::Future;
use std::path::{Path, PathBuf};
use std::pin::Pin;
use std::process::{Command, Stdio};

use liteparse::ocr::{OcrEngine, OcrOptions, OcrResult};
use serde::{Deserialize, Serialize};

use crate::error::{WikiError, WikiResult};
use crate::types::OcrConfig;

const WORKER_PY: &str = include_str!("../resources/ocr_worker.py");

#[derive(Debug, Clone)]
pub struct LocalOcrConfig {
    pub engine: String,
    pub model: String,
    pub model_root: String,
    pub device: String,
    pub auto_download: bool,
}

impl From<OcrConfig> for LocalOcrConfig {
    fn from(value: OcrConfig) -> Self {
        Self {
            engine: if value.engine.is_empty() {
                value.backend
            } else {
                value.engine
            },
            model: value.model,
            model_root: value.model_root,
            device: value.device,
            auto_download: value.auto_download,
        }
    }
}

pub struct LocalOcrEngine {
    config: LocalOcrConfig,
}

impl LocalOcrEngine {
    pub fn new(config: LocalOcrConfig) -> Self {
        Self { config }
    }
}

impl OcrEngine for LocalOcrEngine {
    fn name(&self) -> &str {
        "local-ocr"
    }

    fn recognize<'a, 'b: 'a, 'c: 'a>(
        &'a self,
        image_data: &'c [u8],
        width: u32,
        height: u32,
        options: &'b OcrOptions,
    ) -> Pin<
        Box<
            dyn Future<Output = Result<Vec<OcrResult>, Box<dyn std::error::Error + Send + Sync>>>
                + Send
                + '_,
        >,
    > {
        let cfg = self.config.clone();
        let language = options.language.clone();
        let image = image_data.to_vec();
        Box::pin(async move {
            run_local_ocr(&cfg, &image, width, height, &language)
                .map_err(|e| -> Box<dyn std::error::Error + Send + Sync> { Box::new(e) })
        })
    }
}

#[derive(Serialize)]
struct WorkerRequest<'a> {
    engine: &'a str,
    model: &'a str,
    device: &'a str,
    language: &'a str,
    width: u32,
    height: u32,
    rgb_base64: String,
    model_dir: String,
}

#[derive(Deserialize)]
struct WorkerResponse {
    results: Vec<WorkerItem>,
}

#[derive(Deserialize)]
struct WorkerItem {
    text: String,
    bbox: [f32; 4],
    #[serde(default = "default_confidence")]
    confidence: f32,
    #[serde(default)]
    polygon: Option<[[f32; 2]; 4]>,
}

fn default_confidence() -> f32 {
    1.0
}

fn run_local_ocr(
    config: &LocalOcrConfig,
    image_data: &[u8],
    width: u32,
    height: u32,
    language: &str,
) -> WikiResult<Vec<OcrResult>> {
    match config.engine.as_str() {
        "paddleocr-vl" | "paddleocr_vl" => {
            run_paddleocr_vl(config, image_data, width, height, language)
        }
        "paddleocr" | "paddle" if config.model.contains("PaddleOCR-VL") => {
            run_paddleocr_vl(config, image_data, width, height, language)
        }
        "paddleocr" | "paddle" => run_paddleocr(config, image_data, width, height, language),
        "mineru" => run_model_worker("mineru", config, image_data, width, height, language),
        "deepseek-ocr" | "deepseekocr" => {
            run_model_worker("deepseek-ocr", config, image_data, width, height, language)
        }
        other => Err(WikiError::Ocr(format!(
            "Unsupported local OCR engine: {other}"
        ))),
    }
}

fn run_paddleocr_vl(
    config: &LocalOcrConfig,
    image_data: &[u8],
    width: u32,
    height: u32,
    language: &str,
) -> WikiResult<Vec<OcrResult>> {
    let runtime = ensure_worker_only()?;
    let worker = runtime.join("ocr_worker.py");
    let model_path = resolve_model_path(config)?;
    let model_path_str = model_path.to_string_lossy().to_string();

    let request = WorkerRequest {
        engine: "paddleocr-vl",
        model: &model_path_str,
        device: &config.device,
        language,
        width,
        height,
        rgb_base64: base64::Engine::encode(&base64::engine::general_purpose::STANDARD, image_data),
        model_dir: model_path_str.clone(),
    };

    let mut child = Command::new(find_python()?)
        .arg(worker)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| WikiError::Ocr(format!("Failed to start PaddleOCR-VL worker: {e}")))?;

    if let Some(stdin) = child.stdin.as_mut() {
        serde_json::to_writer(stdin, &request)?;
    }

    let output = child.wait_with_output()?;
    if !output.status.success() {
        return Err(WikiError::Ocr(format!(
            "PaddleOCR-VL worker failed: {}",
            String::from_utf8_lossy(&output.stderr)
        )));
    }

    let response: WorkerResponse = serde_json::from_slice(&output.stdout)?;
    Ok(response
        .results
        .into_iter()
        .filter(|item| !item.text.trim().is_empty())
        .map(|item| OcrResult {
            text: item.text,
            bbox: item.bbox,
            confidence: item.confidence,
            polygon: item.polygon,
        })
        .collect())
}

fn run_model_worker(
    engine: &str,
    config: &LocalOcrConfig,
    image_data: &[u8],
    width: u32,
    height: u32,
    language: &str,
) -> WikiResult<Vec<OcrResult>> {
    let runtime = ensure_worker_only()?;
    let worker = runtime.join("ocr_worker.py");
    let model_path = resolve_model_path(config)?;
    let model_path_str = model_path.to_string_lossy().to_string();

    let request = WorkerRequest {
        engine,
        model: &model_path_str,
        device: &config.device,
        language,
        width,
        height,
        rgb_base64: base64::Engine::encode(&base64::engine::general_purpose::STANDARD, image_data),
        model_dir: model_path_str.clone(),
    };

    let mut child = Command::new(find_python()?)
        .arg(worker)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| WikiError::Ocr(format!("Failed to start {engine} OCR worker: {e}")))?;

    if let Some(stdin) = child.stdin.as_mut() {
        serde_json::to_writer(stdin, &request)?;
    }

    let output = child.wait_with_output()?;
    if !output.status.success() {
        return Err(WikiError::Ocr(format!(
            "{engine} OCR worker failed: {}",
            String::from_utf8_lossy(&output.stderr)
        )));
    }

    let response: WorkerResponse = serde_json::from_slice(&output.stdout)?;
    Ok(response
        .results
        .into_iter()
        .filter(|item| !item.text.trim().is_empty())
        .map(|item| OcrResult {
            text: item.text,
            bbox: item.bbox,
            confidence: item.confidence,
            polygon: item.polygon,
        })
        .collect())
}

fn run_paddleocr(
    config: &LocalOcrConfig,
    image_data: &[u8],
    width: u32,
    height: u32,
    language: &str,
) -> WikiResult<Vec<OcrResult>> {
    if !config.auto_download {
        return Err(WikiError::Ocr(
            "Local PaddleOCR requires ocr.auto_download=true so the runtime and models can be prepared."
                .into(),
        ));
    }

    let runtime = ensure_runtime()?;
    let python = python_bin(&runtime);
    let worker = runtime.join("ocr_worker.py");
    let model_dir = model_root_path(&config.model_root).join(sanitize_name(&config.model));
    std::fs::create_dir_all(&model_dir)?;

    let request = WorkerRequest {
        engine: "paddleocr",
        model: &config.model,
        device: &config.device,
        language,
        width,
        height,
        rgb_base64: base64::Engine::encode(&base64::engine::general_purpose::STANDARD, image_data),
        model_dir: model_dir.to_string_lossy().to_string(),
    };

    let mut child = Command::new(python)
        .arg(worker)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| WikiError::Ocr(format!("Failed to start PaddleOCR worker: {e}")))?;

    if let Some(stdin) = child.stdin.as_mut() {
        serde_json::to_writer(stdin, &request)?;
    }

    let output = child.wait_with_output()?;
    if !output.status.success() {
        return Err(WikiError::Ocr(format!(
            "PaddleOCR worker failed: {}",
            String::from_utf8_lossy(&output.stderr)
        )));
    }

    let response: WorkerResponse = serde_json::from_slice(&output.stdout)?;
    Ok(response
        .results
        .into_iter()
        .filter(|item| !item.text.trim().is_empty())
        .map(|item| OcrResult {
            text: item.text,
            bbox: item.bbox,
            confidence: item.confidence,
            polygon: item.polygon,
        })
        .collect())
}

fn ensure_runtime() -> WikiResult<PathBuf> {
    let runtime = ensure_worker_only()?;
    let python = python_bin(&runtime);
    if !python.exists() {
        let status = Command::new(find_python()?)
            .args(["-m", "venv"])
            .arg(&runtime)
            .status()
            .map_err(|e| WikiError::Ocr(format!("Failed to create OCR Python venv: {e}")))?;
        if !status.success() {
            return Err(WikiError::Ocr(
                "Failed to create OCR Python venv. Set LLM_WIKI_PYTHON to a Python executable with venv support.".into(),
            ));
        }
    }

    let stamp = runtime.join(".paddleocr-installed");
    if !stamp.exists() {
        let status = Command::new(&python)
            .args([
                "-m",
                "pip",
                "install",
                "--upgrade",
                "pip",
                "paddleocr>=3.0.0",
                "pillow",
                "numpy",
            ])
            .status()
            .map_err(|e| WikiError::Ocr(format!("Failed to install PaddleOCR runtime: {e}")))?;
        if !status.success() {
            return Err(WikiError::Ocr(
                "Failed to install PaddleOCR runtime packages".into(),
            ));
        }
        std::fs::write(stamp, "ok\n")?;
    }

    Ok(runtime)
}

fn ensure_worker_only() -> WikiResult<PathBuf> {
    let runtime = crate::config::get_wiki_dir().join("runtime").join("ocr");
    std::fs::create_dir_all(&runtime)?;
    let worker = runtime.join("ocr_worker.py");
    if !worker.exists() || std::fs::read_to_string(&worker).ok().as_deref() != Some(WORKER_PY) {
        std::fs::write(&worker, WORKER_PY)?;
    }
    Ok(runtime)
}

fn resolve_model_path(config: &LocalOcrConfig) -> WikiResult<PathBuf> {
    let model = config.model.trim();
    let candidate = PathBuf::from(model);
    if candidate.exists() {
        return Ok(candidate);
    }

    let root = model_root_path(&config.model_root);
    let root_candidate = root.join(model);
    if root_candidate.exists() {
        return Ok(root_candidate);
    }

    if config.auto_download {
        download_model(config, &root)?;
        if root_candidate.exists() {
            return Ok(root_candidate);
        }
    }

    Err(WikiError::Ocr(format!(
        "Local OCR model not found: {model}. Set ocr.model_root to a directory containing this model or enable ocr.auto_download."
    )))
}

fn model_root_path(configured: &str) -> PathBuf {
    if !configured.trim().is_empty() {
        return PathBuf::from(expand_home(configured.trim()));
    }
    if let Ok(path) = std::env::var("LLM_WIKI_OCR_MODEL_ROOT") {
        return PathBuf::from(path);
    }
    crate::config::get_wiki_dir().join("models").join("ocr")
}

fn expand_home(path: &str) -> String {
    if path == "~" {
        return std::env::var("HOME").unwrap_or_else(|_| path.to_string());
    }
    if let Some(rest) = path.strip_prefix("~/") {
        if let Ok(home) = std::env::var("HOME") {
            return PathBuf::from(home).join(rest).to_string_lossy().to_string();
        }
    }
    path.to_string()
}

fn download_model(config: &LocalOcrConfig, root: &Path) -> WikiResult<()> {
    std::fs::create_dir_all(root)?;
    let model = config.model.trim();
    let engine = config.engine.as_str();
    let model_id = match (engine, model) {
        ("paddleocr-vl" | "paddleocr_vl", "PaddleOCR-VL-1.5-8bit") => {
            if !cfg!(target_os = "macos") {
                return Err(WikiError::Ocr(
                    "PaddleOCR-VL MLX weights are only supported on macOS. Use ocr.engine=paddleocr, mineru, or deepseek-ocr on this platform.".into(),
                ));
            }
            "mlx-community/PaddleOCR-VL-1.5-8bit"
        }
        ("mineru", "MinerU2.5" | "mineru" | "default") => "OpenDataLab/MinerU2.5-2509-1.2B",
        ("deepseek-ocr" | "deepseekocr", "DeepSeek-OCR-2" | "deepseek-ocr-v2" | "default") => {
            "deepseek-ai/DeepSeek-OCR-2"
        }
        (_, other) if other.contains('/') => other,
        (_, other) => {
            return Err(WikiError::Ocr(format!(
                "No downloader is configured for OCR model '{other}'. Use a full ModelScope model id or place it under {}.",
                root.display()
            )))
        }
    };
    let target = root.join(model);
    if let Some(parent) = target.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let code = format!(
        r#"
from modelscope.hub.snapshot_download import snapshot_download
snapshot_download(
    model_id={model_id:?},
    cache_dir={cache_dir:?},
    local_dir={target:?},
    revision="master",
)
"#,
        model_id = model_id,
        cache_dir = root.join(".cache").to_string_lossy(),
        target = target.to_string_lossy(),
    );
    let status = Command::new(find_python()?)
        .arg("-c")
        .arg(code)
        .status()
        .map_err(|e| WikiError::Ocr(format!("Failed to start ModelScope downloader: {e}")))?;
    if !status.success() {
        return Err(WikiError::Ocr(format!(
            "Failed to download OCR model '{model_id}' to {}",
            target.display()
        )));
    }
    Ok(())
}

fn system_python() -> String {
    if let Ok(path) = std::env::var("LLM_WIKI_PYTHON") {
        if !path.trim().is_empty() {
            return path;
        }
    }
    if cfg!(windows) {
        "python".into()
    } else {
        "python3".into()
    }
}

/// Validate that the configured Python interpreter exists and is executable.
/// Returns the path on success, or a clear error explaining how to override.
fn find_python() -> WikiResult<String> {
    let raw = system_python();
    let path = Path::new(&raw);
    if path.exists() {
        return Ok(raw);
    }
    // Try common fallbacks before giving up
    for fallback in &["python3", "python"] {
        if Path::new(fallback).exists() {
            return Ok(fallback.to_string());
        }
    }
    Err(WikiError::Ocr(format!(
        "Python interpreter not found at '{raw}'. \
         Install Python 3 or set LLM_WIKI_PYTHON=/path/to/python in your environment."
    )))
}

fn python_bin(runtime: &Path) -> PathBuf {
    if cfg!(windows) {
        runtime.join("Scripts").join("python.exe")
    } else {
        runtime.join("bin").join("python")
    }
}

fn sanitize_name(name: &str) -> String {
    let sanitized: String = name
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '-' || c == '_' {
                c
            } else {
                '-'
            }
        })
        .collect();
    if sanitized.is_empty() {
        "default".into()
    } else {
        sanitized
    }
}
