use once_cell::sync::OnceCell;
use std::io::Write;
use std::process::{Command, Stdio};

use crate::{Budget, Compressed, CompressionMode, Compressor, Domain};
use anyhow::{anyhow, Context, Result};
use serde::{Deserialize, Serialize};
use serde_json::{json, Map, Value};

/// Error returned when LLMLingua cannot be initialised.
#[derive(thiserror::Error, Debug, Clone)]
pub enum LlmlinguaDetectError {
    #[error("no python interpreter found for llmlingua")]
    NoInterpreter,
    #[error("llmlingua module unavailable via {interpreter}: {error}")]
    ModuleUnavailable { interpreter: String, error: String },
    #[error("llmlingua auto-install via {interpreter} failed: {error}")]
    InstallFailed { interpreter: String, error: String },
    #[error("failed to spawn python probe: {0}")]
    Spawn(String),
    #[error("python probe terminated abnormally: {0}")]
    ProbeFailed(String),
}

#[derive(Debug, Clone)]
struct PythonInterpreter {
    program: String,
}

impl PythonInterpreter {
    fn new(program: String) -> Self {
        Self { program }
    }

    fn spawn(&self) -> Command {
        Command::new(&self.program)
    }

    fn probe_module(&self, module: &str) -> Result<(), LlmlinguaDetectError> {
        let mut cmd = self.spawn();
        cmd.arg("-c")
            .arg(format!("import {}", module))
            .stdout(Stdio::null())
            .stderr(Stdio::piped());
        let output = cmd.output();
        match output {
            Ok(output) => {
                if output.status.success() {
                    Ok(())
                } else {
                    let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
                    Err(LlmlinguaDetectError::ModuleUnavailable {
                        interpreter: self.program.clone(),
                        error: if stderr.is_empty() {
                            "unknown import error".to_string()
                        } else {
                            stderr
                        },
                    })
                }
            }
            Err(err) => {
                if err.kind() == std::io::ErrorKind::NotFound {
                    Err(LlmlinguaDetectError::NoInterpreter)
                } else {
                    Err(LlmlinguaDetectError::Spawn(err.to_string()))
                }
            }
        }
    }
}

/// LLMLingua-backed prompt compressor.
#[derive(Clone)]
pub struct LlmlinguaCompressor {
    interpreter: PythonInterpreter,
}

impl LlmlinguaCompressor {
    /// Try to locate a usable python + llmlingua environment.
    pub fn detect() -> Result<Self, LlmlinguaDetectError> {
        static DETECT: OnceCell<Result<LlmlinguaCompressor, LlmlinguaDetectError>> =
            OnceCell::new();
        let cached = DETECT.get_or_init(Self::detect_inner);
        cached.clone()
    }

    fn detect_inner() -> Result<Self, LlmlinguaDetectError> {
        let env = std::env::var("LLMLINGUA_PYTHON").ok();
        let mut candidates = Vec::new();
        if let Some(bin) = env {
            candidates.push(bin);
        }
        candidates.extend(["python3", "python"].iter().map(|s| s.to_string()));

        let mut last_error: Option<LlmlinguaDetectError> = None;
        for candidate in candidates {
            let interpreter = PythonInterpreter::new(candidate.clone());
            match interpreter.probe_module("llmlingua") {
                Ok(()) => return Ok(Self { interpreter }),
                Err(LlmlinguaDetectError::NoInterpreter) => continue,
                Err(err) => {
                    if matches!(err, LlmlinguaDetectError::ModuleUnavailable { .. })
                        && auto_install_enabled()
                    {
                        if let Err(inst_err) = ensure_llmlingua_installed(&interpreter) {
                            last_error = Some(inst_err);
                            continue;
                        }
                        if interpreter.probe_module("llmlingua").is_ok() {
                            return Ok(Self { interpreter });
                        }
                    }
                    last_error = Some(err);
                }
            }
        }
        Err(last_error.unwrap_or(LlmlinguaDetectError::NoInterpreter))
    }

    /// Build directly from a specific python interpreter.
    pub fn with_python(program: impl Into<String>) -> Result<Self, LlmlinguaDetectError> {
        let interpreter = PythonInterpreter::new(program.into());
        interpreter.probe_module("llmlingua")?;
        Ok(Self { interpreter })
    }
}

const DRIVER_SCRIPT: &str = include_str!("llmlingua_driver.py");

fn auto_install_enabled() -> bool {
    if cfg!(test) {
        return false;
    }
    std::env::var("ARW_LLMLINGUA_AUTO_INSTALL")
        .ok()
        .map(|v| v.trim().to_ascii_lowercase())
        .map(|v| matches!(v.as_str(), "" | "1" | "true" | "yes" | "on"))
        .unwrap_or(true)
}

