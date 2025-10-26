use std::{collections::BTreeMap, fs, path::Path};

use anyhow::Context as _;
use arw_runtime::{RuntimeAccelerator, RuntimeModality};
use once_cell::sync::Lazy;
use regex::Regex;
use schemars::JsonSchema;
use semver::Version;
use serde::{Deserialize, Serialize};
use thiserror::Error;

/// Supported manifest serialization formats.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ManifestFormat {
    Json,
    Toml,
    Unknown,
}

impl ManifestFormat {
    fn detect_from_path(path: &Path) -> Self {
        match path.extension().and_then(|ext| ext.to_str()) {
            Some(ext) if ext.eq_ignore_ascii_case("json") => ManifestFormat::Json,
            Some(ext) if ext.eq_ignore_ascii_case("toml") => ManifestFormat::Toml,
            _ => ManifestFormat::Unknown,
        }
    }
}

/// Wrapper struct for adapter manifests.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub struct RuntimeAdapterManifest {
    pub id: String,
    pub version: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    #[schemars(with = "Vec<String>")]
    pub modalities: Vec<RuntimeModality>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tags: Vec<String>,
    pub entrypoint: AdapterEntrypoint,
    #[serde(default)]
    pub resources: AdapterResources,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub consent: Option<AdapterConsent>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub metrics: Vec<AdapterMetric>,
    #[serde(default)]
    pub health: AdapterHealthSpec,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub metadata: BTreeMap<String, String>,
}

impl RuntimeAdapterManifest {
    /// Load a manifest from a string.
    pub fn from_str(input: &str, format: ManifestFormat) -> Result<Self, ManifestLoadError> {
        match format {
            ManifestFormat::Json => serde_json::from_str::<Self>(input)
                .map_err(|err| ManifestLoadError::Parse(oops::ParseError::Json(err))),
            ManifestFormat::Toml => toml::from_str::<Self>(input)
                .map_err(|err| ManifestLoadError::Parse(oops::ParseError::Toml(err))),
            ManifestFormat::Unknown => match serde_json::from_str::<Self>(input) {
                Ok(value) => Ok(value),
                Err(json_err) => match toml::from_str::<Self>(input) {
                    Ok(value) => Ok(value),
                    Err(toml_err) => Err(ManifestLoadError::Parse(oops::ParseError::Both {
                        json: json_err,
                        toml: toml_err,
                    })),
                },
            },
        }
    }

    /// Load a manifest from disk. The format is inferred from the file extension.
    pub fn from_path<P: AsRef<Path>>(path: P) -> Result<Self, ManifestLoadError> {
        let path = path.as_ref();
        let format = ManifestFormat::detect_from_path(path);
        let raw = fs::read_to_string(path)
            .with_context(|| format!("failed to read manifest at {}", path.display()))
            .map_err(ManifestLoadError::Io)?;
        Self::from_str(&raw, format)
    }

