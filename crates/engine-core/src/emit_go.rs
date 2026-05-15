use serde::{Deserialize, Serialize};

use crate::analyze;
use crate::contract::{AnalyzeRequest, Diagnostic, DiagnosticLevel};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct EmitGoRequest {
    pub analyze: AnalyzeRequest,
    #[serde(default)]
    pub package_name: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct EmitGoResponse {
    pub version: String,
    pub target_backend: String,
    pub files: Vec<GeneratedFile>,
    pub diagnostics: Vec<Diagnostic>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct GeneratedFile {
    pub path: String,
    pub contents: String,
}

pub fn emit_go(request: EmitGoRequest) -> EmitGoResponse {
    let analyzed = analyze(request.analyze);
    let mut diagnostics = analyzed.diagnostics;
    diagnostics.push(Diagnostic {
        level: DiagnosticLevel::Error,
        code: "GO_CODEGEN_NOT_IMPLEMENTED".to_string(),
        message: "engine-core owns Go codegen, but executable JS lowering to Go is not implemented yet; failing closed.".to_string(),
        source: None,
    });

    EmitGoResponse {
        version: "engine-core.emit-go.v1".to_string(),
        target_backend: "go".to_string(),
        files: vec![GeneratedFile {
            path: "main.go".to_string(),
            contents: render_fail_closed_main(
                &sanitize_package_name(request.package_name.as_deref().unwrap_or("main")),
                &diagnostics,
            ),
        }],
        diagnostics,
    }
}

fn sanitize_package_name(value: &str) -> String {
    let cleaned: String = value
        .chars()
        .filter(|ch| ch.is_ascii_alphanumeric() || *ch == '_')
        .collect();
    match cleaned.as_str() {
        "" => "main".to_string(),
        _ if cleaned.starts_with(|ch: char| ch.is_ascii_digit()) => format!("pkg_{cleaned}"),
        _ => cleaned,
    }
}

fn render_fail_closed_main(package_name: &str, diagnostics: &[Diagnostic]) -> String {
    let diagnostics_json =
        serde_json::to_string(diagnostics).expect("diagnostics should serialize");
    format!(
        r#"package {package_name}

import (
	"encoding/json"
	"fmt"
	"os"
)

func main() {{
	diagnostics := json.RawMessage({diagnostics_json:?})
	report := map[string]any{{
		"version": "engine-core.emit-go.fail-closed.v1",
		"unsupported": true,
		"diagnostics": diagnostics,
	}}
	bytes, err := json.Marshal(report)
	if err != nil {{
		fmt.Fprintln(os.Stderr, err)
		os.Exit(1)
	}}
	fmt.Println(string(bytes))
	os.Exit(1)
}}
"#
    )
}
