mod analyze;
mod contract;
mod emit_go;
mod runtime_contract;

pub use analyze::analyze;
pub use contract::{
    AnalyzeConfig, AnalyzeRequest, AnalyzeResponse, Diagnostic, DiagnosticLevel, DiagnosticSource,
    ExecutableModule, Import, InputManifest, IrDocument, JsExpr, JsObjectProp, JsStmt, JsValue,
    Module, Route,
};
pub use emit_go::{
    emit_go, EmitGoOutputKind, EmitGoRequest, EmitGoResponse, GeneratedFile, IrSnapshotRequest,
};
pub use runtime_contract::{
    fail_closed_report_version, unsupported_codegen_diagnostic, ProgramPurpose,
};

#[cfg(test)]
mod tests {
    use super::*;
    use std::{fs, path::PathBuf};

    #[test]
    fn contract_roundtrip_request_json() {
        let request = AnalyzeRequest {
            manifest: InputManifest {
                entry: "src/server.ts".to_string(),
                framework: Some("compiler".to_string()),
            },
            cwd: None,
            config: AnalyzeConfig {
                profile: Some("default".to_string()),
            },
        };

        let encoded = serde_json::to_string(&request).expect("encode request");
        let decoded: AnalyzeRequest = serde_json::from_str(&encoded).expect("decode request");

        assert_eq!(decoded, request);
    }

    #[test]
    fn contract_roundtrip_response_json() {
        let response = AnalyzeResponse {
            ir: IrDocument {
                version: "0.1".to_string(),
                entry: "src/server.ts".to_string(),
                modules: vec![Module {
                    id: "src/server.ts".to_string(),
                    source_path: "src/server.ts".to_string(),
                    exports: vec![],
                    imports: vec![],
                    executable: Some(ExecutableModule {
                        stmts: vec![
                            JsStmt::VarDecl {
                                name: "answer".to_string(),
                                init: Some(JsExpr::Value {
                                    value: JsValue::Number {
                                        value: "42".to_string(),
                                    },
                                }),
                            },
                            JsStmt::FunctionDecl {
                                name: "health".to_string(),
                                params: vec!["request".to_string()],
                                r#async: false,
                                body: vec![JsStmt::Return {
                                    value: Some(JsExpr::Value {
                                        value: JsValue::String {
                                            value: "ok".to_string(),
                                        },
                                    }),
                                }],
                            },
                        ],
                    }),
                }],
                routes: vec![Route {
                    method: "GET".to_string(),
                    path: "/health".to_string(),
                }],
            },
            diagnostics: vec![Diagnostic {
                level: DiagnosticLevel::Info,
                code: "ENGINE_BOOTSTRAP".to_string(),
                message: "bootstrap analyzer executed".to_string(),
                source: None,
            }],
        };

        let encoded = serde_json::to_string(&response).expect("encode response");
        let decoded: AnalyzeResponse = serde_json::from_str(&encoded).expect("decode response");

        assert_eq!(decoded, response);
    }

    #[test]
    fn analyze_maps_manifest_to_ir_entry() {
        let request = AnalyzeRequest {
            manifest: InputManifest {
                entry: "src/server.ts".to_string(),
                framework: Some("compiler".to_string()),
            },
            cwd: None,
            config: AnalyzeConfig::default(),
        };

        let response = analyze(request);

        assert_eq!(response.ir.entry, "src/server.ts");
        assert_eq!(response.ir.version, "0.1");
    }

