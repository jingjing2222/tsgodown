use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum BundleFormat {
    Esm,
    Cjs,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ArtifactBundle {
    pub file: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub map: Option<String>,
    pub format: BundleFormat,
    pub exports: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ArtifactManifest {
    pub build_id: String,
    pub entries: Vec<String>,
    pub bundles: Vec<ArtifactBundle>,
    pub types: Vec<String>,
    pub tsconfig_path: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BuildInput {
    pub entries: Vec<String>,
    pub chunks: Vec<String>,
    pub tsconfig_path: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum DiagnosticLevel {
    Error,
    Warn,
    Info,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ManifestDiagnostic {
    pub level: DiagnosticLevel,
    pub code: String,
    pub message: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BuildManifestOutput {
    pub manifest: ArtifactManifest,
    pub diagnostics: Vec<ManifestDiagnostic>,
}

#[derive(thiserror::Error, Debug)]
pub enum ManifestError {
    #[error("failed to parse manifest json: {0}")]
    Parse(#[from] serde_json::Error),
}