fn ensure_llmlingua_installed(interpreter: &PythonInterpreter) -> Result<(), LlmlinguaDetectError> {
    // If the interpreter itself cannot import pip, bail early to avoid noisy failures.
    let mut probe_pip = interpreter.spawn();
    probe_pip
        .args(["-m", "pip", "--version"])
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    if let Ok(output) = probe_pip.output() {
        if !output.status.success() {
            return Err(LlmlinguaDetectError::InstallFailed {
                interpreter: interpreter.program.clone(),
                error: "pip unavailable on interpreter".to_string(),
            });
        }
    }

    let mut cmd = interpreter.spawn();
    cmd.args(["-m", "pip", "install", "--quiet", "llmlingua>=0.2,<1.0"])
        .stdout(Stdio::null())
        .stderr(Stdio::piped());
    match cmd.output() {
        Ok(output) => {
            if output.status.success() {
                Ok(())
            } else {
                let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
                Err(LlmlinguaDetectError::InstallFailed {
                    interpreter: interpreter.program.clone(),
                    error: if stderr.is_empty() {
                        format!("pip exited with {}", output.status)
                    } else {
                        stderr
                    },
                })
            }
        }
        Err(err) => Err(LlmlinguaDetectError::InstallFailed {
            interpreter: interpreter.program.clone(),
            error: err.to_string(),
        }),
    }
}

#[derive(Debug, Serialize)]
struct BridgePayload<'a> {
    text: &'a str,
    budget: &'a Budget,
    mode: Option<CompressionMode>,
    #[serde(default)]
    extras: Value,
}

#[derive(Debug, Deserialize)]
struct BridgeResponse {
    ok: bool,
    #[serde(default)]
    compressed_text: Option<String>,
    #[serde(default)]
    meta: Option<Value>,
    #[serde(default)]
    error: Option<String>,
    #[serde(default)]
    detail: Option<String>,
}

impl Compressor for LlmlinguaCompressor {
    fn id(&self) -> &'static str {
        "llmlingua"
    }

    fn domain(&self) -> Domain {
        Domain::Prompt
    }

    fn compress(&self, input: &str, budget: Budget) -> Result<Compressed> {
        budget.validate()?;
        let extras = budget.extras.clone().unwrap_or_else(|| json!({}));
        let payload = BridgePayload {
            text: input,
            budget: &budget,
            mode: budget.mode,
            extras,
        };
        let mut cmd = self.interpreter.spawn();
        cmd.arg("-c")
            .arg(DRIVER_SCRIPT)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        let mut child = cmd
            .spawn()
            .with_context(|| "failed to spawn llmlingua bridge subprocess")?;
        {
            let mut stdin = child
                .stdin
                .take()
                .ok_or_else(|| anyhow!("failed to access llmlingua stdin"))?;
            serde_json::to_writer(&mut stdin, &payload)
                .with_context(|| "failed to serialise llmlingua payload")?;
            stdin
                .write_all(b"\n")
                .with_context(|| "failed to flush llmlingua payload")?;
        }
        let output = child
            .wait_with_output()
            .with_context(|| "llmlingua subprocess failed")?;
        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(anyhow!(
                "llmlingua exited with {}: {}",
                output.status,
                stderr
            ));
        }
        let response: BridgeResponse = serde_json::from_slice(&output.stdout)
            .with_context(|| "invalid llmlingua response payload")?;
        if !response.ok {
            let stderr = String::from_utf8_lossy(&output.stderr);
            let mut detail = response.detail.unwrap_or_default();
            if !stderr.trim().is_empty() {
                if !detail.is_empty() {
                    detail.push_str("; ");
                }
                detail.push_str(stderr.trim());
            }
            return Err(anyhow!(
                "llmlingua compression failed: {} {}",
                response.error.unwrap_or_else(|| "unknown".into()),
                detail
            ));
        }
        let compressed_text = response
            .compressed_text
            .unwrap_or_else(|| input.to_string());
        let meta_value = response.meta.unwrap_or_else(|| json!({}));
        let ratio = meta_value
            .get("ratio")
            .and_then(|v| v.as_f64())
            .map(|v| v as f32)
            .or(budget.ratio)
            .unwrap_or_else(|| budget.fallback_ratio(1.0));

        // Ensure meta is an object for downstream augmentation.
        let mut meta: Map<String, Value> = match meta_value {
            Value::Object(map) => map,
            _ => Map::new(),
        };
        meta.entry("compressor")
            .or_insert_with(|| Value::String(self.id().to_string()));
        let ratio_number = serde_json::Number::from_f64(ratio as f64)
            .unwrap_or_else(|| serde_json::Number::from_f64(0.0).unwrap());
        meta.insert("applied_ratio".into(), Value::Number(ratio_number));
        if let Some(mode) = budget.mode {
            meta.entry("mode")
                .or_insert_with(|| Value::String(mode.as_str().to_string()));
        }

        // Stitch stderr into metadata for diagnostics if present.
        if !output.stderr.is_empty() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            if !stderr.trim().is_empty() {
                meta.insert("stderr".into(), Value::String(stderr.to_string()));
            }
        }

        Ok(Compressed::from_text(
            Domain::Prompt,
            compressed_text,
            Value::Object(meta),
        ))
    }
}

impl std::fmt::Debug for LlmlinguaCompressor {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("LlmlinguaCompressor")
            .field("interpreter", &self.interpreter.program)
            .finish()
    }
}
