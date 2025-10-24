use std::sync::Arc;

use anyhow::{Context, Result};
use arw_compress::{
    Budget, Compressor, Domain, KvMethod, KvPolicy, LlmlinguaCompressor, LlmlinguaDetectError,
    NoopCompressor,
};
use tokio::sync::RwLock;
use tokio::task;
use tracing::{info, warn};

#[derive(Clone)]
pub struct PromptCompression {
    primary: Arc<dyn Compressor>,
    fallback: Arc<dyn Compressor>,
}

impl PromptCompression {
    pub fn new(primary: Arc<dyn Compressor>, fallback: Arc<dyn Compressor>) -> Self {
        Self { primary, fallback }
    }

    pub async fn compress(
        &self,
        input: String,
        budget: Budget,
    ) -> Result<arw_compress::Compressed> {
        let primary = Arc::clone(&self.primary);
        let fallback = Arc::clone(&self.fallback);
        task::spawn_blocking(move || match primary.compress(&input, budget.clone()) {
            Ok(blob) => Ok(blob),
            Err(err) => {
                warn!(
                    target = "arw::compression",
                    error = %err,
                    "primary prompt compressor failed; falling back to noop"
                );
                fallback
                    .compress(&input, budget)
                    .context("prompt compression fallback failed")
            }
        })
        .await
        .context("prompt compression task join error")?
    }

    #[allow(dead_code)]
    pub async fn decompress(&self, blob: arw_compress::Compressed) -> Result<String> {
        let compressor = match blob.meta.get("compressor").and_then(|value| value.as_str()) {
            Some(id) if id == self.primary.id() => Arc::clone(&self.primary),
            Some(id) if id == self.fallback.id() => Arc::clone(&self.fallback),
            _ => Arc::clone(&self.primary),
        };
        task::spawn_blocking(move || compressor.decompress(&blob))
            .await
            .context("prompt decompression task join error")?
    }
}

#[derive(Clone)]
pub struct CompressionService {
    prompt: Arc<PromptCompression>,
    #[allow(dead_code)]
    memory: Arc<dyn Compressor>,
    #[allow(dead_code)]
    kv: Arc<dyn Compressor>,
    kv_policy: Arc<RwLock<KvPolicy>>,
}

impl CompressionService {
    pub fn initialise() -> Self {
        let noop_prompt: Arc<dyn Compressor> = Arc::new(NoopCompressor::new(Domain::Prompt));
        let prompt_primary: Arc<dyn Compressor> = match LlmlinguaCompressor::detect() {
            Ok(detector) => {
                info!(
                    target = "arw::compression",
                    interpreter = ?detector,
                    "llmlingua available; enabling prompt compression"
                );
                Arc::new(detector)
            }
            Err(err) => {
                match err {
                    LlmlinguaDetectError::NoInterpreter => {
                        info!(
                            target = "arw::compression",
                            "llmlingua python interpreter not found; using noop prompt compressor"
                        );
                    }
                    _ => warn!(
                        target = "arw::compression",
                        error = %err,
                        "llmlingua unavailable; falling back to noop"
                    ),
                }
                Arc::clone(&noop_prompt)
            }
        };

        let memory = Arc::new(NoopCompressor::new(Domain::Memory)) as Arc<dyn Compressor>;
        let kv = Arc::new(NoopCompressor::new(Domain::Kv)) as Arc<dyn Compressor>;
        let kv_policy = Arc::new(RwLock::new(KvPolicy::default()));

        Self {
            prompt: Arc::new(PromptCompression::new(prompt_primary, noop_prompt)),
            memory,
            kv,
            kv_policy,
        }
    }

    pub fn prompt(&self) -> Arc<PromptCompression> {
        Arc::clone(&self.prompt)
    }

    #[allow(dead_code)]
    pub fn memory(&self) -> Arc<dyn Compressor> {
        Arc::clone(&self.memory)
    }

    #[allow(dead_code)]
    pub fn kv(&self) -> Arc<dyn Compressor> {
        Arc::clone(&self.kv)
    }

    pub async fn set_kv_policy(&self, mut policy: KvPolicy) -> Result<KvPolicy> {
        if policy.method == KvMethod::None && policy.ratio.is_none() && policy.bits.is_none() {
            policy = KvPolicy::default();
        }
        let normalised = policy.normalise()?;
        {
            let mut guard = self.kv_policy.write().await;
            *guard = normalised.clone();
        }
        Ok(normalised)
    }

    #[allow(dead_code)]
    pub async fn kv_policy(&self) -> KvPolicy {
        self.kv_policy.read().await.clone()
    }
}