    #[test]
    fn analyze_reads_project_modules_from_cwd() {
        let root = temp_project("engine-core-analyze");
        write(
            &root,
            "src/index.js",
            r#"
import { value } from "./value.js";
"use value import";
export { value };
"#,
        );
        write(&root, "src/value.js", "export const value = 1;");

        let response = analyze(AnalyzeRequest {
            manifest: InputManifest {
                entry: "src/index.js".to_string(),
                framework: None,
            },
            cwd: Some(root.to_string_lossy().to_string()),
            config: AnalyzeConfig::default(),
        });

        assert_eq!(response.diagnostics, vec![]);
        assert_eq!(
            response
                .ir
                .modules
                .iter()
                .map(|module| module.source_path.as_str())
                .collect::<Vec<_>>(),
            vec!["src/index.js", "src/value.js"]
        );
        assert_eq!(
            response.ir.modules[0].imports[0].resolved.as_deref(),
            Some("src/value.js")
        );
        assert_eq!(
            response.ir.modules[0]
                .executable
                .as_ref()
                .expect("entry executable")
                .stmts,
            vec![JsStmt::Expr {
                expr: JsExpr::Value {
                    value: JsValue::String {
                        value: "use value import".to_string(),
                    },
                },
            }]
        );
        assert_eq!(
            response.ir.modules[1]
                .executable
                .as_ref()
                .expect("executable module")
                .stmts,
            vec![JsStmt::VarDecl {
                name: "value".to_string(),
                init: Some(JsExpr::Value {
                    value: JsValue::Number {
                        value: "1".to_string(),
                    },
                }),
            }]
        );
    }

    #[test]
    fn emit_go_returns_fail_closed_generated_file() {
        let response = emit_go(EmitGoRequest {
            analyze: AnalyzeRequest {
                manifest: InputManifest {
                    entry: "src/server.ts".to_string(),
                    framework: Some("compiler".to_string()),
                },
                cwd: None,
                config: AnalyzeConfig::default(),
            },
            package_name: Some("9-bad package".to_string()),
            module_path: Some("example.com/custom-module".to_string()),
            output_kind: EmitGoOutputKind::Main,
            ir_snapshot: None,
        });

        assert_eq!(response.version, "engine-core.emit-go.v1");
        assert_eq!(response.target_backend, "go");
        assert_eq!(response.files.len(), 3);
        assert_eq!(response.files[0].path, "main.go");
        assert!(response.files[0]
            .contents
            .contains("package pkg_9badpackage"));
        assert!(response.files[0]
            .contents
            .contains("\"example.com/custom-module/tsgodownrt\""));
        assert!(response.files[0]
            .contents
            .contains(fail_closed_report_version(ProgramPurpose::Main)));
        assert_eq!(response.files[1].path, "go.mod");
        assert!(response.files[1]
            .contents
            .contains("module example.com/custom-module"));
        assert_eq!(response.files[2].path, "tsgodownrt/runtime.go");
        assert!(response.files[2].contents.contains("package tsgodownrt"));
        assert!(response
            .diagnostics
            .iter()
            .any(|diagnostic| diagnostic.code == "EXECUTABLE_JS_CODEGEN_NOT_IMPLEMENTED"));
        assert!(response.diagnostics.iter().any(|diagnostic| diagnostic
            .message
            .contains("unsupported features: entry module not found")));
        assert!(!response.files[0].contents.contains("node --"));
        assert!(!response.files[0].contents.contains("exec.Command"));
    }

    #[test]
    fn emit_go_returns_fail_closed_vector_suite_file() {
        let response = emit_go(EmitGoRequest {
            analyze: AnalyzeRequest {
                manifest: InputManifest {
                    entry: "tests/vector-suite-entry.mjs".to_string(),
                    framework: None,
                },
                cwd: None,
                config: AnalyzeConfig::default(),
            },
            package_name: None,
            module_path: Some("example.com/vector-suite".to_string()),
            output_kind: EmitGoOutputKind::VectorSuite,
            ir_snapshot: None,
        });

        assert_eq!(response.files.len(), 3);
        assert_eq!(response.files[0].path, "vector_suite.go");
        assert!(response.files[0]
            .contents
            .contains(fail_closed_report_version(ProgramPurpose::VectorSuite)));
        assert!(response.files[0].contents.contains("\"results\": []any{}"));
        assert!(response.files[0].contents.contains("corpus := \"\""));
    }

