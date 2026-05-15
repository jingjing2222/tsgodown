use serde::{Deserialize, Serialize};

use crate::analyze;
use crate::contract::{AnalyzeRequest, Diagnostic, DiagnosticLevel};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct EmitGoRequest {
    pub analyze: AnalyzeRequest,
    #[serde(default)]
    pub package_name: Option<String>,
    #[serde(default)]
    pub module_path: Option<String>,
    #[serde(default)]
    pub output_kind: EmitGoOutputKind,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "camelCase")]
pub enum EmitGoOutputKind {
    #[default]
    Main,
    VectorSuite,
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
        files: vec![
            GeneratedFile {
                path: match request.output_kind {
                    EmitGoOutputKind::Main => "main.go",
                    EmitGoOutputKind::VectorSuite => "vector_suite.go",
                }
                .to_string(),
                contents: render_fail_closed_program(
                    &sanitize_package_name(request.package_name.as_deref().unwrap_or("main")),
                    &sanitize_module_path(
                        request
                            .module_path
                            .as_deref()
                            .unwrap_or("example.com/tsgodown-generated"),
                    ),
                    &diagnostics,
                    request.output_kind,
                ),
            },
            GeneratedFile {
                path: "tsgodownrt/runtime.go".to_string(),
                contents: render_runtime_package(),
            },
        ],
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

fn sanitize_module_path(value: &str) -> String {
    let cleaned = value.trim();
    if cleaned.is_empty()
        || cleaned.contains('"')
        || cleaned.contains('`')
        || cleaned.contains('\\')
        || cleaned.starts_with('.')
    {
        return "example.com/tsgodown-generated".to_string();
    }
    cleaned.to_string()
}

fn render_fail_closed_program(
    package_name: &str,
    module_path: &str,
    diagnostics: &[Diagnostic],
    output_kind: EmitGoOutputKind,
) -> String {
    let diagnostics_json =
        serde_json::to_string(diagnostics).expect("diagnostics should serialize");
    let (version, extra_report_fields) = match output_kind {
        EmitGoOutputKind::Main => ("engine-core.emit-go.fail-closed.v1", ""),
        EmitGoOutputKind::VectorSuite => (
            "engine-core.emit-go.vector-suite.fail-closed.v1",
            r#"
		"corpus": corpus,
		"total": 0,
		"results": []any{},"#,
        ),
    };
    let argv_setup = match output_kind {
        EmitGoOutputKind::Main => "",
        EmitGoOutputKind::VectorSuite => {
            r#"
	corpus := ""
	if len(os.Args) > 1 {
		corpus = os.Args[1]
	}
"#
        }
    };
    format!(
        r#"package {package_name}

import (
	"encoding/json"
	"fmt"
	"os"

	"{module_path}/tsgodownrt"
)

func main() {{
{argv_setup}
	diagnostics := json.RawMessage({diagnostics_json:?})
	report := tsgodownrt.FailClosedReport("{version}", diagnostics, map[string]any{{{extra_report_fields}
	}})
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

fn render_runtime_package() -> String {
    r#"package tsgodownrt

import "encoding/json"

func FailClosedReport(version string, diagnostics json.RawMessage, extra map[string]any) map[string]any {
	report := map[string]any{
		"version":     version,
		"unsupported": true,
		"diagnostics": diagnostics,
	}
	for key, value := range extra {
		report[key] = value
	}
	return report
}
"#
    .to_string()
}
