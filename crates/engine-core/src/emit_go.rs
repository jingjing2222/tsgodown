use serde::{Deserialize, Serialize};

use crate::analyze;
use crate::backend::{backend_provider, BackendEmitRequest, BackendEmitResponse};
use crate::contract::{AnalyzeRequest, AnalyzeResponse, Diagnostic};
use crate::go_aot::{aot_unsupported_features, render_aot_executable_program};
use crate::runtime_contract::{
    fail_closed_report_version, is_codegen_blocking_diagnostic, runtime_contract,
    unsupported_codegen_diagnostic, unsupported_executable_features, ProgramPurpose,
    RuntimeOperationOwner, RuntimeOperationStatus,
};

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
    #[serde(default)]
    pub ir_snapshot: Option<IrSnapshotRequest>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "camelCase")]
pub enum EmitGoOutputKind {
    #[default]
    Main,
    VectorSuite,
}

impl EmitGoOutputKind {
    fn purpose(self) -> ProgramPurpose {
        match self {
            Self::Main => ProgramPurpose::Main,
            Self::VectorSuite => ProgramPurpose::VectorSuite,
        }
    }
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

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct IrSnapshotRequest {
    pub file_path: String,
    pub const_name: String,
    pub description: String,
}

pub fn emit_go(request: EmitGoRequest) -> EmitGoResponse {
    emit_backend("go", request)
}

pub fn emit_backend(target_backend: &str, request: EmitGoRequest) -> EmitGoResponse {
    let analyzed = analyze(request.analyze);
    let package_name = sanitize_package_name(request.package_name.as_deref().unwrap_or("main"));
    let module_path = sanitize_module_path(
        request
            .module_path
            .as_deref()
            .unwrap_or("example.com/tsgodown-generated"),
    );
    let backend_request = BackendEmitRequest {
        analyzed,
        package_name,
        module_path,
        output_kind: request.output_kind,
        ir_snapshot: request.ir_snapshot,
    };
    match backend_provider(target_backend) {
        Ok(provider) => provider.emit(backend_request).into(),
        Err(diagnostic) => EmitGoResponse {
            version: "engine-core.emit.v1".to_string(),
            target_backend: target_backend.to_string(),
            files: vec![],
            diagnostics: vec![diagnostic],
        },
    }
}

pub(crate) fn emit_go_project(request: BackendEmitRequest) -> BackendEmitResponse {
    let analyzed = request.analyzed;
    let mut diagnostics = analyzed.diagnostics.clone();
    let package_name = request.package_name;
    let module_path = request.module_path;
    let unsupported_features = unsupported_executable_features(&analyzed.ir);
    let can_emit_executable =
        !diagnostics.iter().any(is_codegen_blocking_diagnostic) && unsupported_features.is_empty();
    if !can_emit_executable {
        diagnostics.push(unsupported_codegen_diagnostic(&unsupported_features));
    }
    let output_kind = request.output_kind.clone();
    let purpose = request.output_kind.purpose();
    let contents = if can_emit_executable {
        match render_executable_program(&package_name, &analyzed) {
            Some(program) => program,
            None => {
                let aot_features = aot_unsupported_features(&analyzed.ir);
                let fallback_features;
                let features = if aot_features.is_empty() {
                    fallback_features = vec!["aot emission unsupported by Go backend".to_string()];
                    &fallback_features
                } else {
                    &aot_features
                };
                diagnostics.push(unsupported_codegen_diagnostic(features));
                render_fail_closed_program(&package_name, &module_path, &diagnostics, purpose)
            }
        }
    } else {
        render_fail_closed_program(&package_name, &module_path, &diagnostics, purpose)
    };
    let mut files = vec![
        GeneratedFile {
            path: match output_kind.clone() {
                EmitGoOutputKind::Main => "main.go",
                EmitGoOutputKind::VectorSuite => "vector_suite.go",
            }
            .to_string(),
            contents: add_output_kind_prelude(output_kind, contents),
        },
        GeneratedFile {
            path: "go.mod".to_string(),
            contents: render_go_mod(&module_path),
        },
        GeneratedFile {
            path: "go.sum".to_string(),
            contents: render_go_sum(),
        },
        GeneratedFile {
            path: "tsgodownrt/runtime.go".to_string(),
            contents: render_fail_closed_runtime_package(),
        },
    ];

    if let Some(snapshot) = request.ir_snapshot.as_ref() {
        files.push(GeneratedFile {
            path: sanitize_relative_go_path(&snapshot.file_path),
            contents: render_ir_snapshot_file(
                &package_name,
                &sanitize_go_identifier(&snapshot.const_name),
                &snapshot.description,
                &analyzed,
            ),
        });
    }

    BackendEmitResponse {
        version: "engine-core.emit-go.v1".to_string(),
        target_backend: "go".to_string(),
        files,
        diagnostics,
    }
}

impl From<BackendEmitResponse> for EmitGoResponse {
    fn from(response: BackendEmitResponse) -> Self {
        Self {
            version: response.version,
            target_backend: response.target_backend,
            files: response.files,
            diagnostics: response.diagnostics,
        }
    }
}

fn add_output_kind_prelude(output_kind: EmitGoOutputKind, contents: String) -> String {
    match output_kind {
        EmitGoOutputKind::Main => contents,
        EmitGoOutputKind::VectorSuite => {
            format!("//go:build tsgodown_vector\n\n{contents}")
        }
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

fn sanitize_relative_go_path(value: &str) -> String {
    let cleaned = value.trim();
    if cleaned.is_empty()
        || cleaned.contains('\\')
        || cleaned.starts_with('/')
        || cleaned.starts_with('.')
        || !cleaned.ends_with(".go")
    {
        return "ir_snapshot.go".to_string();
    }
    cleaned.to_string()
}

pub(crate) fn sanitize_go_identifier(value: &str) -> String {
    let cleaned: String = value
        .chars()
        .filter(|ch| ch.is_ascii_alphanumeric() || *ch == '_')
        .collect();
    match cleaned.as_str() {
        "" => "irSnapshotJSON".to_string(),
        _ if cleaned.starts_with(|ch: char| ch.is_ascii_digit()) => format!("ir_{cleaned}"),
        "any" | "bool" | "break" | "case" | "chan" | "const" | "continue" | "default" | "defer"
        | "else" | "fallthrough" | "float32" | "float64" | "for" | "func" | "go" | "goto"
        | "if" | "import" | "int" | "int64" | "interface" | "map" | "nil" | "package" | "range"
        | "return" | "select" | "string" | "struct" | "switch" | "type" | "uint" | "var" => {
            format!("{cleaned}_")
        }
        _ => cleaned,
    }
}

fn render_fail_closed_program(
    package_name: &str,
    module_path: &str,
    diagnostics: &[Diagnostic],
    purpose: ProgramPurpose,
) -> String {
    let diagnostics_json =
        serde_json::to_string(diagnostics).expect("diagnostics should serialize");
    let (extra_report_fields, argv_setup) = match purpose {
        ProgramPurpose::Main => ("", ""),
        ProgramPurpose::VectorSuite => (
            r#"
		"corpus": corpus,
		"total": 0,
		"results": []any{},"#,
            r#"
	corpus := ""
	if len(os.Args) > 1 {
		corpus = os.Args[1]
	}
"#,
        ),
    };
    let version = fail_closed_report_version(purpose);
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

fn render_executable_program(package_name: &str, analyzed: &AnalyzeResponse) -> Option<String> {
    render_aot_executable_program(package_name, analyzed)
}

fn render_fail_closed_runtime_package() -> String {
    format!(
        r#"package tsgodownrt

import "encoding/json"

{}
func FailClosedReport(version string, diagnostics json.RawMessage, extra map[string]any) map[string]any {{
	report := map[string]any{{
		"version":     version,
		"unsupported": true,
		"diagnostics": diagnostics,
	}}
	for key, value := range extra {{
		report[key] = value
	}}
	return report
}}
"#,
        render_runtime_contract_go_metadata()
    )
}

fn render_runtime_contract_go_metadata() -> String {
    let contract = runtime_contract();
    let operations = contract
        .operations
        .iter()
        .map(|operation| {
            format!(
                "\t{{Key: {:?}, Owner: {:?}, Status: {:?}}},",
                operation.key,
                runtime_operation_owner_key(operation.owner),
                runtime_operation_status_key(operation.status)
            )
        })
        .collect::<Vec<_>>()
        .join("\n");
    format!(
        "const runtimeContractVersion = {:?}\n\ntype runtimeContractOperation struct {{\n\tKey string\n\tOwner string\n\tStatus string\n}}\n\nvar runtimeContractOperations = []runtimeContractOperation{{\n{}\n}}\n\n",
        contract.version, operations
    )
}

fn runtime_operation_owner_key(owner: RuntimeOperationOwner) -> &'static str {
    match owner {
        RuntimeOperationOwner::Contract => "contract",
        RuntimeOperationOwner::BackendRuntime => "backend-runtime",
    }
}

fn runtime_operation_status_key(status: RuntimeOperationStatus) -> &'static str {
    match status {
        RuntimeOperationStatus::Done => "done",
        RuntimeOperationStatus::Wip => "wip",
        RuntimeOperationStatus::FailClosed => "fail-closed",
    }
}

fn render_go_mod(module_path: &str) -> String {
    format!("module {module_path}\n\ngo 1.22\n\nrequire github.com/dlclark/regexp2 v1.12.0\n")
}

fn render_go_sum() -> String {
    "github.com/dlclark/regexp2 v1.12.0 h1:0j4c5qQmnC6XOWNjP3PIXURXN2gWx76rd3KvgdPkCz8=\ngithub.com/dlclark/regexp2 v1.12.0/go.mod h1:DHkYz0B9wPfa6wondMfaivmHpzrQ3v9q8cnmRbL6yW8=\n".to_string()
}

fn render_ir_snapshot_file(
    package_name: &str,
    const_name: &str,
    description: &str,
    analyzed: &AnalyzeResponse,
) -> String {
    let snapshot = serde_json::json!({
        "version": analyzed.ir.version,
        "entry": analyzed.ir.entry,
        "modules": analyzed.ir.modules,
        "diagnostics": analyzed.diagnostics,
    });
    let json = serde_json::to_string_pretty(&snapshot).expect("IR snapshot should serialize");
    let description = sanitize_go_comment_line(description);
    let json_literal = go_string_literal(&json);
    format!(
        r#"package {package_name}

// {description}
// It is emitted into generated projects so backend codegen can be driven by source semantics.
const {const_name} = {json_literal}
"#
    )
}

fn sanitize_go_comment_line(value: &str) -> String {
    let cleaned = value.replace(['\r', '\n'], " ");
    if cleaned.trim().is_empty() {
        "Analyzer-lowered executable JS IR snapshot.".to_string()
    } else {
        cleaned
    }
}

pub(crate) fn go_string_literal(value: &str) -> String {
    let mut literal = String::with_capacity(value.len() + 2);
    literal.push('"');
    for ch in value.chars() {
        match ch {
            '\\' => literal.push_str("\\\\"),
            '"' => literal.push_str("\\\""),
            '\n' => literal.push_str("\\n"),
            '\r' => literal.push_str("\\r"),
            '\t' => literal.push_str("\\t"),
            '\u{08}' => literal.push_str("\\b"),
            '\u{0c}' => literal.push_str("\\f"),
            ch if ch.is_ascii() && !ch.is_ascii_control() => literal.push(ch),
            ch => {
                let code = ch as u32;
                if code <= 0xffff {
                    literal.push_str(&format!("\\u{code:04X}"));
                } else {
                    literal.push_str(&format!("\\U{code:08X}"));
                }
            }
        }
    }
    literal.push('"');
    literal
}