    #[test]
    fn emit_go_can_return_executable_vector_suite_file() {
        let root = temp_project("engine-core-vector-suite-executable");
        write(
            &root,
            "src/vector-entry.js",
            r#"
console.log("{\"version\":\"vector\",\"total\":0,\"results\":[]}")
"#,
        );

        let response = emit_go(EmitGoRequest {
            analyze: AnalyzeRequest {
                manifest: InputManifest {
                    entry: "src/vector-entry.js".to_string(),
                    framework: None,
                },
                cwd: Some(root.to_string_lossy().to_string()),
                config: AnalyzeConfig::default(),
            },
            package_name: None,
            module_path: Some("example.com/vector-suite-executable".to_string()),
            output_kind: EmitGoOutputKind::VectorSuite,
            ir_snapshot: None,
        });

        assert_eq!(response.files[0].path, "vector_suite.go");
        assert!(!response
            .diagnostics
            .iter()
            .any(|diagnostic| diagnostic.code == "EXECUTABLE_JS_CODEGEN_NOT_IMPLEMENTED"));
        assert!(response.files[0].contents.contains("tsgodownrt.RunProgram"));
        assert!(!response.files[0]
            .contents
            .contains(fail_closed_report_version(ProgramPurpose::VectorSuite)));
    }

    #[test]
    fn emit_go_can_include_ir_snapshot_file() {
        let response = emit_go(EmitGoRequest {
            analyze: AnalyzeRequest {
                manifest: InputManifest {
                    entry: "src/server.ts".to_string(),
                    framework: None,
                },
                cwd: None,
                config: AnalyzeConfig::default(),
            },
            package_name: None,
            module_path: None,
            output_kind: EmitGoOutputKind::Main,
            ir_snapshot: Some(IrSnapshotRequest {
                file_path: "source_ir.go".to_string(),
                const_name: "sourceIRJSON".to_string(),
                description: "source snapshot".to_string(),
            }),
        });

        let snapshot = response
            .files
            .iter()
            .find(|file| file.path == "source_ir.go")
            .expect("source IR snapshot file");
        assert!(snapshot.contents.contains("package main"));
        assert!(snapshot.contents.contains("const sourceIRJSON = `"));
        assert!(snapshot.contents.contains("\"entry\": \"src/server.ts\""));
    }

