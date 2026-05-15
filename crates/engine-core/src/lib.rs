mod analyze;
mod backend;
mod backends;
mod contract;
mod emit_go;
mod runtime_contract;

pub use analyze::analyze;
pub use backend::{
    backend_provider, registered_backend_names, unsupported_backend_diagnostic, BackendEmitRequest,
    BackendEmitResponse, BackendProvider,
};
pub use contract::{
    AnalyzeConfig, AnalyzeRequest, AnalyzeResponse, Diagnostic, DiagnosticLevel, DiagnosticSource,
    ExecutableModule, Import, InputManifest, IrDocument, JsExpr, JsObjectProp, JsStmt, JsValue,
    Module, Route,
};
pub use emit_go::{
    emit_backend, emit_go, EmitGoOutputKind, EmitGoRequest, EmitGoResponse, GeneratedFile,
    IrSnapshotRequest,
};
pub use runtime_contract::{
    fail_closed_report_version, runtime_contract, unsupported_codegen_diagnostic, ProgramPurpose,
    RuntimeContract, RuntimeOperation, RuntimeOperationOwner, RuntimeOperationStatus,
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
                                rest_param: None,
                                r#async: false,
                                generator: false,
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
    fn backend_registry_exposes_go_provider() {
        assert_eq!(registered_backend_names(), vec!["go"]);
        let provider = backend_provider("go").expect("go backend provider");
        assert_eq!(provider.name(), "go");
    }

    #[test]
    fn emit_backend_fails_closed_for_unregistered_backend() {
        let response = emit_backend(
            "rust",
            EmitGoRequest {
                analyze: AnalyzeRequest {
                    manifest: InputManifest {
                        entry: "src/index.js".to_string(),
                        framework: None,
                    },
                    cwd: None,
                    config: AnalyzeConfig::default(),
                },
                package_name: None,
                module_path: None,
                output_kind: EmitGoOutputKind::Main,
                ir_snapshot: None,
            },
        );

        assert_eq!(response.version, "engine-core.emit.v1");
        assert_eq!(response.target_backend, "rust");
        assert!(response.files.is_empty());
        assert_eq!(response.diagnostics.len(), 1);
        assert_eq!(response.diagnostics[0].code, "BACKEND_PROVIDER_UNSUPPORTED");
        assert!(response.diagnostics[0]
            .message
            .contains("available backends: go"));
    }

    #[test]
    fn runtime_contract_declares_backend_neutral_semantic_operations() {
        let contract = runtime_contract();
        assert_eq!(contract.version, "runtime-contract.v1");
        assert!(contract
            .operations
            .iter()
            .any(|operation| operation.key == "js.value-model"
                && operation.owner == RuntimeOperationOwner::Contract));
        assert!(contract
            .operations
            .iter()
            .any(|operation| operation.key == "node.process"
                && operation.owner == RuntimeOperationOwner::Contract));
        assert!(contract.node_builtins.contains(&"node:fs"));
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
        assert_eq!(response.files.len(), 4);
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
        assert_eq!(response.files[2].path, "go.sum");
        assert!(response.files[2]
            .contents
            .contains("github.com/dlclark/regexp2"));
        assert_eq!(response.files[3].path, "tsgodownrt/runtime.go");
        assert!(response.files[3].contents.contains("package tsgodownrt"));
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

        assert_eq!(response.files.len(), 4);
        assert_eq!(response.files[0].path, "vector_suite.go");
        assert!(response.files[0]
            .contents
            .starts_with("//go:build tsgodown_vector\n\npackage main"));
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
        assert!(response.files[0]
            .contents
            .starts_with("//go:build tsgodown_vector\n\npackage main"));
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
    fn emit_go_does_not_fail_closed_on_route_metadata_only_diagnostics() {
        let root = temp_project("engine-core-route-metadata-nonblocking");
        write(
            &root,
            "src/index.js",
            r#"
const app = {};
const route = makeRoute();
app.route(route);
console.log("still executable");
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
            module_path: Some("example.com/route-metadata-nonblocking".to_string()),
            output_kind: EmitGoOutputKind::Main,
            ir_snapshot: None,
        });

        assert!(response
            .diagnostics
            .iter()
            .any(|diagnostic| diagnostic.code == "ANALYZER_UNSUPPORTED_ROUTE_OBJECT_SHAPE"));
        assert!(!response
            .diagnostics
            .iter()
            .any(|diagnostic| diagnostic.code == "EXECUTABLE_JS_CODEGEN_NOT_IMPLEMENTED"));
        assert!(response.files[0].contents.contains("tsgodownrt.RunProgram"));
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
    fn emit_go_runs_node_style_process_argv_subset() {
        let root = temp_project("engine-core-process-argv");
        write(
            &root,
            "src/index.js",
            r#"
console.log(JSON.stringify({
  node: process.argv[0],
  entry: process.argv[1],
  first: process.argv[2],
  second: process.argv[3],
  argc: process.argv.length
}))
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
            module_path: Some("example.com/process-argv".to_string()),
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
            .args(["run", ".", "alpha", "beta"])
            .current_dir(&out_dir)
            .output()
            .expect("run generated go");
        assert!(
            output.status.success(),
            "go run failed: stdout={} stderr={}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );

        let observed: serde_json::Value =
            serde_json::from_slice(&output.stdout).expect("generated stdout JSON");
        assert!(observed["node"]
            .as_str()
            .is_some_and(|value| !value.is_empty()));
        assert_eq!(observed["entry"], "src/index.js");
        assert_eq!(observed["first"], "alpha");
        assert_eq!(observed["second"], "beta");
        assert_eq!(observed["argc"], 4);
    }

    #[test]
    fn emit_go_runs_runtime_object_error_json_and_child_eval_subset() {
        let root = temp_project("engine-core-runtime-object-error-json-child");
        write(
            &root,
            "src/index.js",
            r#"
import { spawnSync } from "node:child_process"

const assigned = Object.assign(/a/, { _src: "a" })
const globRe = Object.assign(new RegExp("^(?!\\.)[ai][^/]*?\\.ts$", "u"), {
  _src: "(?!\\.)[ai][^/]*?\\.ts",
  _glob: "[ai]*.ts"
})
const globStar = Symbol("globstar **")
const globPattern = ["src", globStar, globRe]
const globFile = "src/app.spec.ts".split("/")
const globTail = globPattern.slice(globPattern.lastIndexOf(globStar) + 1)
const globTailStart = globFile.length - globTail.length
function indexedGlobMatch(file, pattern, fileIndex, patternIndex) {
  let fi
  let pi
  let fl
  let pl
  for (
    fi = fileIndex,
    pi = patternIndex,
    fl = file.length,
    pl = pattern.length;
    fi < fl && pi < pl;
    fi++, pi++
  ) {
    return pattern[pi].test(file[fi])
  }
  return null
}
class MagicTracker {
  #hasMagic
  parse() {
    const [src, needUflag, consumed, magic] = ["[ai]", false, 4, true]
    this.#hasMagic = this.#hasMagic || magic
    return src === "[ai]" && needUflag === false && consumed === 4 && !!this.#hasMagic
  }
}
function parseSimpleClass(glob, position) {
  const braceEscape = (s) => s.replace(/[[\]\\-]/g, "\\$&")
  const regexpEscape = (s) => s.replace(/[-[\]{}()*+?.,\\^$|#\s]/g, "\\$&")
  const rangesToString = (ranges) => ranges.join("")
  const pos = position
  const ranges = []
  const negs = []
  let i = pos + 1
  let sawStart = false
  let escaping = false
  let negate = false
  let endPos = pos
  let rangeStart = ""
  while (i < glob.length) {
    const c = glob.charAt(i)
    if ((c === "!" || c === "^") && i === pos + 1) {
      negate = true
      i++
      continue
    }
    if (c === "]" && sawStart && !escaping) {
      endPos = i + 1
      break
    }
    sawStart = true
    escaping = false
    if (rangeStart) {
      if (c > rangeStart) ranges.push(braceEscape(rangeStart) + "-" + braceEscape(c))
      else if (c === rangeStart) ranges.push(braceEscape(c))
      rangeStart = ""
      i++
      continue
    }
    if (glob.startsWith("-]", i + 1)) {
      ranges.push(braceEscape(c + "-"))
      i += 2
      continue
    }
    if (glob.startsWith("-", i + 1)) {
      rangeStart = c
      i += 2
      continue
    }
    ranges.push(braceEscape(c))
    i++
  }
  if (endPos < i) return ["", false, 0, false]
  if (negs.length === 0 &&
    ranges.length === 1 &&
    /^\\?.$/.test(ranges[0]) &&
    !negate) {
    const r = ranges[0].length === 2 ? ranges[0].slice(-1) : ranges[0]
    return [regexpEscape(r), false, endPos - pos, false]
  }
  const comb = ranges.length ? "[" + rangesToString(ranges) + "]" : "[^" + rangesToString(negs) + "]"
  return [comb, false, endPos - pos, true]
}
const parsed = JSON.parse('{"empty":"","bool":true,"num":42}')
const err = TypeError("Invalid UUID")
const date = new Date(Date.UTC(2026, 4, 15))
const child = spawnSync(
  process.execPath,
  ["-e", 'console.log((process.env.TSGODOWN_VECTOR || "") + ":" + process.argv[1])', "argv-1"],
  { env: { TSGODOWN_VECTOR: "ok-1" }, encoding: "utf8" }
)

console.log(JSON.stringify({
  regexp: assigned.test("a") && assigned._src,
  glob: globRe.test("app.spec.ts"),
  globstar: globPattern.includes(globStar) &&
    globPattern.indexOf(globStar) === 1 &&
    globPattern.lastIndexOf(globStar) === 1 &&
    globTail[0].test(globFile[globTailStart]),
  forSequence: indexedGlobMatch(globFile, globTail, globTailStart, 0),
  privateMagic: new MagicTracker().parse(),
  parseClass: parseSimpleClass("[ai]*.ts", 0),
  keys: Object.keys(parsed),
  error: { name: err.name, message: err.message },
  date: date.toISOString(),
  child: child.stdout.trim()
}))
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
            module_path: Some("example.com/runtime-object-error-json-child".to_string()),
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

        let observed: serde_json::Value =
            serde_json::from_slice(&output.stdout).expect("generated stdout JSON");
        assert_eq!(observed["regexp"], "a");
        assert_eq!(observed["glob"], true);
        assert_eq!(observed["globstar"], true);
        assert_eq!(observed["forSequence"], true);
        assert_eq!(observed["privateMagic"], true);
        assert_eq!(
            observed["parseClass"],
            serde_json::json!(["[ai]", false, 4, true])
        );
        assert_eq!(
            observed["keys"],
            serde_json::json!(["empty", "bool", "num"])
        );
        assert_eq!(
            observed["error"],
            serde_json::json!({"name": "TypeError", "message": "Invalid UUID"})
        );
        assert_eq!(observed["date"], "2026-05-15T00:00:00.000Z");
        assert_eq!(observed["child"], "ok-1:argv-1");
    }

    #[test]
    fn emit_go_escapes_program_json_as_raw_string_literal() {
        let root = temp_project("engine-core-raw-program-json");
        write(
            &root,
            "src/index.js",
            r#"
console.log("escape", "\x1b[31m")
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
            module_path: Some("example.com/raw-program-json".to_string()),
            output_kind: EmitGoOutputKind::Main,
            ir_snapshot: None,
        });

        assert!(response.files[0]
            .contents
            .contains("tsgodownrt.RunProgram(`"));
        assert!(!response.files[0].contents.contains("\\x1b"));

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
            .args(["build", "."])
            .current_dir(&out_dir)
            .output()
            .expect("build generated go");
        assert!(
            output.status.success(),
            "go build failed: stdout={} stderr={}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
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
    fn emit_go_runs_binary_octal_hex_number_coercion_subset() {
        let root = temp_project("engine-core-js-number-prefixes");
        write(
            &root,
            "src/index.js",
            r#"
const fromLiteral = 0b001 + 0o10 + 0x10
const fromNumber = Number("0b101") + Number("0o7") + Number("0x20")
const empty = Number("")
console.log("numbers", fromLiteral, fromNumber, empty)
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
            module_path: Some("example.com/js-number-prefixes".to_string()),
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
        assert_eq!(String::from_utf8_lossy(&output.stdout), "numbers 25 44 0\n");
    }

    #[test]
    fn emit_go_runs_regexp_string_and_array_methods_subset() {
        let root = temp_project("engine-core-regexp-string-array-methods");
        write(
            &root,
            "src/index.js",
            r#"
const normalized = "  a   b  ".trim().replace(/\s+/g, "-")
const parts = "1.2.3".split(/\./).map((value) => Number(value))
const matched = "v1.2.3".match(/^v?(\d+)\.(\d+)\.(\d+)$/)
const replaced = "x1 y2".replace(/([a-z])(\d)/g, (_, letter, number) => letter.toUpperCase() + number)
const guarded = /^(?!bad)[a-z]+$/.test("good") + ":" + /^(?!bad)[a-z]+$/.test("bad")
const protoExec = RegExp.prototype.exec.call(/[a]+/, "baa")[0]
const boundExec = Function.prototype.bind.call(Function.prototype.call, RegExp.prototype.exec)
const boundSegment = boundExec(/(\[[^[\]]*])/g, "user[name]")
const globalScan = /(\[[^[\]]*])/g
const firstScan = globalScan.exec("user[name][role]")
const secondScan = globalScan.exec("user[name][role]")
const comparator = ">=0.0.0-0".match(/^((?:<|>)?=?)\s*(v?(0|[1-9]\d*)\.(0|[1-9]\d*)\.(0|[1-9]\d*)(?:-((?:\d*[a-zA-Z-][a-zA-Z0-9-]*|0|[1-9]\d*)(?:\.(?:\d*[a-zA-Z-][a-zA-Z0-9-]*|0|[1-9]\d*))*))?(?:\+([a-zA-Z0-9-]+(?:\.[a-zA-Z0-9-]+)*))?)$|^$/)
const safeComparator = ">=0.0.0-0".match(/^((?:<|>)?=?)\s{0,1}(v?(0|[1-9]\d{0,256})\.(0|[1-9]\d{0,256})\.(0|[1-9]\d{0,256})(?:-((?:\d{0,256}[a-zA-Z-][a-zA-Z0-9-]{0,250}|0|[1-9]\d{0,256})(?:\.(?:\d{0,256}[a-zA-Z-][a-zA-Z0-9-]{0,250}|0|[1-9]\d{0,256}))*))?(?:\+([a-zA-Z0-9-]{1,250}(?:\.[a-zA-Z0-9-]{1,250})*))?)$|^$/)
const dynamicWhitespace = new RegExp(`^a\\s+b$`).test("a b")
const protoSlice = String.prototype.slice.call("abcdef", 1, 4)
const filtered = [3, 1, 2].sort((a, b) => a - b).filter((value) => value > 1).join("|")
console.log("regex", normalized, parts.join("."), matched[1], replaced, guarded, protoExec, boundSegment[1].slice(1, -1), firstScan.index + ":" + firstScan[1] + ":" + secondScan.index + ":" + secondScan[1], comparator[1] + ":" + comparator[2] + ":" + comparator[6], safeComparator[1] + ":" + safeComparator[2] + ":" + safeComparator[6], dynamicWhitespace, protoSlice, filtered, /^[0-9]+$/.test("123"))
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
            module_path: Some("example.com/regexp-string-array-methods".to_string()),
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
            "regex a-b 1.2.3 1 X1 Y2 true:false aa name 4:[name]:10:[role] >=:0.0.0-0:0 >=:0.0.0-0:0 true bcd 2|3 true\n"
        );
    }

    #[test]
    fn emit_go_runs_array_iteration_search_and_slice_methods_subset() {
        let root = temp_project("engine-core-array-iteration-search-slice");
        write(
            &root,
            "src/index.js",
            r#"
const values = [1, 2, 3, 4]
const seen = []
values.forEach((value, index) => seen.push(value + index))
const queue = ["b", "c"]
function collect(...items) {
  return items.join(",")
}
queue.unshift("a")
const shifted = queue.shift()
const popped = queue.pop()
queue.push("d", "e")
const spliced = queue.splice(1, 1, "x", "y")
queue.push.apply(queue, ["z"])
const result = [
  values.reduce((acc, value) => acc + value, 0),
  ["a", "b", "c"].reduceRight((acc, value) => acc + value, ""),
  values.some((value) => value > 3),
  values.every((value) => value > 0),
  values.find((value) => value > 2),
  values.findIndex((value) => value > 2),
  values.indexOf(3),
  values.includes(5),
  values.concat([5], 6).slice(2, 5).join(":"),
  [1, [2, [3]]].flat(2).join(":"),
  shifted + popped + ":" + spliced.join("") + ":" + queue.join(""),
  collect("a", ...["b", "c"])
].join("|")
console.log("array-more", seen.join(","), result)
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
            module_path: Some("example.com/array-iteration-search-slice".to_string()),
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
            "array-more 1,3,5,7 10|cba|true|true|3|2|2|false|3:4:5|1:2:3|ac:d:bxyez|a,b,c\n"
        );
    }

    #[test]
    fn emit_go_runs_array_member_assignment_subset() {
        let root = temp_project("engine-core-array-member-assignment");
        write(
            &root,
            "src/index.js",
            r#"
const values = []
values[0] = "a"
values[2] = "c"
values[1] = "b"
values.length = 2
values[values.length] = "c"
values[1 + 3] = "e"
const nested = { items: [] }
nested.items[0] = values.join("")
const protoSlice = Array.prototype.slice.call(values, 1, 3).join("")
const popped = values.pop()
const applied = []
Array.prototype.push.apply(applied, ["x", "y"])
const keyedArray = ["m", "n"]
keyedArray.extra = "z"
const objectKeys = Object.keys({ user: 1, role: 2 }).join("|")
const arrayKeys = Object.keys(keyedArray).join("|")
const arrayHas = Object.prototype.hasOwnProperty.call(keyedArray, "extra")
console.log("array-assign", values.length, values.join(","), nested.items[0], protoSlice, popped, applied.join(""), objectKeys, arrayKeys, arrayHas, keyedArray.extra)
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
            module_path: Some("example.com/array-member-assignment".to_string()),
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
            "array-assign 4 a,b,c, abce bc e xy user|role 0|1|extra true z\n"
        );
    }

    #[test]
    fn emit_go_runs_recursive_array_push_apply_subset() {
        let root = temp_project("engine-core-recursive-array-push-apply");
        write(
            &root,
            "src/index.js",
            r#"
function pushToArray(arr, valueOrArray) {
  Array.prototype.push.apply(arr, Array.isArray(valueOrArray) ? valueOrArray : [valueOrArray])
}
function stringify(obj, prefix) {
  const keys = []
  const objKeys = Object.keys(obj)
  for (let j = 0; j < objKeys.length; ++j) {
    const key = objKeys[j]
    const value = obj[key]
    const nextPrefix = Array.isArray(obj) ? prefix + "[" + key + "]" : (prefix ? prefix + "[" + key + "]" : key)
    pushToArray(keys, typeof value === "object" ? stringify(value, nextPrefix) : [nextPrefix + "=" + value])
  }
  return keys
}
console.log("stringify-like", stringify({ user: { name: "kim", roles: ["admin", "ops"] } }, "").join("&"))
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
            module_path: Some("example.com/recursive-array-push-apply".to_string()),
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
            "stringify-like user[name]=kim&user[roles][0]=admin&user[roles][1]=ops\n"
        );
    }

    #[test]
    fn emit_go_runs_object_in_and_option_normalization_subset() {
        let root = temp_project("engine-core-object-in-option-normalization");
        write(
            &root,
            "src/index.js",
            r#"
const arrayPrefixGenerators = {
  brackets: function brackets(prefix) { return prefix + "[]" },
  comma: "comma",
  indices: function indices(prefix, key) { return prefix + "[" + key + "]" },
  repeat: function repeat(prefix) { return prefix }
}
const defaults = { arrayFormat: "indices", indices: false }
function normalize(opts) {
  let arrayFormat
  if (opts.arrayFormat in arrayPrefixGenerators) {
    arrayFormat = opts.arrayFormat
  } else if ("indices" in opts) {
    arrayFormat = opts.indices ? "indices" : "repeat"
  } else {
    arrayFormat = defaults.arrayFormat
  }
  return {
    arrayFormat: arrayFormat,
    generator: arrayPrefixGenerators[arrayFormat]
  }
}
const repeated = normalize({ arrayFormat: "repeat" })
const defaulted = normalize({ encodeValuesOnly: true })
const weak = new WeakMap()
const weakKey = {}
WeakMap.prototype.set.call(weak, weakKey, 7)
console.log(
  "normalize",
  repeated.arrayFormat,
  repeated.generator("tag", "0"),
  repeated.generator === "comma",
  defaulted.arrayFormat,
  defaulted.generator("tag", "0"),
  defaulted.generator === "comma",
  "arrayFormat" in { arrayFormat: "repeat" },
  "missing" in { arrayFormat: "repeat" },
  "prototype" in String,
  "indexOf" in String.prototype,
  "prototype" in WeakMap,
  WeakMap.prototype.get.call(weak, weakKey)
)
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
            module_path: Some("example.com/object-in-option-normalization".to_string()),
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
            "normalize repeat tag false indices tag[0] false true false true true true 7\n"
        );
    }

    #[test]
    fn emit_go_runs_string_search_and_slice_methods_subset() {
        let root = temp_project("engine-core-string-search-slice");
        write(
            &root,
            "src/index.js",
            r#"
const trimmed = "  Alpha-Beta  ".trimStart().trimEnd()
const result = [
  trimmed.charAt(0),
  trimmed.charCodeAt(0),
  trimmed.slice(0, 5),
  trimmed.substring(6, 10),
  trimmed.substr(6, 4),
  trimmed.indexOf("B"),
  trimmed.indexOf("a", 6),
  trimmed.lastIndexOf("a"),
  trimmed.includes("pha"),
  trimmed.startsWith("Al"),
  trimmed.endsWith("eta"),
  "x".repeat(3),
  "a_a".replaceAll("_", "-")
].join("|")
console.log("string-more", result)
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
            module_path: Some("example.com/string-search-slice".to_string()),
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
            "string-more A|65|Alpha|Beta|Beta|6|9|9|true|true|true|xxx|a-a\n"
        );
    }

    #[test]
    fn emit_go_runs_global_object_array_math_builtins_subset() {
        let root = temp_project("engine-core-global-builtins");
        write(
            &root,
            "src/index.js",
            r#"
const target = Object.assign(Object.create(null), { b: 2 }, { a: 1 })
const keys = Object.keys(target).sort().join(",")
const entries = Object.entries(target).map((entry) => entry.join(":")).sort().join("|")
const order = Object.keys({ name: "x", enabled: true, count: 2 }).join(",")
const has = Object.prototype.hasOwnProperty.call(target, "a")
const tag = [Object.prototype.toString.call(/x/), Object.prototype.toString.call("x"), Object.prototype.toString.call(1), Object.prototype.toString.call(false)].join(",")
const isArray = Array.isArray([1])
const bools = [Boolean(0), Boolean("x")].join(",")
const numbers = [Number.isFinite(3), Number.isInteger(3.2), Number.isSafeInteger(Math.floor(parseFloat("42")))].join(",")
const finite = [isFinite("3"), isFinite("no")].join(",")
const fromFill = Array.from({ length: 3 }).fill("x", 1).join(",")
const typed = new Uint8Array(2)
typed[1] = 7
console.log("builtins", String(12), keys, entries, order, has, tag, isArray, Math.min(3, 1), Math.max(3, 1), Math.floor(1.9), bools, numbers, finite, fromFill, typed.length, typed[1], typeof Date.now())
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
            module_path: Some("example.com/global-builtins".to_string()),
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
            "builtins 12 a,b a:1|b:2 name,enabled,count true [object RegExp],[object String],[object Number],[object Boolean] true 1 3 1 false,true true,false,true true,false ,x,x 2 7 number\n"
        );
    }

    #[test]
    fn emit_go_runs_symbol_math_and_number_methods_subset() {
        let root = temp_project("engine-core-symbol-math-number");
        write(
            &root,
            "src/index.js",
            r#"
const symbol = Symbol("x")
const integer = (255).toString(16)
const decimal = Math.random().toString(36).slice(0, 2)
const math = [Math.ceil(1.2), Math.round(1.6), Math.trunc(1.9), Math.abs(-3), Math.pow(2, 3)].join(",")
console.log("symbol-math", typeof Symbol, typeof symbol, symbol.toString(), Symbol.prototype.toString.call(Symbol.iterator), integer, decimal.length, math)
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
            module_path: Some("example.com/symbol-math-number".to_string()),
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
            "symbol-math function symbol Symbol(x) Symbol(Symbol.iterator) ff 2 2,2,1,3,8\n"
        );
    }

    #[test]
    fn emit_go_runs_error_subclass_constructors_subset() {
        let root = temp_project("engine-core-error-subclasses");
        write(
            &root,
            "src/index.js",
            r#"
try {
  throw new TypeError("bad")
} catch (error) {
  console.log("error-subclass", error.name, error.message, new RangeError("range").name)
}
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
            module_path: Some("example.com/error-subclasses".to_string()),
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
            "error-subclass TypeError bad RangeError\n"
        );
    }

    #[test]
    fn emit_go_runs_map_set_iterator_subset() {
        let root = temp_project("engine-core-map-set-iterator");
        write(
            &root,
            "src/index.js",
            r#"
const map = new Map()
map.set("b", 2).set("a", 1)
const firstKey = map.keys().next().value
const values = [...map.values()].join("|")
map.delete("b")
const set = new Set(["x", "x"])
set.add("y")
console.log("collections", firstKey, values, map.has("a"), map.size, [...set.values()].join("|"), set.size)
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
            module_path: Some("example.com/map-set-iterator".to_string()),
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
            "collections b 2|1 true 1 x|y 2\n"
        );
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
import parser from "./parser.js";
console.log("imported", value + 2, parser(["x"]).kind, parser.moduleKind)
"#,
        );
        write(&root, "src/value.js", "export const value = 40;");
        write(
            &root,
            "src/parser.js",
            r#"
const parser = function Parser(args) {
  return { kind: args[0] }
}
parser.moduleKind = "esm-default"
export default parser
export { parser as "module.exports" }
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
        assert_eq!(
            String::from_utf8_lossy(&output.stdout),
            "imported 42 x esm-default\n"
        );
    }

    #[test]
    fn emit_go_runs_commonjs_function_exports_subset() {
        let root = temp_project("engine-core-cjs-function-export");
        write(
            &root,
            "src/index.js",
            r#"
const add = require("./add.js")
console.log("cjs", add(2, 4))
"#,
        );
        write(
            &root,
            "src/add.js",
            r#"
module.exports = function add(left, right) {
  return left + right
}
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
            module_path: Some("example.com/cjs-function-export".to_string()),
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
        assert_eq!(String::from_utf8_lossy(&output.stdout), "cjs 6\n");
    }

    #[test]
    fn emit_go_runs_commonjs_circular_mid_export_subset() {
        let root = temp_project("engine-core-cjs-circular-mid-export");
        write(
            &root,
            "src/index.js",
            r#"
const make = require("./a.js")
console.log("cycle", new make("ok").value)
"#,
        );
        write(
            &root,
            "src/a.js",
            r#"
function Make(value) {
  this.value = value
}
module.exports = Make
require("./b.js")
"#,
        );
        write(
            &root,
            "src/b.js",
            r#"
const Make = require("./a.js")
module.exports = new Make("side")
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
            module_path: Some("example.com/cjs-circular-mid-export".to_string()),
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
        assert_eq!(String::from_utf8_lossy(&output.stdout), "cycle ok\n");
    }

    #[test]
    fn emit_go_runs_function_constructor_subset() {
        let root = temp_project("engine-core-function-constructor");
        write(
            &root,
            "src/index.js",
            r#"
function Box(value) {
  this.value = value
}
const box = new Box(7)
console.log("ctor", box.value)
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
            module_path: Some("example.com/function-constructor".to_string()),
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
        assert_eq!(String::from_utf8_lossy(&output.stdout), "ctor 7\n");
    }

    #[test]
    fn emit_go_runs_array_destructuring_subset() {
        let root = temp_project("engine-core-array-destructuring");
        write(
            &root,
            "src/index.js",
            r#"
const [, , corpus, vectorPath] = ["node", "entry", "semver", "vectors.json"];
const rows = [["index.js", "*.js", { dot: true }], ["index.ts", "*.js"]];
const mapped = rows.map(([path, pattern, options = {}]) => path + ":" + pattern + ":" + (options.dot === true));
const flag = (({ windowsPathsNoEscape = false } = {}) => windowsPathsNoEscape)();
console.log("args", corpus, vectorPath, mapped[0], mapped[1], flag)
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
            "args semver vectors.json index.js:*.js:true index.ts:*.js:false false\n"
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
count |= 2
count *= 3
const mask = (5 & 3) | (1 << 4)
const power = 2 ** 5
const shifted = -8 >> 1
const unsigned = -1 >>> 31
console.log("ops", before, after, count, fallback, mask, power, shifted, unsigned, ~0, void count)
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
            "ops 1 3 30 set 17 32 -4 1 -1 undefined\n"
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

    #[test]
    fn emit_go_runs_array_spread_subset() {
        let root = temp_project("engine-core-array-spread");
        write(
            &root,
            "src/index.js",
            r#"
const base = [1, 2]
const values = [0, ...base, 3]
let total = 0
for (const value of values) {
  total += value
}
console.log("spread", values.length, total)
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
            module_path: Some("example.com/array-spread".to_string()),
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
        assert_eq!(String::from_utf8_lossy(&output.stdout), "spread 4 6\n");
    }

    #[test]
    fn emit_go_runs_function_expression_subset() {
        let root = temp_project("engine-core-function-expression");
        write(
            &root,
            "src/index.js",
            r#"
const factor = 3
const multiply = function (value) {
  return value * factor
}
const add = (left, right) => left + right
const immediate = (function (value) {
  return value - 2
})(11)
console.log("fnexpr", multiply(4), add(5, 7), immediate)
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
            module_path: Some("example.com/function-expression".to_string()),
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
        assert_eq!(String::from_utf8_lossy(&output.stdout), "fnexpr 12 12 9\n");
    }

    #[test]
    fn emit_go_runs_function_object_properties_subset() {
        let root = temp_project("engine-core-function-object-properties");
        write(
            &root,
            "src/index.js",
            r#"
function tag(value) {
  return tag.prefix + value
}
function Box(value) {
  this.value = value
}
tag.prefix = "id:"
tag.count = 1
tag.count += 2
tag.prototype.kind = "tag"
Box.prototype.read = function () {
  return this.value
}
const box = new Box("ok")
const child = Object.create({ inherited: "yes" })
const sized = new Array(3)
sized[1] = "x"
console.log("fn-props", tag("7"), tag.count, typeof tag.prototype, tag.prototype.kind, box.read(), child.inherited, "inherited" in child, box instanceof Box, child instanceof Box, sized.length, sized[1])
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
            module_path: Some("example.com/function-object-properties".to_string()),
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
            "fn-props id:7 3 object tag ok yes true true false 3 x\n"
        );
    }

    #[test]
    fn emit_go_runs_function_reflect_json_uri_intrinsics_subset() {
        let root = temp_project("engine-core-function-reflect-json-uri");
        write(
            &root,
            "src/index.js",
            r#"
function sum(left, right) {
  return this.base + left + right
}
const direct = Function.prototype.call.call(sum, { base: 1 }, 2, 3)
const applied = Reflect.apply(sum, { base: 4 }, [5, 6])
const bound = sum.bind({ base: 7 }, 8)
function mapWithThis() {
  return this.values.map(value => this.base + value).join(",")
}
const lexicalThis = mapWithThis.call({ base: 10, values: [1, 2] })
const re = /x/
Reflect.defineProperty(re, "test", { value: function (value) { return value === "ok" } })
const encoded = encodeURIComponent("a b/[]")
const decoded = decodeURIComponent(encoded)
const json = JSON.stringify({ direct, applied, bound: bound(9), lexicalThis, ok: re.test("ok"), decoded, amp: "a&b" })
console.log("intrinsics", json)
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
            module_path: Some("example.com/function-reflect-json-uri".to_string()),
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
            "intrinsics {\"amp\":\"a&b\",\"applied\":15,\"bound\":24,\"decoded\":\"a b/[]\",\"direct\":6,\"lexicalThis\":\"11,12\",\"ok\":true}\n"
        );
    }

    #[test]
    fn emit_go_runs_function_declaration_hoisting_subset() {
        let root = temp_project("engine-core-function-hoisting");
        write(
            &root,
            "src/index.js",
            r#"
const top = before("top")
function before(value) {
  return wrap(value)
  function wrap(inner) {
    return "wrapped:" + inner
  }
}
console.log("hoist", top, later())
function later() {
  return "late"
}
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
            module_path: Some("example.com/function-hoisting".to_string()),
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
            "hoist wrapped:top late\n"
        );
    }

    #[test]
    fn emit_go_runs_delete_and_subtract_assign_subset() {
        let root = temp_project("engine-core-delete-subassign");
        write(
            &root,
            "src/index.js",
            r#"
const target = { keep: 1, drop: 2, alias: 3 }
let value = 10
value -= 3
const deleted = delete target.drop
const alias = "alias"
delete target[alias]
console.log("delete", value, deleted, "drop" in target, "keep" in target, "alias" in target)
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
            module_path: Some("example.com/delete-subassign".to_string()),
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
            "delete 7 true false true false\n"
        );
    }

    #[test]
    fn emit_go_runs_try_catch_finally_and_throw_subset() {
        let root = temp_project("engine-core-try-throw");
        write(
            &root,
            "src/index.js",
            r#"
let cleanup = 0
function risky(value) {
  try {
    if (value > 2) {
      throw "too-big"
    }
    return value
  } catch (error) {
    return `caught:${error}`
  } finally {
    cleanup += 1
  }
}
function explode() {
  throw "wrapped"
}
let wrapped = "none"
try {
  explode()
} catch (error) {
  wrapped = `wrapped:${error}`
}
console.log("try", risky(1), risky(3), cleanup, wrapped)
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
            module_path: Some("example.com/try-throw".to_string()),
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
            "try 1 caught:too-big 2 wrapped:wrapped\n"
        );
    }

    #[test]
    fn emit_go_runs_basic_class_new_instanceof_subset() {
        let root = temp_project("engine-core-class-new");
        write(
            &root,
            "src/index.js",
            r#"
class Counter {
  static get ANY() {
    return "*"
  }

  constructor(start) {
    if (start instanceof Counter) {
      return start
    }
    this.value = start
  }

  get current() {
    return this.value
  }

  inc(step) {
    this.value += step
    return this.value
  }
}

class DerivedCounter extends Counter {}
class ArrayChild extends Array {}
class CustomError extends Error {}
class PrivateCounter {
  #value = 1
  #bump(step = 1) {
    this.#value += step
  }
  static #seed() {
    return 4
  }
  constructor(start = 2) {
    this.#value = start
  }
  next() {
    this.#bump()
    return this.#value
  }
  static read() {
    return this.#seed()
  }
}
class StaticPrivate {
  static #open = false
  static open() {
    this.#open = true
  }
  static read() {
    return this.#open
  }
}
const counter = new Counter(2)
const reused = new Counter(counter)
const derived = new DerivedCounter(4)
const arrayChild = new ArrayChild(2)
arrayChild.fill("q")
const error = new CustomError("boom")
const privateCounter = new PrivateCounter()
StaticPrivate.open()
console.log("class", counter.inc(3), counter.current, Counter.ANY, derived.inc(1), derived instanceof Counter, typeof ArrayChild, arrayChild.length, arrayChild.join(""), error instanceof Error, error.message, reused === counter, reused.current, privateCounter.next(), PrivateCounter.read(), StaticPrivate.read())
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
            module_path: Some("example.com/class-new".to_string()),
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
            "class 5 5 * 5 true function 2 qq true boom true 5 3 4 true\n"
        );
    }

    #[test]
    fn emit_go_runs_builtin_util_import_subset() {
        let root = temp_project("engine-core-util-import");
        write(
            &root,
            "src/index.js",
            r#"
import { format } from "util"
const util = require("util")
console.log("util", format("name:%s count:%d", "items", 3), util.inspect("quoted"))
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
            module_path: Some("example.com/util-import".to_string()),
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
            "util name:items count:3 \"quoted\"\n"
        );
    }

    #[test]
    fn emit_go_runs_builtin_path_os_import_subset() {
        let root = temp_project("engine-core-path-os-import");
        write(
            &root,
            "src/index.js",
            r#"
import { basename, dirname, join, resolve } from "path"
const os = require("os")
console.log("path", basename("/tmp/app.txt", ".txt"), dirname("/tmp/app.txt"), join("a", "b", "..", "c"), resolve("/tmp", "file"), typeof os.homedir())
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
            module_path: Some("example.com/path-os-import".to_string()),
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
            "path app /tmp a/c /tmp/file string\n"
        );
    }

    #[test]
    fn emit_go_runs_builtin_assert_import_subset() {
        let root = temp_project("engine-core-assert-import");
        write(
            &root,
            "src/index.js",
            r#"
const assert = require("assert")
assert.equal(1, "1")
assert.strictEqual("ok", "ok")
assert.deepStrictEqual({value: 1}, {value: 1})
console.log("assert-ok")
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
            module_path: Some("example.com/assert-import".to_string()),
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
        assert_eq!(String::from_utf8_lossy(&output.stdout), "assert-ok\n");
    }

    #[test]
    fn emit_go_runs_builtin_crypto_buffer_import_subset() {
        let root = temp_project("engine-core-crypto-buffer-import");
        write(
            &root,
            "src/index.js",
            r#"
import { createHash, randomFillSync, randomUUID } from "node:crypto"
const crypto = require("crypto")
const md5 = createHash("md5").update(Buffer.from("abc", "utf8")).digest("hex")
const sha1 = crypto.createHash("sha1").update("abc").digest("hex")
const bytes = Uint8Array.of(1, 2, 255)
randomFillSync(bytes)
const randomBytesOk = bytes.length === 3 && bytes.every((byte) => Number.isInteger(byte) && byte >= 0 && byte <= 255)
const uuidOk = /^[0-9a-f]{8}-[0-9a-f]{4}-4[0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}$/.test(randomUUID())
console.log("crypto", md5, sha1, Buffer.from("ff", "hex")[0], randomBytesOk, uuidOk)
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
            module_path: Some("example.com/crypto-buffer-import".to_string()),
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
            "crypto 900150983cd24fb0d6963f7d28e17f72 a9993e364706816aba3e25717850c26c9cd0d89d 255 true true\n"
        );
    }

    #[test]
    fn emit_go_runs_dynamic_import_thenable_subset() {
        let root = temp_project("engine-core-dynamic-import");
        write(
            &root,
            "src/index.js",
            r#"
let value = "initial"
import("node:diagnostics_channel")
  .then((dc) => {
    value = "loaded"
  })
  .catch(() => {
    value = "failed"
  })
console.log("dynamic-import", value)
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
            module_path: Some("example.com/dynamic-import".to_string()),
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
            "dynamic-import loaded\n"
        );
    }

    #[test]
    fn emit_go_runs_static_package_dynamic_import_subset() {
        let root = temp_project("engine-core-package-dynamic-import");
        write(
            &root,
            "src/index.js",
            r#"
async function main() {
  const mod = await import("pkg-dyn")
  console.log("dynamic-package", mod.value)
}
main()
"#,
        );
        write(
            &root,
            "node_modules/pkg-dyn/package.json",
            r#"{ "name": "pkg-dyn", "main": "index.js" }"#,
        );
        write(
            &root,
            "node_modules/pkg-dyn/index.js",
            r#"
export const value = "ok"
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
            module_path: Some("example.com/package-dynamic-import".to_string()),
            output_kind: EmitGoOutputKind::Main,
            ir_snapshot: None,
        });

        assert!(!response
            .diagnostics
            .iter()
            .any(|diagnostic| diagnostic.code == "DYNAMIC_IMPORT_DETECTED"));
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
            "dynamic-package ok\n"
        );
    }

    #[test]
    fn emit_go_runs_fs_module_process_subset() {
        let root = temp_project("engine-core-fs-module-process");
        write(&root, "src/data.txt", "alpha");
        write(
            &root,
            "src/index.js",
            r#"
import { readFileSync } from "fs"
import { createRequire } from "node:module"
const require = createRequire(import.meta.url)
const fs = require("fs")
const left = readFileSync("src/data.txt", "utf8")
const right = fs.readFileSync("src/data.txt", "utf8")
console.log("fs-module", Number("20") >= 20, typeof process.cwd(), left, right)
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
            module_path: Some("example.com/fs-module-process".to_string()),
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
        write(&out_dir, "src/data.txt", "alpha");

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
            "fs-module true string alpha alpha\n"
        );
    }

    #[test]
    fn emit_go_runs_callback_promise_await_subset() {
        let root = temp_project("engine-core-callback-promise-await");
        write(
            &root,
            "src/index.js",
            r#"
function fromCallback(fn) {
  return function (...args) {
    return new Promise((resolve, reject) => {
      args.push((err, res) => err != null ? reject(err) : resolve(res))
      fn.apply(this, args)
    })
  }
}
const read = (file, options, callback) => callback(null, "ok:" + file)
async function main() {
  const value = await fromCallback(read)("file", {})
  console.log("callback-promise", value)
}
main()
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
            module_path: Some("example.com/callback-promise-await".to_string()),
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
            "callback-promise ok:file\n"
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