    /// Validate manifest fields. The returned report contains errors and warnings.
    pub fn validate(&self) -> ValidationReport {
        let mut report = ValidationReport::default();

        if self.id.trim().is_empty() {
            report.push_error("id", "identifier is required");
        } else if !is_valid_id(&self.id) {
            report.push_error(
                "id",
                "identifier must contain only ASCII letters, digits, '.', '-', or '_'",
            );
        }

        if self.version.trim().is_empty() {
            report.push_error("version", "version is required");
        } else if Version::parse(self.version.trim()).is_err() {
            report.push_error(
                "version",
                "version must use semantic versioning (eg. 0.1.0)",
            );
        }

        if self.entrypoint.crate_name.trim().is_empty() {
            report.push_error("entrypoint.crate_name", "crate_name is required");
        }
        if self.entrypoint.symbol.trim().is_empty() {
            report.push_error("entrypoint.symbol", "symbol is required");
        }

        // Modalities should not be empty.
        if self.modalities.is_empty() {
            report.push_error("modalities", "at least one modality must be declared");
        }

        let mut seen_modalities: Vec<RuntimeModality> = Vec::new();
        for modality in &self.modalities {
            if seen_modalities.contains(modality) {
                report.push_warning(
                    "modalities",
                    format!("duplicate modality declared: {:?}", modality).as_str(),
                );
            } else {
                seen_modalities.push(modality.clone());
            }
        }

        // Tags hygiene: prefer kebab/underscore case, no spaces, dedupe.
        static TAG_RE: Lazy<Regex> =
            Lazy::new(|| Regex::new(r"^[a-z0-9][a-z0-9_-]{0,31}$").unwrap());
        let mut seen_tags: Vec<&str> = Vec::new();
        for tag in &self.tags {
            let t = tag.trim();
            if t.is_empty() {
                report.push_warning("tags", "ignore empty tag entries");
                continue;
            }
            if !TAG_RE.is_match(t) {
                report.push_warning("tags", "tag should be lowercase, 1-32 chars, [a-z0-9_-]");
            }
            if seen_tags.contains(&t) {
                report.push_warning("tags", "duplicate tag");
            } else {
                seen_tags.push(t);
            }
        }

        if let Some(consent) = &self.consent {
            if consent.summary.trim().is_empty() {
                report.push_warning(
                    "consent.summary",
                    "consent summary should describe why elevated access is required",
                );
            }
            if let Some(url) = &consent.details_url {
                let u = url.trim();
                if !(u.starts_with("http://") || u.starts_with("https://")) {
                    report.push_warning(
                        "consent.details_url",
                        "details_url should start with http:// or https://",
                    );
                }
            }
        }

        // Resource sanity checks
        if let Some(cpu) = self.resources.recommended_cpu_threads {
            if cpu == 0 {
                report.push_error(
                    "resources.recommended_cpu_threads",
                    "recommended_cpu_threads must be >= 1 when specified",
                );
            } else if cpu > 128 {
                report.push_warning(
                    "resources.recommended_cpu_threads",
                    "cpu thread hint above 128 is unusual; consider lowering",
                );
            }
        }

        if let Some(mem) = self.resources.recommended_memory_mb {
            if mem == 0 {
                report.push_error(
                    "resources.recommended_memory_mb",
                    "recommended_memory_mb must be > 0 when specified",
                );
            } else if mem < 512 {
                report.push_warning(
                    "resources.recommended_memory_mb",
                    "memory hint below 512MB may be too low for stable operation",
                );
            }
        }

        if self.resources.requires_network.unwrap_or(false) {
            // Recommend an explicit capability token when network is required.
            let mut has_net_cap = false;
            if let Some(consent) = &self.consent {
                let caps = consent
                    .capabilities
                    .iter()
                    .map(|s| s.trim().to_ascii_lowercase());
                for c in caps {
                    if c == "egress" || c == "network" || c == "net" {
                        has_net_cap = true;
                        break;
                    }
                }
            }
            if !has_net_cap {
                report.push_warning(
                    "resources.requires_network",
                    "requires_network is true but consent.capabilities lacks 'egress'",
                );
            }
        }

        // Health grace should not be shorter than poll interval.
        if self.health.grace_period_ms < self.health.poll_interval_ms {
            report.push_warning(
                "health.grace_period_ms",
                "grace_period_ms is shorter than poll_interval_ms; consider increasing",
            );
        }

        // Metric names hygiene: follow Prometheus-ish pattern and dedupe.
        static METRIC_RE: Lazy<Regex> =
            Lazy::new(|| Regex::new(r"^[a-zA-Z_:][a-zA-Z0-9_:]*$").unwrap());
        let mut seen_metrics: Vec<&str> = Vec::new();
        for m in &self.metrics {
            let name = m.name.trim();
            if name.is_empty() {
                report.push_error("metrics.name", "metric name is required");
                continue;
            }
            if !METRIC_RE.is_match(name) {
                report.push_warning(
                    "metrics.name",
                    "metric name should match ^[a-zA-Z_:][a-zA-Z0-9_:]*$",
                );
            }
            if seen_metrics.contains(&name) {
                report.push_warning("metrics", "duplicate metric name");
            } else {
                seen_metrics.push(name);
            }
        }

        if self.health.poll_interval_ms < 500 {
            report.push_warning(
                "health.poll_interval_ms",
                "poll interval below 500ms may cause unnecessary load; consider raising it",
            );
        }

        report
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub struct AdapterEntrypoint {
    #[serde(default)]
    pub crate_name: String,
    #[serde(default)]
    pub symbol: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub kind: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub struct AdapterResources {
    #[schemars(with = "Option<String>")]
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub accelerator: Option<RuntimeAccelerator>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub recommended_memory_mb: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub recommended_cpu_threads: Option<u8>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub requires_network: Option<bool>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub struct AdapterConsent {
    #[serde(default)]
    pub summary: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub details_url: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub capabilities: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub struct AdapterMetric {
    #[serde(default)]
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub unit: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub struct AdapterHealthSpec {
    #[serde(default = "AdapterHealthSpec::default_poll_interval")]
    pub poll_interval_ms: u64,
    #[serde(default = "AdapterHealthSpec::default_grace_period")]
    pub grace_period_ms: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub status_endpoint: Option<String>,
}

impl Default for AdapterHealthSpec {
    fn default() -> Self {
        Self {
            poll_interval_ms: Self::default_poll_interval(),
            grace_period_ms: Self::default_grace_period(),
            status_endpoint: None,
        }
    }
}

impl AdapterHealthSpec {
    const DEFAULT_POLL_MS: u64 = 5_000;
    const DEFAULT_GRACE_MS: u64 = 15_000;

    const fn default_poll_interval() -> u64 {
        Self::DEFAULT_POLL_MS
    }

    const fn default_grace_period() -> u64 {
        Self::DEFAULT_GRACE_MS
    }
}

/// Report emitted by [`RuntimeAdapterManifest::validate`].
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct ValidationReport {
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub errors: Vec<ValidationIssue>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub warnings: Vec<ValidationIssue>,
}

impl ValidationReport {
    pub fn is_success(&self) -> bool {
        self.errors.is_empty()
    }

    pub fn push_error<S: Into<String>>(&mut self, field: S, message: S) {
        self.errors.push(ValidationIssue::new(field, message));
    }

    pub fn push_warning<S: Into<String>>(&mut self, field: S, message: S) {
        self.warnings.push(ValidationIssue::new(field, message));
    }
}

/// Individual validation issue.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct ValidationIssue {
    pub field: String,
    pub message: String,
}

impl ValidationIssue {
    pub fn new<S: Into<String>>(field: S, message: S) -> Self {
        Self {
            field: field.into(),
            message: message.into(),
        }
    }
}

/// Errors encountered while loading or parsing a manifest file.
#[derive(Debug, Error)]
pub enum ManifestLoadError {
    #[error("{0}")]
    Io(#[source] anyhow::Error),
    #[error("{0}")]
    Parse(oops::ParseError),
}

mod oops {
    use thiserror::Error;

    #[derive(Debug, Error)]
    pub enum ParseError {
        #[error("failed to parse manifest as JSON: {0}")]
        Json(#[source] serde_json::Error),
        #[error("failed to parse manifest as TOML: {0}")]
        Toml(#[source] toml::de::Error),
        #[error(
            "failed to parse manifest as JSON ({json}) and TOML ({toml}) – specify format explicitly"
        )]
        Both {
            #[source]
            json: serde_json::Error,
            toml: toml::de::Error,
        },
    }
}

fn is_valid_id(value: &str) -> bool {
    value
        .chars()
        .all(|c| matches!(c, 'a'..='z' | 'A'..='Z' | '0'..='9' | '-' | '_' | '.'))
}

#[cfg(test)]
mod tests {
    use super::*;
    use once_cell::sync::Lazy;
    use tempfile::NamedTempFile;

    static SAMPLE_MANIFEST: Lazy<String> = Lazy::new(|| {
        serde_json::to_string_pretty(&RuntimeAdapterManifest {
            id: "demo.adapter".into(),
            version: "0.1.0".into(),
            name: Some("Demo Adapter".into()),
            description: Some("Example adapter for tests".into()),
            modalities: vec![RuntimeModality::Text],
            tags: vec!["test".into()],
            entrypoint: AdapterEntrypoint {
                crate_name: "demo_adapter".into(),
                symbol: "create_adapter".into(),
                kind: None,
            },
            resources: AdapterResources {
                accelerator: Some(RuntimeAccelerator::Cpu),
                recommended_memory_mb: Some(4096),
                recommended_cpu_threads: Some(8),
                requires_network: Some(false),
            },
            consent: Some(AdapterConsent {
                summary: "Processes local text prompts".into(),
                details_url: None,
                capabilities: vec!["read_files".into()],
            }),
            metrics: vec![AdapterMetric {
                name: "tokens_processed_total".into(),
                description: Some("Total tokens processed by the adapter".into()),
                unit: Some("count".into()),
            }],
            health: AdapterHealthSpec::default(),
            metadata: BTreeMap::new(),
        })
        .unwrap()
    });

    #[test]
    fn manifest_parses_and_validates() {
        let manifest = RuntimeAdapterManifest::from_str(&SAMPLE_MANIFEST, ManifestFormat::Json)
            .expect("manifest parse");
        let report = manifest.validate();
        assert!(report.is_success(), "expected no validation errors");
    }

    #[test]
    fn manifest_detects_invalid_id() {
        let mut manifest =
            RuntimeAdapterManifest::from_str(&SAMPLE_MANIFEST, ManifestFormat::Json).unwrap();
        manifest.id = "spaces not allowed".into();
        let report = manifest.validate();
        assert!(
            report.errors.iter().any(|issue| issue.field == "id"),
            "expected invalid id error"
        );
    }

    #[test]
    fn from_path_detects_format() {
        let _tmp = NamedTempFile::new().unwrap();
        let path = _tmp.path().with_extension("json");
        fs::write(&path, SAMPLE_MANIFEST.as_bytes()).unwrap();
        let manifest = RuntimeAdapterManifest::from_path(&path).unwrap();
        assert_eq!(manifest.id, "demo.adapter");
    }

    #[test]
    fn modalities_required_and_tags_linted() {
        let mut manifest =
            RuntimeAdapterManifest::from_str(&SAMPLE_MANIFEST, ManifestFormat::Json).unwrap();
        manifest.modalities.clear();
        manifest.tags = vec!["".into(), "Bad Tag".into(), "ok".into(), "ok".into()];
        let report = manifest.validate();
        assert!(
            report.errors.iter().any(|i| i.field == "modalities"),
            "missing modalities should error"
        );
        assert!(report
            .warnings
            .iter()
            .any(|i| i.field == "tags" && i.message.contains("ignore empty")));
        assert!(
            report
                .warnings
                .iter()
                .any(|i| i.field == "tags" && i.message.contains("lowercase")),
            "bad tag pattern should warn"
        );
        assert!(report
            .warnings
            .iter()
            .any(|i| i.field == "tags" && i.message.contains("duplicate")));
    }

    #[test]
    fn resources_sanity_and_network_consent_hint() {
        let mut manifest =
            RuntimeAdapterManifest::from_str(&SAMPLE_MANIFEST, ManifestFormat::Json).unwrap();
        manifest.resources.recommended_cpu_threads = Some(0);
        manifest.resources.recommended_memory_mb = Some(0);
        manifest.resources.requires_network = Some(true);
        // Ensure consent lacks 'egress'
        if let Some(consent) = manifest.consent.as_mut() {
            consent.capabilities.clear();
        }
        let report = manifest.validate();
        assert!(report
            .errors
            .iter()
            .any(|i| i.field == "resources.recommended_cpu_threads"));
        assert!(report
            .errors
            .iter()
            .any(|i| i.field == "resources.recommended_memory_mb"));
        assert!(report
            .warnings
            .iter()
            .any(|i| i.field == "resources.requires_network"));
    }

    #[test]
    fn health_grace_should_exceed_poll() {
        let mut manifest =
            RuntimeAdapterManifest::from_str(&SAMPLE_MANIFEST, ManifestFormat::Json).unwrap();
        manifest.health.poll_interval_ms = 2_000;
        manifest.health.grace_period_ms = 1_000;
        let report = manifest.validate();
        assert!(report
            .warnings
            .iter()
            .any(|i| i.field == "health.grace_period_ms"));
    }

    #[test]
    fn metrics_names_and_duplicates() {
        let mut manifest =
            RuntimeAdapterManifest::from_str(&SAMPLE_MANIFEST, ManifestFormat::Json).unwrap();
        manifest.metrics = vec![
            AdapterMetric {
                name: "".into(),
                description: None,
                unit: None,
            },
            AdapterMetric {
                name: "bad@name".into(),
                description: None,
                unit: None,
            },
            AdapterMetric {
                name: "tokens_processed_total".into(),
                description: None,
                unit: None,
            },
            AdapterMetric {
                name: "tokens_processed_total".into(),
                description: None,
                unit: None,
            },
        ];
        let report = manifest.validate();
        assert!(
            report.errors.iter().any(|i| i.field == "metrics.name"),
            "empty metric name should error"
        );
        assert!(
            report.warnings.iter().any(|i| i.field == "metrics.name"),
            "invalid metric pattern should warn"
        );
        assert!(report
            .warnings
            .iter()
            .any(|i| i.field == "metrics" && i.message.contains("duplicate")));
    }

    #[test]
    fn consent_details_url_scheme_hint() {
        let mut manifest =
            RuntimeAdapterManifest::from_str(&SAMPLE_MANIFEST, ManifestFormat::Json).unwrap();
        if let Some(consent) = manifest.consent.as_mut() {
            consent.details_url = Some("ftp://example.com/details".into());
        }
        let report = manifest.validate();
        assert!(report
            .warnings
            .iter()
            .any(|i| i.field == "consent.details_url"));
    }
}
