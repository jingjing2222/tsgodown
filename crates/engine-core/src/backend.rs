use crate::contract::{AnalyzeResponse, Diagnostic, DiagnosticLevel};
use crate::emit_go::{EmitGoOutputKind, GeneratedFile, IrSnapshotRequest};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BackendEmitRequest {
    pub analyzed: AnalyzeResponse,
    pub package_name: String,
    pub module_path: String,
    pub output_kind: EmitGoOutputKind,
    pub ir_snapshot: Option<IrSnapshotRequest>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BackendEmitResponse {
    pub version: String,
    pub target_backend: String,
    pub files: Vec<GeneratedFile>,
    pub diagnostics: Vec<Diagnostic>,
}

pub trait BackendProvider: Sync {
    fn name(&self) -> &'static str;
    fn emit(&self, request: BackendEmitRequest) -> BackendEmitResponse;
}

pub fn registered_backend_names() -> Vec<&'static str> {
    vec![crate::backends::go::GO_BACKEND_PROVIDER.name()]
}

pub fn backend_provider(name: &str) -> Result<&'static dyn BackendProvider, Diagnostic> {
    match name {
        "go" => Ok(&crate::backends::go::GO_BACKEND_PROVIDER),
        other => Err(unsupported_backend_diagnostic(other)),
    }
}

pub fn unsupported_backend_diagnostic(name: &str) -> Diagnostic {
    Diagnostic {
        level: DiagnosticLevel::Error,
        code: "BACKEND_PROVIDER_UNSUPPORTED".to_string(),
        message: format!(
            "Backend provider '{name}' is not registered; available backends: {}.",
            registered_backend_names().join(", ")
        ),
        source: None,
    }
}
