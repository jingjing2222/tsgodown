mod analyze;
mod backend;
mod backends;
mod contract;
mod emit_go;
mod go_aot;
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
                    config: legacy_ir_interpreter_config(),
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
                config: legacy_ir_interpreter_config(),
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
                config: legacy_ir_interpreter_config(),
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
        assert!(!response.files[0].contents.contains("tsgodownrt.RunProgram"));
        assert!(response.files[0].contents.contains("fmt.Println"));
        assert!(!response.files[0]
            .contents
            .contains(fail_closed_report_version(ProgramPurpose::VectorSuite)));
    }

    #[test]
    fn emit_go_fails_closed_when_aot_cannot_render_route_metadata_program() {
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
        assert!(response
            .diagnostics
            .iter()
            .any(|diagnostic| diagnostic.code == "EXECUTABLE_JS_CODEGEN_NOT_IMPLEMENTED"));
        assert!(response.files[0]
            .contents
            .contains(fail_closed_report_version(ProgramPurpose::Main)));
        assert!(!response.files[0].contents.contains("tsgodownrt.RunProgram"));
        let runtime = response
            .files
            .iter()
            .find(|file| file.path == "tsgodownrt/runtime.go")
            .expect("runtime file");
        assert!(!runtime.contents.contains("func RunProgram"));
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
                config: legacy_ir_interpreter_config(),
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
        assert!(snapshot.contents.contains("const sourceIRJSON = \""));
        assert!(snapshot
            .contents
            .contains("\\\"entry\\\": \\\"src/server.ts\\\""));
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
        assert!(!response.files[0].contents.contains("tsgodownrt.RunProgram"));
        assert!(response.files[0].contents.contains("fmt.Println"));

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
  argc: process.argv.length,
  tail: process.argv.slice(2)
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
                config: legacy_ir_interpreter_config(),
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
        assert!(!response.files[0].contents.contains("tsgodownrt.RunProgram"));
        assert!(response.files[0].contents.contains("tsgodownProcessArgv"));

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
        assert_eq!(observed["tail"], serde_json::json!(["alpha", "beta"]));
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
let parseError = ""
try {
  JSON.parse("<html>")
} catch (error) {
  parseError = error.name
}
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
  parseError,
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
                config: legacy_ir_interpreter_config(),
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
        assert_eq!(observed["parseError"], "SyntaxError");
        assert_eq!(
            observed["error"],
            serde_json::json!({"name": "TypeError", "message": "Invalid UUID"})
        );
        assert_eq!(observed["date"], "2026-05-15T00:00:00.000Z");
        assert_eq!(observed["child"], "ok-1:argv-1");
    }

    #[test]
    fn emit_go_escapes_program_json_as_go_string_literal() {
        let root = temp_project("engine-core-string-program-json");
        write(
            &root,
            "src/index.js",
            "\u{feff}\nconsole.log(\"escape\", \"\u{feff}\", \"\\x1b[31m\", \"`\", \"한글\")\n",
        );

        let response = emit_go(EmitGoRequest {
            analyze: AnalyzeRequest {
                manifest: InputManifest {
                    entry: "src/index.js".to_string(),
                    framework: None,
                },
                cwd: Some(root.to_string_lossy().to_string()),
                config: legacy_ir_interpreter_config(),
            },
            package_name: None,
            module_path: Some("example.com/string-program-json".to_string()),
            output_kind: EmitGoOutputKind::Main,
            ir_snapshot: None,
        });

        assert!(!response.files[0].contents.contains("tsgodownrt.RunProgram"));
        assert!(response.files[0].contents.contains("\\uFEFF"));
        assert!(response.files[0].contents.contains("\\uD55C\\uAE00"));
        assert!(!response.files[0].contents.contains('\u{feff}'));

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
                config: legacy_ir_interpreter_config(),
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
        assert!(!response.files[0].contents.contains("tsgodownrt.RunProgram"));
        assert!(response.files[0].contents.contains("func add("));

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
    fn emit_go_runs_aot_named_esm_function_import_subset() {
        let root = temp_project("engine-core-aot-esm-function-import");
        write(
            &root,
            "src/index.js",
            r#"
import { score } from "./score.js"
const value = score(2, 3)
console.log("aot-esm", value)
"#,
        );
        write(
            &root,
            "src/score.js",
            r#"
export function score(left, right) {
  return left * 10 + right
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
                config: legacy_ir_interpreter_config(),
            },
            package_name: None,
            module_path: Some("example.com/aot-esm-function-import".to_string()),
            output_kind: EmitGoOutputKind::Main,
            ir_snapshot: None,
        });

        assert!(!response
            .diagnostics
            .iter()
            .any(|diagnostic| diagnostic.code == "EXECUTABLE_JS_CODEGEN_NOT_IMPLEMENTED"));
        assert!(!response.files[0].contents.contains("tsgodownrt.RunProgram"));
        assert!(response.files[0].contents.contains("src_score_js_score"));

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
        assert_eq!(String::from_utf8_lossy(&output.stdout), "aot-esm 23\n");
    }

    #[test]
    fn emit_go_runs_aot_function_expression_binding_subset() {
        let root = temp_project("engine-core-aot-function-expression-binding");
        write(
            &root,
            "src/index.js",
            r#"
const score = (left, right) => {
  return left * 10 + right
}
console.log("aot-fn-expr", score(2, 3))
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
            module_path: Some("example.com/aot-function-expression-binding".to_string()),
            output_kind: EmitGoOutputKind::Main,
            ir_snapshot: None,
        });

        assert!(!response
            .diagnostics
            .iter()
            .any(|diagnostic| diagnostic.code == "EXECUTABLE_JS_CODEGEN_NOT_IMPLEMENTED"));
        assert!(!response.files[0].contents.contains("tsgodownrt.RunProgram"));
        assert!(response.files[0].contents.contains("func score"));

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
        assert_eq!(String::from_utf8_lossy(&output.stdout), "aot-fn-expr 23\n");
    }

    #[test]
    fn emit_go_runs_aot_named_esm_function_expression_import_subset() {
        let root = temp_project("engine-core-aot-esm-function-expression-import");
        write(
            &root,
            "src/index.js",
            r#"
import { score } from "./score.js"
console.log("aot-esm-fn-expr", score(2, 3))
"#,
        );
        write(
            &root,
            "src/score.js",
            r#"
export const score = (left, right) => {
  return left * 10 + right
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
            module_path: Some("example.com/aot-esm-function-expression-import".to_string()),
            output_kind: EmitGoOutputKind::Main,
            ir_snapshot: None,
        });

        assert!(!response
            .diagnostics
            .iter()
            .any(|diagnostic| diagnostic.code == "EXECUTABLE_JS_CODEGEN_NOT_IMPLEMENTED"));
        assert!(!response.files[0].contents.contains("tsgodownrt.RunProgram"));
        assert!(response.files[0].contents.contains("src_score_js_score"));

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
            "aot-esm-fn-expr 23\n"
        );
    }

    #[test]
    fn emit_go_runs_aot_esm_default_alias_function_import_subset() {
        let root = temp_project("engine-core-aot-esm-default-alias-function");
        write(
            &root,
            "src/index.js",
            r#"
import score from "./score.js"
console.log("aot-esm-default-alias", score(2, 3))
"#,
        );
        write(
            &root,
            "src/score.js",
            r#"
const score = (left, right) => {
  return left * 10 + right
}
export default score
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
            module_path: Some("example.com/aot-esm-default-alias-function".to_string()),
            output_kind: EmitGoOutputKind::Main,
            ir_snapshot: None,
        });

        assert!(!response
            .diagnostics
            .iter()
            .any(|diagnostic| diagnostic.code == "EXECUTABLE_JS_CODEGEN_NOT_IMPLEMENTED"));
        assert!(!response.files[0].contents.contains("tsgodownrt.RunProgram"));
        assert!(response.files[0].contents.contains("src_score_js_score"));

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
            "aot-esm-default-alias 23\n"
        );
    }

    #[test]
    fn emit_go_runs_aot_named_esm_value_import_subset() {
        let root = temp_project("engine-core-aot-esm-value-import");
        write(
            &root,
            "src/index.js",
            r#"
import { value, label, enabled } from "./config.js"
console.log("aot-esm-value", value + 2, label + "-next", enabled)
"#,
        );
        write(
            &root,
            "src/config.js",
            r#"
export const value = 40
export const label = "config"
export const enabled = true
"#,
        );

        let response = emit_go(EmitGoRequest {
            analyze: AnalyzeRequest {
                manifest: InputManifest {
                    entry: "src/index.js".to_string(),
                    framework: None,
                },
                cwd: Some(root.to_string_lossy().to_string()),
                config: legacy_ir_interpreter_config(),
            },
            package_name: None,
            module_path: Some("example.com/aot-esm-value-import".to_string()),
            output_kind: EmitGoOutputKind::Main,
            ir_snapshot: None,
        });

        assert!(!response
            .diagnostics
            .iter()
            .any(|diagnostic| diagnostic.code == "EXECUTABLE_JS_CODEGEN_NOT_IMPLEMENTED"));
        assert!(!response.files[0].contents.contains("tsgodownrt.RunProgram"));
        assert!(response.files[0].contents.contains("src_config_js_value"));

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
            "aot-esm-value 42 config-next true\n"
        );
    }

    #[test]
    fn emit_go_runs_aot_unused_node_builtin_import_subset() {
        let root = temp_project("engine-core-aot-unused-node-builtin");
        write(
            &root,
            "src/index.js",
            r#"
import { randomUUID } from "node:crypto"
import diagnosticsChannel from "node:diagnostics_channel"
import { StringDecoder } from "node:string_decoder"
import tty from "node:tty"
import v8 from "node:v8"
const fs = require("fs")
const constants = require("constants")
console.log("aot-builtin-import", "unused", Boolean(fs.stat), typeof fs.closeSync === "function")
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
            module_path: Some("example.com/aot-unused-node-builtin".to_string()),
            output_kind: EmitGoOutputKind::Main,
            ir_snapshot: None,
        });

        assert!(!response
            .diagnostics
            .iter()
            .any(|diagnostic| diagnostic.code == "EXECUTABLE_JS_CODEGEN_NOT_IMPLEMENTED"));
        assert!(!response.files[0].contents.contains("tsgodownrt.RunProgram"));

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
            "aot-builtin-import unused true true\n"
        );
    }

    #[test]
    fn emit_go_reports_aot_unsupported_feature_details() {
        let root = temp_project("engine-core-aot-unsupported-details");
        write(
            &root,
            "src/index.js",
            r#"
class Box {
  static make() {
    return new Box()
  }
}
console.log("unsupported", Box.make())
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
            module_path: Some("example.com/aot-unsupported-details".to_string()),
            output_kind: EmitGoOutputKind::Main,
            ir_snapshot: None,
        });

        assert!(response.diagnostics.iter().any(|diagnostic| diagnostic.code
            == "EXECUTABLE_JS_CODEGEN_NOT_IMPLEMENTED"
            && diagnostic
                .message
                .contains("aot.class.unsupported:src/index.js:Box")));
        assert!(!response.files[0].contents.contains("tsgodownrt.RunProgram"));
    }

    #[test]
    fn emit_go_does_not_report_shadowed_builtin_names_as_node_builtins() {
        let root = temp_project("engine-core-aot-shadowed-builtin-name");
        write(
            &root,
            "src/index.js",
            r#"
function acceptsPath(path) {
  return path.match(/x/)
}
function localPath(input) {
  const path = input
  return path.split(":")
}
function localFs(fn) {
  const fs = fn
  return fs.access("x")
}
console.log(acceptsPath("x"))
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
            module_path: Some("example.com/aot-shadowed-builtin-name".to_string()),
            output_kind: EmitGoOutputKind::Main,
            ir_snapshot: None,
        });

        let message = response
            .diagnostics
            .iter()
            .find(|diagnostic| diagnostic.code == "EXECUTABLE_JS_CODEGEN_NOT_IMPLEMENTED")
            .map(|diagnostic| diagnostic.message.as_str())
            .expect("expected fail-closed diagnostic");
        assert!(message.contains("aot.function.unsupported_body:src/index.js:acceptsPath"));
        assert!(!message.contains("aot.node.builtin_operation:path.match"));
        assert!(!message.contains("aot.node.builtin_property:path.match"));
        assert!(!message.contains("aot.node.builtin_operation:path.split"));
        assert!(!message.contains("aot.node.builtin_property:path.split"));
        assert!(!message.contains("aot.node.builtin_operation:fs.access"));
        assert!(!message.contains("aot.node.builtin_property:fs.access"));
        assert!(!response.files[0].contents.contains("tsgodownrt.RunProgram"));
    }

    #[test]
    fn emit_go_reports_aot_unsupported_function_body_details() {
        let root = temp_project("engine-core-aot-unsupported-function-details");
        write(
            &root,
            "src/index.js",
            r#"
function collect(items) {
  const result = []
  for (const item of items) {
    result.push(item)
  }
  return result
}
console.log("unsupported", collect(["a"]).length)
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
            module_path: Some("example.com/aot-unsupported-function-details".to_string()),
            output_kind: EmitGoOutputKind::Main,
            ir_snapshot: None,
        });

        assert!(response.diagnostics.iter().any(|diagnostic| diagnostic.code
            == "EXECUTABLE_JS_CODEGEN_NOT_IMPLEMENTED"
            && diagnostic
                .message
                .contains("aot.function.unsupported_body:src/index.js:collect")));
        assert!(!response.files[0].contents.contains("tsgodownrt.RunProgram"));
    }

    #[test]
    fn emit_go_fails_closed_on_aot_function_count_limit() {
        let root = temp_project("engine-core-aot-function-count-limit");
        let mut source = String::new();
        for index in 0..257 {
            source.push_str(&format!("function f{index}() {{ return {index} }}\n"));
        }
        source.push_str("console.log(f0())\n");
        write(&root, "src/index.js", &source);

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
            module_path: Some("example.com/aot-function-count-limit".to_string()),
            output_kind: EmitGoOutputKind::Main,
            ir_snapshot: None,
        });

        let message = response
            .diagnostics
            .iter()
            .find(|diagnostic| diagnostic.code == "EXECUTABLE_JS_CODEGEN_NOT_IMPLEMENTED")
            .map(|diagnostic| diagnostic.message.as_str())
            .expect("expected fail-closed diagnostic");
        assert!(message.contains("aot.program.function_count_limit:257>256"));
        assert!(!response.files[0].contents.contains("tsgodownrt.RunProgram"));
    }

    #[test]
    fn emit_go_reports_aot_unsupported_top_level_statement_details() {
        let root = temp_project("engine-core-aot-unsupported-top-level-details");
        write(
            &root,
            "src/index.js",
            r#"
const value = await Promise.resolve(1)
console.log("unsupported", value)
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
            module_path: Some("example.com/aot-unsupported-top-level-details".to_string()),
            output_kind: EmitGoOutputKind::Main,
            ir_snapshot: None,
        });

        let message = response
            .diagnostics
            .iter()
            .find(|diagnostic| diagnostic.code == "EXECUTABLE_JS_CODEGEN_NOT_IMPLEMENTED")
            .map(|diagnostic| diagnostic.message.as_str())
            .expect("expected fail-closed diagnostic");
        assert!(message.contains("aot.statement.unsupported:src/index.js:var-decl"));
        assert!(message.contains("aot.expression.unsupported:src/index.js:await"));
        assert!(!message.contains("aot emission unsupported by Go backend"));
        assert!(!response.files[0].contents.contains("tsgodownrt.RunProgram"));
    }

    #[test]
    fn emit_go_runs_aot_json_value_model_subset() {
        let root = temp_project("engine-core-aot-json-value-model");
        write(
            &root,
            "src/index.js",
            r#"
const versions = ["1.2.3", "1.2.3-beta.2", "bad"]
const report = {
  package: "holdout",
  versions,
  nested: { ready: true, count: 3 },
  empty: null
}
console.log(JSON.stringify(report, null, 2))
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
            module_path: Some("example.com/aot-json-value-model".to_string()),
            output_kind: EmitGoOutputKind::Main,
            ir_snapshot: None,
        });

        assert!(!response
            .diagnostics
            .iter()
            .any(|diagnostic| diagnostic.code == "EXECUTABLE_JS_CODEGEN_NOT_IMPLEMENTED"));
        assert!(!response.files[0].contents.contains("tsgodownrt.RunProgram"));
        assert!(response.files[0].contents.contains("\"encoding/json\""));
        assert!(response.files[0].contents.contains("map[string]any"));

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
            "{\n  \"empty\": null,\n  \"nested\": {\n    \"count\": 3,\n    \"ready\": true\n  },\n  \"package\": \"holdout\",\n  \"versions\": [\n    \"1.2.3\",\n    \"1.2.3-beta.2\",\n    \"bad\"\n  ]\n}\n"
        );
    }

    #[test]
    fn emit_go_runs_aot_commonjs_default_function_subset() {
        let root = temp_project("engine-core-aot-cjs-default-function");
        write(
            &root,
            "src/index.js",
            r#"
const add = require("./add.js")
console.log("aot-cjs", add(2, 4))
"#,
        );
        write(
            &root,
            "src/add.js",
            r#"
function add(left, right) {
  return left + right
}
module.exports = add
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
            module_path: Some("example.com/aot-cjs-default-function".to_string()),
            output_kind: EmitGoOutputKind::Main,
            ir_snapshot: None,
        });

        assert!(!response
            .diagnostics
            .iter()
            .any(|diagnostic| diagnostic.code == "EXECUTABLE_JS_CODEGEN_NOT_IMPLEMENTED"));
        assert!(!response.files[0].contents.contains("tsgodownrt.RunProgram"));
        assert!(response.files[0].contents.contains("src_add_js_add"));

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
        assert_eq!(String::from_utf8_lossy(&output.stdout), "aot-cjs 6\n");
    }

    #[test]
    fn emit_go_runs_aot_commonjs_default_function_expression_subset() {
        let root = temp_project("engine-core-aot-cjs-default-function-expression");
        write(
            &root,
            "src/index.js",
            r#"
const tag = require("./tag.js")
console.log("aot-cjs-default-fn-expr", tag("go"))
"#,
        );
        write(
            &root,
            "src/tag.js",
            r#"
module.exports = function (name) {
  return name + "!"
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
            module_path: Some("example.com/aot-cjs-default-function-expression".to_string()),
            output_kind: EmitGoOutputKind::Main,
            ir_snapshot: None,
        });

        assert!(!response
            .diagnostics
            .iter()
            .any(|diagnostic| diagnostic.code == "EXECUTABLE_JS_CODEGEN_NOT_IMPLEMENTED"));
        assert!(!response.files[0].contents.contains("tsgodownrt.RunProgram"));
        assert!(response.files[0]
            .contents
            .contains("src_tag_js___cjs_default_export"));

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
            "aot-cjs-default-fn-expr go!\n"
        );
    }

    #[test]
    fn emit_go_runs_aot_commonjs_named_function_namespace_subset() {
        let root = temp_project("engine-core-aot-cjs-named-function");
        write(
            &root,
            "src/index.js",
            r#"
const math = require("./math.js")
console.log("aot-cjs-ns", math.add(2, 4), math.label())
"#,
        );
        write(
            &root,
            "src/math.js",
            r#"
function add(left, right) {
  return left + right
}
function label() {
  return "ok"
}
exports.add = add
module.exports.label = label
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
            module_path: Some("example.com/aot-cjs-named-function".to_string()),
            output_kind: EmitGoOutputKind::Main,
            ir_snapshot: None,
        });

        assert!(!response
            .diagnostics
            .iter()
            .any(|diagnostic| diagnostic.code == "EXECUTABLE_JS_CODEGEN_NOT_IMPLEMENTED"));
        assert!(!response.files[0].contents.contains("tsgodownrt.RunProgram"));
        assert!(response.files[0].contents.contains("src_math_js_add"));
        assert!(response.files[0].contents.contains("src_math_js_label"));

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
        assert_eq!(String::from_utf8_lossy(&output.stdout), "aot-cjs-ns 6 ok\n");
    }

    #[test]
    fn emit_go_runs_aot_commonjs_named_function_expression_namespace_subset() {
        let root = temp_project("engine-core-aot-cjs-named-function-expression");
        write(
            &root,
            "src/index.js",
            r#"
const math = require("./math.js")
console.log("aot-cjs-fn-expr-ns", math.add(2, 4), math.label())
"#,
        );
        write(
            &root,
            "src/math.js",
            r#"
const add = (left, right) => {
  return left + right
}
const label = function () {
  return "ok"
}
exports.add = add
module.exports.label = label
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
            module_path: Some("example.com/aot-cjs-named-function-expression".to_string()),
            output_kind: EmitGoOutputKind::Main,
            ir_snapshot: None,
        });

        assert!(!response
            .diagnostics
            .iter()
            .any(|diagnostic| diagnostic.code == "EXECUTABLE_JS_CODEGEN_NOT_IMPLEMENTED"));
        assert!(!response.files[0].contents.contains("tsgodownrt.RunProgram"));
        assert!(response.files[0].contents.contains("src_math_js_add"));
        assert!(response.files[0].contents.contains("src_math_js_label"));

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
            "aot-cjs-fn-expr-ns 6 ok\n"
        );
    }

    #[test]
    fn emit_go_runs_aot_commonjs_object_function_namespace_subset() {
        let root = temp_project("engine-core-aot-cjs-object-function-namespace");
        write(
            &root,
            "src/index.js",
            r#"
const api = require("./api.js")
console.log("aot-cjs-object-ns", api.add(2, 4), api.label())
"#,
        );
        write(
            &root,
            "src/api.js",
            r#"
const add = (left, right) => {
  return left + right
}
function label() {
  return "ok"
}
const api = { add, label }
module.exports.add = api.add
module.exports.label = api.label
module.exports = api
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
            module_path: Some("example.com/aot-cjs-object-function-namespace".to_string()),
            output_kind: EmitGoOutputKind::Main,
            ir_snapshot: None,
        });

        assert!(!response
            .diagnostics
            .iter()
            .any(|diagnostic| diagnostic.code == "EXECUTABLE_JS_CODEGEN_NOT_IMPLEMENTED"));
        assert!(!response.files[0].contents.contains("tsgodownrt.RunProgram"));
        assert!(response.files[0].contents.contains("src_api_js_add"));
        assert!(response.files[0].contents.contains("src_api_js_label"));

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
            "aot-cjs-object-ns 6 ok\n"
        );
    }

    #[test]
    fn emit_go_runs_aot_local_object_function_namespace_subset() {
        let root = temp_project("engine-core-aot-local-object-function-namespace");
        write(
            &root,
            "src/index.js",
            r#"
function parse(value) {
  return value.toUpperCase()
}
function config() {
  return Api.parse("go") + "!"
}
const Api = { parse, config }
module.exports = Api
console.log("aot-local-object-ns", Api.config())
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
            module_path: Some("example.com/aot-local-object-function-namespace".to_string()),
            output_kind: EmitGoOutputKind::Main,
            ir_snapshot: None,
        });

        assert!(!response
            .diagnostics
            .iter()
            .any(|diagnostic| diagnostic.code == "EXECUTABLE_JS_CODEGEN_NOT_IMPLEMENTED"));
        assert!(!response.files[0].contents.contains("tsgodownrt.RunProgram"));
        assert!(response.files[0].contents.contains("func parse"));
        assert!(response.files[0].contents.contains("func config"));
        assert!(!response.files[0].contents.contains("var Api"));

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
            "aot-local-object-ns GO!\n"
        );
    }

    #[test]
    fn emit_go_runs_aot_commonjs_inline_object_function_namespace_subset() {
        let root = temp_project("engine-core-aot-cjs-inline-object-function-namespace");
        write(
            &root,
            "src/index.js",
            r#"
const api = require("./api.js")
console.log("aot-cjs-inline-object-ns", api.add(2, 4), api.label())
"#,
        );
        write(
            &root,
            "src/api.js",
            r#"
const add = (left, right) => {
  return left + right
}
function label() {
  return "ok"
}
module.exports = { add, label }
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
            module_path: Some("example.com/aot-cjs-inline-object-function-namespace".to_string()),
            output_kind: EmitGoOutputKind::Main,
            ir_snapshot: None,
        });

        assert!(!response
            .diagnostics
            .iter()
            .any(|diagnostic| diagnostic.code == "EXECUTABLE_JS_CODEGEN_NOT_IMPLEMENTED"));
        assert!(!response.files[0].contents.contains("tsgodownrt.RunProgram"));
        assert!(response.files[0].contents.contains("src_api_js_add"));
        assert!(response.files[0].contents.contains("src_api_js_label"));

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
            "aot-cjs-inline-object-ns 6 ok\n"
        );
    }

    #[test]
    fn emit_go_runs_aot_commonjs_reexported_default_functions_subset() {
        let root = temp_project("engine-core-aot-cjs-reexport-default-functions");
        write(
            &root,
            "src/index.js",
            r#"
const api = require("./api.js")
console.log("aot-cjs-reexport", api.add(2, 4), api.tag("go"))
"#,
        );
        write(
            &root,
            "src/add.js",
            r#"
module.exports = function (left, right) {
  return left + right
}
"#,
        );
        write(
            &root,
            "src/tag.js",
            r#"
module.exports = function (name) {
  return name + "!"
}
"#,
        );
        write(
            &root,
            "src/api.js",
            r#"
const add = require("./add.js")
const tag = require("./tag.js")
const unsupported = require("./constants.js")
module.exports = { add, tag, unsupported }
"#,
        );
        write(
            &root,
            "src/constants.js",
            r#"
module.exports = { label: "skip" }
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
            module_path: Some("example.com/aot-cjs-reexport-default-functions".to_string()),
            output_kind: EmitGoOutputKind::Main,
            ir_snapshot: None,
        });

        assert!(!response
            .diagnostics
            .iter()
            .any(|diagnostic| diagnostic.code == "EXECUTABLE_JS_CODEGEN_NOT_IMPLEMENTED"));
        assert!(!response.files[0].contents.contains("tsgodownrt.RunProgram"));
        assert!(response.files[0]
            .contents
            .contains("src_add_js___cjs_default_export"));
        assert!(response.files[0]
            .contents
            .contains("src_tag_js___cjs_default_export"));

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
            "aot-cjs-reexport 6 go!\n"
        );
    }

    #[test]
    fn emit_go_runs_aot_commonjs_spread_reexported_function_namespace_subset() {
        let root = temp_project("engine-core-aot-cjs-spread-reexport-function-namespace");
        write(
            &root,
            "src/index.js",
            r#"
const api = require("./api.js")
console.log("aot-cjs-spread-reexport", api.add(2, 4), api.label(), api.tag("go"))
"#,
        );
        write(
            &root,
            "src/math.js",
            r#"
function add(left, right) {
  return left + right
}
function label() {
  return "ok"
}
module.exports = { add, label }
"#,
        );
        write(
            &root,
            "src/tag.js",
            r#"
module.exports = function (name) {
  return name + "!"
}
"#,
        );
        write(
            &root,
            "src/api.js",
            r#"
module.exports = {
  ...require("./math.js"),
  tag: require("./tag.js")
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
            module_path: Some("example.com/aot-cjs-spread-reexport-function-namespace".to_string()),
            output_kind: EmitGoOutputKind::Main,
            ir_snapshot: None,
        });

        assert!(!response
            .diagnostics
            .iter()
            .any(|diagnostic| diagnostic.code == "EXECUTABLE_JS_CODEGEN_NOT_IMPLEMENTED"));
        assert!(!response.files[0].contents.contains("tsgodownrt.RunProgram"));
        assert!(response.files[0].contents.contains("src_math_js_add"));
        assert!(response.files[0].contents.contains("src_math_js_label"));
        assert!(response.files[0]
            .contents
            .contains("src_tag_js___cjs_default_export"));

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
            "aot-cjs-spread-reexport 6 ok go!\n"
        );
    }

    #[test]
    fn emit_go_runs_aot_function_static_string_member_subset() {
        let root = temp_project("engine-core-aot-function-static-string-member");
        write(
            &root,
            "src/index.js",
            r#"
const LABEL = "ok"
function tag(name) {
  return name + "!"
}
tag.label = LABEL
console.log("aot-fn-static", tag("go"), tag.label)
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
            module_path: Some("example.com/aot-function-static-string-member".to_string()),
            output_kind: EmitGoOutputKind::Main,
            ir_snapshot: None,
        });

        assert!(!response
            .diagnostics
            .iter()
            .any(|diagnostic| diagnostic.code == "EXECUTABLE_JS_CODEGEN_NOT_IMPLEMENTED"));
        assert!(!response.files[0].contents.contains("tsgodownrt.RunProgram"));
        assert!(response.files[0].contents.contains("src_index_js_LABEL"));

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
            "aot-fn-static go! ok\n"
        );
    }

    #[test]
    fn emit_go_runs_aot_conditional_variadic_debug_function_subset() {
        let root = temp_project("engine-core-aot-conditional-variadic-debug-function");
        write(
            &root,
            "src/index.js",
            r#"
const debug = (
  typeof process === "object" &&
  process.env &&
  process.env.TSGODOWN_DEBUG &&
  /\bgo\b/i.test(process.env.TSGODOWN_DEBUG)
) ? (...args) => console.error("DBG", ...args)
  : () => {}
debug("go", 2)
console.log("aot-debug", "done")
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
            module_path: Some("example.com/aot-conditional-variadic-debug-function".to_string()),
            output_kind: EmitGoOutputKind::Main,
            ir_snapshot: None,
        });

        assert!(!response
            .diagnostics
            .iter()
            .any(|diagnostic| diagnostic.code == "EXECUTABLE_JS_CODEGEN_NOT_IMPLEMENTED"));
        assert!(!response.files[0].contents.contains("tsgodownrt.RunProgram"));
        assert!(response.files[0].contents.contains("func(args ...any) any"));

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
            .env("TSGODOWN_DEBUG", "go")
            .current_dir(&out_dir)
            .output()
            .expect("run generated go");
        assert!(
            output.status.success(),
            "go run failed: stdout={} stderr={}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
        assert_eq!(String::from_utf8_lossy(&output.stdout), "aot-debug done\n");
        assert_eq!(String::from_utf8_lossy(&output.stderr), "DBG go 2\n");
    }

    #[test]
    fn emit_go_runs_aot_commonjs_export_alias_var_init_subset() {
        let root = temp_project("engine-core-aot-cjs-export-alias-var-init");
        write(
            &root,
            "src/index.js",
            r#"
exports = module.exports = {}
const src = exports.src = []
const t = exports.t = {}
src[0] = "alpha"
t.alpha = 0
console.log("aot-cjs-export-alias", src[0])
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
            module_path: Some("example.com/aot-cjs-export-alias-var-init".to_string()),
            output_kind: EmitGoOutputKind::Main,
            ir_snapshot: None,
        });

        assert!(!response
            .diagnostics
            .iter()
            .any(|diagnostic| diagnostic.code == "EXECUTABLE_JS_CODEGEN_NOT_IMPLEMENTED"));
        assert!(!response.files[0].contents.contains("tsgodownrt.RunProgram"));
        assert!(response.files[0].contents.contains("var src []string"));
        assert!(response.files[0].contents.contains("var t map[string]any"));

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
            "aot-cjs-export-alias alpha\n"
        );
    }

    #[test]
    fn emit_go_runs_aot_basic_class_instance_subset() {
        let root = temp_project("engine-core-aot-basic-class-instance");
        write(
            &root,
            "src/index.js",
            r#"
class Box {
  constructor(label, count) {
    this.label = label
    this.count = count
  }
  name() {
    return this.label
  }
  size() {
    return this.count
  }
}
const item = new Box("crate", 3)
console.log("aot-class", item.name(), item.size())
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
            module_path: Some("example.com/aot-basic-class-instance".to_string()),
            output_kind: EmitGoOutputKind::Main,
            ir_snapshot: None,
        });

        assert!(!response
            .diagnostics
            .iter()
            .any(|diagnostic| diagnostic.code == "EXECUTABLE_JS_CODEGEN_NOT_IMPLEMENTED"));
        assert!(!response.files[0].contents.contains("tsgodownrt.RunProgram"));
        assert!(response.files[0].contents.contains("type Box struct"));
        assert!(response.files[0]
            .contents
            .contains("func (self *Box) name() any"));

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
            "aot-class crate 3\n"
        );
    }

    #[test]
    fn emit_go_runs_aot_class_getter_subset() {
        let root = temp_project("engine-core-aot-class-getter");
        write(
            &root,
            "src/index.js",
            r#"
class Box {
  constructor(input) {
    this.input = input
  }
  get doubled() {
    return this.input * 2
  }
}
const item = new Box(4)
console.log("aot-getter", item.doubled, new Box(5).doubled)
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
            module_path: Some("example.com/aot-class-getter".to_string()),
            output_kind: EmitGoOutputKind::Main,
            ir_snapshot: None,
        });

        assert!(!response
            .diagnostics
            .iter()
            .any(|diagnostic| diagnostic.code == "EXECUTABLE_JS_CODEGEN_NOT_IMPLEMENTED"));
        assert!(!response.files[0].contents.contains("tsgodownrt.RunProgram"));
        assert!(response.files[0]
            .contents
            .contains("func (self *Box) doubled() any"));
        assert!(response.files[0].contents.contains(".doubled()"));

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
        assert_eq!(String::from_utf8_lossy(&output.stdout), "aot-getter 8 10\n");
    }

    #[test]
    fn emit_go_runs_aot_commonjs_default_class_subset() {
        let root = temp_project("engine-core-aot-cjs-default-class");
        write(
            &root,
            "src/index.js",
            r#"
const Box = require("./box.js")
const item = new Box("crate", 3)
console.log("aot-cjs-class", item.name(), item.size())
"#,
        );
        write(
            &root,
            "src/box.js",
            r#"
class Box {
  constructor(label, count) {
    this.label = label
    this.count = count
  }
  name() {
    return this.label
  }
  size() {
    return this.count
  }
}
module.exports = Box
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
            module_path: Some("example.com/aot-cjs-default-class".to_string()),
            output_kind: EmitGoOutputKind::Main,
            ir_snapshot: None,
        });

        assert!(!response
            .diagnostics
            .iter()
            .any(|diagnostic| diagnostic.code == "EXECUTABLE_JS_CODEGEN_NOT_IMPLEMENTED"));
        assert!(!response.files[0].contents.contains("tsgodownrt.RunProgram"));
        assert!(response.files[0].contents.contains("src_box_js_Box"));

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
            "aot-cjs-class crate 3\n"
        );
    }

    #[test]
    fn emit_go_runs_aot_named_esm_class_import_subset() {
        let root = temp_project("engine-core-aot-esm-class-import");
        write(
            &root,
            "src/index.js",
            r#"
import { Box } from "./box.js"
const item = new Box("crate", 3)
console.log("aot-esm-class", item.name(), item.size())
"#,
        );
        write(
            &root,
            "src/box.js",
            r#"
export class Box {
  constructor(label, count) {
    this.label = label
    this.count = count
  }
  name() {
    return this.label
  }
  size() {
    return this.count
  }
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
            module_path: Some("example.com/aot-esm-class-import".to_string()),
            output_kind: EmitGoOutputKind::Main,
            ir_snapshot: None,
        });

        assert!(!response
            .diagnostics
            .iter()
            .any(|diagnostic| diagnostic.code == "EXECUTABLE_JS_CODEGEN_NOT_IMPLEMENTED"));
        assert!(!response.files[0].contents.contains("tsgodownrt.RunProgram"));
        assert!(response.files[0].contents.contains("src_box_js_Box"));

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
            "aot-esm-class crate 3\n"
        );
    }

    #[test]
    fn emit_go_runs_aot_top_level_numeric_if_subset() {
        let root = temp_project("engine-core-aot-top-level-if");
        write(
            &root,
            "src/index.js",
            r#"
const count = 3
if (count >= 3) {
  console.log("branch", count + 2)
} else {
  console.log("branch", 0)
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
                config: legacy_ir_interpreter_config(),
            },
            package_name: None,
            module_path: Some("example.com/aot-top-level-if".to_string()),
            output_kind: EmitGoOutputKind::Main,
            ir_snapshot: None,
        });

        assert!(!response
            .diagnostics
            .iter()
            .any(|diagnostic| diagnostic.code == "EXECUTABLE_JS_CODEGEN_NOT_IMPLEMENTED"));
        assert!(!response.files[0].contents.contains("tsgodownrt.RunProgram"));
        assert!(response.files[0]
            .contents
            .contains("var src_index_js_count float64 = 3"));
        assert!(response.files[0]
            .contents
            .contains("if (src_index_js_count >= 3)"));

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
        assert_eq!(String::from_utf8_lossy(&output.stdout), "branch 5\n");
    }

    #[test]
    fn emit_go_runs_aot_top_level_for_loop_subset() {
        let root = temp_project("engine-core-aot-top-level-for");
        write(
            &root,
            "src/index.js",
            r#"
for (let i = 0; i < 3; i++) {
  console.log("loop", i)
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
                config: legacy_ir_interpreter_config(),
            },
            package_name: None,
            module_path: Some("example.com/aot-top-level-for".to_string()),
            output_kind: EmitGoOutputKind::Main,
            ir_snapshot: None,
        });

        assert!(!response
            .diagnostics
            .iter()
            .any(|diagnostic| diagnostic.code == "EXECUTABLE_JS_CODEGEN_NOT_IMPLEMENTED"));
        assert!(!response.files[0].contents.contains("tsgodownrt.RunProgram"));
        assert!(response.files[0]
            .contents
            .contains("for i := float64(0); (i < 3); i++"));

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
            "loop 0\nloop 1\nloop 2\n"
        );
    }

    #[test]
    fn emit_go_runs_aot_numeric_assignment_loop_subset() {
        let root = temp_project("engine-core-aot-assignment-loop");
        write(
            &root,
            "src/index.js",
            r#"
let total = 0
for (let i = 1; i <= 4; i++) {
  total += i
}
console.log("total", total)
"#,
        );

        let response = emit_go(EmitGoRequest {
            analyze: AnalyzeRequest {
                manifest: InputManifest {
                    entry: "src/index.js".to_string(),
                    framework: None,
                },
                cwd: Some(root.to_string_lossy().to_string()),
                config: legacy_ir_interpreter_config(),
            },
            package_name: None,
            module_path: Some("example.com/aot-assignment-loop".to_string()),
            output_kind: EmitGoOutputKind::Main,
            ir_snapshot: None,
        });

        assert!(!response
            .diagnostics
            .iter()
            .any(|diagnostic| diagnostic.code == "EXECUTABLE_JS_CODEGEN_NOT_IMPLEMENTED"));
        assert!(!response.files[0].contents.contains("tsgodownrt.RunProgram"));
        assert!(response.files[0].contents.contains("total += i"));

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
        assert_eq!(String::from_utf8_lossy(&output.stdout), "total 10\n");
    }

    #[test]
    fn emit_go_runs_aot_string_concat_subset() {
        let root = temp_project("engine-core-aot-string-concat");
        write(
            &root,
            "src/index.js",
            r#"
const name = "tsgodown"
const label = "hello " + name
console.log(label)
"#,
        );

        let response = emit_go(EmitGoRequest {
            analyze: AnalyzeRequest {
                manifest: InputManifest {
                    entry: "src/index.js".to_string(),
                    framework: None,
                },
                cwd: Some(root.to_string_lossy().to_string()),
                config: legacy_ir_interpreter_config(),
            },
            package_name: None,
            module_path: Some("example.com/aot-string-concat".to_string()),
            output_kind: EmitGoOutputKind::Main,
            ir_snapshot: None,
        });

        assert!(!response
            .diagnostics
            .iter()
            .any(|diagnostic| diagnostic.code == "EXECUTABLE_JS_CODEGEN_NOT_IMPLEMENTED"));
        assert!(!response.files[0].contents.contains("tsgodownrt.RunProgram"));
        assert!(response.files[0]
            .contents
            .contains("var src_index_js_label string = (\"hello \" + src_index_js_name)"));

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
        assert_eq!(String::from_utf8_lossy(&output.stdout), "hello tsgodown\n");
    }

    #[test]
    fn emit_go_runs_aot_bool_branch_subset() {
        let root = temp_project("engine-core-aot-bool-branch");
        write(
            &root,
            "src/index.js",
            r#"
const ready = true
if (!ready) {
  console.log("ready", "no")
} else {
  console.log("ready", "yes")
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
                config: legacy_ir_interpreter_config(),
            },
            package_name: None,
            module_path: Some("example.com/aot-bool-branch".to_string()),
            output_kind: EmitGoOutputKind::Main,
            ir_snapshot: None,
        });

        assert!(!response
            .diagnostics
            .iter()
            .any(|diagnostic| diagnostic.code == "EXECUTABLE_JS_CODEGEN_NOT_IMPLEMENTED"));
        assert!(!response.files[0].contents.contains("tsgodownrt.RunProgram"));
        assert!(response.files[0]
            .contents
            .contains("var src_index_js_ready bool = true"));
        assert!(response.files[0]
            .contents
            .contains("if (!src_index_js_ready)"));

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
        assert_eq!(String::from_utf8_lossy(&output.stdout), "ready yes\n");
    }

    #[test]
    fn emit_go_runs_aot_static_object_property_subset() {
        let root = temp_project("engine-core-aot-static-object");
        write(
            &root,
            "src/index.js",
            r#"
const item = { count: 3, label: "box", ready: true }
console.log("item", item.label, item.count + 2, item.ready)
"#,
        );

        let response = emit_go(EmitGoRequest {
            analyze: AnalyzeRequest {
                manifest: InputManifest {
                    entry: "src/index.js".to_string(),
                    framework: None,
                },
                cwd: Some(root.to_string_lossy().to_string()),
                config: legacy_ir_interpreter_config(),
            },
            package_name: None,
            module_path: Some("example.com/aot-static-object".to_string()),
            output_kind: EmitGoOutputKind::Main,
            ir_snapshot: None,
        });

        assert!(!response
            .diagnostics
            .iter()
            .any(|diagnostic| diagnostic.code == "EXECUTABLE_JS_CODEGEN_NOT_IMPLEMENTED"));
        assert!(!response.files[0].contents.contains("tsgodownrt.RunProgram"));
        assert!(response.files[0].contents.contains("item.label"));
        assert!(response.files[0].contents.contains("item.count"));

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
        assert_eq!(String::from_utf8_lossy(&output.stdout), "item box 5 true\n");
    }

    #[test]
    fn emit_go_runs_aot_object_freeze_literal_subset() {
        let root = temp_project("engine-core-aot-object-freeze");
        write(
            &root,
            "src/index.js",
            r#"
const opts = Object.freeze({ loose: true, label: "box" })
const empty = Object.freeze({})
console.log("object-freeze", opts.loose, opts.label, JSON.stringify(empty))
"#,
        );

        let response = emit_go(EmitGoRequest {
            analyze: AnalyzeRequest {
                manifest: InputManifest {
                    entry: "src/index.js".to_string(),
                    framework: None,
                },
                cwd: Some(root.to_string_lossy().to_string()),
                config: legacy_ir_interpreter_config(),
            },
            package_name: None,
            module_path: Some("example.com/aot-object-freeze".to_string()),
            output_kind: EmitGoOutputKind::Main,
            ir_snapshot: None,
        });

        assert!(
            !response
                .diagnostics
                .iter()
                .any(|diagnostic| diagnostic.code == "EXECUTABLE_JS_CODEGEN_NOT_IMPLEMENTED"),
            "diagnostics: {:?}",
            response.diagnostics
        );
        assert!(
            !response.files[0].contents.contains("tsgodownrt.RunProgram"),
            "diagnostics: {:?}",
            response.diagnostics
        );
        assert!(response.files[0].contents.contains("object-freeze"));

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
            "object-freeze true box {}\n"
        );
    }

    #[test]
    fn emit_go_runs_aot_object_create_assign_subset() {
        let root = temp_project("engine-core-aot-object-create-assign");
        write(
            &root,
            "src/index.js",
            r#"
const base = Object.create(null)
base.name = "alpha"
const merged = Object.assign(Object.create(null), base, { count: 2 })
Object.assign(base, { ready: true }, { count: merged.count + 1 })
console.log("object-assign", merged.name, merged.count, base.ready, base.count)
"#,
        );

        let response = emit_go(EmitGoRequest {
            analyze: AnalyzeRequest {
                manifest: InputManifest {
                    entry: "src/index.js".to_string(),
                    framework: None,
                },
                cwd: Some(root.to_string_lossy().to_string()),
                config: legacy_ir_interpreter_config(),
            },
            package_name: None,
            module_path: Some("example.com/aot-object-create-assign".to_string()),
            output_kind: EmitGoOutputKind::Main,
            ir_snapshot: None,
        });

        assert!(
            !response
                .diagnostics
                .iter()
                .any(|diagnostic| diagnostic.code == "EXECUTABLE_JS_CODEGEN_NOT_IMPLEMENTED"),
            "diagnostics: {:?}",
            response.diagnostics
        );
        assert!(!response.files[0].contents.contains("tsgodownrt.RunProgram"));

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
            "object-assign alpha 2 true 3\n"
        );
    }

    #[test]
    fn emit_go_runs_aot_object_spread_map_subset() {
        let root = temp_project("engine-core-aot-object-spread");
        write(
            &root,
            "src/index.js",
            r#"
const base = { value: 7, nested: { flag: true } }
const clone = { ...base, value: 9, extra: ["a", "b"] }
console.log("object-spread", clone.value, clone.nested.flag, clone.extra.at(-1), JSON.stringify(clone.extra))
"#,
        );

        let response = emit_go(EmitGoRequest {
            analyze: AnalyzeRequest {
                manifest: InputManifest {
                    entry: "src/index.js".to_string(),
                    framework: None,
                },
                cwd: Some(root.to_string_lossy().to_string()),
                config: legacy_ir_interpreter_config(),
            },
            package_name: None,
            module_path: Some("example.com/aot-object-spread".to_string()),
            output_kind: EmitGoOutputKind::Main,
            ir_snapshot: None,
        });

        assert!(!response
            .diagnostics
            .iter()
            .any(|diagnostic| diagnostic.code == "EXECUTABLE_JS_CODEGEN_NOT_IMPLEMENTED"));
        assert!(!response.files[0].contents.contains("tsgodownrt.RunProgram"));
        assert!(response.files[0]
            .contents
            .contains("for key, value := range tsgodownObjectFromAny"));

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
            "object-spread 9 true b [\"a\",\"b\"]\n"
        );
    }

    #[test]
    fn emit_go_runs_aot_top_level_async_iife_subset() {
        let root = temp_project("engine-core-aot-top-level-async-iife");
        write(
            &root,
            "src/index.mjs",
            r#"
const value = await (async () => {
  const log = []
  const base = { value: 4, nested: { flag: false } }
  const clone = { ...base, extra: [4, 5] }
  class Box {
    constructor(input) {
      this.input = input
    }
    get doubled() {
      return this.input * 2
    }
  }
  log.push(["last", clone.extra.at(-1)])
  return { value: clone.value, flag: clone.nested.flag, last: clone.extra.at(-1), doubled: new Box(6).doubled, log }
})()
console.log(JSON.stringify(value))
"#,
        );

        let response = emit_go(EmitGoRequest {
            analyze: AnalyzeRequest {
                manifest: InputManifest {
                    entry: "src/index.mjs".to_string(),
                    framework: None,
                },
                cwd: Some(root.to_string_lossy().to_string()),
                config: legacy_ir_interpreter_config(),
            },
            package_name: None,
            module_path: Some("example.com/aot-top-level-async-iife".to_string()),
            output_kind: EmitGoOutputKind::Main,
            ir_snapshot: None,
        });

        assert!(!response
            .diagnostics
            .iter()
            .any(|diagnostic| diagnostic.code == "EXECUTABLE_JS_CODEGEN_NOT_IMPLEMENTED"));
        assert!(!response.files[0].contents.contains("tsgodownrt.RunProgram"));
        assert!(response.files[0].contents.contains("func() any"));

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
            "{\"doubled\":12,\"flag\":false,\"last\":5,\"log\":[[\"last\",5]],\"value\":4}\n"
        );
    }

    #[test]
    fn emit_go_runs_aot_function_if_return_subset() {
        let root = temp_project("engine-core-aot-function-if-return");
        write(
            &root,
            "src/index.js",
            r#"
function score(value) {
  if (value > 10 && value !== 13) {
    return value * 2
  }
  return value + 1
}
console.log("scores", score(12), score(13), score(4))
"#,
        );

        let response = emit_go(EmitGoRequest {
            analyze: AnalyzeRequest {
                manifest: InputManifest {
                    entry: "src/index.js".to_string(),
                    framework: None,
                },
                cwd: Some(root.to_string_lossy().to_string()),
                config: legacy_ir_interpreter_config(),
            },
            package_name: None,
            module_path: Some("example.com/aot-function-if-return".to_string()),
            output_kind: EmitGoOutputKind::Main,
            ir_snapshot: None,
        });

        assert!(!response
            .diagnostics
            .iter()
            .any(|diagnostic| diagnostic.code == "EXECUTABLE_JS_CODEGEN_NOT_IMPLEMENTED"));
        assert!(!response.files[0].contents.contains("tsgodownrt.RunProgram"));
        assert!(response.files[0]
            .contents
            .contains("if ((value > 10) && (value != 13))"));

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
        assert_eq!(String::from_utf8_lossy(&output.stdout), "scores 24 14 5\n");
    }

    #[test]
    fn emit_go_runs_aot_function_local_slots_subset() {
        let root = temp_project("engine-core-aot-function-local-slots");
        write(
            &root,
            "src/index.js",
            r#"
function score(value) {
  const bonus = 3
  const total = value + bonus
  return total * 2
}
console.log("local-slots", score(4))
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
            module_path: Some("example.com/aot-function-local-slots".to_string()),
            output_kind: EmitGoOutputKind::Main,
            ir_snapshot: None,
        });

        assert!(!response
            .diagnostics
            .iter()
            .any(|diagnostic| diagnostic.code == "EXECUTABLE_JS_CODEGEN_NOT_IMPLEMENTED"));
        assert!(!response.files[0].contents.contains("tsgodownrt.RunProgram"));
        assert!(response.files[0].contents.contains("var bonus float64 = 3"));

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
        assert_eq!(String::from_utf8_lossy(&output.stdout), "local-slots 14\n");
    }

    #[test]
    fn emit_go_runs_aot_function_bare_return_subset() {
        let root = temp_project("engine-core-aot-function-bare-return");
        write(
            &root,
            "src/index.js",
            r#"
function stop(flag) {
  if (flag) {
    return
  }
  return "kept"
}
console.log("aot-bare-return", stop(false), stop(true) === undefined)
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
            module_path: Some("example.com/aot-function-bare-return".to_string()),
            output_kind: EmitGoOutputKind::Main,
            ir_snapshot: None,
        });

        assert!(!response
            .diagnostics
            .iter()
            .any(|diagnostic| diagnostic.code == "EXECUTABLE_JS_CODEGEN_NOT_IMPLEMENTED"));
        assert!(!response.files[0].contents.contains("tsgodownrt.RunProgram"));

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
            "aot-bare-return kept true\n"
        );
    }

    #[test]
    fn emit_go_runs_aot_directive_prologue_subset() {
        let root = temp_project("engine-core-aot-directive-prologue");
        write(
            &root,
            "src/index.js",
            r#"
"use strict"
function label(value) {
  "use strict"
  return "dir-" + value
}
console.log("directive", label("ok"))
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
            module_path: Some("example.com/aot-directive-prologue".to_string()),
            output_kind: EmitGoOutputKind::Main,
            ir_snapshot: None,
        });

        assert!(
            !response
                .diagnostics
                .iter()
                .any(|diagnostic| diagnostic.code == "EXECUTABLE_JS_CODEGEN_NOT_IMPLEMENTED"),
            "diagnostics: {:?}",
            response.diagnostics
        );
        assert!(!response.files[0].contents.contains("tsgodownrt.RunProgram"));

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
            "directive dir-ok\n"
        );
    }

    #[test]
    fn emit_go_runs_aot_function_for_loop_subset() {
        let root = temp_project("engine-core-aot-function-for-loop");
        write(
            &root,
            "src/index.js",
            r#"
function sum(limit) {
  let total = 0
  for (let index = 0; index < limit; index++) {
    total += index
  }
  return total
}
console.log("function-loop", sum(5))
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
            module_path: Some("example.com/aot-function-for-loop".to_string()),
            output_kind: EmitGoOutputKind::Main,
            ir_snapshot: None,
        });

        assert!(!response
            .diagnostics
            .iter()
            .any(|diagnostic| diagnostic.code == "EXECUTABLE_JS_CODEGEN_NOT_IMPLEMENTED"));
        assert!(!response.files[0].contents.contains("tsgodownrt.RunProgram"));
        assert!(response.files[0]
            .contents
            .contains("for index := float64(0); (index < limit); index++"));

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
            "function-loop 10\n"
        );
    }

    #[test]
    fn emit_go_runs_aot_while_break_continue_subset() {
        let root = temp_project("engine-core-aot-while-break-continue");
        write(
            &root,
            "src/index.js",
            r#"
function scan(limit) {
  let index = 0
  let total = 0
  while (index < limit) {
    index++
    if (index === 2) {
      continue
    }
    total += index
    if (total > 6) {
      break
    }
  }
  return total
}
console.log("while-flow", scan(6))
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
            module_path: Some("example.com/aot-while-break-continue".to_string()),
            output_kind: EmitGoOutputKind::Main,
            ir_snapshot: None,
        });

        assert!(!response
            .diagnostics
            .iter()
            .any(|diagnostic| diagnostic.code == "EXECUTABLE_JS_CODEGEN_NOT_IMPLEMENTED"));
        assert!(!response.files[0].contents.contains("tsgodownrt.RunProgram"));
        assert!(response.files[0].contents.contains("for (index < limit)"));
        assert!(response.files[0].contents.contains("continue"));
        assert!(response.files[0].contents.contains("break"));

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
        assert_eq!(String::from_utf8_lossy(&output.stdout), "while-flow 8\n");
    }

    #[test]
    fn emit_go_runs_aot_string_function_param_subset() {
        let root = temp_project("engine-core-aot-string-function-param");
        write(
            &root,
            "src/index.js",
            r#"
function tag(text) {
  return text + "!"
}

function marks(count) {
  let out = ""
  for (let index = 0; index < count; index++) {
    out += "x"
  }
  return out
}

console.log("string-fn", tag("go"), marks(3))
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
            module_path: Some("example.com/aot-string-function-param".to_string()),
            output_kind: EmitGoOutputKind::Main,
            ir_snapshot: None,
        });

        assert!(!response
            .diagnostics
            .iter()
            .any(|diagnostic| diagnostic.code == "EXECUTABLE_JS_CODEGEN_NOT_IMPLEMENTED"));
        assert!(!response.files[0].contents.contains("tsgodownrt.RunProgram"));
        assert!(response.files[0].contents.contains("func tag(text string)"));
        assert!(response.files[0].contents.contains("out += \"x\""));

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
            "string-fn go! xxx\n"
        );
    }

    #[test]
    fn emit_go_runs_aot_bool_function_param_subset() {
        let root = temp_project("engine-core-aot-bool-function-param");
        write(
            &root,
            "src/index.js",
            r#"
function choose(flag, fallback) {
  if (flag && !fallback) {
    return 10
  }
  return 3
}
console.log("bool-fn", choose(true, false), choose(false, false))
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
            module_path: Some("example.com/aot-bool-function-param".to_string()),
            output_kind: EmitGoOutputKind::Main,
            ir_snapshot: None,
        });

        assert!(!response
            .diagnostics
            .iter()
            .any(|diagnostic| diagnostic.code == "EXECUTABLE_JS_CODEGEN_NOT_IMPLEMENTED"));
        assert!(!response.files[0].contents.contains("tsgodownrt.RunProgram"));
        assert!(response.files[0]
            .contents
            .contains("func choose(flag any, fallback any)"));
        assert!(response.files[0].contents.contains("tsgodownToBool"));

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
        assert_eq!(String::from_utf8_lossy(&output.stdout), "bool-fn 10 3\n");
    }

    #[test]
    fn emit_go_runs_aot_conditional_expression_subset() {
        let root = temp_project("engine-core-aot-conditional-expression");
        write(
            &root,
            "src/index.js",
            r#"
function label(flag) {
  const word = flag ? "yes" : "no"
  return word
}
function score(flag) {
  const points = flag ? 10 : 3
  return points + 1
}
console.log("conditional", label(true), label(false), score(true), score(false))
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
            module_path: Some("example.com/aot-conditional-expression".to_string()),
            output_kind: EmitGoOutputKind::Main,
            ir_snapshot: None,
        });

        assert!(!response
            .diagnostics
            .iter()
            .any(|diagnostic| diagnostic.code == "EXECUTABLE_JS_CODEGEN_NOT_IMPLEMENTED"));
        assert!(!response.files[0].contents.contains("tsgodownrt.RunProgram"));
        assert!(response.files[0].contents.contains("func() string"));
        assert!(response.files[0].contents.contains("return \"yes\""));
        assert!(response.files[0].contents.contains("return \"no\""));
        assert!(response.files[0].contents.contains("func() float64"));
        assert!(response.files[0].contents.contains("return 10"));
        assert!(response.files[0].contents.contains("return 3"));

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
            "conditional yes no 11 4\n"
        );
    }

    #[test]
    fn emit_go_runs_aot_primitive_comparison_subset() {
        let root = temp_project("engine-core-aot-primitive-comparison");
        write(
            &root,
            "src/index.js",
            r#"
function matchText(value) {
  if (value === "go") {
    return "hit"
  }
  return "miss"
}
function matchBool(value) {
  if (value !== false) {
    return 1
  }
  return 0
}
console.log("compare", matchText("go"), matchText("ts"), matchBool(true), matchBool(false))
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
            module_path: Some("example.com/aot-primitive-comparison".to_string()),
            output_kind: EmitGoOutputKind::Main,
            ir_snapshot: None,
        });

        assert!(!response
            .diagnostics
            .iter()
            .any(|diagnostic| diagnostic.code == "EXECUTABLE_JS_CODEGEN_NOT_IMPLEMENTED"));
        assert!(!response.files[0].contents.contains("tsgodownrt.RunProgram"));
        assert!(response.files[0].contents.contains("(value == \"go\")"));
        assert!(response.files[0].contents.contains("(value != false)"));

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
            "compare hit miss 1 0\n"
        );
    }

    #[test]
    fn emit_go_runs_aot_number_static_properties_subset() {
        let root = temp_project("engine-core-aot-number-static-properties");
        write(
            &root,
            "src/index.js",
            r#"
const max = Number.MAX_SAFE_INTEGER
const min = Number.MIN_SAFE_INTEGER
const fallback = Number.MAX_SAFE_INTEGER || 42
const zeroFallback = 0 || 42
const span = max - min
console.log("number-static", max > 9000, min < 0, span === 18014398509481982, fallback === max, zeroFallback === 42)
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
            module_path: Some("example.com/aot-number-static-properties".to_string()),
            output_kind: EmitGoOutputKind::Main,
            ir_snapshot: None,
        });

        assert!(
            !response
                .diagnostics
                .iter()
                .any(|diagnostic| diagnostic.code == "EXECUTABLE_JS_CODEGEN_NOT_IMPLEMENTED"),
            "diagnostics: {:?}",
            response.diagnostics
        );
        assert!(!response.files[0].contents.contains("tsgodownrt.RunProgram"));

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
            "number-static true true true true true\n"
        );
    }

    #[test]
    fn emit_go_runs_aot_string_method_subset() {
        let root = temp_project("engine-core-aot-string-methods");
        write(
            &root,
            "src/index.js",
            r#"
function normalize(value) {
  const trimmed = value.trim()
  return trimmed.toLowerCase()
}
function inspect(value) {
  if (value.includes("go")) {
    return value.indexOf("go") + value.length
  }
  return -1
}
console.log("string-methods", normalize(" Go "), inspect("tsgodown"), inspect("rust"))
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
            module_path: Some("example.com/aot-string-methods".to_string()),
            output_kind: EmitGoOutputKind::Main,
            ir_snapshot: None,
        });

        assert!(!response
            .diagnostics
            .iter()
            .any(|diagnostic| diagnostic.code == "EXECUTABLE_JS_CODEGEN_NOT_IMPLEMENTED"));
        assert!(!response.files[0].contents.contains("tsgodownrt.RunProgram"));
        assert!(response.files[0].contents.contains("strings.TrimSpace"));
        assert!(response.files[0].contents.contains("strings.ToLower"));
        assert!(response.files[0].contents.contains("strings.Contains"));
        assert!(response.files[0].contents.contains("strings.Index"));
        assert!(response.files[0].contents.contains("float64(len(value))"));

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
            "string-methods go 10 -1\n"
        );
    }

    #[test]
    fn emit_go_runs_aot_typeof_comparison_subset() {
        let root = temp_project("engine-core-aot-typeof-comparison");
        write(
            &root,
            "src/index.js",
            r#"
function kind(value) {
  if (typeof value === "string") {
    return "text"
  }
  if (typeof value === "boolean") {
    return "flag"
  }
  return typeof value
}
console.log("typeof", kind("go"), kind(true), kind(3))
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
            module_path: Some("example.com/aot-typeof-comparison".to_string()),
            output_kind: EmitGoOutputKind::Main,
            ir_snapshot: None,
        });

        assert!(!response
            .diagnostics
            .iter()
            .any(|diagnostic| diagnostic.code == "EXECUTABLE_JS_CODEGEN_NOT_IMPLEMENTED"));
        assert!(!response.files[0].contents.contains("tsgodownrt.RunProgram"));
        assert!(response.files[0]
            .contents
            .contains("switch any(value).(type)"));

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
            "typeof text flag number\n"
        );
    }

    #[test]
    fn emit_go_runs_aot_global_primitive_cast_subset() {
        let root = temp_project("engine-core-aot-global-primitive-cast");
        write(
            &root,
            "src/index.js",
            r#"
function describe(value) {
  return String(value) + ":" + String(Boolean(value))
}
console.log("casts", String(12), String(true), String(null), String(undefined), Boolean(0), Boolean("x"), describe("go"), describe(0))
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
            module_path: Some("example.com/aot-global-primitive-cast".to_string()),
            output_kind: EmitGoOutputKind::Main,
            ir_snapshot: None,
        });

        assert!(
            !response
                .diagnostics
                .iter()
                .any(|diagnostic| diagnostic.code == "EXECUTABLE_JS_CODEGEN_NOT_IMPLEMENTED"),
            "diagnostics={:?}",
            response.diagnostics
        );
        assert!(!response.files[0].contents.contains("tsgodownrt.RunProgram"));
        assert!(response.files[0].contents.contains("tsgodownToString"));

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
            "casts 12 true null undefined false true go:true 0:false\n"
        );
    }

    #[test]
    fn emit_go_runs_aot_typeof_narrowed_array_includes_subset() {
        let root = temp_project("engine-core-aot-typeof-narrowed-array-includes");
        write(
            &root,
            "src/index.js",
            r#"
function parseBoolean(value) {
  if (typeof value === "string") {
    return !["false", "0", "no", "off", ""].includes(value.toLowerCase())
  }
  return Boolean(value)
}
console.log("narrow-includes", parseBoolean("YES"), parseBoolean("off"), parseBoolean(1), parseBoolean(0))
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
            module_path: Some("example.com/aot-typeof-narrowed-array-includes".to_string()),
            output_kind: EmitGoOutputKind::Main,
            ir_snapshot: None,
        });

        assert!(
            !response
                .diagnostics
                .iter()
                .any(|diagnostic| diagnostic.code == "EXECUTABLE_JS_CODEGEN_NOT_IMPLEMENTED"),
            "diagnostics={:?}",
            response.diagnostics
        );
        assert!(!response.files[0].contents.contains("tsgodownrt.RunProgram"));
        assert!(response.files[0].contents.contains("strings.ToLower"));

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
            "narrow-includes true false true false\n"
        );
    }

    #[test]
    fn emit_go_runs_aot_process_stdout_istty_subset() {
        let root = temp_project("engine-core-aot-process-stdout-istty");
        write(
            &root,
            "src/index.js",
            r#"
function supportsAnsi() {
  return process.stdout.isTTY
}
console.log("tty", supportsAnsi())
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
            module_path: Some("example.com/aot-process-stdout-istty".to_string()),
            output_kind: EmitGoOutputKind::Main,
            ir_snapshot: None,
        });

        assert!(
            !response
                .diagnostics
                .iter()
                .any(|diagnostic| diagnostic.code == "EXECUTABLE_JS_CODEGEN_NOT_IMPLEMENTED"),
            "diagnostics={:?}",
            response.diagnostics
        );
        assert!(!response.files[0].contents.contains("tsgodownrt.RunProgram"));
        assert!(response.files[0].contents.contains("tsgodownStdoutIsTTY"));

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
        assert_eq!(String::from_utf8_lossy(&output.stdout), "tty false\n");
    }

    #[test]
    fn emit_go_runs_aot_template_conditional_string_subset() {
        let root = temp_project("engine-core-aot-template-conditional-string");
        write(
            &root,
            "src/index.js",
            r#"
function supportsAnsi() {
  return false
}
function dim(text) {
  return supportsAnsi() ? `dim:${text}` : text
}
console.log("template", dim("plain"))
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
            module_path: Some("example.com/aot-template-conditional-string".to_string()),
            output_kind: EmitGoOutputKind::Main,
            ir_snapshot: None,
        });

        assert!(
            !response
                .diagnostics
                .iter()
                .any(|diagnostic| diagnostic.code == "EXECUTABLE_JS_CODEGEN_NOT_IMPLEMENTED"),
            "diagnostics={:?}",
            response.diagnostics
        );
        assert!(!response.files[0].contents.contains("tsgodownrt.RunProgram"));
        assert!(response.files[0].contents.contains("dim(\"plain\")"));

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
        assert_eq!(String::from_utf8_lossy(&output.stdout), "template plain\n");
    }

    #[test]
    fn emit_go_runs_aot_console_error_subset() {
        let root = temp_project("engine-core-aot-console-error");
        write(
            &root,
            "src/index.js",
            r#"
function warn(message) {
  console.error(`warn:${message}`)
}
warn("disk")
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
            module_path: Some("example.com/aot-console-error".to_string()),
            output_kind: EmitGoOutputKind::Main,
            ir_snapshot: None,
        });

        assert!(
            !response
                .diagnostics
                .iter()
                .any(|diagnostic| diagnostic.code == "EXECUTABLE_JS_CODEGEN_NOT_IMPLEMENTED"),
            "diagnostics={:?}",
            response.diagnostics
        );
        assert!(!response.files[0].contents.contains("tsgodownrt.RunProgram"));
        assert!(response.files[0]
            .contents
            .contains("fmt.Fprintln(os.Stderr"));

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
        assert_eq!(String::from_utf8_lossy(&output.stdout), "");
        assert_eq!(String::from_utf8_lossy(&output.stderr), "warn:disk\n");
    }

    #[test]
    fn emit_go_runs_aot_process_env_static_lookup_subset() {
        let root = temp_project("engine-core-aot-process-env-static");
        write(
            &root,
            "src/index.js",
            r#"
function flag() {
  return process.env.TSGODOWN_AOT_ENV && process.env.TSGODOWN_AOT_ENV.length > 2
}
console.log("env", process.env.TSGODOWN_AOT_ENV, flag())
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
            module_path: Some("example.com/aot-process-env-static".to_string()),
            output_kind: EmitGoOutputKind::Main,
            ir_snapshot: None,
        });

        assert!(
            !response
                .diagnostics
                .iter()
                .any(|diagnostic| diagnostic.code == "EXECUTABLE_JS_CODEGEN_NOT_IMPLEMENTED"),
            "diagnostics={:?}",
            response.diagnostics
        );
        assert!(!response.files[0].contents.contains("tsgodownrt.RunProgram"));
        assert!(response.files[0].contents.contains("os.Getenv"));

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
            .env("TSGODOWN_AOT_ENV", "on")
            .current_dir(&out_dir)
            .output()
            .expect("run generated go");
        assert!(
            output.status.success(),
            "go run failed: stdout={} stderr={}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
        assert_eq!(String::from_utf8_lossy(&output.stdout), "env on false\n");
    }

    #[test]
    fn emit_go_runs_aot_process_cwd_and_version_subset() {
        let root = temp_project("engine-core-aot-process-cwd-version");
        write(
            &root,
            "src/index.js",
            r#"
const cwd = process.cwd
const options = { cwd: process.cwd }
process.chdir(process.cwd())
process.on("exit", function () {})
process.emitWarning("tsgodown")
console.log("process", process.version, process.versions.node, process.cwd().length > 0, cwd().length > 0, options.cwd().length > 0, process.execPath.length > 0, process.arch.length > 0, process.getuid() >= 0, process.getgid() >= 0, typeof process.getuid === "function", Boolean(process.chdir), Boolean(process.emitWarning), Boolean(process.nextTick), Boolean(process.on), Boolean(process.stdin), Boolean(process.stdout), Boolean(process.stderr), Boolean(process.channel))
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
            module_path: Some("example.com/aot-process-cwd-version".to_string()),
            output_kind: EmitGoOutputKind::Main,
            ir_snapshot: None,
        });

        assert!(
            !response
                .diagnostics
                .iter()
                .any(|diagnostic| diagnostic.code == "EXECUTABLE_JS_CODEGEN_NOT_IMPLEMENTED"),
            "diagnostics={:?}",
            response.diagnostics
        );
        assert!(!response.files[0].contents.contains("tsgodownrt.RunProgram"));

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
            "process v24.15.0 24.15.0 true true true true true true true true true true true true true true true false\n"
        );
    }

    #[test]
    fn emit_go_runs_aot_process_env_versions_platform_subset() {
        let root = temp_project("engine-core-aot-process-env-versions-platform");
        write(
            &root,
            "src/index.js",
            r#"
const env = process.env
console.log("process-props", Boolean(process), Boolean(env), process.versions.node, process.platform.length > 0)
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
            module_path: Some("example.com/aot-process-env-versions-platform".to_string()),
            output_kind: EmitGoOutputKind::Main,
            ir_snapshot: None,
        });

        assert!(
            !response
                .diagnostics
                .iter()
                .any(|diagnostic| diagnostic.code == "EXECUTABLE_JS_CODEGEN_NOT_IMPLEMENTED"),
            "diagnostics={:?}",
            response.diagnostics
        );
        assert!(!response.files[0].contents.contains("tsgodownrt.RunProgram"));

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
            "process-props true true 24.15.0 true\n"
        );
    }

    #[test]
    fn emit_go_runs_aot_path_join_resolve_and_os_homedir_subset() {
        let root = temp_project("engine-core-aot-path-os-subset");
        write(
            &root,
            "src/index.js",
            r#"
const path = require("path")
const os = require("os")
const parsed = path.parse("/tmp/app.txt")
console.log("path-os", path.join("a", "b", "..", "c"), path.resolve("file").length > 0, os.homedir().length > 0, path.posix.sep, path.win32.sep, path.basename("/tmp/app.exe", ".exe"), path.dirname("/tmp/app.txt"), path.isAbsolute("/tmp"), path.relative("/tmp/a", "/tmp/a/b/c"), parsed.dir, parsed.base, parsed.ext, parsed.name, path.normalize("a/../b").length > 0, path.delimiter.length > 0)
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
            module_path: Some("example.com/aot-path-os-subset".to_string()),
            output_kind: EmitGoOutputKind::Main,
            ir_snapshot: None,
        });

        assert!(
            !response
                .diagnostics
                .iter()
                .any(|diagnostic| diagnostic.code == "EXECUTABLE_JS_CODEGEN_NOT_IMPLEMENTED"),
            "diagnostics={:?}",
            response.diagnostics
        );
        assert!(!response.files[0].contents.contains("tsgodownrt.RunProgram"));

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
            "path-os a/c true true / \\ app /tmp true b/c /tmp app.txt .txt app true true\n"
        );
    }

    #[test]
    fn emit_go_runs_aot_fs_exists_sync_subset() {
        let root = temp_project("engine-core-aot-fs-exists-sync");
        write(
            &root,
            "src/index.js",
            r#"
const fs = require("fs")
const file = fs.statSync("present.txt")
const dir = fs.statSync(".")
console.log("fs", fs.existsSync("present.txt"), fs.existsSync("missing.txt"), file.isFile(), file.isDirectory(), file.mode >= 0, dir.isDirectory(), file.isSymbolicLink())
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
            module_path: Some("example.com/aot-fs-exists-sync".to_string()),
            output_kind: EmitGoOutputKind::Main,
            ir_snapshot: None,
        });

        assert!(
            !response
                .diagnostics
                .iter()
                .any(|diagnostic| diagnostic.code == "EXECUTABLE_JS_CODEGEN_NOT_IMPLEMENTED"),
            "diagnostics={:?}",
            response.diagnostics
        );
        assert!(!response.files[0].contents.contains("tsgodownrt.RunProgram"));

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
        write(&out_dir, "present.txt", "ok");

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
            "fs true false true false true true false\n"
        );
    }

    #[test]
    fn emit_go_runs_aot_buffer_from_subset() {
        let root = temp_project("engine-core-aot-buffer-from");
        write(
            &root,
            "src/index.js",
            r#"
const utf8 = Buffer.from("abc", "utf8")
const hex = Buffer.from("ff", "hex")
const base64 = Buffer.from("YQ==", "base64")
const array = Buffer.from([65, 66])
const empty = Buffer.alloc(2)
const filled = Buffer.alloc(2, 7)
console.log("buffer", utf8.length, hex[0], base64[0], array.length, array[1], empty.length, filled[0], Buffer.isBuffer(filled), Buffer.isBuffer("x"))
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
            module_path: Some("example.com/aot-buffer-from".to_string()),
            output_kind: EmitGoOutputKind::Main,
            ir_snapshot: None,
        });

        assert!(
            !response
                .diagnostics
                .iter()
                .any(|diagnostic| diagnostic.code == "EXECUTABLE_JS_CODEGEN_NOT_IMPLEMENTED"),
            "diagnostics={:?}",
            response.diagnostics
        );
        assert!(!response.files[0].contents.contains("tsgodownrt.RunProgram"));

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
            "buffer 3 255 97 2 66 2 7 true false\n"
        );
    }

    #[test]
    fn emit_go_runs_aot_uint8array_random_fill_slice_subset() {
        let root = temp_project("engine-core-aot-uint8array-random-fill-slice");
        write(
            &root,
            "src/index.js",
            r#"
import { randomFillSync } from "node:crypto"
const pool = new Uint8Array(8)
let ptr = pool.length
function take() {
  if (ptr > pool.length - 4) {
    randomFillSync(pool)
    ptr = 0
  }
  const out = pool.slice(ptr, (ptr += 4))
  out[0] = 7
  return out.length + out[0] + ptr
}
console.log("bytes", take(), take(), Uint8Array.of(1, 2, 255)[2])
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
            module_path: Some("example.com/aot-uint8array-random-fill-slice".to_string()),
            output_kind: EmitGoOutputKind::Main,
            ir_snapshot: None,
        });

        assert!(
            !response
                .diagnostics
                .iter()
                .any(|diagnostic| diagnostic.code == "EXECUTABLE_JS_CODEGEN_NOT_IMPLEMENTED"),
            "diagnostics={:?}",
            response.diagnostics
        );
        assert!(!response.files[0].contents.contains("tsgodownrt.RunProgram"));
        assert!(response.files[0].contents.contains("rand.Read"));
        assert!(response.files[0].contents.contains("make([]byte"));

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
        assert_eq!(String::from_utf8_lossy(&output.stdout), "bytes 15 19 255\n");
    }

    #[test]
    fn emit_go_runs_aot_any_array_index_number_to_string_subset() {
        let root = temp_project("engine-core-aot-any-array-index-number-to-string");
        write(
            &root,
            "src/index.js",
            r#"
const values = []
for (let i = 0; i < 4; ++i) {
  values.push((i + 0x100).toString(16).slice(1))
}
console.log("hex", values[0], values[3], [values[1]][0])
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
            module_path: Some("example.com/aot-any-array-index-number-to-string".to_string()),
            output_kind: EmitGoOutputKind::Main,
            ir_snapshot: None,
        });

        assert!(
            !response
                .diagnostics
                .iter()
                .any(|diagnostic| diagnostic.code == "EXECUTABLE_JS_CODEGEN_NOT_IMPLEMENTED"),
            "diagnostics={:?}",
            response.diagnostics
        );
        assert!(!response.files[0].contents.contains("tsgodownrt.RunProgram"));
        assert!(response.files[0]
            .contents
            .contains("var src_index_js_values []any"));
        assert!(response.files[0].contents.contains("strconv.FormatInt"));
        assert!(response.files[0].contents.contains("tsgodownAnyArrayAt"));

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
        assert_eq!(String::from_utf8_lossy(&output.stdout), "hex 00 03 01\n");
    }

    #[test]
    fn emit_go_runs_aot_nested_any_array_numeric_index_subset() {
        let root = temp_project("engine-core-aot-nested-any-array-numeric-index");
        write(
            &root,
            "src/index.js",
            r#"
const byteToHex = []
for (let i = 0; i < 256; ++i) {
  byteToHex.push((i + 0x100).toString(16).slice(1))
}
function unsafeEncode(bytes, offset) {
  return (
    byteToHex[bytes[offset + 0]] +
    byteToHex[bytes[offset + 1]] +
    "-" +
    byteToHex[bytes[offset + 2]]
  ).toLowerCase()
}
function encode(arr, offset) {
  const value = unsafeEncode(arr, offset)
  return value
}
console.log("hex-nested", encode([0, 15, 255], 0))
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
            module_path: Some("example.com/aot-nested-any-array-numeric-index".to_string()),
            output_kind: EmitGoOutputKind::Main,
            ir_snapshot: None,
        });

        assert!(
            !response
                .diagnostics
                .iter()
                .any(|diagnostic| diagnostic.code == "EXECUTABLE_JS_CODEGEN_NOT_IMPLEMENTED"),
            "diagnostics={:?}",
            response.diagnostics
        );
        assert!(!response.files[0].contents.contains("tsgodownrt.RunProgram"));
        assert!(response.files[0].contents.contains("tsgodownAnyArrayAt"));
        assert!(response.files[0].contents.contains("tsgodownToFloat64"));

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
            "hex-nested 000f-ff\n"
        );
    }

    #[test]
    fn emit_go_runs_aot_string_array_length_assignment_subset() {
        let root = temp_project("engine-core-aot-string-array-length-assignment");
        write(
            &root,
            "src/index.js",
            r#"
const hex = (function () {
  const table = []
  for (let i = 0; i < 4; ++i) {
    table[table.length] = "%" + ((i < 2 ? "0" : "") + i.toString(16)).toUpperCase()
  }
  return table
}())
function encode(input) {
  const out = []
  for (let i = 0; i < input.length; ++i) {
    const c = input.charCodeAt(i)
    out[out.length] = c < 4 ? hex[c] : input.charAt(i)
  }
  return out.join("")
}
console.log("string-array-set", encode("\u0000\u0001AZ"))
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
            module_path: Some("example.com/aot-string-array-length-assignment".to_string()),
            output_kind: EmitGoOutputKind::Main,
            ir_snapshot: None,
        });

        assert!(
            !response
                .diagnostics
                .iter()
                .any(|diagnostic| diagnostic.code == "EXECUTABLE_JS_CODEGEN_NOT_IMPLEMENTED"),
            "diagnostics={:?}",
            response.diagnostics
        );
        assert!(!response.files[0].contents.contains("tsgodownrt.RunProgram"));
        assert!(response.files[0]
            .contents
            .contains("tsgodownStringArraySet"));
        assert!(response.files[0].contents.contains("strings.Join"));

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
            "string-array-set %00%01AZ\n"
        );
    }

    #[test]
    fn emit_go_runs_aot_regexp_test_subset() {
        let root = temp_project("engine-core-aot-regexp-test");
        write(
            &root,
            "src/index.js",
            r#"
function looksLikeNumber(x) {
  if (x === null || x === undefined) {
    return false
  }
  if (typeof x === "number") {
    return true
  }
  if (/^0x[0-9a-f]+$/i.test(x)) {
    return true
  }
  if (/^0[^.]/.test(x)) {
    return false
  }
  return /^[-]?(?:\d+(?:\.\d*)?|\.\d+)(e[-+]?\d+)?$/.test(x)
}
console.log("regexp", looksLikeNumber(null), looksLikeNumber(undefined), looksLikeNumber(12), looksLikeNumber("0x1f"), looksLikeNumber("012"), looksLikeNumber("1.5e-2"), looksLikeNumber("no"))
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
            module_path: Some("example.com/aot-regexp-test".to_string()),
            output_kind: EmitGoOutputKind::Main,
            ir_snapshot: None,
        });

        assert!(
            !response
                .diagnostics
                .iter()
                .any(|diagnostic| diagnostic.code == "EXECUTABLE_JS_CODEGEN_NOT_IMPLEMENTED"),
            "diagnostics={:?}",
            response.diagnostics
        );
        assert!(!response.files[0].contents.contains("tsgodownrt.RunProgram"));
        assert!(response.files[0].contents.contains("regexp.MustCompile"));

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
            "regexp false false true true false true false\n"
        );
    }

    #[test]
    fn emit_go_runs_aot_regexp_constructor_array_subset() {
        let root = temp_project("engine-core-aot-regexp-constructor-array");
        write(
            &root,
            "src/index.js",
            r#"
const patterns = []
let index = 0
function add(pattern, ignoreCase) {
  patterns[index] = new RegExp(pattern, ignoreCase ? "i" : undefined)
  index++
}
add("^abc$", true)
add("^abc$", false)
console.log("regexp-new", patterns[0].test("ABC"), patterns[1].test("ABC"))
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
            module_path: Some("example.com/aot-regexp-constructor-array".to_string()),
            output_kind: EmitGoOutputKind::Main,
            ir_snapshot: None,
        });

        assert!(
            !response
                .diagnostics
                .iter()
                .any(|diagnostic| diagnostic.code == "EXECUTABLE_JS_CODEGEN_NOT_IMPLEMENTED"),
            "diagnostics={:?}",
            response.diagnostics
        );
        assert!(!response.files[0].contents.contains("tsgodownrt.RunProgram"));
        assert!(response.files[0].contents.contains("tsgodownRegexpPattern"));
        assert!(response.files[0]
            .contents
            .contains("var src_index_js_patterns []string"));

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
            "regexp-new true false\n"
        );
    }

    #[test]
    fn emit_go_runs_aot_imported_default_regexp_test_subset() {
        let root = temp_project("engine-core-aot-imported-default-regexp-test");
        write(
            &root,
            "src/regex.js",
            r#"
export default /^(?:[a-f]{2}|000)$/i
"#,
        );
        write(
            &root,
            "src/index.js",
            r#"
import REGEX from "./regex.js"
function validate(value) {
  return typeof value === "string" && REGEX.test(value)
}
console.log("regex-import", validate("AF"), validate("000"), validate("xyz"), validate(null))
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
            module_path: Some("example.com/aot-imported-default-regexp-test".to_string()),
            output_kind: EmitGoOutputKind::Main,
            ir_snapshot: None,
        });

        assert!(
            !response
                .diagnostics
                .iter()
                .any(|diagnostic| diagnostic.code == "EXECUTABLE_JS_CODEGEN_NOT_IMPLEMENTED"),
            "diagnostics={:?}",
            response.diagnostics
        );
        assert!(!response.files[0].contents.contains("tsgodownrt.RunProgram"));
        assert!(response.files[0]
            .contents
            .contains("var src_regex_js_default_ string"));
        assert!(response.files[0].contents.contains("regexp.MustCompile"));

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
            "regex-import true true false false\n"
        );
    }

    #[test]
    fn emit_go_runs_aot_default_object_builtin_function_subset() {
        let root = temp_project("engine-core-aot-default-object-builtin-function");
        write(
            &root,
            "src/native.js",
            r#"
import { randomUUID } from "node:crypto"
export default { randomUUID }
"#,
        );
        write(
            &root,
            "src/index.js",
            r#"
import native from "./native.js"
const uuid = native.randomUUID()
const ok = /^[0-9a-f]{8}-[0-9a-f]{4}-4[0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}$/.test(uuid)
console.log("native-object", ok, typeof native.randomUUID)
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
            module_path: Some("example.com/aot-default-object-builtin-function".to_string()),
            output_kind: EmitGoOutputKind::Main,
            ir_snapshot: None,
        });

        assert!(
            !response
                .diagnostics
                .iter()
                .any(|diagnostic| diagnostic.code == "EXECUTABLE_JS_CODEGEN_NOT_IMPLEMENTED"),
            "diagnostics={:?}",
            response.diagnostics
        );
        assert!(!response.files[0].contents.contains("tsgodownrt.RunProgram"));
        assert!(response.files[0]
            .contents
            .contains("tsgodownCryptoRandomUUID"));
        assert!(response.files[0].contents.contains("crypto/rand"));
        assert!(response.files[0].contents.contains("encoding/hex"));

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
            "native-object true function\n"
        );
    }

    #[test]
    fn emit_go_runs_aot_error_guarded_bool_function_subset() {
        let root = temp_project("engine-core-aot-error-guarded-bool-function");
        write(
            &root,
            "src/regex.js",
            r#"
export default /^[0-9a-f]{8}-[0-9a-f]{4}-[1-8][0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}$/i
"#,
        );
        write(
            &root,
            "src/validate.js",
            r#"
import REGEX from "./regex.js"
function validate(value) {
  return typeof value === "string" && REGEX.test(value)
}
export default validate
"#,
        );
        write(
            &root,
            "src/version.js",
            r#"
import validate from "./validate.js"
function version(uuid) {
  if (!validate(uuid)) {
    throw TypeError("Invalid UUID")
  }
  return parseInt(uuid.slice(14, 15), 16)
}
export default version
"#,
        );
        write(
            &root,
            "src/index.js",
            r#"
import version from "./version.js"
console.log("guarded-version", version("6fa459ea-ee8a-3ca4-894e-db77e160355e"))
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
            module_path: Some("example.com/aot-error-guarded-bool-function".to_string()),
            output_kind: EmitGoOutputKind::Main,
            ir_snapshot: None,
        });

        assert!(
            !response
                .diagnostics
                .iter()
                .any(|diagnostic| diagnostic.code == "EXECUTABLE_JS_CODEGEN_NOT_IMPLEMENTED"),
            "diagnostics={:?}",
            response.diagnostics
        );
        assert!(!response.files[0].contents.contains("tsgodownrt.RunProgram"));
        assert!(response.files[0].contents.contains("panic(tsgodownError"));
        assert!(response.files[0].contents.contains("regexp.MustCompile"));

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
            "guarded-version 3\n"
        );
    }

    #[test]
    fn emit_go_runs_aot_string_match_subset() {
        let root = temp_project("engine-core-aot-string-match");
        write(
            &root,
            "src/index.js",
            r#"
function camelCase(str) {
  const isCamelCase = str !== str.toLowerCase() && str !== str.toUpperCase()
  if (!isCamelCase) {
    str = str.toLowerCase()
  }
  if (str.indexOf("-") === -1 && str.indexOf("_") === -1) {
    return str
  } else {
    let camelcase = ""
    let nextChrUpper = false
    const leadingHyphens = str.match(/^-+/)
    for (let i = leadingHyphens ? leadingHyphens[0].length : 0; i < str.length; i++) {
      let chr = str.charAt(i)
      if (nextChrUpper) {
        nextChrUpper = false
        chr = chr.toUpperCase()
      }
      if (i !== 0 && (chr === "-" || chr === "_")) {
        nextChrUpper = true
      } else if (chr !== "-" && chr !== "_") {
        camelcase += chr
      }
    }
    return camelcase
  }
}
console.log("string-match", camelCase("--foo-bar"), camelCase("foo_bar"), camelCase("fooBar"))
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
            module_path: Some("example.com/aot-string-match".to_string()),
            output_kind: EmitGoOutputKind::Main,
            ir_snapshot: None,
        });

        assert!(
            !response
                .diagnostics
                .iter()
                .any(|diagnostic| diagnostic.code == "EXECUTABLE_JS_CODEGEN_NOT_IMPLEMENTED"),
            "diagnostics={:?}",
            response.diagnostics
        );
        assert!(!response.files[0].contents.contains("tsgodownrt.RunProgram"));
        assert!(response.files[0].contents.contains("tsgodownStringMatch"));

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
            "string-match fooBar fooBar fooBar\n"
        );
    }

    #[test]
    fn emit_go_runs_aot_char_code_subset() {
        let root = temp_project("engine-core-aot-char-code");
        write(
            &root,
            "src/index.js",
            r#"
function charFromCodepoint(c) {
  if (c <= 0xFFFF) {
    return String.fromCharCode(c)
  }
  return String.fromCharCode(
    ((c - 0x010000) >> 10) + 0xD800,
    ((c - 0x010000) & 0x03FF) + 0xDC00
  )
}
function firstCode(string, pos) {
  return string.charCodeAt(pos)
}
console.log("char-code", charFromCodepoint(65), firstCode("AZ", 1), ((1024 >> 4) & 63))
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
            module_path: Some("example.com/aot-char-code".to_string()),
            output_kind: EmitGoOutputKind::Main,
            ir_snapshot: None,
        });

        assert!(
            !response
                .diagnostics
                .iter()
                .any(|diagnostic| diagnostic.code == "EXECUTABLE_JS_CODEGEN_NOT_IMPLEMENTED"),
            "diagnostics={:?}",
            response.diagnostics
        );
        assert!(!response.files[0].contents.contains("tsgodownrt.RunProgram"));
        assert!(response.files[0]
            .contents
            .contains("tsgodownStringFromCharCode"));

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
            "char-code A 90 0\n"
        );
    }

    #[test]
    fn emit_go_runs_aot_string_replace_subset() {
        let root = temp_project("engine-core-aot-string-replace");
        write(
            &root,
            "src/index.js",
            r#"
function normalize(value) {
  return value
    .replace(/\r\n?/mg, "\n")
    .replace(/_/g, "")
    .replace(/(li)(ne)/, "$2$1")
    .replace(/\n$/, "")
    .replace(/\n/g, "|")
}
console.log("replace", normalize("a_b\r\nline\n"), "feed".replace("e", "E"))
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
            module_path: Some("example.com/aot-string-replace".to_string()),
            output_kind: EmitGoOutputKind::Main,
            ir_snapshot: None,
        });

        assert!(
            !response
                .diagnostics
                .iter()
                .any(|diagnostic| diagnostic.code == "EXECUTABLE_JS_CODEGEN_NOT_IMPLEMENTED"),
            "diagnostics={:?}",
            response.diagnostics
        );
        assert!(!response.files[0].contents.contains("tsgodownrt.RunProgram"));
        assert!(response.files[0].contents.contains("tsgodownRegexpReplace"));

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
            "replace ab|neli fEed\n"
        );
    }

    #[test]
    fn emit_go_runs_aot_string_prototype_replace_alias_subset() {
        let root = temp_project("engine-core-aot-string-prototype-replace-alias");
        write(
            &root,
            "src/index.js",
            r#"
const replace = String.prototype.replace
const percentTwenties = /%20/g
function format(value) {
  return replace.call(value, percentTwenties, "+")
}
console.log("replace-alias", format("a%20b%20c"))
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
            module_path: Some("example.com/aot-string-prototype-replace-alias".to_string()),
            output_kind: EmitGoOutputKind::Main,
            ir_snapshot: None,
        });

        assert!(
            !response
                .diagnostics
                .iter()
                .any(|diagnostic| diagnostic.code == "EXECUTABLE_JS_CODEGEN_NOT_IMPLEMENTED"),
            "diagnostics={:?}",
            response.diagnostics
        );
        assert!(!response.files[0].contents.contains("tsgodownrt.RunProgram"));
        assert!(response.files[0].contents.contains("tsgodownRegexpReplace"));
        assert!(!response.files[0].contents.contains("var replace"));

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
            "replace-alias a+b+c\n"
        );
    }

    #[test]
    fn emit_go_runs_aot_builtin_function_alias_subset() {
        let root = temp_project("engine-core-aot-builtin-function-alias");
        write(
            &root,
            "src/index.js",
            r#"
const isArray = Array.isArray
const has = Object.prototype.hasOwnProperty
function describe(value, key) {
  return String(isArray(value)) + ":" + String(has.call(value, key)) + ":" + String(has.call(Object.prototype, key))
}
console.log("builtin-alias", describe(["x", "y"], "1"), describe({ name: "kim" }, "name"), describe({ safe: true }, "toString"))
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
            module_path: Some("example.com/aot-builtin-function-alias".to_string()),
            output_kind: EmitGoOutputKind::Main,
            ir_snapshot: None,
        });

        assert!(
            !response
                .diagnostics
                .iter()
                .any(|diagnostic| diagnostic.code == "EXECUTABLE_JS_CODEGEN_NOT_IMPLEMENTED"),
            "diagnostics={:?}",
            response.diagnostics
        );
        assert!(!response.files[0].contents.contains("tsgodownrt.RunProgram"));
        assert!(!response.files[0].contents.contains("var isArray"));
        assert!(!response.files[0].contents.contains("var has"));
        assert!(response.files[0].contents.contains("tsgodownObjectHasOwn"));

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
            "builtin-alias true:true:false false:true:false false:false:true\n"
        );
    }

    #[test]
    fn emit_go_runs_aot_array_push_apply_alias_subset() {
        let root = temp_project("engine-core-aot-array-push-apply-alias");
        write(
            &root,
            "src/index.js",
            r#"
const isArray = Array.isArray
const push = Array.prototype.push
function pushToArray(arr, valueOrArray) {
  push.apply(arr, isArray(valueOrArray) ? valueOrArray : [valueOrArray])
}
const values = []
pushToArray(values, "a")
pushToArray(values, ["b", "c"])
console.log("push-apply-alias", values.join("|"))
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
            module_path: Some("example.com/aot-array-push-apply-alias".to_string()),
            output_kind: EmitGoOutputKind::Main,
            ir_snapshot: None,
        });

        assert!(
            !response
                .diagnostics
                .iter()
                .any(|diagnostic| diagnostic.code == "EXECUTABLE_JS_CODEGEN_NOT_IMPLEMENTED"),
            "diagnostics={:?}",
            response.diagnostics
        );
        assert!(!response.files[0].contents.contains("tsgodownrt.RunProgram"));
        assert!(!response.files[0].contents.contains("var push"));

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
            "push-apply-alias a|b|c\n"
        );
    }

    #[test]
    fn emit_go_runs_aot_default_object_options_subset() {
        let root = temp_project("engine-core-aot-default-object-options");
        write(
            &root,
            "src/index.js",
            r#"
const escape = (s, { windowsPathsNoEscape = false, magicalBraces = false } = {}) => {
  if (magicalBraces) {
    return windowsPathsNoEscape
      ? s.replace(/[?*()[\]{}]/g, "[$&]")
      : s.replace(/[?*()[\]\\{}]/g, "\\$&")
  }
  return windowsPathsNoEscape
    ? s.replace(/[?*()[\]]/g, "[$&]")
    : s.replace(/[?*()[\]\\]/g, "\\$&")
}
console.log("options", escape("a*b"), escape("a*b", { windowsPathsNoEscape: true }), escape("{a*b}", { magicalBraces: true }))
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
            module_path: Some("example.com/aot-default-object-options".to_string()),
            output_kind: EmitGoOutputKind::Main,
            ir_snapshot: None,
        });

        assert!(
            !response
                .diagnostics
                .iter()
                .any(|diagnostic| diagnostic.code == "EXECUTABLE_JS_CODEGEN_NOT_IMPLEMENTED"),
            "diagnostics={:?}",
            response.diagnostics
        );
        assert!(!response.files[0].contents.contains("tsgodownrt.RunProgram"));
        assert!(response.files[0].contents.contains("tsgodownObjectProp"));

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
            "options a\\*b a[*]b \\{a\\*b\\}\n"
        );
    }

    #[test]
    fn emit_go_runs_aot_parse_int_nan_subset() {
        let root = temp_project("engine-core-aot-parse-int-nan");
        write(
            &root,
            "src/index.js",
            r#"
function numeric(str) {
  return !isNaN(str) ? parseInt(str, 10) : str.charCodeAt(0)
}
console.log("parse-int", numeric("42"), numeric("A"), parseInt("ff", 16))
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
            module_path: Some("example.com/aot-parse-int-nan".to_string()),
            output_kind: EmitGoOutputKind::Main,
            ir_snapshot: None,
        });

        assert!(
            !response
                .diagnostics
                .iter()
                .any(|diagnostic| diagnostic.code == "EXECUTABLE_JS_CODEGEN_NOT_IMPLEMENTED"),
            "diagnostics={:?}",
            response.diagnostics
        );
        assert!(!response.files[0].contents.contains("tsgodownrt.RunProgram"));
        assert!(response.files[0].contents.contains("tsgodownParseInt"));

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
            "parse-int 42 65 255\n"
        );
    }

    #[test]
    fn emit_go_runs_aot_number_array_stack_subset() {
        let root = temp_project("engine-core-aot-number-array-stack");
        write(
            &root,
            "src/index.js",
            r#"
function collect() {
  let stack, result
  stack = []
  stack.push(1)
  stack.push(7)
  result = [stack.length, stack.pop(), stack[0]]
  return result
}
const values = collect()
console.log("num-array", values[0], values[1], values[2])
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
            module_path: Some("example.com/aot-number-array-stack".to_string()),
            output_kind: EmitGoOutputKind::Main,
            ir_snapshot: None,
        });

        assert!(
            !response
                .diagnostics
                .iter()
                .any(|diagnostic| diagnostic.code == "EXECUTABLE_JS_CODEGEN_NOT_IMPLEMENTED"),
            "diagnostics={:?}",
            response.diagnostics
        );
        assert!(!response.files[0].contents.contains("tsgodownrt.RunProgram"));
        assert!(response.files[0]
            .contents
            .contains("tsgodownNumberArrayPop"));

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
        assert_eq!(String::from_utf8_lossy(&output.stdout), "num-array 2 7 1\n");
    }

    #[test]
    fn emit_go_runs_aot_var_retyped_for_loop_subset() {
        let root = temp_project("engine-core-aot-var-retyped-for-loop");
        write(
            &root,
            "src/index.js",
            r#"
function repeat(string, count) {
  var result = "", cycle
  for (cycle = 0; cycle < count; cycle += 1) {
    result += string
  }
  return result
}
console.log("repeat", repeat("ab", 3), repeat(".", 0))
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
            module_path: Some("example.com/aot-var-retyped-for-loop".to_string()),
            output_kind: EmitGoOutputKind::Main,
            ir_snapshot: None,
        });

        assert!(
            !response
                .diagnostics
                .iter()
                .any(|diagnostic| diagnostic.code == "EXECUTABLE_JS_CODEGEN_NOT_IMPLEMENTED"),
            "diagnostics={:?}",
            response.diagnostics
        );
        assert!(!response.files[0].contents.contains("tsgodownrt.RunProgram"));
        assert!(response.files[0].contents.contains("tsgodownToFloat64"));

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
        assert_eq!(String::from_utf8_lossy(&output.stdout), "repeat ababab \n");
    }

    #[test]
    fn emit_go_runs_aot_string_index_slice_subset() {
        let root = temp_project("engine-core-aot-string-index-slice");
        write(
            &root,
            "src/index.js",
            r#"
function dropEndingNewline(string) {
  return string[string.length - 1] === "\n" ? string.slice(0, -1) : string
}
console.log("string-index-slice", dropEndingNewline("a\n"), dropEndingNewline("b"), "abcdef".slice(1, -1))
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
            module_path: Some("example.com/aot-string-index-slice".to_string()),
            output_kind: EmitGoOutputKind::Main,
            ir_snapshot: None,
        });

        assert!(
            !response
                .diagnostics
                .iter()
                .any(|diagnostic| diagnostic.code == "EXECUTABLE_JS_CODEGEN_NOT_IMPLEMENTED"),
            "diagnostics={:?}",
            response.diagnostics
        );
        assert!(!response.files[0].contents.contains("tsgodownrt.RunProgram"));
        assert!(response.files[0].contents.contains("tsgodownStringSlice"));

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
            "string-index-slice a b bcde\n"
        );
    }

    #[test]
    fn emit_go_runs_aot_closure_try_finally_promise_then_subset() {
        let root = temp_project("engine-core-aot-closure-try-finally-promise");
        write(
            &root,
            "src/index.mjs",
            r#"
const value = await (async () => {
  const log = []
  const base = { value: 1, nested: { flag: false } }
  const clone = { ...base, extra: [1, 2] }
  class Box {
    constructor(input) { this.input = input }
    get doubled() { return this.input * 2 }
  }
  function closure(seed) {
    let state = seed
    return (delta) => { state += delta; return state }
  }
  const next = closure(1)
  try {
    log.push(["try", next(1)])
  } finally {
    log.push(["finally", new Box(1).doubled])
  }
  await Promise.resolve().then(() => log.push(["microtask", clone.extra.at(-1)]))
  return {
    value: clone.value,
    flag: clone.nested.flag,
    eq: 1 == "1",
    strict: 1 === "1",
    log
  }
})()
console.log(JSON.stringify(value))
"#,
        );

        let response = emit_go(EmitGoRequest {
            analyze: AnalyzeRequest {
                manifest: InputManifest {
                    entry: "src/index.mjs".to_string(),
                    framework: None,
                },
                cwd: Some(root.to_string_lossy().to_string()),
                config: legacy_ir_interpreter_config(),
            },
            package_name: None,
            module_path: Some("example.com/aot-closure-try-finally-promise".to_string()),
            output_kind: EmitGoOutputKind::Main,
            ir_snapshot: None,
        });

        assert!(!response
            .diagnostics
            .iter()
            .any(|diagnostic| diagnostic.code == "EXECUTABLE_JS_CODEGEN_NOT_IMPLEMENTED"));
        assert!(!response.files[0].contents.contains("tsgodownrt.RunProgram"));
        assert!(response.files[0]
            .contents
            .contains("func(delta float64) any"));

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
            "{\"eq\":true,\"flag\":false,\"log\":[[\"try\",2],[\"finally\",2],[\"microtask\",2]],\"strict\":false,\"value\":1}\n"
        );
    }

    #[test]
    fn emit_go_runs_aot_map_function_value_cycle_subset() {
        let root = temp_project("engine-core-aot-map-function-value-cycle");
        write(
            &root,
            "src/index.mjs",
            r#"
const value = await (async () => {
  const registry = new Map()
  function define(name, factory) {
    registry.set(name, { factory, exports: {}, initialized: false })
  }
  function requireLocal(name) {
    const record = registry.get(name)
    if (!record.initialized) {
      record.initialized = true
      record.factory(record.exports, requireLocal)
    }
    return record.exports
  }
  define("a", (exports, require) => {
    exports.name = "a-1"
    exports.peer = () => require("b").name
  })
  define("b", (exports, require) => {
    exports.name = "b-1"
    exports.peer = () => require("a").name
  })
  const a = requireLocal("a")
  const b = requireLocal("b")
  return {
    argvShape: process.argv.slice(0, 1).length,
    cwdType: typeof process.cwd(),
    cycle: [a.name, a.peer(), b.peer()],
    objectKeys: Object.keys({ z: 1, a: 2, [String(1)]: 3 })
  }
})()
console.log(JSON.stringify(value))
"#,
        );

        let response = emit_go(EmitGoRequest {
            analyze: AnalyzeRequest {
                manifest: InputManifest {
                    entry: "src/index.mjs".to_string(),
                    framework: None,
                },
                cwd: Some(root.to_string_lossy().to_string()),
                config: AnalyzeConfig::default(),
            },
            package_name: None,
            module_path: Some("example.com/aot-map-function-value-cycle".to_string()),
            output_kind: EmitGoOutputKind::Main,
            ir_snapshot: None,
        });

        assert!(
            !response
                .diagnostics
                .iter()
                .any(|diagnostic| diagnostic.code == "EXECUTABLE_JS_CODEGEN_NOT_IMPLEMENTED"),
            "diagnostics={:?}",
            response.diagnostics
        );
        assert!(!response.files[0].contents.contains("tsgodownrt.RunProgram"));

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
            "{\"argvShape\":1,\"cwdType\":\"string\",\"cycle\":[\"a-1\",\"b-1\",\"a-1\"],\"objectKeys\":[\"1\",\"z\",\"a\"]}\n"
        );
    }

    #[test]
    fn emit_go_runs_aot_dynamic_node_builtin_import_subset() {
        let root = temp_project("engine-core-aot-dynamic-node-builtin-import");
        write(
            &root,
            "src/index.mjs",
            r#"
const value = await (async () => {
  const path = (await import("node:path")).default
  const { URL } = await import("node:url")
  const { Buffer } = await import("node:buffer")
  const crypto = (await import("node:crypto")).default
  const { EventEmitter } = await import("node:events")
  const querystring = (await import("node:querystring")).default
  const fs = (await import("node:fs")).default
  const os = (await import("node:os")).default
  const url = new URL("/items/1?q=a+b", "https://example.test/base/")
  const emitter = new EventEmitter()
  const events = []
  emitter.on("data", (payload) => events.push(payload))
  emitter.emit("data", { value: 1 })
  const dir = fs.mkdtempSync(path.join(os.tmpdir(), "tsgodown-fuzz-"))
  const file = path.join(dir, "value.json")
  fs.writeFileSync(file, JSON.stringify({ value: 1 }))
  const read = JSON.parse(fs.readFileSync(file, "utf8"))
  fs.rmSync(dir, { recursive: true, force: true })
  return {
    joined: path.posix.join("a", "b", "..", "c"),
    url: { pathname: url.pathname, q: url.searchParams.get("q") },
    buffer: Buffer.from("value-1").toString("base64"),
    hash: crypto.createHash("sha256").update("value-1").digest("hex").slice(0, 12),
    query: querystring.parse("a=1&a=2&b=1"),
    events,
    read
  }
})()
console.log(JSON.stringify(value))
"#,
        );

        let response = emit_go(EmitGoRequest {
            analyze: AnalyzeRequest {
                manifest: InputManifest {
                    entry: "src/index.mjs".to_string(),
                    framework: None,
                },
                cwd: Some(root.to_string_lossy().to_string()),
                config: AnalyzeConfig::default(),
            },
            package_name: None,
            module_path: Some("example.com/aot-dynamic-node-builtin-import".to_string()),
            output_kind: EmitGoOutputKind::Main,
            ir_snapshot: None,
        });

        assert!(
            !response
                .diagnostics
                .iter()
                .any(|diagnostic| diagnostic.code == "EXECUTABLE_JS_CODEGEN_NOT_IMPLEMENTED"),
            "diagnostics={:?}",
            response.diagnostics
        );
        assert!(!response.files[0].contents.contains("tsgodownrt.RunProgram"));
        assert!(response.files[0].contents.contains("tsgodownNewURL"));
        assert!(response.files[0]
            .contents
            .contains("tsgodownEventEmitterOn"));

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
            "{\"buffer\":\"dmFsdWUtMQ==\",\"events\":[{\"value\":1}],\"hash\":\"eff9eb68b7ea\",\"joined\":\"a/c\",\"query\":{\"a\":[\"1\",\"2\"],\"b\":\"1\"},\"read\":{\"value\":1},\"url\":{\"pathname\":\"/items/1\",\"q\":\"a b\"}}\n"
        );
    }

    #[test]
    fn emit_go_runs_aot_map_object_get_set_subset() {
        let root = temp_project("engine-core-aot-map-object-get-set");
        write(
            &root,
            "src/index.js",
            r#"
const registry = new Map()
registry.set("a", { name: "alpha" }).set("b", { name: "beta" })
const record = registry.get("a")
record.initialized = true
registry.delete("b")
console.log(JSON.stringify({
  size: registry.size,
  hasA: registry.has("a"),
  hasB: registry.has("b"),
  name: record.name,
  initialized: record.initialized,
  missing: registry.get("missing") == null
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
            module_path: Some("example.com/aot-map-object-get-set".to_string()),
            output_kind: EmitGoOutputKind::Main,
            ir_snapshot: None,
        });

        assert!(
            !response
                .diagnostics
                .iter()
                .any(|diagnostic| diagnostic.code == "EXECUTABLE_JS_CODEGEN_NOT_IMPLEMENTED"),
            "diagnostics={:?}",
            response.diagnostics
        );
        assert!(!response.files[0].contents.contains("tsgodownrt.RunProgram"));
        assert!(response.files[0].contents.contains("tsgodownMapSet"));

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
            "{\"hasA\":true,\"hasB\":false,\"initialized\":true,\"missing\":true,\"name\":\"alpha\",\"size\":1}\n"
        );
    }

    #[test]
    fn emit_go_runs_aot_dynamic_object_member_assignment_subset() {
        let root = temp_project("engine-core-aot-dynamic-object-member-assignment");
        write(
            &root,
            "src/index.js",
            r#"
const target = {}
target.value = 4
target.name = "node"
const nested = { child: {} }
nested.child.ready = true
console.log(JSON.stringify({
  value: target.value + 1,
  name: target.name,
  ready: nested.child.ready
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
            module_path: Some("example.com/aot-dynamic-object-member-assignment".to_string()),
            output_kind: EmitGoOutputKind::Main,
            ir_snapshot: None,
        });

        assert!(
            !response
                .diagnostics
                .iter()
                .any(|diagnostic| diagnostic.code == "EXECUTABLE_JS_CODEGEN_NOT_IMPLEMENTED"),
            "diagnostics={:?}",
            response.diagnostics
        );
        assert!(!response.files[0].contents.contains("tsgodownrt.RunProgram"));
        assert!(response.files[0].contents.contains("tsgodownObjectSetProp"));

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
            "{\"name\":\"node\",\"ready\":true,\"value\":5}\n"
        );
    }

    #[test]
    fn emit_go_runs_aot_dynamic_object_key_iteration_subset() {
        let root = temp_project("engine-core-aot-dynamic-object-key-iteration");
        write(
            &root,
            "src/index.js",
            r#"
const input = { A: "one", B: "two" }
const out = {}
for (const key of Object.keys(input)) {
  out[key] = input[key]
}
console.log("iter", Object.keys(out).join("|"), out.A)
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
            module_path: Some("example.com/aot-dynamic-object-key-iteration".to_string()),
            output_kind: EmitGoOutputKind::Main,
            ir_snapshot: None,
        });

        assert!(
            !response
                .diagnostics
                .iter()
                .any(|diagnostic| diagnostic.code == "EXECUTABLE_JS_CODEGEN_NOT_IMPLEMENTED"),
            "diagnostics={:?}",
            response.diagnostics
        );
        assert!(!response.files[0].contents.contains("tsgodownrt.RunProgram"));
        assert!(response.files[0].contents.contains("tsgodownObjectMapKeys"));
        assert!(response.files[0].contents.contains("for _, key := range"));
        assert!(response.files[0].contents.contains("tsgodownObjectSetProp"));

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
        assert_eq!(String::from_utf8_lossy(&output.stdout), "iter A|B one\n");
    }

    #[test]
    fn emit_go_runs_aot_object_keys_computed_key_subset() {
        let root = temp_project("engine-core-aot-object-keys-computed");
        write(
            &root,
            "src/index.js",
            r#"
const value = 3
console.log(JSON.stringify({
  keys: Object.keys({ z: 1, a: 2, [String(value)]: 3, "10": 4 })
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
            module_path: Some("example.com/aot-object-keys-computed".to_string()),
            output_kind: EmitGoOutputKind::Main,
            ir_snapshot: None,
        });

        assert!(
            !response
                .diagnostics
                .iter()
                .any(|diagnostic| diagnostic.code == "EXECUTABLE_JS_CODEGEN_NOT_IMPLEMENTED"),
            "diagnostics={:?}",
            response.diagnostics
        );
        assert!(!response.files[0].contents.contains("tsgodownrt.RunProgram"));
        assert!(response.files[0].contents.contains("tsgodownObjectKeys"));

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
            "{\"keys\":[\"3\",\"10\",\"z\",\"a\"]}\n"
        );
    }

    #[test]
    fn emit_go_runs_aot_tokenize_string_array_subset() {
        let root = temp_project("engine-core-aot-tokenize-string-array");
        write(
            &root,
            "src/index.js",
            r#"
function tokenizeArgString(argString) {
  if (Array.isArray(argString)) {
    return argString.map(e => typeof e !== "string" ? e + "" : e)
  }
  argString = argString.trim()
  let i = 0
  let prevC = null
  let c = null
  let opening = null
  const args = []
  for (let ii = 0; ii < argString.length; ii++) {
    prevC = c
    c = argString.charAt(ii)
    if (c === " " && !opening) {
      if (!(prevC === " ")) {
        i++
      }
      continue
    }
    if (c === opening) {
      opening = null
    } else if ((c === "'" || c === '"') && !opening) {
      opening = c
    }
    if (!args[i]) args[i] = ""
    args[i] += c
  }
  return args
}
console.log("tokenize", tokenizeArgString(["--x", 3]).join("|"), tokenizeArgString(" a 'b c'  d ").join("|"))
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
            module_path: Some("example.com/aot-tokenize-string-array".to_string()),
            output_kind: EmitGoOutputKind::Main,
            ir_snapshot: None,
        });

        assert!(
            !response
                .diagnostics
                .iter()
                .any(|diagnostic| diagnostic.code == "EXECUTABLE_JS_CODEGEN_NOT_IMPLEMENTED"),
            "diagnostics={:?}",
            response.diagnostics
        );
        assert!(!response.files[0].contents.contains("tsgodownrt.RunProgram"));
        assert!(response.files[0]
            .contents
            .contains("tsgodownStringArraySet"));

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
            "tokenize --x|3 a|'b c'|d\n"
        );
    }

    #[test]
    fn emit_go_runs_aot_string_charat_upper_fallback_subset() {
        let root = temp_project("engine-core-aot-string-charat-upper");
        write(
            &root,
            "src/index.js",
            r#"
function decamelize(str, joinString) {
  const lowercase = str.toLowerCase()
  joinString = joinString || "-"
  let notCamelcase = ""
  for (let i = 0; i < str.length; i++) {
    const chrLower = lowercase.charAt(i)
    const chrString = str.charAt(i)
    if (chrLower !== chrString && i > 0) {
      notCamelcase += `${joinString}${lowercase.charAt(i)}`
    } else {
      notCamelcase += chrString.toUpperCase()
    }
  }
  return notCamelcase
}
console.log("string-more", decamelize("fooBar", "_"), decamelize("hi", ""))
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
            module_path: Some("example.com/aot-string-charat-upper".to_string()),
            output_kind: EmitGoOutputKind::Main,
            ir_snapshot: None,
        });

        assert!(
            !response
                .diagnostics
                .iter()
                .any(|diagnostic| diagnostic.code == "EXECUTABLE_JS_CODEGEN_NOT_IMPLEMENTED"),
            "diagnostics={:?}",
            response.diagnostics
        );
        assert!(!response.files[0].contents.contains("tsgodownrt.RunProgram"));
        assert!(response.files[0].contents.contains("strings.ToUpper"));

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
            "string-more FOO_bAR HI\n"
        );
    }

    #[test]
    fn emit_go_runs_aot_truthiness_subset() {
        let root = temp_project("engine-core-aot-truthiness");
        write(
            &root,
            "src/index.js",
            r#"
const missing = null
const text = ""
const count = 2
function check(flag, fallback) {
  if (!flag) {
    return "none"
  }
  return Boolean(fallback)
}
console.log("truthy", !missing, !text, Boolean(count), check(false, "x"), check(true, "go"))
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
            module_path: Some("example.com/aot-truthiness".to_string()),
            output_kind: EmitGoOutputKind::Main,
            ir_snapshot: None,
        });

        assert!(
            !response
                .diagnostics
                .iter()
                .any(|diagnostic| diagnostic.code == "EXECUTABLE_JS_CODEGEN_NOT_IMPLEMENTED"),
            "diagnostics={:?}",
            response.diagnostics
        );
        assert!(!response.files[0].contents.contains("tsgodownrt.RunProgram"));

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
            "truthy true true true none true\n"
        );
    }

    #[test]
    fn emit_go_runs_aot_truthy_any_param_subset() {
        let root = temp_project("engine-core-aot-truthy-any-param");
        write(
            &root,
            "src/index.js",
            r#"
function pick(value) {
  if (!value) {
    return "empty"
  }
  if (typeof value !== "object") {
    return "scalar"
  }
  return value.name
}
console.log("truthy-any", pick(0), pick("x"), pick({ name: "object" }))
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
            module_path: Some("example.com/aot-truthy-any-param".to_string()),
            output_kind: EmitGoOutputKind::Main,
            ir_snapshot: None,
        });

        assert!(
            !response
                .diagnostics
                .iter()
                .any(|diagnostic| diagnostic.code == "EXECUTABLE_JS_CODEGEN_NOT_IMPLEMENTED"),
            "diagnostics={:?}",
            response.diagnostics
        );
        assert!(!response.files[0].contents.contains("tsgodownrt.RunProgram"));

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
            "truthy-any empty scalar object\n"
        );
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
                config: legacy_ir_interpreter_config(),
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
    fn emit_go_runs_bitwise_and_compound_assignment_subset() {
        let root = temp_project("engine-core-bitwise-compound-assignment");
        write(
            &root,
            "src/index.js",
            r#"
const trace = []
function mark(value) {
  trace.push(value)
  return value
}
const bit = 5 ^ 3
let value = 5
value ^= 3
value &= 6
value <<= 2
value >>= 1
value >>>= 1
value |= 1
let math = 20
math /= 4
math %= 3
math **= 4
let truthy = "keep"
truthy ||= mark("bad")
let empty = ""
empty ||= mark("fallback")
let falsy = 0
falsy &&= mark("bad2")
let yes = 2
yes &&= mark("and")
let nil = null
nil ??= mark("nil")
let defined = false
defined ??= mark("bad3")
console.log("compound", bit, value, math, truthy, empty, falsy, yes, nil, defined, trace.join("|"))
"#,
        );

        let response = emit_go(EmitGoRequest {
            analyze: AnalyzeRequest {
                manifest: InputManifest {
                    entry: "src/index.js".to_string(),
                    framework: None,
                },
                cwd: Some(root.to_string_lossy().to_string()),
                config: legacy_ir_interpreter_config(),
            },
            package_name: None,
            module_path: Some("example.com/bitwise-compound-assignment".to_string()),
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
            "compound 6 7 16 keep fallback 0 and nil false fallback|and|nil\n"
        );
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
const jsIdentifier = /^[$_\p{ID_Start}][$_\p{ID_Continue}]*$/u
const idStart = /^[$_\p{ID_Start}]$/u
const idContinue = /^[$\u200c\u200d\p{ID_Continue}]$/u
const pathChars = [..."/items/:id"]
let pathIndex = 8
let pathName = ""
if (idStart.test(pathChars[pathIndex])) {
  do {
    pathName += pathChars[pathIndex++]
  } while (idContinue.test(pathChars[pathIndex]))
}
const spreadString = [..."ab"].join("")
const protoSlice = String.prototype.slice.call("abcdef", 1, 4)
const filtered = [3, 1, 2].sort((a, b) => a - b).filter((value) => value > 1).join("|")
console.log("regex", normalized, parts.join("."), matched[1], replaced, guarded, protoExec, boundSegment[1].slice(1, -1), firstScan.index + ":" + firstScan[1] + ":" + secondScan.index + ":" + secondScan[1], comparator[1] + ":" + comparator[2] + ":" + comparator[6], safeComparator[1] + ":" + safeComparator[2] + ":" + safeComparator[6], dynamicWhitespace, jsIdentifier.test("item_1") + ":" + jsIdentifier.test("$x") + ":" + jsIdentifier.test("9x"), pathChars[8] + ":" + pathName + ":" + pathIndex, spreadString, protoSlice, filtered, /^[0-9]+$/.test("123"))
"#,
        );

        let response = emit_go(EmitGoRequest {
            analyze: AnalyzeRequest {
                manifest: InputManifest {
                    entry: "src/index.js".to_string(),
                    framework: None,
                },
                cwd: Some(root.to_string_lossy().to_string()),
                config: legacy_ir_interpreter_config(),
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
            "regex a-b 1.2.3 1 X1 Y2 true:false aa name 4:[name]:10:[role] >=:0.0.0-0:0 >=:0.0.0-0:0 true true:true:false i:id:10 ab bcd 2|3 true\n"
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
                config: legacy_ir_interpreter_config(),
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
                config: legacy_ir_interpreter_config(),
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
                config: legacy_ir_interpreter_config(),
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
                config: legacy_ir_interpreter_config(),
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
                config: legacy_ir_interpreter_config(),
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
                config: legacy_ir_interpreter_config(),
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
                config: legacy_ir_interpreter_config(),
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
                config: legacy_ir_interpreter_config(),
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
    fn emit_go_runs_derived_error_super_constructor_subset() {
        let root = temp_project("engine-core-derived-error-super");
        write(
            &root,
            "src/index.js",
            r#"
class PathError extends TypeError {
  constructor(message, originalPath) {
    let text = message
    if (originalPath) text += `: ${originalPath}`
    super(text)
    this.originalPath = originalPath
  }
}

try {
  throw new PathError("Unexpected token", "/items/:id")
} catch (error) {
  console.log(JSON.stringify({
    name: error.name,
    message: error.message,
    originalPath: error.originalPath,
    instanceOfTypeError: error instanceof TypeError
  }))
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
                config: legacy_ir_interpreter_config(),
            },
            package_name: None,
            module_path: Some("example.com/derived-error-super".to_string()),
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
            "{\"instanceOfTypeError\":true,\"message\":\"Unexpected token: /items/:id\",\"name\":\"TypeError\",\"originalPath\":\"/items/:id\"}\n"
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
                config: legacy_ir_interpreter_config(),
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
                config: legacy_ir_interpreter_config(),
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
                config: legacy_ir_interpreter_config(),
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
                config: legacy_ir_interpreter_config(),
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
const outer = require("./outer.js")
console.log("cjs", add(2, 4), add.extra(), Object.hasOwn(add, "extra"), outer.extra())
"#,
        );
        write(
            &root,
            "src/add.js",
            r#"
exports = module.exports = function add(left, right) {
  return left + right
}
Object.defineProperty(exports, "extra", {
  enumerable: true,
  get: function () {
    return function extra() {
      return "getter"
    }
  }
})
"#,
        );
        write(
            &root,
            "src/outer.js",
            r#"
const add = require("./add.js")
exports = module.exports = function outer() {
  return "outer"
}
exports.extra = add.extra
"#,
        );

        let response = emit_go(EmitGoRequest {
            analyze: AnalyzeRequest {
                manifest: InputManifest {
                    entry: "src/index.js".to_string(),
                    framework: None,
                },
                cwd: Some(root.to_string_lossy().to_string()),
                config: legacy_ir_interpreter_config(),
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
        assert_eq!(
            String::from_utf8_lossy(&output.stdout),
            "cjs 6 getter true getter\n"
        );
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
                config: legacy_ir_interpreter_config(),
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
    fn emit_go_runs_json_module_require_subset() {
        let root = temp_project("engine-core-json-module");
        write(
            &root,
            "src/index.js",
            r#"
const data = require("./data.json")
console.log("json-module", data.name, data.nested.ok, data.items.length)
"#,
        );
        write(
            &root,
            "src/data.json",
            r#"{ "name": "fixture", "nested": { "ok": true }, "items": [1, 2, 3] }"#,
        );

        let response = emit_go(EmitGoRequest {
            analyze: AnalyzeRequest {
                manifest: InputManifest {
                    entry: "src/index.js".to_string(),
                    framework: None,
                },
                cwd: Some(root.to_string_lossy().to_string()),
                config: legacy_ir_interpreter_config(),
            },
            package_name: None,
            module_path: Some("example.com/json-module".to_string()),
            output_kind: EmitGoOutputKind::Main,
            ir_snapshot: None,
        });

        assert!(!response
            .diagnostics
            .iter()
            .any(|diagnostic| diagnostic.code == "PARSER_SYNTAX_ERROR"));
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
            "json-module fixture true 3\n"
        );
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
function Factory(value) {
  function callable() {
    return callable.value
  }
  Object.setPrototypeOf(callable, this)
  callable.value = value
  return callable
}
const box = new Box(7)
const factory = new Factory("fn")
console.log("ctor", box.value, typeof factory, factory(), factory instanceof Factory)
"#,
        );

        let response = emit_go(EmitGoRequest {
            analyze: AnalyzeRequest {
                manifest: InputManifest {
                    entry: "src/index.js".to_string(),
                    framework: None,
                },
                cwd: Some(root.to_string_lossy().to_string()),
                config: legacy_ir_interpreter_config(),
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
        assert_eq!(
            String::from_utf8_lossy(&output.stdout),
            "ctor 7 function fn true\n"
        );
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
                config: legacy_ir_interpreter_config(),
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
                config: legacy_ir_interpreter_config(),
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
                config: legacy_ir_interpreter_config(),
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
                config: legacy_ir_interpreter_config(),
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
                config: legacy_ir_interpreter_config(),
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
                config: legacy_ir_interpreter_config(),
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
                config: legacy_ir_interpreter_config(),
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
                config: legacy_ir_interpreter_config(),
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
function copyFunctionProps(env) {
  copied.load = () => "stale"
  function copied() {}
  Object.keys(env).forEach(key => {
    copied[key] = env[key]
  })
  return copied.load()
}
function configure() {
  this.cache = Object.create(null)
  this.cache.value = "cached"
}
configure.call(tag)
const callableProto = {
  use: function (value) {
    this.stack.push(value)
    return this.stack.length
  }
}
function makeCallable() {
  function router() {}
  Object.setPrototypeOf(router, callableProto)
  router.stack = []
  return router
}
const callable = makeCallable()
const tri = (req, res, next) => next
console.log("fn-props", tag("7"), tag.count, tag.length, Box.length, tri.length, tri.bind(null, "req").length, typeof tag.prototype, tag.prototype.kind, box.read(), child.inherited, "inherited" in child, box instanceof Box, child instanceof Box, sized.length, sized[1], copyFunctionProps({ load: () => "loaded" }), tag.cache.value, callable.use("layer"), callable.stack[0], Object.getPrototypeOf(callable) === callableProto)
"#,
        );

        let response = emit_go(EmitGoRequest {
            analyze: AnalyzeRequest {
                manifest: InputManifest {
                    entry: "src/index.js".to_string(),
                    framework: None,
                },
                cwd: Some(root.to_string_lossy().to_string()),
                config: legacy_ir_interpreter_config(),
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
            "fn-props id:7 3 1 1 3 2 object tag ok yes true true false 3 x loaded cached 1 layer true\n"
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
                config: legacy_ir_interpreter_config(),
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
                config: legacy_ir_interpreter_config(),
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
                config: legacy_ir_interpreter_config(),
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
let optional = "not-run"
try {
  explode()
} catch (error) {
  wrapped = `wrapped:${error}`
}
try {
  require("missing-optional")
} catch (error) {
  optional = error.name
}
console.log("try", risky(1), risky(3), cleanup, wrapped, optional)
"#,
        );

        let response = emit_go(EmitGoRequest {
            analyze: AnalyzeRequest {
                manifest: InputManifest {
                    entry: "src/index.js".to_string(),
                    framework: None,
                },
                cwd: Some(root.to_string_lossy().to_string()),
                config: legacy_ir_interpreter_config(),
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
            "try 1 caught:too-big 2 wrapped:wrapped Error\n"
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
  static get label() {
    return this._label
  }

  static set label(value) {
    this._label = value
  }

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

  set current(value) {
    this.value = value * 2
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
counter.current = 7
Counter.label = "C"
derived.current = 6
console.log("class", counter.current, Counter.label, Counter.ANY, derived.current, derived instanceof Counter, typeof ArrayChild, arrayChild.length, arrayChild.join(""), error instanceof Error, error.message, reused === counter, reused.current, privateCounter.next(), PrivateCounter.read(), StaticPrivate.read())
"#,
        );

        let response = emit_go(EmitGoRequest {
            analyze: AnalyzeRequest {
                manifest: InputManifest {
                    entry: "src/index.js".to_string(),
                    framework: None,
                },
                cwd: Some(root.to_string_lossy().to_string()),
                config: legacy_ir_interpreter_config(),
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
            "class 14 C * 12 true function 2 qq true boom true 14 3 4 true\n"
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
const Stream = require("stream")
const EventEmitter = require("node:events").EventEmitter
const wrapped = util.deprecate(function add(left, right) {
  return this.base + left + right
}, "add is deprecated")
function Parent() {
  this.parent = "p"
}
Parent.prototype.read = function () {
  return this.parent
}
function Child() {
  Parent.call(this)
  this.child = "c"
}
util.inherits(Child, Parent)
const child = new Child()
function SendStream() {}
util.inherits(SendStream, Stream)
const send = new SendStream()
const app = function () {}
Object.getOwnPropertyNames(EventEmitter.prototype).forEach(function (name) {
  Object.defineProperty(app, name, Object.getOwnPropertyDescriptor(EventEmitter.prototype, name))
})
const hasEmitterOn = Object.hasOwn(app, "on")
app.on("ready", function (value) {
  this.ready = value
})
app.emit("ready", "ok")
console.log("util", format("name:%s count:%d", "items", 3), util.inspect("quoted"), wrapped.call({ base: 1 }, 2, 4), child.read(), child instanceof Child, child instanceof Parent, Child.super_ === Parent, child.constructor === Child, typeof Stream, Stream === Stream.Stream, send instanceof Stream, app.ready, app.listenerCount("ready"), hasEmitterOn)
"#,
        );

        let response = emit_go(EmitGoRequest {
            analyze: AnalyzeRequest {
                manifest: InputManifest {
                    entry: "src/index.js".to_string(),
                    framework: None,
                },
                cwd: Some(root.to_string_lossy().to_string()),
                config: legacy_ir_interpreter_config(),
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
            "util name:items count:3 \"quoted\" 7 p true true true true function true true ok 1 true\n"
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
                config: legacy_ir_interpreter_config(),
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
                config: legacy_ir_interpreter_config(),
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
                config: legacy_ir_interpreter_config(),
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
                config: legacy_ir_interpreter_config(),
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
                config: legacy_ir_interpreter_config(),
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
import { readFile as readFileAsync, writeFile as writeFileAsync, readdir as readdirAsync } from "fs/promises"
import { createRequire } from "node:module"
const require = createRequire(import.meta.url)
const fs = require("fs")
const left = readFileSync("src/data.txt", "utf8")
const right = fs.readFileSync("src/data.txt", "utf8")
async function main() {
  const asyncLeft = await readFileAsync("src/data.txt", "utf8")
  await writeFileAsync("src/out.txt", "beta")
  const names = await readdirAsync("src")
  console.log("fs-module", Number("20") >= 20, typeof process.cwd(), left, right, asyncLeft, names.includes("out.txt"))
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
                config: legacy_ir_interpreter_config(),
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
            "fs-module true string alpha alpha alpha true\n"
        );
    }

    #[test]
    fn emit_go_runs_create_require_relative_module_subset() {
        let root = temp_project("engine-core-create-require-relative");
        write(
            &root,
            "packages/pkg/package.json",
            r#"
{
  "name": "pkg",
  "main": "lib/main.js"
}
"#,
        );
        write(
            &root,
            "packages/pkg/lib/main.js",
            r#"
module.exports = {
  label: "pkg",
  add(left, right) {
    return left + right
  }
}
"#,
        );
        write(
            &root,
            "src/index.js",
            r#"
import { createRequire } from "node:module"
const require = createRequire(import.meta.url)
const pkg = require("../packages/pkg")
console.log("create-require", pkg.label, pkg.add(2, 5))
"#,
        );

        let response = emit_go(EmitGoRequest {
            analyze: AnalyzeRequest {
                manifest: InputManifest {
                    entry: "src/index.js".to_string(),
                    framework: None,
                },
                cwd: Some(root.to_string_lossy().to_string()),
                config: legacy_ir_interpreter_config(),
            },
            package_name: None,
            module_path: Some("example.com/create-require-relative".to_string()),
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
            "create-require pkg 7\n"
        );
    }

    #[test]
    fn emit_go_runs_node_querystring_zlib_timers_async_hooks_subset() {
        let root = temp_project("engine-core-node-api-subset");
        write(
            &root,
            "src/index.js",
            r#"
import qs from "querystring"
import { gzipSync, gunzipSync, deflateSync, inflateSync } from "zlib"
import { performance } from "perf_hooks"
import { AsyncLocalStorage } from "async_hooks"
import timers from "timers"
import net from "net"

const parsed = qs.parse("a=1&b=x&b=y")
const encoded = qs.stringify({ a: 1, b: ["x", "y"] })
const params = new URLSearchParams({ a: 1, b: "x y" })
params.append("b", "z")
const gzipText = gunzipSync(gzipSync("abc")).toString()
const inflateText = inflateSync(deflateSync("def")).toString()
const storage = new AsyncLocalStorage()
let stored = 0
storage.run({ id: 3 }, () => {
  stored = storage.getStore().id
})
let tick = "pending"
timers.setImmediate(() => {
  tick = "immediate"
})
let globalTick = "pending"
setImmediate((value) => {
  globalTick = value
}, "global-immediate")
const stackTarget = { name: "Trace", message: "ok" }
Error.captureStackTrace(stackTarget)
const previousPrepareStackTrace = Error.prepareStackTrace
Error.prepareStackTrace = (_error, stack) => stack
const callSiteTarget = {}
Error.captureStackTrace(callSiteTarget)
const firstCallSite = callSiteTarget.stack[0]
Error.prepareStackTrace = previousPrepareStackTrace
const headers = new Headers({ "X-Trace": "one" })
headers.append("x-trace", "two")
const response = new Response(null, { status: 302, headers })
const legacyUtil = process.binding("util")
const legacyBuffer = process.binding("buffer")
const combined = Buffer.concat([Buffer.from("ab"), Buffer.from("cd")])
console.log("node-api", parsed.a, parsed.b.join("|"), encoded, params.toString(), params.get("b"), gzipText, inflateText, stored, tick, globalTick, performance.now() >= 0)
console.log("net-api", net.isIP("127.0.0.1"), net.isIPv4("127.0.0.1"), net.isIPv6("::1"), net.isIP("bad"))
console.log("web-api", stackTarget.stack, typeof firstCallSite.getFileName, firstCallSite.getLineNumber(), firstCallSite.getColumnNumber(), firstCallSite.isEval(), firstCallSite.toString().includes("generated.js"), response.status, response.headers.get("X-Trace"), Buffer.byteLength("가"), combined.toString())
console.log("binding-api", legacyUtil.isRegExp(/x/), legacyUtil.isDate(new Date(0)), legacyUtil.isMap(new Map()), legacyUtil.isSet(new Set()), legacyUtil.isTypedArray(Buffer.from("x")), legacyBuffer.kStringMaxLength > 0)
"#,
        );

        let response = emit_go(EmitGoRequest {
            analyze: AnalyzeRequest {
                manifest: InputManifest {
                    entry: "src/index.js".to_string(),
                    framework: None,
                },
                cwd: Some(root.to_string_lossy().to_string()),
                config: legacy_ir_interpreter_config(),
            },
            package_name: None,
            module_path: Some("example.com/node-api-subset".to_string()),
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
            "node-api 1 x|y a=1&b=x&b=y a=1&b=x+y&b=z x y abc def 3 immediate global-immediate true\nnet-api 4 true true 0\nweb-api Trace: ok function 1 1 false true 302 one, two 3 abcd\nbinding-api true true true true true true\n"
        );
    }

    #[test]
    fn emit_go_runs_in_process_http_fetch_subset() {
        let root = temp_project("engine-core-http-fetch");
        write(
            &root,
            "src/index.js",
            r#"
import { createServer, METHODS, STATUS_CODES } from "node:http"

async function main() {
  const methodSummary = METHODS.slice(0, 3).map((method) => method.toLowerCase()).join("|")
  const server = createServer((req, res) => {
    req.pause()
    req.unpipe()
    res.setHeader("x-vector", "ok")
    res.writeHead(req.method === "POST" ? 201 : 200, { "content-type": "application/json" })
    res.end(JSON.stringify({ method: req.method, url: req.url }))
  })
  await new Promise((resolve) => server.listen(0, "127.0.0.1", resolve))
  const port = server.address().port
  const deferredListen = await new Promise((resolve) => {
    const scoped = createServer((_req, res) => res.end("ok"))
    const assigned = scoped.listen(0, "127.0.0.1", () => {
      assigned.off("error", resolve)
      resolve(scoped.address().port > 0)
    })
    assigned.once("error", resolve)
  })
  async function requestLike() {
    try {
      const response = await fetch(`http://127.0.0.1:${port}/items/7?q=x`, { method: "POST", body: "payload" })
      const headers = Object.fromEntries(response.headers)
      const text = await response.text()
      return { status: response.status, header: headers["x-vector"], text }
    } finally {
      await new Promise((resolve) => server.close(resolve))
    }
  }
  async function outerRequestLike() {
    return requestLike()
  }
  const result = await outerRequestLike()
  function appLike(req, res) {
    res.setHeader("x-vector", "app")
    res.end(JSON.stringify({ method: req.method, url: req.url }))
  }
  appLike.listen = function listen() {
    const server = createServer(this)
    return server.listen.apply(server, arguments)
  }
  async function requestHttp(appOrHandler, vector) {
    const server = typeof appOrHandler.listen === "function"
      ? await new Promise((resolve, reject) => {
          const server = appOrHandler.listen(0, "127.0.0.1", () => {
            server.off("error", reject)
            resolve(server)
          })
          server.once("error", reject)
        })
      : createServer(appOrHandler)
    try {
      const response = await fetch(`http://127.0.0.1:${server.address().port}/items/${vector.pathId}?${new URLSearchParams(vector.query)}`, {
        method: vector.method,
        headers: { "content-type": "application/json" }
      })
      return {
        status: response.status,
        headers: Object.fromEntries(response.headers),
        body: await response.text()
      }
    } finally {
      await new Promise((resolve) => server.close(resolve))
    }
  }
  const appResult = await requestHttp(appLike, { pathId: "9", query: { q: "z" }, method: "GET" })
  console.log("http", methodSummary, STATUS_CODES[200], STATUS_CODES[404], result.status, result.header, result.text, deferredListen, appResult.status, appResult.headers["x-vector"], appResult.body)
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
                config: legacy_ir_interpreter_config(),
            },
            package_name: None,
            module_path: Some("example.com/http-fetch".to_string()),
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
            "http acl|bind|checkout OK Not Found 201 ok {\"method\":\"POST\",\"url\":\"/items/7?q=x\"} true 200 app {\"method\":\"GET\",\"url\":\"/items/9?q=z\"}\n"
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
                config: legacy_ir_interpreter_config(),
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

    fn legacy_ir_interpreter_config() -> AnalyzeConfig {
        AnalyzeConfig {
            profile: Some("legacy-ir-interpreter".to_string()),
        }
    }
}