    #[test]
    fn emit_go_runs_simple_console_log_subset_without_fail_closed_diagnostic() {
        let root = temp_project("engine-core-simple-console-log");
        write(
            &root,
            "src/index.js",
            r#"
const value = 1 + 2
console.log("hello", value)
"#,
        );

        let response = emit_go(EmitGoRequest {
            analyze: AnalyzeRequest {
                manifest: InputManifest {
                    entry: "src/index.js".to_string(),
                    framework: None,
                },
                cwd: Some(root.to_string_lossy().to_string()),
                config: AnalyzeConfig::default(),
            },
            package_name: None,
            module_path: Some("example.com/simple-console-log".to_string()),
            output_kind: EmitGoOutputKind::Main,
            ir_snapshot: None,
        });

        assert!(!response
            .diagnostics
            .iter()
            .any(|diagnostic| diagnostic.code == "EXECUTABLE_JS_CODEGEN_NOT_IMPLEMENTED"));
        assert!(response.files[0].contents.contains("tsgodownrt.RunProgram"));

        if std::process::Command::new("go")
            .arg("version")
            .output()
            .is_err()
        {
            return;
        }

        let out_dir = root.join("dist-go");
        for file in &response.files {
            write(&out_dir, &file.path, &file.contents);
        }

        let output = std::process::Command::new("go")
            .args(["run", "."])
            .current_dir(&out_dir)
            .output()
            .expect("run generated go");
        assert!(
            output.status.success(),
            "go run failed: stdout={} stderr={}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
        assert_eq!(String::from_utf8_lossy(&output.stdout), "hello 3\n");
    }

    #[test]
    fn emit_go_runs_simple_function_call_subset() {
        let root = temp_project("engine-core-simple-function-call");
        write(
            &root,
            "src/index.js",
            r#"
function add(left, right) {
  return left + right
}
console.log("sum", add(2, 3))
"#,
        );

        let response = emit_go(EmitGoRequest {
            analyze: AnalyzeRequest {
                manifest: InputManifest {
                    entry: "src/index.js".to_string(),
                    framework: None,
                },
                cwd: Some(root.to_string_lossy().to_string()),
                config: AnalyzeConfig::default(),
            },
            package_name: None,
            module_path: Some("example.com/simple-function-call".to_string()),
            output_kind: EmitGoOutputKind::Main,
            ir_snapshot: None,
        });

        assert!(!response
            .diagnostics
            .iter()
            .any(|diagnostic| diagnostic.code == "EXECUTABLE_JS_CODEGEN_NOT_IMPLEMENTED"));

        if std::process::Command::new("go")
            .arg("version")
            .output()
            .is_err()
        {
            return;
        }

        let out_dir = root.join("dist-go");
        for file in &response.files {
            write(&out_dir, &file.path, &file.contents);
        }

        let output = std::process::Command::new("go")
            .args(["run", "."])
            .current_dir(&out_dir)
            .output()
            .expect("run generated go");
        assert!(
            output.status.success(),
            "go run failed: stdout={} stderr={}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
        assert_eq!(String::from_utf8_lossy(&output.stdout), "sum 5\n");
    }

    #[test]
    fn emit_go_runs_member_assignment_subset() {
        let root = temp_project("engine-core-member-assignment");
        write(
            &root,
            "src/index.js",
            r#"
const target = {}
target.value = 4
module.exports.answer = target.value + 1
console.log("answer", module.exports.answer)
"#,
        );

        let response = emit_go(EmitGoRequest {
            analyze: AnalyzeRequest {
                manifest: InputManifest {
                    entry: "src/index.js".to_string(),
                    framework: None,
                },
                cwd: Some(root.to_string_lossy().to_string()),
                config: AnalyzeConfig::default(),
            },
            package_name: None,
            module_path: Some("example.com/member-assignment".to_string()),
            output_kind: EmitGoOutputKind::Main,
            ir_snapshot: None,
        });

        assert!(!response
            .diagnostics
            .iter()
            .any(|diagnostic| diagnostic.code == "EXECUTABLE_JS_CODEGEN_NOT_IMPLEMENTED"));

        if std::process::Command::new("go")
            .arg("version")
            .output()
            .is_err()
        {
            return;
        }

        let out_dir = root.join("dist-go");
        for file in &response.files {
            write(&out_dir, &file.path, &file.contents);
        }

        let output = std::process::Command::new("go")
            .args(["run", "."])
            .current_dir(&out_dir)
            .output()
            .expect("run generated go");
        assert!(
            output.status.success(),
            "go run failed: stdout={} stderr={}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
        assert_eq!(String::from_utf8_lossy(&output.stdout), "answer 5\n");
    }

    #[test]
    fn emit_go_runs_if_and_logical_subset() {
        let root = temp_project("engine-core-if-logical");
        write(
            &root,
            "src/index.js",
            r#"
const target = { ready: true }
if (target.ready && typeof target === "object") {
  console.log("branch", "yes")
} else {
  console.log("branch", "no")
}
console.log("types", typeof null, typeof undefined, null, undefined)
"#,
        );

        let response = emit_go(EmitGoRequest {
            analyze: AnalyzeRequest {
                manifest: InputManifest {
                    entry: "src/index.js".to_string(),
                    framework: None,
                },
                cwd: Some(root.to_string_lossy().to_string()),
                config: AnalyzeConfig::default(),
            },
            package_name: None,
            module_path: Some("example.com/if-logical".to_string()),
            output_kind: EmitGoOutputKind::Main,
            ir_snapshot: None,
        });

        assert!(!response
            .diagnostics
            .iter()
            .any(|diagnostic| diagnostic.code == "EXECUTABLE_JS_CODEGEN_NOT_IMPLEMENTED"));

        if std::process::Command::new("go")
            .arg("version")
            .output()
            .is_err()
        {
            return;
        }

        let out_dir = root.join("dist-go");
        for file in &response.files {
            write(&out_dir, &file.path, &file.contents);
        }

        let output = std::process::Command::new("go")
            .args(["run", "."])
            .current_dir(&out_dir)
            .output()
            .expect("run generated go");
        assert!(
            output.status.success(),
            "go run failed: stdout={} stderr={}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
        assert_eq!(
            String::from_utf8_lossy(&output.stdout),
            "branch yes\ntypes object undefined null undefined\n"
        );
    }

    #[test]
    fn emit_go_runs_basic_esm_import_subset() {
        let root = temp_project("engine-core-esm-import");
        write(
            &root,
            "src/index.js",
            r#"
import { value } from "./value.js";
console.log("imported", value + 2)
"#,
        );
        write(&root, "src/value.js", "export const value = 40;");

        let response = emit_go(EmitGoRequest {
            analyze: AnalyzeRequest {
                manifest: InputManifest {
                    entry: "src/index.js".to_string(),
                    framework: None,
                },
                cwd: Some(root.to_string_lossy().to_string()),
                config: AnalyzeConfig::default(),
            },
            package_name: None,
            module_path: Some("example.com/esm-import".to_string()),
            output_kind: EmitGoOutputKind::Main,
            ir_snapshot: None,
        });

        assert!(!response
            .diagnostics
            .iter()
            .any(|diagnostic| diagnostic.code == "EXECUTABLE_JS_CODEGEN_NOT_IMPLEMENTED"));

        if std::process::Command::new("go")
            .arg("version")
            .output()
            .is_err()
        {
            return;
        }

        let out_dir = root.join("dist-go");
        for file in &response.files {
            write(&out_dir, &file.path, &file.contents);
        }

        let output = std::process::Command::new("go")
            .args(["run", "."])
            .current_dir(&out_dir)
            .output()
            .expect("run generated go");
        assert!(
            output.status.success(),
            "go run failed: stdout={} stderr={}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
        assert_eq!(String::from_utf8_lossy(&output.stdout), "imported 42\n");
    }

    #[test]
    fn emit_go_runs_array_destructuring_subset() {
        let root = temp_project("engine-core-array-destructuring");
        write(
            &root,
            "src/index.js",
            r#"
const [, , corpus, vectorPath] = ["node", "entry", "semver", "vectors.json"];
console.log("args", corpus, vectorPath)
"#,
        );

        let response = emit_go(EmitGoRequest {
            analyze: AnalyzeRequest {
                manifest: InputManifest {
                    entry: "src/index.js".to_string(),
                    framework: None,
                },
                cwd: Some(root.to_string_lossy().to_string()),
                config: AnalyzeConfig::default(),
            },
            package_name: None,
            module_path: Some("example.com/array-destructuring".to_string()),
            output_kind: EmitGoOutputKind::Main,
            ir_snapshot: None,
        });

        assert!(!response
            .diagnostics
            .iter()
            .any(|diagnostic| diagnostic.code == "EXECUTABLE_JS_CODEGEN_NOT_IMPLEMENTED"));

        if std::process::Command::new("go")
            .arg("version")
            .output()
            .is_err()
        {
            return;
        }

        let out_dir = root.join("dist-go");
        for file in &response.files {
            write(&out_dir, &file.path, &file.contents);
        }

        let output = std::process::Command::new("go")
            .args(["run", "."])
            .current_dir(&out_dir)
            .output()
            .expect("run generated go");
        assert!(
            output.status.success(),
            "go run failed: stdout={} stderr={}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
        assert_eq!(
            String::from_utf8_lossy(&output.stdout),
            "args semver vectors.json\n"
        );
    }

    #[test]
    fn emit_go_runs_for_of_await_and_array_push_subset() {
        let root = temp_project("engine-core-for-of-await");
        write(
            &root,
            "src/index.js",
            r#"
function double(value) {
  return value * 2
}
const results = []
for (const value of [1, 2, 3]) {
  results.push(await double(value))
}
console.log("loop", results.length, results[0], results[2])
"#,
        );

        let response = emit_go(EmitGoRequest {
            analyze: AnalyzeRequest {
                manifest: InputManifest {
                    entry: "src/index.js".to_string(),
                    framework: None,
                },
                cwd: Some(root.to_string_lossy().to_string()),
                config: AnalyzeConfig::default(),
            },
            package_name: None,
            module_path: Some("example.com/for-of-await".to_string()),
            output_kind: EmitGoOutputKind::Main,
            ir_snapshot: None,
        });

        assert!(!response
            .diagnostics
            .iter()
            .any(|diagnostic| diagnostic.code == "EXECUTABLE_JS_CODEGEN_NOT_IMPLEMENTED"));

        if std::process::Command::new("go")
            .arg("version")
            .output()
            .is_err()
        {
            return;
        }

        let out_dir = root.join("dist-go");
        for file in &response.files {
            write(&out_dir, &file.path, &file.contents);
        }

        let output = std::process::Command::new("go")
            .args(["run", "."])
            .current_dir(&out_dir)
            .output()
            .expect("run generated go");
        assert!(
            output.status.success(),
            "go run failed: stdout={} stderr={}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
        assert_eq!(String::from_utf8_lossy(&output.stdout), "loop 3 2 6\n");
    }

    #[test]
    fn emit_go_runs_update_and_bitwise_operator_subset() {
        let root = temp_project("engine-core-operators");
        write(
            &root,
            "src/index.js",
            r#"
let count = 1
const before = count++
const after = ++count
let fallback = null
fallback ??= "set"
count += 5
const mask = (5 & 3) | (1 << 4)
const shifted = -8 >> 1
const unsigned = -1 >>> 31
console.log("ops", before, after, count, fallback, mask, shifted, unsigned, void count)
"#,
        );

        let response = emit_go(EmitGoRequest {
            analyze: AnalyzeRequest {
                manifest: InputManifest {
                    entry: "src/index.js".to_string(),
                    framework: None,
                },
                cwd: Some(root.to_string_lossy().to_string()),
                config: AnalyzeConfig::default(),
            },
            package_name: None,
            module_path: Some("example.com/operators".to_string()),
            output_kind: EmitGoOutputKind::Main,
            ir_snapshot: None,
        });

        assert!(!response
            .diagnostics
            .iter()
            .any(|diagnostic| diagnostic.code == "EXECUTABLE_JS_CODEGEN_NOT_IMPLEMENTED"));

        if std::process::Command::new("go")
            .arg("version")
            .output()
            .is_err()
        {
            return;
        }

        let out_dir = root.join("dist-go");
        for file in &response.files {
            write(&out_dir, &file.path, &file.contents);
        }

        let output = std::process::Command::new("go")
            .args(["run", "."])
            .current_dir(&out_dir)
            .output()
            .expect("run generated go");
        assert!(
            output.status.success(),
            "go run failed: stdout={} stderr={}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
        assert_eq!(
            String::from_utf8_lossy(&output.stdout),
            "ops 1 3 8 set 17 -4 1 undefined\n"
        );
    }

    #[test]
    fn emit_go_runs_while_break_continue_subset() {
        let root = temp_project("engine-core-while");
        write(
            &root,
            "src/index.js",
            r#"
let value = 0
let total = 0
while (value < 8) {
  value++
  if (value === 2) continue
  if (value === 6) break
  total += value
}
console.log("while", value, total)
"#,
        );

        let response = emit_go(EmitGoRequest {
            analyze: AnalyzeRequest {
                manifest: InputManifest {
                    entry: "src/index.js".to_string(),
                    framework: None,
                },
                cwd: Some(root.to_string_lossy().to_string()),
                config: AnalyzeConfig::default(),
            },
            package_name: None,
            module_path: Some("example.com/while".to_string()),
            output_kind: EmitGoOutputKind::Main,
            ir_snapshot: None,
        });

        assert!(!response
            .diagnostics
            .iter()
            .any(|diagnostic| diagnostic.code == "EXECUTABLE_JS_CODEGEN_NOT_IMPLEMENTED"));

        if std::process::Command::new("go")
            .arg("version")
            .output()
            .is_err()
        {
            return;
        }

        let out_dir = root.join("dist-go");
        for file in &response.files {
            write(&out_dir, &file.path, &file.contents);
        }

        let output = std::process::Command::new("go")
            .args(["run", "."])
            .current_dir(&out_dir)
            .output()
            .expect("run generated go");
        assert!(
            output.status.success(),
            "go run failed: stdout={} stderr={}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
        assert_eq!(String::from_utf8_lossy(&output.stdout), "while 6 13\n");
    }

    #[test]
    fn emit_go_runs_for_loop_subset() {
        let root = temp_project("engine-core-for");
        write(
            &root,
            "src/index.js",
            r#"
let total = 0
for (let index = 0; index < 6; index++) {
  if (index === 1) continue
  if (index === 5) break
  total += index
}
console.log("for", total)
"#,
        );

        let response = emit_go(EmitGoRequest {
            analyze: AnalyzeRequest {
                manifest: InputManifest {
                    entry: "src/index.js".to_string(),
                    framework: None,
                },
                cwd: Some(root.to_string_lossy().to_string()),
                config: AnalyzeConfig::default(),
            },
            package_name: None,
            module_path: Some("example.com/for".to_string()),
            output_kind: EmitGoOutputKind::Main,
            ir_snapshot: None,
        });

        assert!(!response
            .diagnostics
            .iter()
            .any(|diagnostic| diagnostic.code == "EXECUTABLE_JS_CODEGEN_NOT_IMPLEMENTED"));

        if std::process::Command::new("go")
            .arg("version")
            .output()
            .is_err()
        {
            return;
        }

        let out_dir = root.join("dist-go");
        for file in &response.files {
            write(&out_dir, &file.path, &file.contents);
        }

        let output = std::process::Command::new("go")
            .args(["run", "."])
            .current_dir(&out_dir)
            .output()
            .expect("run generated go");
        assert!(
            output.status.success(),
            "go run failed: stdout={} stderr={}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
        assert_eq!(String::from_utf8_lossy(&output.stdout), "for 9\n");
    }

    #[test]
    fn emit_go_runs_switch_and_in_operator_subset() {
        let root = temp_project("engine-core-switch-in");
        write(
            &root,
            "src/index.js",
            r#"
const target = { mode: "b", value: 3 }
let result = 0
switch (target.mode) {
  case "a":
    result = 1
    break
  case "b":
    result = "value" in target ? target.value : 2
    break
  default:
    result = 9
}
console.log("switch", result, "length" in [1, 2], "4" in [1, 2])
"#,
        );

        let response = emit_go(EmitGoRequest {
            analyze: AnalyzeRequest {
                manifest: InputManifest {
                    entry: "src/index.js".to_string(),
                    framework: None,
                },
                cwd: Some(root.to_string_lossy().to_string()),
                config: AnalyzeConfig::default(),
            },
            package_name: None,
            module_path: Some("example.com/switch-in".to_string()),
            output_kind: EmitGoOutputKind::Main,
            ir_snapshot: None,
        });

        assert!(!response
            .diagnostics
            .iter()
            .any(|diagnostic| diagnostic.code == "EXECUTABLE_JS_CODEGEN_NOT_IMPLEMENTED"));

        if std::process::Command::new("go")
            .arg("version")
            .output()
            .is_err()
        {
            return;
        }

        let out_dir = root.join("dist-go");
        for file in &response.files {
            write(&out_dir, &file.path, &file.contents);
        }

        let output = std::process::Command::new("go")
            .args(["run", "."])
            .current_dir(&out_dir)
            .output()
            .expect("run generated go");
        assert!(
            output.status.success(),
            "go run failed: stdout={} stderr={}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
        assert_eq!(
            String::from_utf8_lossy(&output.stdout),
            "switch 3 true false\n"
        );
    }

    fn write(root: &std::path::Path, rel: &str, source: &str) {
        let path = root.join(rel);
        fs::create_dir_all(path.parent().expect("parent")).expect("create parent");
        fs::write(path, source).expect("write source");
    }

    fn temp_project(name: &str) -> PathBuf {
        let root = std::env::temp_dir().join(format!("tsgodown-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).expect("create temp project");
        root
    }
}
