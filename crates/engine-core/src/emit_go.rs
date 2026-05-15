use serde::{Deserialize, Serialize};

use crate::analyze;
use crate::backend::{backend_provider, BackendEmitRequest, BackendEmitResponse};
use crate::contract::{AnalyzeRequest, AnalyzeResponse, Diagnostic};
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
    let mut files = vec![
        GeneratedFile {
            path: match output_kind.clone() {
                EmitGoOutputKind::Main => "main.go",
                EmitGoOutputKind::VectorSuite => "vector_suite.go",
            }
            .to_string(),
            contents: add_output_kind_prelude(
                output_kind,
                if can_emit_executable {
                    render_executable_program(&package_name, &module_path, &analyzed)
                } else {
                    render_fail_closed_program(
                        &package_name,
                        &module_path,
                        &diagnostics,
                        request.output_kind.purpose(),
                    )
                },
            ),
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
            contents: render_runtime_package(),
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

fn sanitize_go_identifier(value: &str) -> String {
    let cleaned: String = value
        .chars()
        .filter(|ch| ch.is_ascii_alphanumeric() || *ch == '_')
        .collect();
    match cleaned.as_str() {
        "" => "irSnapshotJSON".to_string(),
        _ if cleaned.starts_with(|ch: char| ch.is_ascii_digit()) => format!("ir_{cleaned}"),
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

fn render_executable_program(
    package_name: &str,
    module_path: &str,
    analyzed: &AnalyzeResponse,
) -> String {
    let program_json = serde_json::to_string(&analyzed.ir).expect("analyzed IR should serialize");
    let program_literal = go_raw_string_literal(&program_json);
    format!(
        r#"package {package_name}

import (
	"fmt"
	"os"

	"{module_path}/tsgodownrt"
)

func main() {{
	if err := tsgodownrt.RunProgram({program_literal}); err != nil {{
		fmt.Fprintln(os.Stderr, err)
		os.Exit(1)
	}}
}}
"#
    )
}

fn render_runtime_package() -> String {
    let source = r#"package tsgodownrt

import (
	"bytes"
	"compress/gzip"
	"compress/zlib"
	"crypto/md5"
	cryptorand "crypto/rand"
	"crypto/sha1"
	"encoding/base64"
	"encoding/hex"
	"encoding/json"
	"errors"
	"fmt"
	"io"
	"math"
	stdnet "net"
	"net/url"
	"os"
	"path/filepath"
	"reflect"
	"regexp"
	"runtime"
	"sort"
	"strconv"
	"strings"
	"time"

	"github.com/dlclark/regexp2"
)

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

type Program struct {
	Entry   string   `json:"entry"`
	Modules []Module `json:"modules"`
}

type Module struct {
	ID         string           `json:"id"`
	SourcePath string           `json:"sourcePath"`
	Exports    []string         `json:"exports"`
	Imports    []Import         `json:"imports"`
	Executable ExecutableModule `json:"executable"`
}

type Import struct {
	Spec     string          `json:"spec"`
	Kind     string          `json:"kind"`
	Resolved string          `json:"resolved"`
	Bindings []ImportBinding `json:"bindings"`
}

type ImportBinding struct {
	Local    string `json:"local"`
	Imported string `json:"imported"`
	Kind     string `json:"kind"`
}

type ExecutableModule struct {
	Stmts []map[string]any `json:"stmts"`
}

type Env map[string]any

type FunctionValue struct {
	Params []string
	RestParam string
	Body   []map[string]any
	Env    Env
	Props  map[string]any
	LexicalThis bool
	Async       bool
	Generator   bool
}

type ClassValue struct {
	Constructor *FunctionValue
	Methods     map[string]FunctionValue
	Getters     map[string]FunctionValue
	Setters     map[string]FunctionValue
	Static      map[string]FunctionValue
	StaticGetters map[string]FunctionValue
	StaticSetters map[string]FunctionValue
	Super       *ClassValue
	SuperCtor   any
	Callable    bool
	Props       map[string]any
}

type BoundFunctionValue struct {
	Function FunctionValue
	This     any
	Args     []any
}

type NativeFunctionValue struct {
	Call         func(args []any) (any, error)
	CallWithThis func(thisValue any, args []any) (any, error)
	Props        map[string]any
}

type ArrayValue struct {
	Items []any
	Props map[string]any
}

type RegExpValue struct {
	Pattern string
	Flags   string
	Regex   *regexp.Regexp
	Regex2  *regexp2.Regexp
	Global  bool
	LastIndex int
	Props   map[string]any
}

type DateValue struct {
	Time time.Time
}

type SymbolValue struct {
	Description string
}

type MapEntry struct {
	Key   any
	Value any
}

type MapValue struct {
	Entries []MapEntry
}

type SetValue struct {
	Values []any
}

type IteratorValue struct {
	Values []any
	Index  int
}

type UndefinedValue struct{}

type NullValue struct{}

var jsUndefined = UndefinedValue{}
var jsNull = NullValue{}

type completion struct {
	value     any
	returned  bool
	broke     bool
	continued bool
	breakLabel string
	continueLabel string
	yields    []any
}

type moduleState struct {
	exports   any
	evaluated bool
	evaluating bool
}

var sharedProcess map[string]any
var sharedGlobal map[string]any

type jsThrow struct {
	value any
}

func (throw jsThrow) Error() string {
	return jsString(throw.value)
}

func RunProgram(programJSON string) error {
	var program Program
	if err := json.Unmarshal([]byte(programJSON), &program); err != nil {
		return err
	}
	sharedProcess = processObject(program.Entry)
	sharedGlobal = map[string]any{}
	module, ok := entryModule(program)
	if !ok {
		return errors.New("entry module not found")
	}
	cache := map[string]*moduleState{}
	_, err := executeModule(module, program, cache)
	return err
}

func executeModule(module Module, program Program, cache map[string]*moduleState) (any, error) {
	if state, ok := cache[module.SourcePath]; ok {
		if state.evaluated || state.evaluating {
			return state.exports, nil
		}
	}
	exports := map[string]any{}
	state := &moduleState{exports: exports, evaluating: true}
	cache[module.SourcePath] = state
	process := sharedProcess
	env := Env{
		"exports": exports,
		"Error":   builtinErrorClass("Error"),
		"TypeError": builtinErrorClass("TypeError"),
		"RangeError": builtinErrorClass("RangeError"),
		"SyntaxError": builtinErrorClass("SyntaxError"),
		"ReferenceError": builtinErrorClass("ReferenceError"),
		"Function": functionGlobal(),
		"Number": numberGlobal(),
		"String": stringGlobal(),
		"Boolean": nativeFunction(func(args []any) (any, error) {
			if len(args) == 0 {
				return false, nil
			}
			return isTruthy(args[0]), nil
		}),
		"RegExp": regexpGlobal(),
			"Symbol": symbolGlobal(),
			"Array":  arrayGlobal(),
			"Buffer": bufferGlobal(),
			"Date":   dateGlobal(),
			"Promise": promiseGlobal(),
			"AbortController": abortControllerGlobal(),
			"TextEncoder": textEncoderGlobal(),
			"TextDecoder": textDecoderGlobal(),
			"URL": urlGlobal(),
			"URLSearchParams": urlSearchParamsGlobal(),
			"Uint8Array": typedArrayGlobal(),
			"Uint16Array": typedArrayGlobal(),
			"Uint32Array": typedArrayGlobal(),
			"Object": objectGlobal(),
		"Reflect": reflectGlobal(),
		"JSON": jsonGlobal(),
		"Math":   mathGlobal(),
		"Map":    mapGlobal(),
		"Set":    setGlobal(),
		"WeakMap": mapGlobal(),
		"WeakSet": setGlobal(),
		"console": consoleObject(),
		"parseInt": nativeFunction(func(args []any) (any, error) {
			if len(args) == 0 {
				return math.NaN(), nil
			}
			base := 10
			if len(args) > 1 && !isNullish(args[1]) {
				base = int(toNumber(args[1]))
				if base == 0 {
					base = 10
				}
			}
			parsed, err := strconv.ParseInt(strings.TrimSpace(jsString(args[0])), base, 64)
			if err != nil {
				return math.NaN(), nil
			}
			return float64(parsed), nil
		}),
		"parseFloat": nativeFunction(func(args []any) (any, error) {
			if len(args) == 0 {
				return math.NaN(), nil
			}
			number, _ := parseJSNumberLiteral(jsString(args[0]))
			return number, nil
		}),
			"isNaN": nativeFunction(func(args []any) (any, error) {
				if len(args) == 0 {
					return true, nil
				}
				return math.IsNaN(toNumber(args[0])), nil
			}),
			"isFinite": nativeFunction(func(args []any) (any, error) {
				if len(args) == 0 {
					return false, nil
				}
				number := toNumber(args[0])
				return !math.IsNaN(number) && !math.IsInf(number, 0), nil
			}),
			"setTimeout": nativeFunction(func(args []any) (any, error) {
				delay := 0
				if len(args) > 1 {
					delay = jsInteger(args[1])
				}
				if delay > 0 {
					time.Sleep(time.Duration(delay) * time.Millisecond)
				}
				if len(args) > 0 {
					if _, err := callFunctionWithValues(args[0], []any{}, Env{}, jsUndefined); err != nil {
						return nil, err
					}
				}
				return map[string]any{
					"unref": nativeFunction(func(args []any) (any, error) {
						return jsUndefined, nil
					}),
				}, nil
			}),
			"clearTimeout": nativeFunction(func(args []any) (any, error) {
				return jsUndefined, nil
			}),
		"decodeURIComponent": nativeFunction(func(args []any) (any, error) {
			if len(args) == 0 {
				return "", nil
			}
			decoded, err := url.PathUnescape(jsString(args[0]))
			if err != nil {
				return jsString(args[0]), nil
			}
			return decoded, nil
		}),
		"encodeURIComponent": nativeFunction(func(args []any) (any, error) {
			if len(args) == 0 {
				return "", nil
			}
			encoded := url.QueryEscape(jsString(args[0]))
			return strings.ReplaceAll(encoded, "+", "%20"), nil
		}),
		"escape": nativeFunction(func(args []any) (any, error) {
			if len(args) == 0 {
				return "", nil
			}
			encoded := url.QueryEscape(jsString(args[0]))
			return strings.ReplaceAll(encoded, "+", "%20"), nil
		}),
		"unescape": nativeFunction(func(args []any) (any, error) {
			if len(args) == 0 {
				return "", nil
			}
			decoded, err := url.QueryUnescape(jsString(args[0]))
			if err != nil {
				return jsString(args[0]), nil
			}
			return decoded, nil
		}),
		"process": process,
		"global":  sharedGlobal,
		"module": map[string]any{
			"exports": exports,
		},
	}
	env["globalThis"] = sharedGlobal
	sharedGlobal["process"] = process
	for _, name := range []string{"Buffer", "Promise", "AbortController", "TextEncoder", "TextDecoder", "URL", "URLSearchParams"} {
		sharedGlobal[name] = env[name]
	}
	env["require"] = NativeFunctionValue{Call: func(args []any) (any, error) {
		if len(args) == 0 {
			return nil, errors.New("require specifier is required")
		}
		spec := jsString(args[0])
		if exports, ok := builtinModuleExports(spec); ok {
			return exports, nil
		}
		for _, importDecl := range module.Imports {
			if importDecl.Spec != spec {
				continue
			}
			importedModule, ok := moduleByID(program, importDecl.Resolved)
			if !ok {
				return nil, fmt.Errorf("module import %s is not resolved", importDecl.Spec)
			}
			return executeModule(importedModule, program, cache)
		}
		if importedModule, ok := resolveRelativeModuleAtRuntime(program, module.SourcePath, spec); ok {
			return executeModule(importedModule, program, cache)
		}
		if importedModule, ok := resolveBareModule(program, spec); ok {
			return executeModule(importedModule, program, cache)
		}
		return nil, fmt.Errorf("module import %s is not resolved", spec)
	}}
	env["import"] = NativeFunctionValue{Call: func(args []any) (any, error) {
		if len(args) == 0 {
			return promiseRejected(nodeError("ERR_MODULE_NOT_FOUND", "dynamic import specifier is required")), nil
		}
		spec := jsString(args[0])
		if exports, ok := builtinModuleExports(spec); ok {
			return promiseFulfilled(exports), nil
		}
		for _, importDecl := range module.Imports {
			if importDecl.Spec != spec {
				continue
			}
			importedModule, ok := moduleByID(program, importDecl.Resolved)
			if !ok {
				return promiseRejected(nodeError("ERR_MODULE_NOT_FOUND", fmt.Sprintf("module import %s is not resolved", spec))), nil
			}
			importedValue, err := executeModule(importedModule, program, cache)
			if err != nil {
				return promiseRejectedFromError(err), nil
			}
			return promiseFulfilled(importedValue), nil
		}
		if importedModule, ok := resolveRelativeModuleAtRuntime(program, module.SourcePath, spec); ok {
			importedValue, err := executeModule(importedModule, program, cache)
			if err != nil {
				return promiseRejectedFromError(err), nil
			}
			return promiseFulfilled(importedValue), nil
		}
		if importedModule, ok := resolveBareModule(program, spec); ok {
			importedValue, err := executeModule(importedModule, program, cache)
			if err != nil {
				return promiseRejectedFromError(err), nil
			}
			return promiseFulfilled(importedValue), nil
		}
		return promiseRejected(nodeError("ERR_MODULE_NOT_FOUND", fmt.Sprintf("module import %s is not resolved", spec))), nil
	}}
	for _, importDecl := range module.Imports {
		if importDecl.Kind == "cjs" {
			continue
		}
		importedExports, ok := builtinModuleExports(importDecl.Spec)
		var importedValue any = importedExports
		if !ok {
			importedModule, moduleOk := moduleByID(program, importDecl.Resolved)
			if !moduleOk {
				return nil, fmt.Errorf("module import %s is not resolved", importDecl.Spec)
			}
			var err error
			importedValue, err = executeModule(importedModule, program, cache)
			if err != nil {
				return nil, err
			}
		}
		bindImport(env, importDecl, importedValue)
	}
	hoistFunctionDeclarations(module.Executable.Stmts, env)
	for _, stmt := range module.Executable.Stmts {
		if result, err := evalStmt(stmt, env); err != nil {
			return nil, err
		} else if result.returned {
			return nil, errors.New("top-level return is not supported")
		} else if result.broke || result.continued {
			return nil, errors.New("break/continue outside loop")
		}
		state.exports = currentModuleExports(env, exports)
	}
	exported := currentModuleExports(env, exports)
	if exportedMap, ok := exported.(map[string]any); ok {
		for _, name := range module.Exports {
			if name == "*" {
				exportedMap["__cjs"] = true
			}
		}
		for _, name := range module.Exports {
			if value, ok := env[name]; ok {
				exportedMap[name] = value
			}
		}
	}
	state.exports = exported
	state.evaluating = false
	state.evaluated = true
	return exported, nil
}

func currentModuleExports(env Env, fallback map[string]any) any {
	if moduleObject, ok := env["module"].(map[string]any); ok {
		return moduleObject["exports"]
	}
	return fallback
}

func entryModule(program Program) (Module, bool) {
	for _, module := range program.Modules {
		if module.SourcePath == program.Entry || module.ID == program.Entry {
			return module, true
		}
	}
	if len(program.Modules) > 0 {
		return program.Modules[0], true
	}
	return Module{}, false
}

func moduleByID(program Program, id string) (Module, bool) {
	for _, module := range program.Modules {
		if module.SourcePath == id || module.ID == id {
			return module, true
		}
	}
	return Module{}, false
}

func resolveRelativeModuleAtRuntime(program Program, fromModule string, spec string) (Module, bool) {
	if !strings.HasPrefix(spec, "./") && !strings.HasPrefix(spec, "../") {
		return Module{}, false
	}
	base := filepath.ToSlash(filepath.Clean(filepath.Join(filepath.Dir(fromModule), spec)))
	candidates := []string{
		base,
		base + ".js",
		base + ".mjs",
		base + ".cjs",
		base + ".json",
		filepath.ToSlash(filepath.Join(base, "index.js")),
		filepath.ToSlash(filepath.Join(base, "index.mjs")),
		filepath.ToSlash(filepath.Join(base, "index.cjs")),
		filepath.ToSlash(filepath.Join(base, "index.json")),
	}
	for _, candidate := range candidates {
		if module, ok := moduleByID(program, candidate); ok {
			return module, true
		}
	}
	return Module{}, false
}

func resolveBareModule(program Program, spec string) (Module, bool) {
	if strings.HasPrefix(spec, ".") || strings.HasPrefix(spec, "/") || strings.HasPrefix(spec, "node:") {
		return Module{}, false
	}
	prefix := "node_modules/" + spec + "/"
	preferred := []string{
		prefix + "index.js",
		prefix + "index.json",
		prefix + filepath.Base(spec) + ".js",
		prefix + filepath.Base(spec) + ".json",
		prefix + filepath.Base(spec) + "/" + filepath.Base(spec) + ".js",
		prefix + filepath.Base(spec) + "/" + filepath.Base(spec) + ".json",
	}
	for _, candidate := range preferred {
		if module, ok := moduleByID(program, candidate); ok {
			return module, true
		}
	}
	for _, module := range program.Modules {
		if strings.HasPrefix(module.SourcePath, prefix) && strings.HasSuffix(module.SourcePath, ".js") {
			return module, true
		}
	}
	return Module{}, false
}

func consoleObject() map[string]any {
	return map[string]any{
		"log": nativeFunction(func(args []any) (any, error) {
			fmt.Println(jsFormat(args))
			return jsUndefined, nil
		}),
		"error": nativeFunction(func(args []any) (any, error) {
			fmt.Fprintln(os.Stderr, jsFormat(args))
			return jsUndefined, nil
		}),
	}
}

func builtinModuleExports(spec string) (map[string]any, bool) {
	switch spec {
	case "assert", "node:assert":
		exports := assertModuleExports()
		exports["default"] = exports
		return exports, true
	case "util", "node:util":
		exports := map[string]any{}
		inspect := nativeFunction(func(args []any) (any, error) {
			if len(args) == 0 {
				return "undefined", nil
			}
			return jsInspect(args[0]), nil
		})
		format := nativeFunction(func(args []any) (any, error) {
			if len(args) == 0 {
				return "", nil
			}
			return jsFormat(args), nil
		})
		exports["inspect"] = inspect
		exports["format"] = format
		exports["debuglog"] = nativeFunction(func(args []any) (any, error) {
			return nativeFunction(func(args []any) (any, error) {
				return jsUndefined, nil
			}), nil
		})
		exports["stripVTControlCharacters"] = nativeFunction(func(args []any) (any, error) {
			if len(args) == 0 {
				return "", nil
			}
			re := regexp.MustCompile(`\x1b\[[0-?]*[ -/]*[@-~]`)
			return re.ReplaceAllString(jsString(args[0]), ""), nil
		})
		exports["callbackify"] = nativeFunction(func(args []any) (any, error) {
			if len(args) == 0 {
				return jsUndefined, nil
			}
			fn := args[0]
			return nativeFunction(func(callArgs []any) (any, error) {
				callback := lastCallback(callArgs)
				if callback != nil {
					callArgs = callArgs[:len(callArgs)-1]
				}
				result, err := callFunctionWithValues(fn, callArgs, Env{}, jsUndefined)
				if err != nil {
					if callback != nil {
						_, cbErr := callFunctionWithValues(callback, []any{nodeError("ERR_CALLBACKIFY", err.Error())}, Env{}, jsUndefined)
						return jsUndefined, cbErr
					}
					return nil, err
				}
				value, awaitErr := awaitValue(result)
				if awaitErr != nil {
					if callback != nil {
						_, cbErr := callFunctionWithValues(callback, []any{nodeError("ERR_CALLBACKIFY", awaitErr.Error())}, Env{}, jsUndefined)
						return jsUndefined, cbErr
					}
					return nil, awaitErr
				}
				if callback != nil {
					_, cbErr := callFunctionWithValues(callback, []any{jsNull, value}, Env{}, jsUndefined)
					return jsUndefined, cbErr
				}
				return value, nil
			}), nil
		})
		exports["promisify"] = nativeFunction(func(args []any) (any, error) {
			if len(args) == 0 {
				return jsUndefined, nil
			}
			fn := args[0]
			return nativeFunction(func(callArgs []any) (any, error) {
				var callbackErr error
				var callbackValues []any
				callback := nativeFunction(func(cbArgs []any) (any, error) {
					if len(cbArgs) > 0 && !isNullish(cbArgs[0]) {
						callbackErr = jsThrow{value: cbArgs[0]}
						return jsUndefined, nil
					}
					callbackValues = append([]any{}, cbArgs[1:]...)
					return jsUndefined, nil
				})
				_, err := callFunctionWithValues(fn, append(callArgs, callback), Env{}, jsUndefined)
				if err != nil {
					return promiseRejectedFromError(err), nil
				}
				if callbackErr != nil {
					return promiseRejectedFromError(callbackErr), nil
				}
				if len(callbackValues) == 0 {
					return promiseFulfilled(jsUndefined), nil
				}
				return promiseFulfilled(callbackValues[0]), nil
			}), nil
		})
		exports["default"] = exports
		return exports, true
	case "path", "node:path":
		return pathModuleExports(), true
	case "os", "node:os":
		return osModuleExports(), true
	case "node:diagnostics_channel":
		return diagnosticsChannelModuleExports(), true
	case "process", "node:process":
		exports := sharedProcess
		exports["default"] = exports
		return exports, true
	case "buffer", "node:buffer":
		exports := map[string]any{"Buffer": bufferGlobal()}
		exports["default"] = exports
		return exports, true
	case "child_process", "node:child_process":
		return childProcessModuleExports(), true
	case "events", "node:events":
		return eventsModuleExports(), true
	case "crypto", "node:crypto":
		return cryptoModuleExports(), true
	case "constants", "node:constants":
		exports := constantsModuleExports()
		exports["default"] = exports
		return exports, true
	case "perf_hooks", "node:perf_hooks":
		return perfHooksModuleExports(), true
	case "querystring", "node:querystring":
		return querystringModuleExports(), true
	case "stream", "node:stream":
		return streamModuleExports(), true
	case "node:stream/promises":
		return streamPromisesModuleExports(), true
	case "fs", "node:fs":
		return fsModuleExports(), true
	case "fs/promises", "node:fs/promises":
		return fsPromisesModuleExports(), true
	case "node:string_decoder", "string_decoder":
		return stringDecoderModuleExports(), true
	case "node:timers/promises":
		return timersPromisesModuleExports(), true
	case "timers", "node:timers":
		return timersModuleExports(), true
	case "async_hooks", "node:async_hooks":
		return asyncHooksModuleExports(), true
	case "tty", "node:tty":
		return ttyModuleExports(), true
	case "url", "node:url":
		return urlModuleExports(), true
	case "v8", "node:v8":
		return v8ModuleExports(), true
	case "module", "node:module":
		return moduleModuleExports(), true
	case "net", "node:net":
		return netModuleExports(), true
	case "zlib", "node:zlib":
		return zlibModuleExports(), true
	default:
		return nil, false
	}
}

func assertModuleExports() map[string]any {
	exports := map[string]any{}
	fail := func(code string, actual any, expected any, message any) error {
		text := jsString(message)
		if text == "" || isUndefined(message) {
			text = fmt.Sprintf("%s: %s != %s", code, jsInspect(actual), jsInspect(expected))
		}
		return jsThrow{value: nodeError(code, text)}
	}
	ok := nativeFunction(func(args []any) (any, error) {
		value := any(false)
		if len(args) > 0 {
			value = args[0]
		}
		if isTruthy(value) {
			return jsUndefined, nil
		}
		message := any(jsUndefined)
		if len(args) > 1 {
			message = args[1]
		}
		return jsUndefined, fail("ERR_ASSERTION", value, true, message)
	})
	equal := nativeFunction(func(args []any) (any, error) {
		actual := any(jsUndefined)
		expected := any(jsUndefined)
		message := any(jsUndefined)
		if len(args) > 0 {
			actual = args[0]
		}
		if len(args) > 1 {
			expected = args[1]
		}
		if len(args) > 2 {
			message = args[2]
		}
		if jsLooseEqual(actual, expected) {
			return jsUndefined, nil
		}
		return jsUndefined, fail("ERR_ASSERTION", actual, expected, message)
	})
	strictEqual := nativeFunction(func(args []any) (any, error) {
		actual := any(jsUndefined)
		expected := any(jsUndefined)
		message := any(jsUndefined)
		if len(args) > 0 {
			actual = args[0]
		}
		if len(args) > 1 {
			expected = args[1]
		}
		if len(args) > 2 {
			message = args[2]
		}
		if jsSameValue(actual, expected) {
			return jsUndefined, nil
		}
		return jsUndefined, fail("ERR_ASSERTION", actual, expected, message)
	})
	notStrictEqual := nativeFunction(func(args []any) (any, error) {
		actual := any(jsUndefined)
		expected := any(jsUndefined)
		message := any(jsUndefined)
		if len(args) > 0 {
			actual = args[0]
		}
		if len(args) > 1 {
			expected = args[1]
		}
		if len(args) > 2 {
			message = args[2]
		}
		if !jsSameValue(actual, expected) {
			return jsUndefined, nil
		}
		return jsUndefined, fail("ERR_ASSERTION", actual, expected, message)
	})
	deepStrictEqual := nativeFunction(func(args []any) (any, error) {
		actual := any(jsUndefined)
		expected := any(jsUndefined)
		message := any(jsUndefined)
		if len(args) > 0 {
			actual = args[0]
		}
		if len(args) > 1 {
			expected = args[1]
		}
		if len(args) > 2 {
			message = args[2]
		}
		if reflect.DeepEqual(actual, expected) {
			return jsUndefined, nil
		}
		return jsUndefined, fail("ERR_ASSERTION", actual, expected, message)
	})
	exports["ok"] = ok
	exports["equal"] = equal
	exports["strictEqual"] = strictEqual
	exports["notStrictEqual"] = notStrictEqual
	exports["deepStrictEqual"] = deepStrictEqual
	exports["deepEqual"] = deepStrictEqual
	exports["AssertionError"] = builtinErrorClass("AssertionError")
	exports["throws"] = nativeFunction(func(args []any) (any, error) {
		if len(args) == 0 {
			return jsUndefined, fail("ERR_ASSERTION", jsUndefined, "throwing function", jsUndefined)
		}
		_, err := callFunctionWithValues(args[0], []any{}, Env{}, jsUndefined)
		if err != nil {
			return jsUndefined, nil
		}
		return jsUndefined, fail("ERR_ASSERTION", jsUndefined, "throw", jsUndefined)
	})
	exports["doesNotThrow"] = nativeFunction(func(args []any) (any, error) {
		if len(args) == 0 {
			return jsUndefined, nil
		}
		_, err := callFunctionWithValues(args[0], []any{}, Env{}, jsUndefined)
		if err != nil {
			return jsUndefined, fail("ERR_ASSERTION", err.Error(), "no throw", jsUndefined)
		}
		return jsUndefined, nil
	})
	return exports
}

func nativeFunction(call func(args []any) (any, error)) NativeFunctionValue {
	return NativeFunctionValue{
		Call: call,
		CallWithThis: func(thisValue any, args []any) (any, error) {
			return call(args)
		},
		Props: map[string]any{},
	}
}

func nativeMethod(call func(thisValue any, args []any) (any, error)) NativeFunctionValue {
	return NativeFunctionValue{
		Call: func(args []any) (any, error) {
			return call(jsUndefined, args)
		},
		CallWithThis: call,
		Props:        map[string]any{},
	}
}

func userFunctionValue(params []string, restParam string, body []map[string]any, env Env, lexicalThis bool, async bool, generator bool) FunctionValue {
	props := map[string]any{}
	function := FunctionValue{
		Params:      params,
		RestParam:   restParam,
		Body:        body,
		Env:         env,
		Props:       props,
		LexicalThis: lexicalThis,
		Async:       async,
		Generator:   generator,
	}
	if !lexicalThis {
		prototype := map[string]any{}
		prototype["constructor"] = function
		props["prototype"] = prototype
	}
	return function
}

func objectWithPrototype(prototype any) map[string]any {
	object := map[string]any{}
	if !isNullish(prototype) {
		object["__prototype"] = prototype
	}
	return object
}

func functionGlobal() map[string]any {
	prototype := map[string]any{}
	prototype["call"] = nativeMethod(func(thisValue any, args []any) (any, error) {
		callThis := any(jsUndefined)
		if len(args) > 0 {
			callThis = args[0]
			args = args[1:]
		}
		return callFunctionWithValues(thisValue, args, Env{}, callThis)
	})
	prototype["apply"] = nativeMethod(func(thisValue any, args []any) (any, error) {
		callThis := any(jsUndefined)
		callArgs := []any{}
		if len(args) > 0 {
			callThis = args[0]
		}
		if len(args) > 1 {
			callArgs = iterableValues(args[1])
		}
		return callFunctionWithValues(thisValue, callArgs, Env{}, callThis)
	})
	prototype["bind"] = nativeMethod(func(thisValue any, args []any) (any, error) {
		boundThis := any(jsUndefined)
		boundArgs := []any{}
		if len(args) > 0 {
			boundThis = args[0]
			boundArgs = append(boundArgs, args[1:]...)
		}
		return bindCallable(thisValue, boundThis, boundArgs), nil
	})
	return map[string]any{
		"__call": nativeFunction(func(args []any) (any, error) {
			return userFunctionValue([]string{}, "", []map[string]any{}, Env{}, false, false, false), nil
		}),
		"prototype": prototype,
	}
}

func regexpGlobal() NativeFunctionValue {
	constructor := nativeFunction(func(args []any) (any, error) {
		pattern := ""
		flags := ""
		if len(args) > 0 {
			if existing, ok := args[0].(*RegExpValue); ok && len(args) == 1 {
				return existing, nil
			}
			pattern = jsString(args[0])
		}
		if len(args) > 1 {
			flags = jsString(args[1])
		}
		return newRegExp(pattern, flags)
	})
	constructor.Props["prototype"] = regexpPrototype()
	return constructor
}

func regexpPrototype() map[string]any {
	return map[string]any{
		"exec": nativeMethod(func(thisValue any, args []any) (any, error) {
			regexpValue, ok := thisValue.(*RegExpValue)
			if !ok {
				return nil, errors.New("RegExp.prototype.exec called on incompatible receiver")
			}
			text := ""
			if len(args) > 0 {
				text = jsString(args[0])
			}
			return regexpExec(regexpValue, text), nil
		}),
		"test": nativeMethod(func(thisValue any, args []any) (any, error) {
			regexpValue, ok := thisValue.(*RegExpValue)
			if !ok {
				return false, errors.New("RegExp.prototype.test called on incompatible receiver")
			}
			text := ""
			if len(args) > 0 {
				text = jsString(args[0])
			}
			return regexpTest(regexpValue, text)
		}),
	}
}

func stringGlobal() NativeFunctionValue {
	constructor := nativeFunction(func(args []any) (any, error) {
		if len(args) == 0 {
			return "", nil
		}
		return jsString(args[0]), nil
	})
	prototype := map[string]any{}
	for _, property := range []string{
		"trim", "toLowerCase", "toUpperCase", "trimStart", "trimLeft", "trimEnd", "trimRight",
		"split", "replace", "replaceAll", "match", "slice", "substring", "substr", "charAt",
			"charCodeAt", "codePointAt", "at", "indexOf", "lastIndexOf", "includes", "startsWith", "endsWith", "repeat",
	} {
		current := property
		prototype[current] = nativeMethod(func(thisValue any, args []any) (any, error) {
			member, ok := stringMember(jsString(thisValue), current, Env{})
			if !ok {
				return jsUndefined, nil
			}
			return callFunctionWithValues(member, args, Env{}, thisValue)
		})
	}
	constructor.Props["prototype"] = prototype
	return constructor
}

func numberGlobal() NativeFunctionValue {
	constructor := nativeFunction(func(args []any) (any, error) {
		if len(args) == 0 {
			return float64(0), nil
		}
		return toNumber(args[0]), nil
	})
	constructor.Props["isFinite"] = nativeFunction(func(args []any) (any, error) {
		if len(args) == 0 {
			return false, nil
		}
		number, ok := args[0].(float64)
		return ok && !math.IsNaN(number) && !math.IsInf(number, 0), nil
	})
	constructor.Props["isInteger"] = nativeFunction(func(args []any) (any, error) {
		if len(args) == 0 {
			return false, nil
		}
		number, ok := args[0].(float64)
		return ok && !math.IsNaN(number) && !math.IsInf(number, 0) && math.Trunc(number) == number, nil
	})
	constructor.Props["isSafeInteger"] = nativeFunction(func(args []any) (any, error) {
		if len(args) == 0 {
			return false, nil
		}
		number, ok := args[0].(float64)
		return ok && !math.IsNaN(number) && !math.IsInf(number, 0) && math.Trunc(number) == number && math.Abs(number) <= 9007199254740991, nil
	})
	constructor.Props["POSITIVE_INFINITY"] = math.Inf(1)
	constructor.Props["NEGATIVE_INFINITY"] = math.Inf(-1)
	constructor.Props["NaN"] = math.NaN()
	return constructor
}

func arrayGlobal() map[string]any {
	prototype := map[string]any{
		"push": nativeMethod(func(thisValue any, args []any) (any, error) {
			array, ok := thisValue.(*ArrayValue)
			if !ok {
				return nil, fmt.Errorf("push receiver is not array: %T %s", thisValue, jsInspect(thisValue))
			}
			array.Items = append(array.Items, args...)
			return float64(len(array.Items)), nil
		}),
	}
	for _, property := range []string{
		"pop", "shift", "unshift", "splice", "join", "map", "filter", "forEach", "reduce",
		"reduceRight", "some", "every", "find", "findIndex", "indexOf", "lastIndexOf",
		"includes", "concat", "slice", "flat", "sort",
	} {
		current := property
		prototype[current] = nativeMethod(func(thisValue any, args []any) (any, error) {
			array, ok := thisValue.(*ArrayValue)
			if !ok {
				return jsUndefined, nil
			}
			member, ok := arrayMember(array, current, Env{})
			if !ok {
				return jsUndefined, nil
			}
			return callFunctionWithValues(member, args, Env{}, thisValue)
		})
		}
		return map[string]any{
			"__call": nativeFunction(func(args []any) (any, error) {
				if len(args) == 1 {
					if length, ok := args[0].(float64); ok {
						items := []any{}
						for index := 0; index < int(length); index++ {
							items = append(items, jsUndefined)
						}
						return &ArrayValue{Items: items}, nil
					}
				}
				return &ArrayValue{Items: append([]any{}, args...)}, nil
			}),
			"from": nativeFunction(func(args []any) (any, error) {
				if len(args) == 0 {
					return &ArrayValue{Items: []any{}}, nil
				}
				if array, ok := args[0].(*ArrayValue); ok {
					return &ArrayValue{Items: append([]any{}, array.Items...)}, nil
				}
				if object, ok := args[0].(map[string]any); ok {
					length := jsInteger(object["length"])
					items := []any{}
					for index := 0; index < length; index++ {
						key := strconv.Itoa(index)
						if item, ok := lookupObjectProperty(object, key); ok {
							items = append(items, item)
						} else {
							items = append(items, jsUndefined)
						}
					}
					return &ArrayValue{Items: items}, nil
				}
				return &ArrayValue{Items: iterableValues(args[0])}, nil
			}),
			"isArray": nativeFunction(func(args []any) (any, error) {
				if len(args) == 0 {
					return false, nil
				}
			_, ok := args[0].(*ArrayValue)
			return ok, nil
		}),
		"prototype": prototype,
	}
}

func typedArrayGlobal() NativeFunctionValue {
	constructor := nativeFunction(func(args []any) (any, error) {
		length := 0
		if len(args) > 0 {
			length = jsInteger(args[0])
		}
		items := []any{}
		for index := 0; index < length; index++ {
			items = append(items, float64(0))
		}
		return &ArrayValue{Items: items}, nil
	})
	constructor.Props["of"] = nativeFunction(func(args []any) (any, error) {
		items := []any{}
		for _, arg := range args {
			items = append(items, float64(byte(toUint32(arg))))
		}
		return &ArrayValue{Items: items}, nil
	})
	constructor.Props["from"] = nativeFunction(func(args []any) (any, error) {
		if len(args) == 0 {
			return &ArrayValue{Items: []any{}}, nil
		}
		return arrayFromBytes(bytesFromJSValue(args[0])), nil
	})
	return constructor
}

func bufferGlobal() map[string]any {
	return map[string]any{
		"from": nativeFunction(func(args []any) (any, error) {
			if len(args) == 0 {
				return &ArrayValue{Items: []any{}}, nil
			}
			encoding := "utf8"
			if len(args) > 1 {
				encoding = strings.ToLower(jsString(args[1]))
			}
			if text, ok := args[0].(string); ok {
				switch encoding {
				case "hex":
					decoded, err := hex.DecodeString(text)
					if err != nil {
						return nil, err
					}
					return arrayFromBytes(decoded), nil
				case "base64":
					decoded, err := base64.StdEncoding.DecodeString(text)
					if err != nil {
						return nil, err
					}
					return arrayFromBytes(decoded), nil
				default:
					return arrayFromBytes([]byte(text)), nil
				}
			}
			return arrayFromBytes(bytesFromJSValue(args[0])), nil
		}),
		"isBuffer": nativeFunction(func(args []any) (any, error) {
			if len(args) == 0 {
				return false, nil
			}
			_, ok := args[0].(*ArrayValue)
			return ok, nil
		}),
	}
}

func cryptoModuleExports() map[string]any {
	exports := map[string]any{}
	exports["createHash"] = nativeFunction(func(args []any) (any, error) {
		algorithm := ""
		if len(args) > 0 {
			algorithm = strings.ToLower(jsString(args[0]))
		}
		var digest func([]byte) []byte
		switch algorithm {
		case "md5":
			digest = func(input []byte) []byte {
				sum := md5.Sum(input)
				return sum[:]
			}
		case "sha1":
			digest = func(input []byte) []byte {
				sum := sha1.Sum(input)
				return sum[:]
			}
		default:
			return nil, fmt.Errorf("unsupported crypto hash %s", algorithm)
		}
		data := []byte{}
		hashObject := map[string]any{}
		hashObject["update"] = nativeFunction(func(args []any) (any, error) {
			if len(args) > 0 {
				data = append(data, bytesFromJSValue(args[0])...)
			}
			return hashObject, nil
		})
		hashObject["digest"] = nativeFunction(func(args []any) (any, error) {
			output := digest(data)
			if len(args) > 0 {
				switch strings.ToLower(jsString(args[0])) {
				case "hex":
					return hex.EncodeToString(output), nil
				case "base64":
					return base64.StdEncoding.EncodeToString(output), nil
				}
			}
			return arrayFromBytes(output), nil
		})
		return hashObject, nil
	})
	exports["randomFillSync"] = nativeFunction(func(args []any) (any, error) {
		if len(args) == 0 {
			return jsUndefined, nil
		}
		array, ok := args[0].(*ArrayValue)
		if !ok {
			return args[0], nil
		}
		bytes := make([]byte, len(array.Items))
		if _, err := cryptorand.Read(bytes); err != nil {
			return nil, err
		}
		for index, item := range bytes {
			array.Items[index] = float64(item)
		}
		return array, nil
	})
	exports["randomUUID"] = nativeFunction(func(args []any) (any, error) {
		bytes := make([]byte, 16)
		if _, err := cryptorand.Read(bytes); err != nil {
			return nil, err
		}
		bytes[6] = (bytes[6] & 0x0f) | 0x40
		bytes[8] = (bytes[8] & 0x3f) | 0x80
		text := hex.EncodeToString(bytes)
		return text[0:8] + "-" + text[8:12] + "-" + text[12:16] + "-" + text[16:20] + "-" + text[20:32], nil
	})
	exports["default"] = exports
	return exports
}

func arrayFromBytes(bytes []byte) *ArrayValue {
	items := []any{}
	for _, item := range bytes {
		items = append(items, float64(item))
	}
	return &ArrayValue{Items: items}
}

func bytesFromJSValue(value any) []byte {
	switch typed := value.(type) {
	case string:
		return []byte(typed)
	case *ArrayValue:
		output := []byte{}
		for _, item := range typed.Items {
			output = append(output, byte(toUint32(item)))
		}
		return output
	default:
		return []byte(jsString(value))
	}
}

func dateGlobal() NativeFunctionValue {
	constructor := nativeFunction(func(args []any) (any, error) {
		if len(args) == 0 {
			return &DateValue{Time: time.Now().UTC()}, nil
		}
		if text, ok := args[0].(string); ok {
			if parsed, err := time.Parse(time.RFC3339, text); err == nil {
				return &DateValue{Time: parsed.UTC()}, nil
			}
			if parsed, err := time.Parse("2006-01-02", text); err == nil {
				return &DateValue{Time: parsed.UTC()}, nil
			}
		}
		return &DateValue{Time: time.UnixMilli(int64(toNumber(args[0]))).UTC()}, nil
	})
	constructor.Props["now"] = nativeFunction(func(args []any) (any, error) {
		return float64(time.Now().UnixMilli()), nil
	})
	constructor.Props["UTC"] = nativeFunction(func(args []any) (any, error) {
		year := 0
		month := 0
		day := 1
		hour := 0
		minute := 0
		second := 0
		millisecond := 0
		if len(args) > 0 {
			year = int(toNumber(args[0]))
		}
		if len(args) > 1 {
			month = int(toNumber(args[1]))
		}
		if len(args) > 2 {
			day = int(toNumber(args[2]))
		}
		if len(args) > 3 {
			hour = int(toNumber(args[3]))
		}
		if len(args) > 4 {
			minute = int(toNumber(args[4]))
		}
		if len(args) > 5 {
			second = int(toNumber(args[5]))
		}
		if len(args) > 6 {
			millisecond = int(toNumber(args[6]))
		}
		value := time.Date(year, time.Month(month+1), day, hour, minute, second, millisecond*int(time.Millisecond), time.UTC)
		return float64(value.UnixMilli()), nil
	})
	return constructor
}

func promiseGlobal() NativeFunctionValue {
	constructor := nativeFunction(func(args []any) (any, error) {
		promise := promiseFulfilled(jsUndefined)
		promise["state"] = "pending"
		if len(args) > 0 {
			resolve := nativeFunction(func(args []any) (any, error) {
				value := any(jsUndefined)
				if len(args) > 0 {
					value = args[0]
				}
				promise["state"] = "fulfilled"
				promise["value"] = value
				return jsUndefined, nil
			})
			reject := nativeFunction(func(args []any) (any, error) {
				value := any(jsUndefined)
				if len(args) > 0 {
					value = args[0]
				}
				promise["state"] = "rejected"
				promise["value"] = value
				return jsUndefined, nil
			})
			if _, err := callFunctionWithValues(args[0], []any{resolve, reject}, Env{}, jsUndefined); err != nil {
				return nil, err
			}
		}
		return promise, nil
	})
	prototype := map[string]any{}
	prototype["then"] = nativeMethod(promiseThen)
	prototype["catch"] = nativeMethod(promiseCatch)
	prototype["finally"] = nativeMethod(promiseFinally)
	constructor.Props["prototype"] = prototype
	constructor.Props["all"] = nativeFunction(func(args []any) (any, error) {
		if len(args) == 0 {
			return promiseFulfilled(&ArrayValue{Items: []any{}}), nil
		}
		values := iterableValues(args[0])
		out := []any{}
		for _, value := range values {
			next, err := awaitValue(value)
			if err != nil {
				return promiseRejected(err), nil
			}
			out = append(out, next)
		}
		return promiseFulfilled(&ArrayValue{Items: out}), nil
	})
	constructor.Props["allSettled"] = nativeFunction(func(args []any) (any, error) {
		out := []any{}
		if len(args) > 0 {
			for _, value := range iterableValues(args[0]) {
				if promise, ok := value.(map[string]any); ok && promise["__promise"] == true && promise["state"] == "rejected" {
					out = append(out, map[string]any{"status": "rejected", "reason": promise["value"]})
					continue
				}
				resolved, err := awaitValue(value)
				if err != nil {
					if thrown, ok := err.(jsThrow); ok {
						out = append(out, map[string]any{"status": "rejected", "reason": thrown.value})
						continue
					}
					out = append(out, map[string]any{"status": "rejected", "reason": nodeError("ERR_PROMISE_REJECTION", err.Error())})
					continue
				}
				out = append(out, map[string]any{"status": "fulfilled", "value": resolved})
			}
		}
		return promiseFulfilled(&ArrayValue{Items: out}), nil
	})
	constructor.Props["race"] = nativeFunction(func(args []any) (any, error) {
		if len(args) == 0 {
			return promisePending(), nil
		}
		values := iterableValues(args[0])
		if len(values) == 0 {
			return promisePending(), nil
		}
		for _, value := range values {
			if promise, ok := value.(map[string]any); ok && promise["__promise"] == true {
				if promise["state"] == "pending" {
					continue
				}
				return promise, nil
			}
			return promiseFulfilled(value), nil
		}
		return promisePending(), nil
	})
	constructor.Props["resolve"] = nativeFunction(func(args []any) (any, error) {
		if len(args) == 0 {
			return promiseFulfilled(jsUndefined), nil
		}
		return promiseFulfilled(args[0]), nil
	})
	constructor.Props["reject"] = nativeFunction(func(args []any) (any, error) {
		if len(args) == 0 {
			return promiseRejected(jsUndefined), nil
		}
		return promiseRejected(args[0]), nil
	})
	return constructor
}

func promiseFulfilled(value any) map[string]any {
	return promiseObject("fulfilled", value)
}

func promisePending() map[string]any {
	return promiseObject("pending", jsUndefined)
}

func promiseRejected(value any) map[string]any {
	return promiseObject("rejected", value)
}

func promiseObject(state string, value any) map[string]any {
	promise := map[string]any{
		"__promise": true,
		"state":     state,
		"value":     value,
	}
	promise["then"] = nativeMethod(promiseThen)
	promise["catch"] = nativeMethod(promiseCatch)
	promise["finally"] = nativeMethod(promiseFinally)
	return promise
}

func promiseThen(thisValue any, args []any) (any, error) {
	promise, ok := thisValue.(map[string]any)
	if !ok || promise["__promise"] != true {
		return nil, errors.New("Promise.prototype.then called on incompatible receiver")
	}
	if promise["state"] == "fulfilled" {
		if len(args) > 0 && !isNullish(args[0]) {
			next, err := callFunctionWithValues(args[0], []any{promise["value"]}, Env{}, jsUndefined)
			if err != nil {
				return promiseRejectedFromError(err), nil
			}
			return promiseFulfilled(next), nil
		}
		return promise, nil
	}
	if promise["state"] == "rejected" && len(args) > 1 && !isNullish(args[1]) {
		next, err := callFunctionWithValues(args[1], []any{promise["value"]}, Env{}, jsUndefined)
		if err != nil {
			return promiseRejectedFromError(err), nil
		}
		return promiseFulfilled(next), nil
	}
	return promise, nil
}

func promiseCatch(thisValue any, args []any) (any, error) {
	promise, ok := thisValue.(map[string]any)
	if !ok || promise["__promise"] != true {
		return nil, errors.New("Promise.prototype.catch called on incompatible receiver")
	}
	if promise["state"] == "rejected" && len(args) > 0 && !isNullish(args[0]) {
		next, err := callFunctionWithValues(args[0], []any{promise["value"]}, Env{}, jsUndefined)
		if err != nil {
			return promiseRejectedFromError(err), nil
		}
		return promiseFulfilled(next), nil
	}
	return promise, nil
}

func promiseFinally(thisValue any, args []any) (any, error) {
	promise, ok := thisValue.(map[string]any)
	if !ok || promise["__promise"] != true {
		return nil, errors.New("Promise.prototype.finally called on incompatible receiver")
	}
	if len(args) > 0 && !isNullish(args[0]) {
		if _, err := callFunctionWithValues(args[0], []any{}, Env{}, jsUndefined); err != nil {
			return promiseRejectedFromError(err), nil
		}
	}
	return promise, nil
}

func awaitValue(value any) (any, error) {
	return awaitValueDepth(value, 0)
}

func awaitValueDepth(value any, depth int) (any, error) {
	if depth > 64 {
		return nil, errors.New("promise resolution exceeded maximum depth")
	}
	if promise, ok := value.(map[string]any); ok && promise["__promise"] == true {
		if promise["state"] == "pending" {
			return nil, pendingAwait{promise: promise}
		}
		if promise["state"] == "rejected" {
			return nil, jsThrow{value: promise["value"]}
		}
		resolved := promise["value"]
		if resolvedMap, ok := resolved.(map[string]any); ok {
			then, hasThen := lookupObjectProperty(resolvedMap, "then")
			if resolvedMap["__promise"] == true || (hasThen && isCallable(then)) {
				return awaitValueDepth(resolvedMap, depth+1)
			}
		}
		return resolved, nil
	}
	if object, ok := value.(map[string]any); ok {
		if then, ok := lookupObjectProperty(object, "then"); ok && isCallable(then) {
			resolved := any(jsUndefined)
			rejected := any(nil)
			settled := false
			resolve := nativeFunction(func(args []any) (any, error) {
				settled = true
				if len(args) > 0 {
					resolved = args[0]
				}
				return jsUndefined, nil
			})
			reject := nativeFunction(func(args []any) (any, error) {
				settled = true
				if len(args) > 0 {
					rejected = args[0]
				} else {
					rejected = jsUndefined
				}
				return jsUndefined, nil
			})
			result, err := callFunctionWithValues(then, []any{resolve, reject}, Env{}, object)
			if err != nil {
				return nil, err
			}
			if settled {
				if rejected != nil {
					return nil, jsThrow{value: rejected}
				}
				return resolved, nil
			}
			if resultMap, ok := result.(map[string]any); ok && resultMap["__promise"] == true {
				return awaitValueDepth(resultMap, depth+1)
			}
			return result, nil
		}
	}
	return value, nil
}

type pendingAwait struct {
	promise map[string]any
}

func (pendingAwait) Error() string {
	return "promise is pending"
}

func promiseRejectedFromError(err error) map[string]any {
	var thrown jsThrow
	if errors.As(err, &thrown) {
		return promiseRejected(thrown.value)
	}
	return promiseRejected(nodeError("ERR_PROMISE_REJECTION", err.Error()))
}

func isCallable(value any) bool {
	switch typed := value.(type) {
	case FunctionValue, BoundFunctionValue, NativeFunctionValue, *ClassValue:
		return true
	case map[string]any:
		_, ok := typed["__call"]
		return ok
	default:
		return false
	}
}

func objectGlobal() map[string]any {
	prototype := map[string]any{}
	prototype["hasOwnProperty"] = nativeMethod(func(thisValue any, args []any) (any, error) {
		if len(args) == 0 {
			return false, nil
		}
		if array, ok := thisValue.(*ArrayValue); ok {
			key := jsPropertyKey(args[0])
			if key == "length" {
				return true, nil
			}
			index, err := strconv.Atoi(key)
			if err == nil && index >= 0 && index < len(array.Items) {
				return true, nil
			}
			_, exists := array.Props[key]
			return exists, nil
		}
		object, ok := thisValue.(map[string]any)
		if !ok {
			return false, nil
		}
		_, exists := object[jsPropertyKey(args[0])]
		return exists, nil
	})
	prototype["toString"] = nativeMethod(func(thisValue any, args []any) (any, error) {
		return objectTag(thisValue), nil
	})
	assign := nativeFunction(func(args []any) (any, error) {
		if len(args) == 0 || isNullish(args[0]) {
			return map[string]any{}, nil
		}
		target := args[0]
		for _, source := range args[1:] {
			if sourceMap, ok := source.(map[string]any); ok {
				for _, key := range objectKeys(sourceMap) {
					value, exists := sourceMap[key]
					if !exists {
						continue
					}
					setDynamicProperty(target, key, value)
				}
			}
		}
		return target, nil
	})
	return map[string]any{
			"assign": assign,
			"create": nativeFunction(func(args []any) (any, error) {
				if len(args) == 0 || isNullish(args[0]) {
					return map[string]any{}, nil
				}
				return objectWithPrototype(args[0]), nil
			}),
			"defineProperty": nativeFunction(func(args []any) (any, error) {
			if len(args) < 3 {
				return jsUndefined, nil
			}
			descriptor, ok := args[2].(map[string]any)
			if ok {
				setDynamicProperty(args[0], jsPropertyKey(args[1]), descriptor["value"])
			}
			return args[0], nil
		}),
		"entries": nativeFunction(func(args []any) (any, error) {
			if len(args) == 0 {
				return &ArrayValue{Items: []any{}}, nil
			}
			if array, ok := args[0].(*ArrayValue); ok {
				result := []any{}
				for index, item := range array.Items {
					result = append(result, &ArrayValue{Items: []any{strconv.Itoa(index), item}})
				}
				for _, key := range objectKeys(array.Props) {
					result = append(result, &ArrayValue{Items: []any{key, array.Props[key]}})
				}
				return &ArrayValue{Items: result}, nil
			}
			object, ok := args[0].(map[string]any)
			if !ok {
				return &ArrayValue{Items: []any{}}, nil
			}
			keys := objectKeys(object)
			result := []any{}
			for _, key := range keys {
				result = append(result, &ArrayValue{Items: []any{key, object[key]}})
			}
			return &ArrayValue{Items: result}, nil
		}),
		"values": nativeFunction(func(args []any) (any, error) {
			if len(args) == 0 {
				return &ArrayValue{Items: []any{}}, nil
			}
			if array, ok := args[0].(*ArrayValue); ok {
				result := append([]any{}, array.Items...)
				for _, key := range objectKeys(array.Props) {
					result = append(result, array.Props[key])
				}
				return &ArrayValue{Items: result}, nil
			}
			object, ok := args[0].(map[string]any)
			if !ok {
				return &ArrayValue{Items: []any{}}, nil
			}
			result := []any{}
			for _, key := range objectKeys(object) {
				result = append(result, object[key])
			}
			return &ArrayValue{Items: result}, nil
		}),
		"freeze": nativeFunction(func(args []any) (any, error) {
			if len(args) == 0 {
				return jsUndefined, nil
			}
			return args[0], nil
		}),
		"fromEntries": nativeFunction(func(args []any) (any, error) {
			out := map[string]any{}
			if len(args) == 0 {
				return out, nil
			}
			for _, entry := range iterableValues(args[0]) {
				values := iterableValues(entry)
				if len(values) >= 2 {
					setObjectProperty(out, jsPropertyKey(values[0]), values[1])
				}
			}
			return out, nil
		}),
		"getOwnPropertyNames": nativeFunction(func(args []any) (any, error) {
			if len(args) == 0 {
				return &ArrayValue{Items: []any{}}, nil
			}
			if object, ok := args[0].(map[string]any); ok {
				out := []any{}
				for _, key := range objectKeys(object) {
					out = append(out, key)
				}
				return &ArrayValue{Items: out}, nil
			}
			return &ArrayValue{Items: []any{}}, nil
		}),
		"getOwnPropertyDescriptor": nativeFunction(func(args []any) (any, error) {
			if len(args) < 2 {
				return jsUndefined, nil
			}
			key := jsPropertyKey(args[1])
			if object, ok := args[0].(map[string]any); ok {
				if value, ok := lookupObjectProperty(object, key); ok {
					return map[string]any{
						"value":        value,
						"enumerable":   true,
						"configurable": true,
						"writable":     true,
					}, nil
				}
			}
			return jsUndefined, nil
		}),
		"getPrototypeOf": nativeFunction(func(args []any) (any, error) {
			if len(args) > 0 {
				if object, ok := args[0].(map[string]any); ok {
					if prototype, ok := object["__prototype"]; ok {
						return prototype, nil
					}
				}
			}
			return jsNull, nil
		}),
		"setPrototypeOf": nativeFunction(func(args []any) (any, error) {
			if len(args) >= 2 {
				if object, ok := args[0].(map[string]any); ok {
					object["__prototype"] = args[1]
				}
				return args[0], nil
			}
			return jsUndefined, nil
		}),
		"keys": nativeFunction(func(args []any) (any, error) {
			if len(args) == 0 {
				return &ArrayValue{Items: []any{}}, nil
			}
			if array, ok := args[0].(*ArrayValue); ok {
				result := []any{}
				for index := range array.Items {
					result = append(result, strconv.Itoa(index))
				}
				for _, key := range objectKeys(array.Props) {
					result = append(result, key)
				}
				return &ArrayValue{Items: result}, nil
			}
			object, ok := args[0].(map[string]any)
			if !ok {
				return &ArrayValue{Items: []any{}}, nil
			}
			keys := objectKeys(object)
			result := []any{}
			for _, key := range keys {
				result = append(result, key)
			}
			return &ArrayValue{Items: result}, nil
		}),
		"prototype": prototype,
	}
}

func reflectGlobal() map[string]any {
	return map[string]any{
		"apply": nativeFunction(func(args []any) (any, error) {
			if len(args) == 0 {
				return nil, errors.New("Reflect.apply target is required")
			}
			thisValue := any(jsUndefined)
			if len(args) > 1 {
				thisValue = args[1]
			}
			callArgs := []any{}
			if len(args) > 2 {
				callArgs = iterableValues(args[2])
			}
			return callFunctionWithValues(args[0], callArgs, Env{}, thisValue)
		}),
		"defineProperty": nativeFunction(func(args []any) (any, error) {
			if len(args) < 3 {
				return false, nil
			}
			if descriptor, ok := args[2].(map[string]any); ok {
				setDynamicProperty(args[0], jsPropertyKey(args[1]), descriptor["value"])
				return true, nil
			}
			return false, nil
		}),
		"getOwnPropertyDescriptor": nativeFunction(func(args []any) (any, error) {
			if len(args) < 2 {
				return jsUndefined, nil
			}
			key := jsPropertyKey(args[1])
			if object, ok := args[0].(map[string]any); ok {
				if value, ok := lookupObjectProperty(object, key); ok {
					return map[string]any{
						"value":        value,
						"enumerable":   true,
						"configurable": true,
						"writable":     true,
					}, nil
				}
			}
			return jsUndefined, nil
		}),
		"deleteProperty": nativeFunction(func(args []any) (any, error) {
			if len(args) < 2 {
				return false, nil
			}
			return deleteDynamicProperty(args[0], jsPropertyKey(args[1])), nil
		}),
	}
}

func jsonGlobal() map[string]any {
	return map[string]any{
		"stringify": nativeFunction(func(args []any) (any, error) {
			if len(args) == 0 {
				return jsUndefined, nil
			}
			var out bytes.Buffer
			encoder := json.NewEncoder(&out)
			encoder.SetEscapeHTML(false)
			if err := encoder.Encode(jsonCompatible(args[0], map[uintptr]bool{})); err != nil {
				return nil, err
			}
			return strings.TrimSuffix(out.String(), "\n"), nil
		}),
		"parse": nativeFunction(func(args []any) (any, error) {
			if len(args) == 0 {
				return jsUndefined, nil
			}
			decoder := json.NewDecoder(strings.NewReader(jsString(args[0])))
			value, err := parseJSONValue(decoder)
			if err != nil {
				return nil, err
			}
			return value, nil
		}),
	}
}

func mathGlobal() map[string]any {
	return map[string]any{
		"random": nativeFunction(func(args []any) (any, error) {
			return float64(time.Now().UnixNano()%1000000) / 1000000, nil
		}),
		"ceil": nativeFunction(func(args []any) (any, error) {
			if len(args) == 0 {
				return math.NaN(), nil
			}
			return math.Ceil(toNumber(args[0])), nil
		}),
		"floor": nativeFunction(func(args []any) (any, error) {
			if len(args) == 0 {
				return math.NaN(), nil
			}
			return math.Floor(toNumber(args[0])), nil
		}),
		"round": nativeFunction(func(args []any) (any, error) {
			if len(args) == 0 {
				return math.NaN(), nil
			}
			return math.Round(toNumber(args[0])), nil
		}),
		"trunc": nativeFunction(func(args []any) (any, error) {
			if len(args) == 0 {
				return math.NaN(), nil
			}
			return math.Trunc(toNumber(args[0])), nil
		}),
			"abs": nativeFunction(func(args []any) (any, error) {
				if len(args) == 0 {
					return math.NaN(), nil
				}
				return math.Abs(toNumber(args[0])), nil
			}),
			"pow": nativeFunction(func(args []any) (any, error) {
				if len(args) < 2 {
					return math.NaN(), nil
				}
				return math.Pow(toNumber(args[0]), toNumber(args[1])), nil
			}),
		"max": nativeFunction(func(args []any) (any, error) {
			if len(args) == 0 {
				return math.Inf(-1), nil
			}
			value := toNumber(args[0])
			for _, arg := range args[1:] {
				value = math.Max(value, toNumber(arg))
			}
			return value, nil
		}),
		"min": nativeFunction(func(args []any) (any, error) {
			if len(args) == 0 {
				return math.Inf(1), nil
			}
			value := toNumber(args[0])
			for _, arg := range args[1:] {
				value = math.Min(value, toNumber(arg))
			}
			return value, nil
		}),
	}
}

func symbolGlobal() map[string]any {
	prototype := map[string]any{}
	prototype["toString"] = nativeMethod(func(thisValue any, args []any) (any, error) {
		return jsString(thisValue), nil
	})
	call := nativeFunction(func(args []any) (any, error) {
		description := ""
		if len(args) > 0 {
			description = jsString(args[0])
		}
		return &SymbolValue{Description: description}, nil
	})
	return map[string]any{
		"__call":    call,
		"iterator":  &SymbolValue{Description: "Symbol.iterator"},
		"asyncIterator": &SymbolValue{Description: "Symbol.asyncIterator"},
		"prototype": prototype,
		"for": nativeFunction(func(args []any) (any, error) {
			description := ""
			if len(args) > 0 {
				description = jsString(args[0])
			}
			return &SymbolValue{Description: description}, nil
		}),
		"keyFor": nativeFunction(func(args []any) (any, error) {
			if len(args) == 0 {
				return jsUndefined, nil
			}
			if symbol, ok := args[0].(*SymbolValue); ok {
				return symbol.Description, nil
			}
			return jsUndefined, nil
		}),
	}
}

func mapGlobal() NativeFunctionValue {
	constructor := nativeFunction(func(args []any) (any, error) {
		value := &MapValue{Entries: []MapEntry{}}
		if len(args) > 0 {
			for _, entry := range iterableValues(args[0]) {
				if pair, ok := entry.(*ArrayValue); ok && len(pair.Items) >= 2 {
					mapSet(value, pair.Items[0], pair.Items[1])
				}
			}
		}
		return value, nil
	})
	prototype := map[string]any{}
	for _, property := range []string{"get", "set", "has", "delete", "clear", "keys", "values", "entries"} {
		current := property
		prototype[current] = nativeMethod(func(thisValue any, args []any) (any, error) {
			value, ok := thisValue.(*MapValue)
			if !ok {
				return jsUndefined, nil
			}
			member, ok := mapMember(value, current)
			if !ok {
				return jsUndefined, nil
			}
			return callFunctionWithValues(member, args, Env{}, thisValue)
		})
	}
	constructor.Props["prototype"] = prototype
	return constructor
}

func setGlobal() NativeFunctionValue {
	constructor := nativeFunction(func(args []any) (any, error) {
		value := &SetValue{Values: []any{}}
		if len(args) > 0 {
			for _, item := range iterableValues(args[0]) {
				setAdd(value, item)
			}
		}
		return value, nil
	})
	prototype := map[string]any{}
	for _, property := range []string{"add", "has", "delete", "clear", "keys", "values", "entries"} {
		current := property
		prototype[current] = nativeMethod(func(thisValue any, args []any) (any, error) {
			value, ok := thisValue.(*SetValue)
			if !ok {
				return jsUndefined, nil
			}
			member, ok := setMember(value, current)
			if !ok {
				return jsUndefined, nil
			}
			return callFunctionWithValues(member, args, Env{}, thisValue)
		})
	}
	constructor.Props["prototype"] = prototype
	return constructor
}

func objectKeys(object map[string]any) []string {
	seen := map[string]bool{}
	keys := []string{}
	if ordered, ok := object["__keys"].([]string); ok {
		for _, key := range ordered {
			if strings.HasPrefix(key, "__") || seen[key] {
				continue
			}
			if _, exists := object[key]; exists {
				keys = append(keys, key)
				seen[key] = true
			}
		}
	}
	unordered := []string{}
	for key := range object {
		if strings.HasPrefix(key, "__") || seen[key] {
			continue
		}
		unordered = append(unordered, key)
	}
	sort.Strings(unordered)
	keys = append(keys, unordered...)
	return keys
}

func setObjectProperty(object map[string]any, key string, value any) {
	if !strings.HasPrefix(key, "__") {
		if _, exists := object[key]; !exists {
			keys, _ := object["__keys"].([]string)
			object["__keys"] = append(keys, key)
		}
	}
	object[key] = value
}

func setDynamicProperty(target any, property string, value any) bool {
	switch typed := target.(type) {
	case map[string]any:
		setObjectProperty(typed, property, value)
		return true
	case *RegExpValue:
		if property == "lastIndex" {
			typed.LastIndex = jsInteger(value)
			return true
		}
		if typed.Props == nil {
			typed.Props = map[string]any{}
		}
		typed.Props[property] = value
		return true
	case *ArrayValue:
		return assignArrayMember(typed, property, value)
	case FunctionValue:
		if typed.Props == nil {
			typed.Props = map[string]any{}
		}
		typed.Props[property] = value
		return true
	case NativeFunctionValue:
		if typed.Props == nil {
			typed.Props = map[string]any{}
		}
		typed.Props[property] = value
		return true
	default:
		return false
	}
}

func deleteDynamicProperty(target any, property string) bool {
	switch typed := target.(type) {
	case map[string]any:
		delete(typed, property)
		return true
	case *ArrayValue:
		index, err := strconv.Atoi(property)
		if err == nil && index >= 0 && index < len(typed.Items) {
			typed.Items[index] = jsUndefined
			return true
		}
		if typed.Props != nil {
			delete(typed.Props, property)
		}
		return true
	case FunctionValue:
		if typed.Props != nil {
			delete(typed.Props, property)
		}
		return true
	case NativeFunctionValue:
		if typed.Props != nil {
			delete(typed.Props, property)
		}
		return true
	default:
		return true
	}
}

func jsonCompatible(value any, seen map[uintptr]bool) any {
	switch typed := value.(type) {
	case nil, UndefinedValue, NullValue:
		return nil
	case bool, string, float64:
		return typed
	case *SymbolValue, FunctionValue, BoundFunctionValue, NativeFunctionValue, *ClassValue:
		return nil
	case *ArrayValue:
		id := referenceIdentity(typed)
		if id != 0 {
			if seen[id] {
				return nil
			}
			seen[id] = true
			defer delete(seen, id)
		}
		out := []any{}
		for _, item := range typed.Items {
			out = append(out, jsonCompatible(item, seen))
		}
		return out
	case map[string]any:
		id := referenceIdentity(typed)
		if id != 0 {
			if seen[id] {
				return nil
			}
			seen[id] = true
			defer delete(seen, id)
		}
		out := map[string]any{}
		for _, key := range objectKeys(typed) {
			if value, ok := typed[key]; ok {
				converted := jsonCompatible(value, seen)
				if _, skip := value.(UndefinedValue); !skip {
					out[key] = converted
				}
			}
		}
		return out
	case *DateValue:
		return formatDateISO(typed.Time)
	case *RegExpValue:
		return map[string]any{}
	case *MapValue:
		return map[string]any{}
	case *SetValue:
		return map[string]any{}
	default:
		return typed
	}
}

func parseJSONValue(decoder *json.Decoder) (any, error) {
	token, err := decoder.Token()
	if err != nil {
		return nil, err
	}
	switch typed := token.(type) {
	case json.Delim:
		switch typed {
		case '{':
			out := map[string]any{}
			for decoder.More() {
				keyToken, err := decoder.Token()
				if err != nil {
					return nil, err
				}
				key, ok := keyToken.(string)
				if !ok {
					return nil, fmt.Errorf("JSON object key is not a string: %T", keyToken)
				}
				value, err := parseJSONValue(decoder)
				if err != nil {
					return nil, err
				}
				setObjectProperty(out, key, value)
			}
			if end, err := decoder.Token(); err != nil {
				return nil, err
			} else if end != json.Delim('}') {
				return nil, fmt.Errorf("JSON object ended with %v", end)
			}
			return out, nil
		case '[':
			items := []any{}
			for decoder.More() {
				value, err := parseJSONValue(decoder)
				if err != nil {
					return nil, err
				}
				items = append(items, value)
			}
			if end, err := decoder.Token(); err != nil {
				return nil, err
			} else if end != json.Delim(']') {
				return nil, fmt.Errorf("JSON array ended with %v", end)
			}
			return &ArrayValue{Items: items}, nil
		default:
			return nil, fmt.Errorf("unsupported JSON delimiter %q", typed)
		}
	case nil:
		return jsNull, nil
	case bool, string, float64:
		return typed, nil
	case json.Number:
		number, err := typed.Float64()
		if err != nil {
			return nil, err
		}
		return number, nil
	default:
		return nil, fmt.Errorf("unsupported JSON token %T", token)
	}
}

func jsValueFromJSON(value any) any {
	switch typed := value.(type) {
	case nil:
		return jsNull
	case []any:
		out := []any{}
		for _, item := range typed {
			out = append(out, jsValueFromJSON(item))
		}
		return &ArrayValue{Items: out}
	case map[string]any:
		out := map[string]any{}
		for key, item := range typed {
			out[key] = jsValueFromJSON(item)
		}
		return out
	default:
		return typed
	}
}

func objectTag(value any) string {
	switch value.(type) {
	case string:
		return "[object String]"
	case float64:
		return "[object Number]"
	case bool:
		return "[object Boolean]"
	case *ArrayValue:
		return "[object Array]"
	case *RegExpValue:
		return "[object RegExp]"
	case *MapValue:
		return "[object Map]"
	case *SetValue:
		return "[object Set]"
	case *SymbolValue:
		return "[object Symbol]"
	case NullValue:
		return "[object Null]"
	case UndefinedValue:
		return "[object Undefined]"
	case FunctionValue, BoundFunctionValue, NativeFunctionValue:
		return "[object Function]"
	default:
		if object, ok := value.(map[string]any); ok {
			if tag, ok := object["__tag"].(string); ok {
				return tag
			}
		}
		return "[object Object]"
	}
}

func newRegExp(pattern string, flags string) (*RegExpValue, error) {
	goPattern := pattern
	if strings.Contains(flags, "m") {
		goPattern = "(?m)" + goPattern
	}
	if strings.Contains(flags, "s") {
		goPattern = "(?s)" + goPattern
	}
	if strings.Contains(flags, "i") {
		goPattern = "(?i)" + goPattern
	}
	compiled, stdErr := regexp.Compile(goPattern)
	var compiled2 *regexp2.Regexp
	if stdErr != nil {
		options := regexp2.RegexOptions(regexp2.ECMAScript)
		if strings.Contains(flags, "i") {
			options |= regexp2.IgnoreCase
		}
		if strings.Contains(flags, "m") {
			options |= regexp2.Multiline
		}
		if strings.Contains(flags, "s") {
			options |= regexp2.Singleline
		}
		var err error
		compiled2, err = regexp2.Compile(pattern, options)
		if err != nil {
			return nil, err
		}
	}
	return &RegExpValue{
		Pattern: pattern,
		Flags:   flags,
		Regex:   compiled,
		Regex2:  compiled2,
		Global:  strings.Contains(flags, "g"),
		LastIndex: 0,
		Props:   map[string]any{},
	}, nil
}

func regexpMatches(value *RegExpValue, text string) any {
	if value.Global {
		matches, err := regexpFindAll(value, text)
		if err != nil {
			return jsNull
		}
		if len(matches) == 0 {
			return jsNull
		}
		result := []any{}
		for _, match := range matches {
			result = append(result, match.Groups[0])
		}
		return &ArrayValue{Items: result}
	}
	match, err := regexpFindFirst(value, text)
	if err != nil || match == nil {
		return jsNull
	}
	result := []any{}
	for _, group := range match.Groups {
		if group == "" {
			result = append(result, jsUndefined)
		} else {
			result = append(result, group)
		}
	}
	return &ArrayValue{Items: result}
}

func regexpExec(value *RegExpValue, text string) any {
	start := 0
	if value.Global {
		start = value.LastIndex
	}
	match, err := regexpFindFirstFrom(value, text, start)
	if err != nil || match == nil {
		if value.Global {
			value.LastIndex = 0
		}
		return jsNull
	}
	if value.Global {
		value.LastIndex = match.Index[1]
	}
	result := &ArrayValue{
		Items: []any{},
		Props: map[string]any{
			"index": float64(match.Index[0]),
			"input": text,
		},
	}
	for _, group := range match.Groups {
		if group == "" {
			result.Items = append(result.Items, jsUndefined)
		} else {
			result.Items = append(result.Items, group)
		}
	}
	return result
}

type regexpMatch struct {
	Groups []string
	Index  []int
}

func regexpFindFirst(value *RegExpValue, text string) (*regexpMatch, error) {
	return regexpFindFirstFrom(value, text, 0)
}

func regexpFindFirstFrom(value *RegExpValue, text string, start int) (*regexpMatch, error) {
	if start < 0 {
		start = 0
	}
	if start > len(text) {
		return nil, nil
	}
	if value.Regex != nil {
		raw := value.Regex.FindStringSubmatchIndex(text[start:])
		if raw == nil {
			return nil, nil
		}
		for index := range raw {
			if raw[index] >= 0 {
				raw[index] += start
			}
		}
		matches := regexpMatchesFromStd(text, [][]int{raw})
		if len(matches) == 0 {
			return nil, nil
		}
		return &matches[0], nil
	}
	if value.Regex2 == nil {
		return nil, nil
	}
	match, err := value.Regex2.FindStringMatchStartingAt(text, byteIndexToRuneIndex(text, start))
	if err != nil || match == nil {
		return nil, err
	}
	result := regexpMatchFromRegexp2(text, match)
	return &result, nil
}

func regexpFindAll(value *RegExpValue, text string) ([]regexpMatch, error) {
	if value.Regex != nil {
		raw := value.Regex.FindAllStringSubmatchIndex(text, -1)
		return regexpMatchesFromStd(text, raw), nil
	}
	if value.Regex2 == nil {
		return nil, nil
	}
	out := []regexpMatch{}
	match, err := value.Regex2.FindStringMatch(text)
	for match != nil {
		if err != nil {
			return nil, err
		}
		out = append(out, regexpMatchFromRegexp2(text, match))
		match, err = value.Regex2.FindNextMatch(match)
	}
	return out, err
}

func regexpMatchesFromStd(text string, raw [][]int) []regexpMatch {
	if raw == nil {
		return nil
	}
	out := []regexpMatch{}
	for _, indexes := range raw {
		match := regexpMatch{Groups: []string{}, Index: append([]int{}, indexes...)}
		for index := 0; index < len(indexes); index += 2 {
			if indexes[index] < 0 || indexes[index+1] < 0 {
				match.Groups = append(match.Groups, "")
			} else {
				match.Groups = append(match.Groups, text[indexes[index]:indexes[index+1]])
			}
		}
		out = append(out, match)
	}
	return out
}

func regexpMatchFromRegexp2(text string, match *regexp2.Match) regexpMatch {
	groups := match.Groups()
	indexes := []int{}
	values := []string{}
	for _, group := range groups {
		if len(group.Captures) == 0 {
			values = append(values, "")
			indexes = append(indexes, -1, -1)
			continue
		}
		start := runeIndexToByteIndex(text, group.Index)
		end := runeIndexToByteIndex(text, group.Index+group.Length)
		values = append(values, text[start:end])
		indexes = append(indexes, start, end)
	}
	return regexpMatch{Groups: values, Index: indexes}
}

func runeIndexToByteIndex(value string, runeIndex int) int {
	if runeIndex <= 0 {
		return 0
	}
	current := 0
	for byteIndex := range value {
		if current == runeIndex {
			return byteIndex
		}
		current++
	}
	return len(value)
}

func byteIndexToRuneIndex(value string, byteIndex int) int {
	if byteIndex <= 0 {
		return 0
	}
	runeIndex := 0
	for current := range value {
		if current >= byteIndex {
			return runeIndex
		}
		runeIndex++
	}
	return len([]rune(value))
}

func pathModuleExports() map[string]any {
	exports := map[string]any{}
	exports["sep"] = string(os.PathSeparator)
	exports["delimiter"] = string(os.PathListSeparator)
	exports["normalize"] = nativeFunction(func(args []any) (any, error) {
		if len(args) == 0 {
			return ".", nil
		}
		return filepath.Clean(jsString(args[0])), nil
	})
	exports["join"] = nativeFunction(func(args []any) (any, error) {
		parts := []string{}
		for _, arg := range args {
			parts = append(parts, jsString(arg))
		}
		return filepath.Clean(filepath.Join(parts...)), nil
	})
	exports["resolve"] = nativeFunction(func(args []any) (any, error) {
		parts := []string{}
		for _, arg := range args {
			parts = append(parts, jsString(arg))
		}
		if len(parts) == 0 || !filepath.IsAbs(parts[0]) {
			cwd, err := os.Getwd()
			if err != nil {
				return nil, err
			}
			parts = append([]string{cwd}, parts...)
		}
		return filepath.Clean(filepath.Join(parts...)), nil
	})
	exports["dirname"] = nativeFunction(func(args []any) (any, error) {
		if len(args) == 0 {
			return ".", nil
		}
		return filepath.Dir(jsString(args[0])), nil
	})
	exports["basename"] = nativeFunction(func(args []any) (any, error) {
		if len(args) == 0 {
			return "", nil
		}
		base := filepath.Base(jsString(args[0]))
		if len(args) > 1 {
			suffix := jsString(args[1])
			base = strings.TrimSuffix(base, suffix)
		}
		return base, nil
	})
	exports["extname"] = nativeFunction(func(args []any) (any, error) {
		if len(args) == 0 {
			return "", nil
		}
		return filepath.Ext(jsString(args[0])), nil
	})
	exports["isAbsolute"] = nativeFunction(func(args []any) (any, error) {
		return len(args) > 0 && filepath.IsAbs(jsString(args[0])), nil
	})
	exports["relative"] = nativeFunction(func(args []any) (any, error) {
		if len(args) < 2 {
			return "", nil
		}
		relative, err := filepath.Rel(jsString(args[0]), jsString(args[1]))
		if err != nil {
			return jsString(args[1]), nil
		}
		return relative, nil
	})
	exports["parse"] = nativeFunction(func(args []any) (any, error) {
		if len(args) == 0 {
			return map[string]any{"root": "", "dir": "", "base": "", "ext": "", "name": ""}, nil
		}
		value := jsString(args[0])
		base := filepath.Base(value)
		ext := filepath.Ext(base)
		root := ""
		if filepath.IsAbs(value) {
			root = string(os.PathSeparator)
		}
		return map[string]any{
			"root": root,
			"dir":  filepath.Dir(value),
			"base": base,
			"ext":  ext,
			"name": strings.TrimSuffix(base, ext),
		}, nil
	})
	exports["default"] = exports
	return exports
}

func osModuleExports() map[string]any {
	exports := map[string]any{}
	exports["EOL"] = "\n"
	exports["constants"] = constantsModuleExports()
	exports["homedir"] = nativeFunction(func(args []any) (any, error) {
		dir, err := os.UserHomeDir()
		if err != nil {
			return "", nil
		}
		return dir, nil
	})
	exports["tmpdir"] = nativeFunction(func(args []any) (any, error) {
		return os.TempDir(), nil
	})
	exports["platform"] = nativeFunction(func(args []any) (any, error) {
		return runtime.GOOS, nil
	})
	exports["default"] = exports
	return exports
}

func fsModuleExports() map[string]any {
	exports := map[string]any{}
	readFileSync := nativeFunction(func(args []any) (any, error) {
		if len(args) == 0 {
			return nil, errors.New("readFileSync path is required")
		}
		bytes, err := os.ReadFile(jsString(args[0]))
		if err != nil {
			return nil, err
		}
		return string(bytes), nil
	})
	exports["readFileSync"] = readFileSync
	exports["readFile"] = nativeFunction(func(args []any) (any, error) {
		if len(args) == 0 {
			return jsUndefined, nodeCallback(args, nodeError("ERR_INVALID_ARG_TYPE", "readFile path is required"))
		}
		bytes, err := os.ReadFile(jsString(args[0]))
		if err != nil {
			return jsUndefined, nodeCallback(args, nodeFsError(err))
		}
		return jsUndefined, nodeCallback(args, nil, string(bytes))
	})
	exports["writeFileSync"] = nativeFunction(func(args []any) (any, error) {
		if len(args) < 2 {
			return nil, errors.New("writeFileSync path and data are required")
		}
		return jsUndefined, os.WriteFile(jsString(args[0]), bytesFromJSValue(args[1]), 0o666)
	})
	exports["writeFile"] = nativeFunction(func(args []any) (any, error) {
		if len(args) < 2 {
			return jsUndefined, nodeCallback(args, nodeError("ERR_INVALID_ARG_TYPE", "writeFile path and data are required"))
		}
		err := os.WriteFile(jsString(args[0]), bytesFromJSValue(args[1]), 0o666)
		return jsUndefined, nodeCallback(args, nodeFsError(err))
	})
	exports["mkdtempSync"] = nativeFunction(func(args []any) (any, error) {
		prefix := ""
		if len(args) > 0 {
			prefix = jsString(args[0])
		}
		parent := filepath.Dir(prefix)
		pattern := filepath.Base(prefix) + "*"
		path, err := os.MkdirTemp(parent, pattern)
		if err != nil {
			return nil, err
		}
		return path, nil
	})
	exports["mkdtemp"] = nativeFunction(func(args []any) (any, error) {
		prefix := ""
		if len(args) > 0 {
			prefix = jsString(args[0])
		}
		path, err := os.MkdirTemp(filepath.Dir(prefix), filepath.Base(prefix)+"*")
		if err != nil {
			return jsUndefined, nodeCallback(args, nodeFsError(err))
		}
		return jsUndefined, nodeCallback(args, nil, path)
	})
	exports["rmSync"] = nativeFunction(func(args []any) (any, error) {
		if len(args) == 0 {
			return nil, errors.New("rmSync path is required")
		}
		if len(args) > 1 {
			if options, ok := args[1].(map[string]any); ok && isTruthy(options["recursive"]) {
				if isTruthy(options["force"]) {
					return jsUndefined, os.RemoveAll(jsString(args[0]))
				}
				return jsUndefined, os.RemoveAll(jsString(args[0]))
			}
		}
		err := os.Remove(jsString(args[0]))
		if err != nil && len(args) > 1 {
			if options, ok := args[1].(map[string]any); ok && isTruthy(options["force"]) {
				return jsUndefined, nil
			}
		}
		return jsUndefined, err
	})
	exports["rm"] = nativeFunction(func(args []any) (any, error) {
		if len(args) == 0 {
			return jsUndefined, nodeCallback(args, nodeError("ERR_INVALID_ARG_TYPE", "rm path is required"))
		}
		err := os.RemoveAll(jsString(args[0]))
		return jsUndefined, nodeCallback(args, nodeFsError(err))
	})
	exports["existsSync"] = nativeFunction(func(args []any) (any, error) {
		if len(args) == 0 {
			return false, nil
		}
		_, err := os.Stat(jsString(args[0]))
		return err == nil, nil
	})
	exports["exists"] = nativeFunction(func(args []any) (any, error) {
		ok := false
		if len(args) > 0 {
			_, err := os.Stat(jsString(args[0]))
			ok = err == nil
		}
		if callback := lastCallback(args); callback != nil {
			_, err := callFunctionWithValues(callback, []any{ok}, Env{}, jsUndefined)
			return jsUndefined, err
		}
		return ok, nil
	})
	exports["access"] = nativeFunction(func(args []any) (any, error) {
		if len(args) == 0 {
			return jsUndefined, nodeCallback(args, nodeError("ERR_INVALID_ARG_TYPE", "access path is required"))
		}
		_, err := os.Stat(jsString(args[0]))
		return jsUndefined, nodeCallback(args, nodeFsError(err))
	})
	exports["accessSync"] = nativeFunction(func(args []any) (any, error) {
		if len(args) == 0 {
			return nil, errors.New("accessSync path is required")
		}
		_, err := os.Stat(jsString(args[0]))
		if err != nil {
			return nil, jsThrow{value: nodeFsError(err)}
		}
		return jsUndefined, nil
	})
	exports["mkdir"] = nativeFunction(func(args []any) (any, error) {
		if len(args) == 0 {
			return jsUndefined, nodeCallback(args, nodeError("ERR_INVALID_ARG_TYPE", "mkdir path is required"))
		}
		err := os.MkdirAll(jsString(args[0]), 0o777)
		return jsUndefined, nodeCallback(args, nodeFsError(err))
	})
	exports["mkdirSync"] = nativeFunction(func(args []any) (any, error) {
		if len(args) == 0 {
			return nil, errors.New("mkdirSync path is required")
		}
		return jsUndefined, os.MkdirAll(jsString(args[0]), 0o777)
	})
	exports["copyFile"] = nativeFunction(func(args []any) (any, error) {
		if len(args) < 2 {
			return jsUndefined, nodeCallback(args, nodeError("ERR_INVALID_ARG_TYPE", "copyFile source and destination are required"))
		}
		err := copyPath(jsString(args[0]), jsString(args[1]))
		return jsUndefined, nodeCallback(args, nodeFsError(err))
	})
	exports["copyFileSync"] = nativeFunction(func(args []any) (any, error) {
		if len(args) < 2 {
			return nil, errors.New("copyFileSync source and destination are required")
		}
		return jsUndefined, copyPath(jsString(args[0]), jsString(args[1]))
	})
	for _, name := range []string{"stat", "lstat"} {
		current := name
		exports[current] = nativeFunction(func(args []any) (any, error) {
			if len(args) == 0 {
				return jsUndefined, nodeCallback(args, nodeError("ERR_INVALID_ARG_TYPE", current+" path is required"))
			}
			info, err := os.Lstat(jsString(args[0]))
			if err != nil {
				return jsUndefined, nodeCallback(args, nodeFsError(err))
			}
			return jsUndefined, nodeCallback(args, nil, statObject(info))
		})
		exports[current+"Sync"] = nativeFunction(func(args []any) (any, error) {
			if len(args) == 0 {
				return nil, errors.New(current + "Sync path is required")
			}
			info, err := os.Lstat(jsString(args[0]))
			if err != nil {
				return nil, jsThrow{value: nodeFsError(err)}
			}
			return statObject(info), nil
		})
	}
	exports["chmod"] = nativeFunction(func(args []any) (any, error) {
		var err error
		if len(args) >= 2 {
			err = os.Chmod(jsString(args[0]), os.FileMode(jsInteger(args[1])))
		}
		return jsUndefined, nodeCallback(args, nodeFsError(err))
	})
	exports["chmodSync"] = nativeFunction(func(args []any) (any, error) {
		if len(args) >= 2 {
			return jsUndefined, os.Chmod(jsString(args[0]), os.FileMode(jsInteger(args[1])))
		}
		return jsUndefined, nil
	})
	exports["unlink"] = nativeFunction(func(args []any) (any, error) {
		if len(args) == 0 {
			return jsUndefined, nodeCallback(args, nodeError("ERR_INVALID_ARG_TYPE", "unlink path is required"))
		}
		return jsUndefined, nodeCallback(args, nodeFsError(os.Remove(jsString(args[0]))))
	})
	exports["unlinkSync"] = nativeFunction(func(args []any) (any, error) {
		if len(args) == 0 {
			return nil, errors.New("unlinkSync path is required")
		}
		return jsUndefined, os.Remove(jsString(args[0]))
	})
	exports["opendir"] = nativeFunction(func(args []any) (any, error) {
		if len(args) == 0 {
			return jsUndefined, nodeCallback(args, nodeError("ERR_INVALID_ARG_TYPE", "opendir path is required"))
		}
		entries, err := os.ReadDir(jsString(args[0]))
		if err != nil {
			return jsUndefined, nodeCallback(args, nodeFsError(err))
		}
		out := []any{}
		for _, entry := range entries {
			out = append(out, direntObject(entry))
		}
		return jsUndefined, nodeCallback(args, nil, &ArrayValue{Items: out})
	})
	exports["readdir"] = nativeFunction(func(args []any) (any, error) {
		if len(args) == 0 {
			return jsUndefined, nodeCallback(args, nodeError("ERR_INVALID_ARG_TYPE", "readdir path is required"))
		}
		entries, err := os.ReadDir(jsString(args[0]))
		if err != nil {
			return jsUndefined, nodeCallback(args, nodeFsError(err))
		}
		out := []any{}
		for _, entry := range entries {
			out = append(out, entry.Name())
		}
		return jsUndefined, nodeCallback(args, nil, &ArrayValue{Items: out})
	})
	realpath := nativeFunction(func(args []any) (any, error) {
		if len(args) == 0 {
			return jsUndefined, nodeCallback(args, nodeError("ERR_INVALID_ARG_TYPE", "realpath path is required"))
		}
		resolved, err := filepath.EvalSymlinks(jsString(args[0]))
		if err != nil {
			resolved = jsString(args[0])
		}
		return jsUndefined, nodeCallback(args, nil, resolved)
	})
	realpath.Props["native"] = realpath
	exports["realpath"] = realpath
	exports["default"] = exports
	return exports
}

func fsPromiseResult(value any, err error) map[string]any {
	if err != nil {
		return promiseRejected(nodeFsError(err))
	}
	return promiseFulfilled(value)
}

func fsPromisesModuleExports() map[string]any {
	exports := map[string]any{}
	exports["readFile"] = nativeFunction(func(args []any) (any, error) {
		if len(args) == 0 {
			return promiseRejected(nodeError("ERR_INVALID_ARG_TYPE", "readFile path is required")), nil
		}
		bytes, err := os.ReadFile(jsString(args[0]))
		if err != nil {
			return fsPromiseResult(jsUndefined, err), nil
		}
		return promiseFulfilled(string(bytes)), nil
	})
	exports["writeFile"] = nativeFunction(func(args []any) (any, error) {
		if len(args) < 2 {
			return promiseRejected(nodeError("ERR_INVALID_ARG_TYPE", "writeFile path and data are required")), nil
		}
		return fsPromiseResult(jsUndefined, os.WriteFile(jsString(args[0]), bytesFromJSValue(args[1]), 0o666)), nil
	})
	exports["mkdir"] = nativeFunction(func(args []any) (any, error) {
		if len(args) == 0 {
			return promiseRejected(nodeError("ERR_INVALID_ARG_TYPE", "mkdir path is required")), nil
		}
		return fsPromiseResult(jsUndefined, os.MkdirAll(jsString(args[0]), 0o777)), nil
	})
	exports["rm"] = nativeFunction(func(args []any) (any, error) {
		if len(args) == 0 {
			return promiseRejected(nodeError("ERR_INVALID_ARG_TYPE", "rm path is required")), nil
		}
		return fsPromiseResult(jsUndefined, os.RemoveAll(jsString(args[0]))), nil
	})
	exports["copyFile"] = nativeFunction(func(args []any) (any, error) {
		if len(args) < 2 {
			return promiseRejected(nodeError("ERR_INVALID_ARG_TYPE", "copyFile source and destination are required")), nil
		}
		return fsPromiseResult(jsUndefined, copyPath(jsString(args[0]), jsString(args[1]))), nil
	})
	exports["unlink"] = nativeFunction(func(args []any) (any, error) {
		if len(args) == 0 {
			return promiseRejected(nodeError("ERR_INVALID_ARG_TYPE", "unlink path is required")), nil
		}
		return fsPromiseResult(jsUndefined, os.Remove(jsString(args[0]))), nil
	})
	exports["readdir"] = nativeFunction(func(args []any) (any, error) {
		if len(args) == 0 {
			return promiseRejected(nodeError("ERR_INVALID_ARG_TYPE", "readdir path is required")), nil
		}
		entries, err := os.ReadDir(jsString(args[0]))
		if err != nil {
			return fsPromiseResult(jsUndefined, err), nil
		}
		out := []any{}
		for _, entry := range entries {
			out = append(out, entry.Name())
		}
		return promiseFulfilled(&ArrayValue{Items: out}), nil
	})
	for _, name := range []string{"stat", "lstat"} {
		current := name
		exports[current] = nativeFunction(func(args []any) (any, error) {
			if len(args) == 0 {
				return promiseRejected(nodeError("ERR_INVALID_ARG_TYPE", current+" path is required")), nil
			}
			info, err := os.Lstat(jsString(args[0]))
			if err != nil {
				return fsPromiseResult(jsUndefined, err), nil
			}
			return promiseFulfilled(statObject(info)), nil
		})
	}
	exports["realpath"] = nativeFunction(func(args []any) (any, error) {
		if len(args) == 0 {
			return promiseRejected(nodeError("ERR_INVALID_ARG_TYPE", "realpath path is required")), nil
		}
		resolved, err := filepath.EvalSymlinks(jsString(args[0]))
		if err != nil {
			resolved = jsString(args[0])
		}
		return promiseFulfilled(resolved), nil
	})
	exports["access"] = nativeFunction(func(args []any) (any, error) {
		if len(args) == 0 {
			return promiseRejected(nodeError("ERR_INVALID_ARG_TYPE", "access path is required")), nil
		}
		_, err := os.Stat(jsString(args[0]))
		return fsPromiseResult(jsUndefined, err), nil
	})
	exports["default"] = exports
	return exports
}

func constantsModuleExports() map[string]any {
	return map[string]any{
		"O_SYMLINK": float64(0),
		"signals": map[string]any{
			"SIGHUP":    float64(1),
			"SIGINT":    float64(2),
			"SIGQUIT":   float64(3),
			"SIGILL":    float64(4),
			"SIGTRAP":   float64(5),
			"SIGABRT":   float64(6),
			"SIGBUS":    float64(10),
			"SIGFPE":    float64(8),
			"SIGKILL":   float64(9),
			"SIGUSR1":   float64(30),
			"SIGSEGV":   float64(11),
			"SIGUSR2":   float64(31),
			"SIGPIPE":   float64(13),
			"SIGALRM":   float64(14),
			"SIGTERM":   float64(15),
			"SIGCHLD":   float64(20),
			"SIGCONT":   float64(19),
			"SIGSTOP":   float64(17),
			"SIGTSTP":   float64(18),
			"SIGTTIN":   float64(21),
			"SIGTTOU":   float64(22),
			"SIGURG":    float64(16),
			"SIGXCPU":   float64(24),
			"SIGXFSZ":   float64(25),
			"SIGVTALRM": float64(26),
			"SIGPROF":   float64(27),
			"SIGWINCH":  float64(28),
			"SIGIO":     float64(23),
			"SIGSYS":    float64(12),
		},
	}
}

func perfHooksModuleExports() map[string]any {
	origin := time.Now()
	performance := map[string]any{}
	performance["now"] = nativeFunction(func(args []any) (any, error) {
		return float64(time.Since(origin).Microseconds()) / 1000, nil
	})
	performance["timeOrigin"] = float64(origin.UnixMilli())
	exports := map[string]any{"performance": performance}
	exports["default"] = exports
	return exports
}

func querystringModuleExports() map[string]any {
	exports := map[string]any{}
	exports["parse"] = nativeFunction(func(args []any) (any, error) {
		if len(args) == 0 {
			return map[string]any{}, nil
		}
		values, err := url.ParseQuery(jsString(args[0]))
		if err != nil {
			return map[string]any{}, nil
		}
		out := map[string]any{}
		for key, entries := range values {
			if len(entries) == 1 {
				out[key] = entries[0]
			} else {
				items := []any{}
				for _, entry := range entries {
					items = append(items, entry)
				}
				out[key] = &ArrayValue{Items: items}
			}
		}
		return out, nil
	})
	exports["stringify"] = nativeFunction(func(args []any) (any, error) {
		if len(args) == 0 {
			return "", nil
		}
		values := url.Values{}
		if object, ok := args[0].(map[string]any); ok {
			for _, key := range objectKeys(object) {
				value := object[key]
				if array, ok := value.(*ArrayValue); ok {
					for _, item := range array.Items {
						values.Add(key, jsString(item))
					}
					continue
				}
				values.Set(key, jsString(value))
			}
		}
		return values.Encode(), nil
	})
	exports["escape"] = nativeFunction(func(args []any) (any, error) {
		if len(args) == 0 {
			return "", nil
		}
		return url.QueryEscape(jsString(args[0])), nil
	})
	exports["unescape"] = nativeFunction(func(args []any) (any, error) {
		if len(args) == 0 {
			return "", nil
		}
		value, err := url.QueryUnescape(jsString(args[0]))
		if err != nil {
			return jsString(args[0]), nil
		}
		return value, nil
	})
	exports["default"] = exports
	return exports
}

func zlibModuleExports() map[string]any {
	exports := map[string]any{}
	exports["gzipSync"] = nativeFunction(func(args []any) (any, error) {
		var out bytes.Buffer
		writer := gzip.NewWriter(&out)
		if len(args) > 0 {
			if _, err := writer.Write(bytesFromJSValue(args[0])); err != nil {
				return nil, err
			}
		}
		if err := writer.Close(); err != nil {
			return nil, err
		}
		return arrayFromBytes(out.Bytes()), nil
	})
	exports["gunzipSync"] = nativeFunction(func(args []any) (any, error) {
		if len(args) == 0 {
			return arrayFromBytes(nil), nil
		}
		reader, err := gzip.NewReader(bytes.NewReader(bytesFromJSValue(args[0])))
		if err != nil {
			return nil, err
		}
		defer reader.Close()
		data, err := io.ReadAll(reader)
		if err != nil {
			return nil, err
		}
		return arrayFromBytes(data), nil
	})
	exports["deflateSync"] = nativeFunction(func(args []any) (any, error) {
		var out bytes.Buffer
		writer := zlib.NewWriter(&out)
		if len(args) > 0 {
			if _, err := writer.Write(bytesFromJSValue(args[0])); err != nil {
				return nil, err
			}
		}
		if err := writer.Close(); err != nil {
			return nil, err
		}
		return arrayFromBytes(out.Bytes()), nil
	})
	exports["inflateSync"] = nativeFunction(func(args []any) (any, error) {
		if len(args) == 0 {
			return arrayFromBytes(nil), nil
		}
		reader, err := zlib.NewReader(bytes.NewReader(bytesFromJSValue(args[0])))
		if err != nil {
			return nil, err
		}
		defer reader.Close()
		data, err := io.ReadAll(reader)
		if err != nil {
			return nil, err
		}
		return arrayFromBytes(data), nil
	})
	exports["constants"] = map[string]any{}
	exports["default"] = exports
	return exports
}

func streamModuleExports() map[string]any {
	stream := map[string]any{}
	streamCtor := nativeFunction(func(args []any) (any, error) {
		return newDuplexStream(nil), nil
	})
	stream["Stream"] = streamCtor
	stream["Readable"] = streamCtor
	stream["Writable"] = streamCtor
	stream["Duplex"] = streamCtor
	stream["PassThrough"] = streamCtor
	stream["getDefaultHighWaterMark"] = nativeFunction(func(args []any) (any, error) {
		return float64(16 * 1024), nil
	})
	stream["default"] = stream
	return stream
}

func eventsModuleExports() map[string]any {
	exports := map[string]any{}
	eventEmitter := nativeFunction(func(args []any) (any, error) {
		return newEventEmitter(), nil
	})
	exports["EventEmitter"] = eventEmitter
	exports["once"] = nativeFunction(func(args []any) (any, error) {
		if len(args) < 2 {
			return promisePending(), nil
		}
		if emitter, ok := args[0].(map[string]any); ok {
			name := jsString(args[1])
			if !hasEventPayload(emitter, name) {
				return promisePending(), nil
			}
			return promiseFulfilled(lastEventPayload(emitter, name)), nil
		}
		return promisePending(), nil
	})
	exports["on"] = nativeFunction(func(args []any) (any, error) {
		if len(args) < 2 {
			return &ArrayValue{Items: []any{}}, nil
		}
		if emitter, ok := args[0].(map[string]any); ok {
			return eventPayloads(emitter, jsString(args[1])), nil
		}
		return &ArrayValue{Items: []any{}}, nil
	})
	exports["addAbortListener"] = nativeFunction(func(args []any) (any, error) {
		return map[string]any{"dispose": nativeFunction(func(args []any) (any, error) {
			return jsUndefined, nil
		})}, nil
	})
	exports["setMaxListeners"] = nativeFunction(func(args []any) (any, error) {
		return jsUndefined, nil
	})
	exports["default"] = exports
	return exports
}

func streamPromisesModuleExports() map[string]any {
	exports := map[string]any{}
	exports["finished"] = nativeFunction(func(args []any) (any, error) {
		return promiseFulfilled(jsUndefined), nil
	})
	exports["pipeline"] = nativeFunction(func(args []any) (any, error) {
		return promiseFulfilled(jsUndefined), nil
	})
	exports["default"] = exports
	return exports
}

func timersPromisesModuleExports() map[string]any {
	exports := map[string]any{}
	setTimeoutFn := nativeFunction(func(args []any) (any, error) {
		delay := 0
		if len(args) > 0 {
			delay = jsInteger(args[0])
		}
		if delay > 0 {
			time.Sleep(time.Duration(delay) * time.Millisecond)
		}
		value := any(jsUndefined)
		if len(args) > 1 {
			value = args[1]
		}
		return promiseFulfilled(value), nil
	})
	exports["setTimeout"] = setTimeoutFn
	exports["setImmediate"] = nativeFunction(func(args []any) (any, error) {
		value := any(jsUndefined)
		if len(args) > 0 {
			value = args[0]
		}
		return promiseFulfilled(value), nil
	})
	exports["scheduler"] = map[string]any{
		"yield": nativeFunction(func(args []any) (any, error) {
			return promiseFulfilled(jsUndefined), nil
		}),
		"wait": setTimeoutFn,
	}
	exports["default"] = exports
	return exports
}

func timersModuleExports() map[string]any {
	exports := map[string]any{}
	setTimeoutFn := nativeFunction(func(args []any) (any, error) {
		delay := 0
		if len(args) > 1 {
			delay = jsInteger(args[1])
		}
		if delay > 0 {
			time.Sleep(time.Duration(delay) * time.Millisecond)
		}
		if len(args) > 0 {
			callArgs := []any{}
			if len(args) > 2 {
				callArgs = args[2:]
			}
			if _, err := callFunctionWithValues(args[0], callArgs, Env{}, jsUndefined); err != nil {
				return nil, err
			}
		}
		return map[string]any{"_idleTimeout": float64(delay)}, nil
	})
	setImmediateFn := nativeFunction(func(args []any) (any, error) {
		if len(args) > 0 {
			callArgs := []any{}
			if len(args) > 1 {
				callArgs = args[1:]
			}
			if _, err := callFunctionWithValues(args[0], callArgs, Env{}, jsUndefined); err != nil {
				return nil, err
			}
		}
		return map[string]any{}, nil
	})
	clearFn := nativeFunction(func(args []any) (any, error) { return jsUndefined, nil })
	exports["setTimeout"] = setTimeoutFn
	exports["setInterval"] = setTimeoutFn
	exports["setImmediate"] = setImmediateFn
	exports["clearTimeout"] = clearFn
	exports["clearInterval"] = clearFn
	exports["clearImmediate"] = clearFn
	exports["default"] = exports
	return exports
}

func asyncHooksModuleExports() map[string]any {
	exports := map[string]any{}
	exports["AsyncLocalStorage"] = nativeFunction(func(args []any) (any, error) {
		store := any(jsUndefined)
		instance := map[string]any{}
		instance["run"] = nativeFunction(func(args []any) (any, error) {
			if len(args) < 2 {
				return jsUndefined, nil
			}
			previous := store
			store = args[0]
			result, err := callFunctionWithValues(args[1], args[2:], Env{}, jsUndefined)
			store = previous
			return result, err
		})
		instance["getStore"] = nativeFunction(func(args []any) (any, error) {
			return store, nil
		})
		instance["enterWith"] = nativeFunction(func(args []any) (any, error) {
			if len(args) > 0 {
				store = args[0]
			}
			return jsUndefined, nil
		})
		instance["disable"] = nativeFunction(func(args []any) (any, error) {
			store = jsUndefined
			return jsUndefined, nil
		})
		return instance, nil
	})
	exports["AsyncResource"] = nativeFunction(func(args []any) (any, error) {
		instance := map[string]any{}
		instance["runInAsyncScope"] = nativeFunction(func(args []any) (any, error) {
			if len(args) == 0 {
				return jsUndefined, nil
			}
			thisValue := any(jsUndefined)
			callArgs := []any{}
			if len(args) > 1 {
				thisValue = args[1]
				callArgs = args[2:]
			}
			return callFunctionWithValues(args[0], callArgs, Env{}, thisValue)
		})
		instance["emitDestroy"] = nativeFunction(func(args []any) (any, error) { return jsUndefined, nil })
		instance["asyncId"] = nativeFunction(func(args []any) (any, error) { return float64(1), nil })
		instance["triggerAsyncId"] = nativeFunction(func(args []any) (any, error) { return float64(0), nil })
		return instance, nil
	})
	exports["executionAsyncId"] = nativeFunction(func(args []any) (any, error) { return float64(1), nil })
	exports["triggerAsyncId"] = nativeFunction(func(args []any) (any, error) { return float64(0), nil })
	exports["createHook"] = nativeFunction(func(args []any) (any, error) {
		return map[string]any{
			"enable":  nativeFunction(func(args []any) (any, error) { return jsUndefined, nil }),
			"disable": nativeFunction(func(args []any) (any, error) { return jsUndefined, nil }),
		}, nil
	})
	exports["default"] = exports
	return exports
}

func stringDecoderModuleExports() map[string]any {
	exports := map[string]any{}
	exports["StringDecoder"] = nativeFunction(func(args []any) (any, error) {
		decoder := map[string]any{}
		decoder["write"] = nativeFunction(func(args []any) (any, error) {
			if len(args) == 0 {
				return "", nil
			}
			return jsString(args[0]), nil
		})
		decoder["end"] = nativeFunction(func(args []any) (any, error) {
			if len(args) == 0 {
				return "", nil
			}
			return jsString(args[0]), nil
		})
		return decoder, nil
	})
	exports["default"] = exports
	return exports
}

func ttyModuleExports() map[string]any {
	exports := map[string]any{}
	exports["isatty"] = nativeFunction(func(args []any) (any, error) {
		return false, nil
	})
	exports["ReadStream"] = nativeFunction(func(args []any) (any, error) {
		return newReadableStream(nil), nil
	})
	exports["WriteStream"] = nativeFunction(func(args []any) (any, error) {
		return newWritableStream(), nil
	})
	exports["default"] = exports
	return exports
}

func urlModuleExports() map[string]any {
	exports := map[string]any{}
	exports["fileURLToPath"] = nativeFunction(func(args []any) (any, error) {
		if len(args) == 0 {
			return "", nil
		}
		value := jsString(args[0])
		parsed, err := url.Parse(value)
		if err == nil && parsed.Scheme == "file" {
			return parsed.Path, nil
		}
		if object, ok := args[0].(map[string]any); ok {
			if pathname, ok := object["pathname"]; ok {
				return jsString(pathname), nil
			}
		}
		return value, nil
	})
	exports["pathToFileURL"] = nativeFunction(func(args []any) (any, error) {
		if len(args) == 0 {
			return urlObject("file://"), nil
		}
		path := filepath.ToSlash(jsString(args[0]))
		if !strings.HasPrefix(path, "/") {
			path = "/" + path
		}
		return urlObject("file://" + path), nil
	})
	exports["URL"] = urlGlobal()
	exports["default"] = exports
	return exports
}

func v8ModuleExports() map[string]any {
	exports := map[string]any{}
	exports["serialize"] = nativeFunction(func(args []any) (any, error) {
		if len(args) == 0 {
			return arrayFromBytes(nil), nil
		}
		bytes, err := json.Marshal(jsonCompatible(args[0], map[uintptr]bool{}))
		if err != nil {
			return nil, err
		}
		return arrayFromBytes(bytes), nil
	})
	exports["deserialize"] = nativeFunction(func(args []any) (any, error) {
		return jsUndefined, nil
	})
	exports["default"] = exports
	return exports
}

func childProcessModuleExports() map[string]any {
	exports := map[string]any{}
	childProcessCtor := nativeFunction(func(args []any) (any, error) {
		return newEventEmitter(), nil
	})
	exports["ChildProcess"] = childProcessCtor
	exports["spawn"] = nativeFunction(func(args []any) (any, error) {
		return spawnChildProcess(args)
	})
	exports["spawnSync"] = nativeFunction(func(args []any) (any, error) {
		result, err := runCommand(args)
		if err != nil {
			return map[string]any{"error": nodeError("ENOENT", err.Error())}, nil
		}
		stdout := commandOutputValue(result.Stdout, args)
		stderr := commandOutputValue(result.Stderr, args)
		return map[string]any{
			"pid":    float64(os.Getpid()),
			"status": float64(result.ExitCode),
			"signal": jsNull,
			"stdout": stdout,
			"stderr": stderr,
			"output": &ArrayValue{Items: []any{jsNull, stdout, stderr}},
			"error":  jsUndefined,
		}, nil
	})
	exports["default"] = exports
	return exports
}

func commandOutputValue(value string, args []any) any {
	if len(args) > 2 {
		if options, ok := args[2].(map[string]any); ok {
			encoding := jsString(options["encoding"])
			if encoding == "utf8" || encoding == "utf-8" {
				return value
			}
		}
	}
	return arrayFromBytes([]byte(value))
}

type commandResult struct {
	Stdout   string
	Stderr   string
	ExitCode int
}

func spawnChildProcess(args []any) (any, error) {
	result, err := runCommand(args)
	subprocess := newEventEmitter()
	subprocess["pid"] = float64(os.Getpid())
	subprocess["stdin"] = newWritableStream()
	subprocess["stdout"] = newReadableStream([]any{result.Stdout})
	subprocess["stderr"] = newReadableStream([]any{result.Stderr})
	subprocess["stdio"] = &ArrayValue{Items: []any{subprocess["stdin"], subprocess["stdout"], subprocess["stderr"]}}
	subprocess["killed"] = false
	subprocess["exitCode"] = float64(result.ExitCode)
	subprocess["signalCode"] = jsNull
	subprocess["kill"] = nativeFunction(func(args []any) (any, error) {
		subprocess["killed"] = true
		return true, nil
	})
	subprocess["ref"] = nativeFunction(func(args []any) (any, error) {
		return subprocess, nil
	})
	subprocess["unref"] = nativeFunction(func(args []any) (any, error) {
		return subprocess, nil
	})
	emitEvent(subprocess, "spawn")
	if err != nil {
		emitEvent(subprocess, "error", nodeError("ENOENT", err.Error()))
	}
	emitEvent(subprocess, "exit", float64(result.ExitCode), jsNull)
	emitEvent(subprocess, "close", float64(result.ExitCode), jsNull)
	return subprocess, nil
}

func runCommand(args []any) (commandResult, error) {
	if len(args) == 0 {
		return commandResult{ExitCode: 1}, errors.New("spawn file is required")
	}
	file := jsString(args[0])
	commandArgs := []string{}
	if len(args) > 1 {
		for _, value := range iterableValues(args[1]) {
			commandArgs = append(commandArgs, jsString(value))
		}
	}
	env := map[string]string{}
	for _, entry := range os.Environ() {
		parts := strings.SplitN(entry, "=", 2)
		value := ""
		if len(parts) == 2 {
			value = parts[1]
		}
		env[parts[0]] = value
	}
	if len(args) > 2 {
		if options, ok := args[2].(map[string]any); ok {
			if envValue, ok := options["env"].(map[string]any); ok {
				for key, value := range envValue {
					if strings.HasPrefix(key, "__") || isNullish(value) {
						continue
					}
					env[key] = jsString(value)
				}
			}
		}
	}
	if len(commandArgs) >= 2 && commandArgs[0] == "-e" && sameExecutablePath(file, nodeExecutablePath()) {
		processArgv := append([]string{file}, commandArgs[2:]...)
		result, err := runNodeEvalSnippet(commandArgs[1], processArgv, env)
		return result, err
	}
	return commandResult{ExitCode: 1}, fmt.Errorf("unsupported child_process command %s", file)
}

func sameExecutablePath(left string, right string) bool {
	if left == right {
		return true
	}
	if leftResolved, err := filepath.EvalSymlinks(left); err == nil {
		if rightResolved, err := filepath.EvalSymlinks(right); err == nil && leftResolved == rightResolved {
			return true
		}
	}
	leftAbs, leftErr := filepath.Abs(left)
	rightAbs, rightErr := filepath.Abs(right)
	if leftErr == nil && rightErr == nil && leftAbs == rightAbs {
		return true
	}
	return filepath.Base(left) == "node" && filepath.Base(right) == "node"
}

func runNodeEvalSnippet(source string, argv []string, env map[string]string) (commandResult, error) {
	result := commandResult{ExitCode: 0}
	statements := splitSimpleStatements(source)
	for _, statement := range statements {
		statement = strings.TrimSpace(statement)
		switch {
		case strings.HasPrefix(statement, "console.log(") && strings.HasSuffix(statement, ")"):
			expr := strings.TrimSuffix(strings.TrimPrefix(statement, "console.log("), ")")
			result.Stdout += evalSimpleNodeExpression(expr, argv, env) + "\n"
		case strings.HasPrefix(statement, "console.error(") && strings.HasSuffix(statement, ")"):
			expr := strings.TrimSuffix(strings.TrimPrefix(statement, "console.error("), ")")
			result.Stderr += evalSimpleNodeExpression(expr, argv, env) + "\n"
		case strings.HasPrefix(statement, "process.exit(") && strings.HasSuffix(statement, ")"):
			expr := strings.TrimSuffix(strings.TrimPrefix(statement, "process.exit("), ")")
			result.ExitCode = jsInteger(evalSimpleNodeExpression(expr, argv, env))
		case statement == "":
		default:
			return commandResult{ExitCode: 1}, fmt.Errorf("unsupported node -e statement %q", statement)
		}
	}
	result.Stdout = strings.TrimSuffix(result.Stdout, "\n")
	result.Stderr = strings.TrimSuffix(result.Stderr, "\n")
	return result, nil
}

func splitSimpleStatements(source string) []string {
	out := []string{}
	var current strings.Builder
	quote := rune(0)
	escaped := false
	for _, ch := range source {
		if escaped {
			current.WriteRune(ch)
			escaped = false
			continue
		}
		if ch == '\\' && quote != 0 {
			current.WriteRune(ch)
			escaped = true
			continue
		}
		if quote != 0 {
			current.WriteRune(ch)
			if ch == quote {
				quote = 0
			}
			continue
		}
		if ch == '\'' || ch == '"' || ch == '`' {
			quote = ch
			current.WriteRune(ch)
			continue
		}
		if ch == ';' {
			out = append(out, current.String())
			current.Reset()
			continue
		}
		current.WriteRune(ch)
	}
	out = append(out, current.String())
	return out
}

func evalSimpleNodeExpression(expr string, argv []string, env map[string]string) string {
	parts := splitSimpleConcat(expr)
	var out strings.Builder
	for _, part := range parts {
		out.WriteString(evalSimpleNodeExpressionPart(part, argv, env))
	}
	return out.String()
}

func evalSimpleNodeExpressionPart(part string, argv []string, env map[string]string) string {
	part = stripBalancedParens(strings.TrimSpace(part))
	if orIndex := findTopLevelLogicalOr(part); orIndex >= 0 {
		left := evalSimpleNodeExpressionPart(part[:orIndex], argv, env)
		if left != "" {
			return left
		}
		return evalSimpleNodeExpressionPart(part[orIndex+2:], argv, env)
	}
	switch {
	case len(part) >= 2 && ((part[0] == '\'' && part[len(part)-1] == '\'') || (part[0] == '"' && part[len(part)-1] == '"')):
		return unquoteSimpleJSString(part)
	case strings.HasPrefix(part, "process.env."):
		return env[strings.TrimPrefix(part, "process.env.")]
	case strings.HasPrefix(part, "process.argv[") && strings.HasSuffix(part, "]"):
		indexText := strings.TrimSuffix(strings.TrimPrefix(part, "process.argv["), "]")
		index, _ := strconv.Atoi(indexText)
		if index >= 0 && index < len(argv) {
			return argv[index]
		}
		return ""
	default:
		return part
	}
}

func stripBalancedParens(value string) string {
	for strings.HasPrefix(value, "(") && strings.HasSuffix(value, ")") {
		depth := 0
		quote := rune(0)
		escaped := false
		balanced := true
		for index, ch := range value {
			if escaped {
				escaped = false
				continue
			}
			if quote != 0 {
				if ch == '\\' {
					escaped = true
				} else if ch == quote {
					quote = 0
				}
				continue
			}
			if ch == '\'' || ch == '"' || ch == '`' {
				quote = ch
				continue
			}
			switch ch {
			case '(':
				depth++
			case ')':
				depth--
				if depth == 0 && index != len(value)-1 {
					balanced = false
				}
			}
			if depth < 0 {
				balanced = false
				break
			}
		}
		if !balanced || depth != 0 {
			return value
		}
		value = strings.TrimSpace(value[1 : len(value)-1])
	}
	return value
}

func findTopLevelLogicalOr(value string) int {
	depth := 0
	quote := rune(0)
	escaped := false
	for index, ch := range value {
		if escaped {
			escaped = false
			continue
		}
		if quote != 0 {
			if ch == '\\' {
				escaped = true
			} else if ch == quote {
				quote = 0
			}
			continue
		}
		if ch == '\'' || ch == '"' || ch == '`' {
			quote = ch
			continue
		}
		switch ch {
		case '(':
			depth++
		case ')':
			if depth > 0 {
				depth--
			}
		case '|':
			if depth == 0 && index+1 < len(value) && value[index+1] == '|' {
				return index
			}
		}
	}
	return -1
}

func splitSimpleConcat(expr string) []string {
	out := []string{}
	var current strings.Builder
	quote := rune(0)
	escaped := false
	for _, ch := range expr {
		if escaped {
			current.WriteRune(ch)
			escaped = false
			continue
		}
		if ch == '\\' && quote != 0 {
			current.WriteRune(ch)
			escaped = true
			continue
		}
		if quote != 0 {
			current.WriteRune(ch)
			if ch == quote {
				quote = 0
			}
			continue
		}
		if ch == '\'' || ch == '"' || ch == '`' {
			quote = ch
			current.WriteRune(ch)
			continue
		}
		if ch == '+' {
			out = append(out, current.String())
			current.Reset()
			continue
		}
		current.WriteRune(ch)
	}
	out = append(out, current.String())
	return out
}

func unquoteSimpleJSString(value string) string {
	if len(value) < 2 {
		return value
	}
	unquoted := value[1 : len(value)-1]
	unquoted = strings.ReplaceAll(unquoted, "\\'", "'")
	unquoted = strings.ReplaceAll(unquoted, "\\\"", "\"")
	unquoted = strings.ReplaceAll(unquoted, "\\n", "\n")
	unquoted = strings.ReplaceAll(unquoted, "\\t", "\t")
	unquoted = strings.ReplaceAll(unquoted, "\\\\", "\\")
	return unquoted
}

func newReadableStream(chunks []any) map[string]any {
	stream := newEventEmitter()
	stream["readable"] = true
	stream["writable"] = false
	stream["readableObjectMode"] = false
	stream["writableObjectMode"] = false
	stream["destroyed"] = false
	stream["readableFlowing"] = jsNull
	stream["__chunks"] = &ArrayValue{Items: append([]any{}, chunks...)}
	stream["pipe"] = nativeMethod(func(thisValue any, args []any) (any, error) {
		if len(args) > 0 {
			return args[0], nil
		}
		return thisValue, nil
	})
	stream["read"] = nativeMethod(func(thisValue any, args []any) (any, error) {
		chunks := iterableValues(stream["__chunks"])
		if len(chunks) == 0 {
			return jsNull, nil
		}
		stream["__chunks"] = &ArrayValue{Items: chunks[1:]}
		return chunks[0], nil
	})
	stream["Symbol.asyncIterator"] = nativeMethod(func(thisValue any, args []any) (any, error) {
		return &IteratorValue{Values: iterableValues(stream["__chunks"])}, nil
	})
	stream["resume"] = nativeMethod(func(thisValue any, args []any) (any, error) {
		stream["readableFlowing"] = true
		return thisValue, nil
	})
	stream["destroy"] = nativeMethod(func(thisValue any, args []any) (any, error) {
		stream["destroyed"] = true
		return thisValue, nil
	})
	for _, chunk := range chunks {
		if chunk != "" {
			emitEvent(stream, "data", chunk)
		}
	}
	emitEvent(stream, "end")
	emitEvent(stream, "close")
	return stream
}

func newWritableStream() map[string]any {
	stream := newEventEmitter()
	stream["readable"] = false
	stream["writable"] = true
	stream["readableObjectMode"] = false
	stream["writableObjectMode"] = false
	stream["destroyed"] = false
	stream["write"] = nativeMethod(func(thisValue any, args []any) (any, error) {
		if len(args) > 0 {
			emitEvent(stream, "data", args[0])
		}
		return true, nil
	})
	stream["end"] = nativeMethod(func(thisValue any, args []any) (any, error) {
		emitEvent(stream, "finish")
		emitEvent(stream, "close")
		return thisValue, nil
	})
	stream["destroy"] = nativeMethod(func(thisValue any, args []any) (any, error) {
		stream["destroyed"] = true
		return thisValue, nil
	})
	stream["pipe"] = nativeMethod(func(thisValue any, args []any) (any, error) {
		if len(args) > 0 {
			return args[0], nil
		}
		return thisValue, nil
	})
	return stream
}

func newDuplexStream(chunks []any) map[string]any {
	stream := newReadableStream(chunks)
	stream["writable"] = true
	stream["write"] = nativeMethod(func(thisValue any, args []any) (any, error) {
		if len(args) > 0 {
			emitEvent(stream, "data", args[0])
		}
		return true, nil
	})
	stream["end"] = nativeMethod(func(thisValue any, args []any) (any, error) {
		emitEvent(stream, "finish")
		return thisValue, nil
	})
	return stream
}

func newEventEmitter() map[string]any {
	emitter := map[string]any{
		"__events": map[string][]any{},
	}
	emitter["emit"] = nativeMethod(func(thisValue any, args []any) (any, error) {
		if len(args) == 0 {
			return false, nil
		}
		emitEvent(emitter, jsString(args[0]), args[1:]...)
		return true, nil
	})
	emitter["on"] = nativeMethod(func(thisValue any, args []any) (any, error) {
		return emitter, nil
	})
	emitter["once"] = nativeMethod(func(thisValue any, args []any) (any, error) {
		return emitter, nil
	})
	emitter["addListener"] = emitter["on"]
	emitter["removeListener"] = nativeMethod(func(thisValue any, args []any) (any, error) {
		return emitter, nil
	})
	emitter["removeAllListeners"] = nativeMethod(func(thisValue any, args []any) (any, error) {
		return emitter, nil
	})
	emitter["listenerCount"] = nativeMethod(func(thisValue any, args []any) (any, error) {
		return float64(0), nil
	})
	emitter["setMaxListeners"] = nativeMethod(func(thisValue any, args []any) (any, error) {
		return emitter, nil
	})
	return emitter
}

func emitEvent(emitter map[string]any, name string, args ...any) {
	events, ok := emitter["__events"].(map[string][]any)
	if !ok {
		events = map[string][]any{}
		emitter["__events"] = events
	}
	events[name] = append(events[name], &ArrayValue{Items: append([]any{}, args...)})
}

func lastEventPayload(emitter map[string]any, name string) any {
	events, ok := emitter["__events"].(map[string][]any)
	if !ok || len(events[name]) == 0 {
		return &ArrayValue{Items: []any{}}
	}
	return events[name][len(events[name])-1]
}

func hasEventPayload(emitter map[string]any, name string) bool {
	events, ok := emitter["__events"].(map[string][]any)
	return ok && len(events[name]) > 0
}

func eventPayloads(emitter map[string]any, name string) any {
	events, ok := emitter["__events"].(map[string][]any)
	if !ok {
		return &ArrayValue{Items: []any{}}
	}
	return &ArrayValue{Items: append([]any{}, events[name]...)}
}

func abortControllerGlobal() NativeFunctionValue {
	return nativeFunction(func(args []any) (any, error) {
		signal := map[string]any{"aborted": false, "__tag": "[object AbortSignal]"}
		controller := map[string]any{"signal": signal, "__tag": "[object AbortController]"}
		controller["abort"] = nativeFunction(func(args []any) (any, error) {
			signal["aborted"] = true
			return jsUndefined, nil
		})
		return controller, nil
	})
}

func textEncoderGlobal() NativeFunctionValue {
	return nativeFunction(func(args []any) (any, error) {
		return map[string]any{
			"encode": nativeFunction(func(args []any) (any, error) {
				if len(args) == 0 {
					return arrayFromBytes(nil), nil
				}
				return arrayFromBytes([]byte(jsString(args[0]))), nil
			}),
			"encodeInto": nativeFunction(func(args []any) (any, error) {
				if len(args) < 2 {
					return map[string]any{"read": float64(0), "written": float64(0)}, nil
				}
				bytes := []byte(jsString(args[0]))
				target, ok := args[1].(*ArrayValue)
				if !ok {
					return map[string]any{"read": float64(0), "written": float64(0)}, nil
				}
				written := minInt(len(bytes), len(target.Items))
				for index := 0; index < written; index++ {
					target.Items[index] = float64(bytes[index])
				}
				return map[string]any{"read": float64(written), "written": float64(written)}, nil
			}),
			"encoding": "utf-8",
		}, nil
	})
}

func textDecoderGlobal() NativeFunctionValue {
	return nativeFunction(func(args []any) (any, error) {
		return map[string]any{
			"decode": nativeFunction(func(args []any) (any, error) {
				if len(args) == 0 {
					return "", nil
				}
				return string(bytesFromJSValue(args[0])), nil
			}),
			"encoding": "utf-8",
		}, nil
	})
}

func urlGlobal() NativeFunctionValue {
	return nativeFunction(func(args []any) (any, error) {
		if len(args) == 0 {
			return urlObject(""), nil
		}
		return urlObject(jsString(args[0])), nil
	})
}

func urlSearchParamsGlobal() NativeFunctionValue {
	return nativeFunction(func(args []any) (any, error) {
		values := url.Values{}
		if len(args) > 0 && !isNullish(args[0]) {
			switch typed := args[0].(type) {
			case string:
				parsed, err := url.ParseQuery(typed)
				if err == nil {
					values = parsed
				}
			case map[string]any:
				for _, key := range objectKeys(typed) {
					value := typed[key]
					if array, ok := value.(*ArrayValue); ok {
						for _, item := range array.Items {
							values.Add(key, jsString(item))
						}
					} else {
						values.Set(key, jsString(value))
					}
				}
			case *ArrayValue:
				for _, pair := range typed.Items {
					if tuple, ok := pair.(*ArrayValue); ok && len(tuple.Items) >= 2 {
						values.Add(jsString(tuple.Items[0]), jsString(tuple.Items[1]))
					}
				}
			}
		}
		object := map[string]any{}
		object["append"] = nativeFunction(func(args []any) (any, error) {
			if len(args) >= 2 {
				values.Add(jsString(args[0]), jsString(args[1]))
			}
			return jsUndefined, nil
		})
		object["set"] = nativeFunction(func(args []any) (any, error) {
			if len(args) >= 2 {
				values.Set(jsString(args[0]), jsString(args[1]))
			}
			return jsUndefined, nil
		})
		object["get"] = nativeFunction(func(args []any) (any, error) {
			if len(args) == 0 {
				return jsNull, nil
			}
			value := values.Get(jsString(args[0]))
			if value == "" {
				if _, ok := values[jsString(args[0])]; !ok {
					return jsNull, nil
				}
			}
			return value, nil
		})
		object["toString"] = nativeFunction(func(args []any) (any, error) {
			return values.Encode(), nil
		})
		return object, nil
	})
}

func urlObject(raw string) map[string]any {
	parsed, err := url.Parse(raw)
	pathname := raw
	protocol := ""
	href := raw
	if err == nil {
		pathname = parsed.Path
		protocol = parsed.Scheme + ":"
		href = parsed.String()
	}
	return map[string]any{
		"__tag":    "[object URL]",
		"href":     href,
		"pathname": pathname,
		"protocol": protocol,
	}
}

func lastCallback(args []any) any {
	if len(args) == 0 {
		return nil
	}
	last := args[len(args)-1]
	switch last.(type) {
	case FunctionValue, BoundFunctionValue, NativeFunctionValue:
		return last
	default:
		return nil
	}
}

func nodeCallback(args []any, err any, results ...any) error {
	callback := lastCallback(args)
	if callback == nil {
		if !isNullish(err) {
			return jsThrow{value: err}
		}
		return nil
	}
	callArgs := []any{jsNull}
	if !isNullish(err) {
		callArgs[0] = err
	}
	callArgs = append(callArgs, results...)
	_, callErr := callFunctionWithValues(callback, callArgs, Env{}, jsUndefined)
	return callErr
}

func nodeError(code string, message string) map[string]any {
	return map[string]any{
		"name":    "Error",
		"message": message,
		"code":    code,
	}
}

func nodeFsError(err error) any {
	if err == nil {
		return jsNull
	}
	code := "EIO"
	if os.IsNotExist(err) {
		code = "ENOENT"
	}
	if os.IsExist(err) {
		code = "EEXIST"
	}
	if os.IsPermission(err) {
		code = "EACCES"
	}
	return nodeError(code, err.Error())
}

func statObject(info os.FileInfo) map[string]any {
	mode := float64(info.Mode().Perm())
	object := map[string]any{
		"dev":   float64(0),
		"ino":   float64(0),
		"mode":  mode,
		"size":  float64(info.Size()),
		"mtime": float64(info.ModTime().UnixMilli()),
		"atime": float64(info.ModTime().UnixMilli()),
	}
	object["isDirectory"] = nativeFunction(func(args []any) (any, error) { return info.IsDir(), nil })
	object["isFile"] = nativeFunction(func(args []any) (any, error) { return info.Mode().IsRegular(), nil })
	object["isCharacterDevice"] = nativeFunction(func(args []any) (any, error) { return false, nil })
	object["isBlockDevice"] = nativeFunction(func(args []any) (any, error) { return false, nil })
	object["isSymbolicLink"] = nativeFunction(func(args []any) (any, error) { return info.Mode()&os.ModeSymlink != 0, nil })
	object["isSocket"] = nativeFunction(func(args []any) (any, error) { return info.Mode()&os.ModeSocket != 0, nil })
	object["isFIFO"] = nativeFunction(func(args []any) (any, error) { return info.Mode()&os.ModeNamedPipe != 0, nil })
	return object
}

func direntObject(entry os.DirEntry) map[string]any {
	object := map[string]any{"name": entry.Name()}
	object["isDirectory"] = nativeFunction(func(args []any) (any, error) { return entry.IsDir(), nil })
	object["isFile"] = nativeFunction(func(args []any) (any, error) {
		info, err := entry.Info()
		return err == nil && info.Mode().IsRegular(), nil
	})
	return object
}

func copyPath(src string, dest string) error {
	info, err := os.Lstat(src)
	if err != nil {
		return err
	}
	if info.IsDir() {
		if err := os.MkdirAll(dest, info.Mode().Perm()); err != nil {
			return err
		}
		return filepath.WalkDir(src, func(path string, entry os.DirEntry, walkErr error) error {
			if walkErr != nil {
				return walkErr
			}
			rel, err := filepath.Rel(src, path)
			if err != nil {
				return err
			}
			target := filepath.Join(dest, rel)
			if entry.IsDir() {
				return os.MkdirAll(target, 0o777)
			}
			return copyFileContents(path, target)
		})
	}
	return copyFileContents(src, dest)
}

func copyFileContents(src string, dest string) error {
	if err := os.MkdirAll(filepath.Dir(dest), 0o777); err != nil {
		return err
	}
	bytes, err := os.ReadFile(src)
	if err != nil {
		return err
	}
	return os.WriteFile(dest, bytes, 0o666)
}

func moduleModuleExports() map[string]any {
	exports := map[string]any{}
	exports["createRequire"] = nativeFunction(func(args []any) (any, error) {
		return NativeFunctionValue{Call: func(args []any) (any, error) {
			if len(args) == 0 {
				return nil, errors.New("require specifier is required")
			}
			if exports, ok := builtinModuleExports(jsString(args[0])); ok {
				return exports, nil
			}
			return nil, fmt.Errorf("module import %s is not resolved", jsString(args[0]))
		}}, nil
	})
	exports["default"] = exports
	return exports
}

func netModuleExports() map[string]any {
	exports := map[string]any{}
	isIP := nativeFunction(func(args []any) (any, error) {
		if len(args) == 0 {
			return float64(0), nil
		}
		ip := stdnet.ParseIP(jsString(args[0]))
		if ip == nil {
			return float64(0), nil
		}
		if ip.To4() != nil {
			return float64(4), nil
		}
		return float64(6), nil
	})
	exports["isIP"] = isIP
	exports["isIPv4"] = nativeFunction(func(args []any) (any, error) {
		value, err := isIP.Call(args)
		if err != nil {
			return nil, err
		}
		return toNumber(value) == 4, nil
	})
	exports["isIPv6"] = nativeFunction(func(args []any) (any, error) {
		value, err := isIP.Call(args)
		if err != nil {
			return nil, err
		}
		return toNumber(value) == 6, nil
	})
	socketCtor := nativeFunction(func(args []any) (any, error) {
		socket := newEventEmitter()
		socket["setTimeout"] = nativeFunction(func(args []any) (any, error) { return socket, nil })
		socket["setNoDelay"] = nativeFunction(func(args []any) (any, error) { return socket, nil })
		socket["setKeepAlive"] = nativeFunction(func(args []any) (any, error) { return socket, nil })
		socket["destroy"] = nativeFunction(func(args []any) (any, error) {
			emitEvent(socket, "close")
			return socket, nil
		})
		return socket, nil
	})
	exports["Socket"] = socketCtor
	exports["Stream"] = socketCtor
	exports["connect"] = nativeFunction(func(args []any) (any, error) {
		return callFunctionWithValues(socketCtor, []any{}, Env{}, jsUndefined)
	})
	exports["createConnection"] = exports["connect"]
	exports["createServer"] = nativeFunction(func(args []any) (any, error) {
		server := newEventEmitter()
		server["listen"] = nativeFunction(func(args []any) (any, error) {
			emitEvent(server, "listening")
			return server, nil
		})
		server["close"] = nativeFunction(func(args []any) (any, error) {
			if callback := lastCallback(args); callback != nil {
				if _, err := callFunctionWithValues(callback, []any{}, Env{}, jsUndefined); err != nil {
					return nil, err
				}
			}
			emitEvent(server, "close")
			return server, nil
		})
		server["address"] = nativeFunction(func(args []any) (any, error) {
			return map[string]any{"address": "127.0.0.1", "family": "IPv4", "port": float64(0)}, nil
		})
		return server, nil
	})
	exports["default"] = exports
	return exports
}

func processObject(entry string) map[string]any {
	env := map[string]any{}
	for _, entry := range os.Environ() {
		parts := strings.SplitN(entry, "=", 2)
		value := ""
		if len(parts) == 2 {
			value = parts[1]
		}
		env[parts[0]] = value
	}
	argv := []string{nodeExecutablePath(), entry}
	if len(os.Args) > 1 {
		argv = append(argv, os.Args[1:]...)
	}
	process := map[string]any{
		"env":      env,
		"argv":     &ArrayValue{Items: stringsToAny(argv)},
		"execArgv": &ArrayValue{Items: []any{}},
		"execPath": nodeExecutablePath(),
		"version":  "v20.0.0",
		"versions": map[string]any{"node": "20.0.0"},
		"platform": runtime.GOOS,
		"arch":     runtime.GOARCH,
		"cwd": nativeFunction(func(args []any) (any, error) {
			cwd, err := os.Getwd()
			if err != nil {
				return "", nil
			}
			return cwd, nil
		}),
		"nextTick": nativeFunction(func(args []any) (any, error) {
			if len(args) > 0 {
				_, err := callFunctionWithValues(args[0], args[1:], Env{}, jsUndefined)
				return jsUndefined, err
			}
			return jsUndefined, nil
		}),
		"emitWarning": nativeFunction(func(args []any) (any, error) {
			return jsUndefined, nil
		}),
	}
	process["stdin"] = newWritableStream()
	process["stdout"] = newWritableStream()
	process["stderr"] = newWritableStream()
	process["exit"] = nativeFunction(func(args []any) (any, error) {
		code := 0
		if len(args) > 0 {
			code = jsInteger(args[0])
		}
		os.Exit(code)
		return jsUndefined, nil
	})
	hrtime := nativeFunction(func(args []any) (any, error) {
		now := time.Now().UnixNano()
		seconds := float64(now / int64(time.Second))
		nanos := float64(now % int64(time.Second))
		return &ArrayValue{Items: []any{seconds, nanos}}, nil
	})
	hrtime.Props = map[string]any{
		"bigint": nativeFunction(func(args []any) (any, error) {
			return float64(time.Now().UnixNano()), nil
		}),
	}
	process["hrtime"] = hrtime
	return process
}

func nodeExecutablePath() string {
	if len(os.Args) > 0 {
		return resolveExecutablePath(os.Args[0])
	}
	return "tsgodown-generated"
}

func resolveExecutablePath(path string) string {
	resolved, err := filepath.EvalSymlinks(path)
	if err == nil && resolved != "" {
		return resolved
	}
	absolute, err := filepath.Abs(path)
	if err == nil && absolute != "" {
		return absolute
	}
	return path
}

func diagnosticsChannelModuleExports() map[string]any {
	exports := map[string]any{}
	channel := nativeFunction(func(args []any) (any, error) {
		return map[string]any{"hasSubscribers": false}, nil
	})
	exports["channel"] = channel
	exports["tracingChannel"] = channel
	exports["default"] = exports
	return exports
}

func dynamicImportThenable() map[string]any {
	thenable := map[string]any{}
	thenable["then"] = nativeFunction(func(args []any) (any, error) {
		return map[string]any{
			"catch": nativeFunction(func(args []any) (any, error) {
				return jsUndefined, nil
			}),
		}, nil
	})
	thenable["catch"] = nativeFunction(func(args []any) (any, error) {
		return jsUndefined, nil
	})
	return thenable
}

func bindImport(env Env, importDecl Import, importedExports any) {
	importedMap, _ := importedExports.(map[string]any)
	for _, binding := range importDecl.Bindings {
		switch binding.Kind {
		case "named":
			env[binding.Local] = importedMap[binding.Imported]
		case "default":
			if importedMap["__cjs"] == true {
				env[binding.Local] = importedExports
			} else if value, ok := importedMap["default"]; ok {
				env[binding.Local] = value
			} else {
				env[binding.Local] = importedExports
			}
		case "namespace":
			env[binding.Local] = importedExports
		case "require":
			env[binding.Local] = importedExports
		case "destructure":
			env[binding.Local] = importedMap[binding.Imported]
		}
	}
}

func evalStmt(stmt map[string]any, env Env) (completion, error) {
	switch stmt["kind"] {
	case "expr":
		_, err := evalExpr(asMap(stmt["expr"]), env)
		return completion{}, err
	case "function-decl":
		env[asString(stmt["name"])] = userFunctionValue(
			asStringSlice(stmt["params"]),
			asString(stmt["restParam"]),
				asStmtSlice(stmt["body"]),
				env,
				false,
				stmt["async"] == true,
				stmt["generator"] == true,
			)
		return completion{}, nil
	case "class-decl":
		classValue, err := evalClass(optionalExpr(stmt, "superClass"), asSlice(stmt["methods"]), env)
		if err != nil {
			return completion{}, err
		}
		env[asString(stmt["name"])] = classValue
		return completion{}, nil
	case "var-decl":
		value := any(jsUndefined)
		var err error
		if init, ok := stmt["init"]; ok {
			value, err = evalExpr(asMap(init), env)
			if err != nil {
				return completion{}, err
			}
		}
		env[asString(stmt["name"])] = value
		return completion{}, nil
	case "if":
		test, err := evalExpr(asMap(stmt["test"]), env)
		if err != nil {
			return completion{}, err
		}
		branch := asStmtSlice(stmt["alternate"])
			if isTruthy(test) {
				branch = asStmtSlice(stmt["consequent"])
			}
			return evalStmtList(branch, env)
	case "label":
		label := asString(stmt["label"])
		body := asStmtSlice(stmt["body"])
		if len(body) == 1 {
			wrapped := cloneStmtMap(body[0])
			wrapped["__label"] = label
			result, err := evalStmt(wrapped, env)
			if err != nil {
				return completion{}, err
			}
			if result.broke && result.breakLabel == label {
				result.broke = false
				result.breakLabel = ""
			}
			return result, nil
		}
		result, err := evalStmtList(body, env)
		if err != nil {
			return completion{}, err
		}
		if result.broke && result.breakLabel == label {
			result.broke = false
			result.breakLabel = ""
		}
		return result, nil
	case "for-of":
		iterable, err := evalExpr(asMap(stmt["right"]), env)
		if err != nil {
			return completion{}, err
		}
			loopLabel := asString(stmt["__label"])
			out := completion{}
			for _, value := range iterableValues(iterable) {
				env[asString(stmt["left"])] = value
				result, err := evalStmtList(asStmtSlice(stmt["body"]), env)
				if err != nil {
					return completion{}, err
				}
				out.yields = append(out.yields, result.yields...)
				if result.returned {
					result.yields = out.yields
					return result, nil
				}
				if result.broke {
					if result.breakLabel == "" || result.breakLabel == loopLabel {
						return out, nil
					}
					result.yields = out.yields
					return result, nil
				}
				if result.continued {
					if result.continueLabel == "" || result.continueLabel == loopLabel {
						continue
					}
					result.yields = out.yields
					return result, nil
				}
			}
			return out, nil
	case "for":
		loopLabel := asString(stmt["__label"])
		for _, init := range asStmtSlice(stmt["init"]) {
			result, err := evalStmt(init, env)
			if err != nil {
				return completion{}, err
			}
			if result.returned || result.broke || result.continued {
				return completion{}, errors.New("invalid for initializer completion")
			}
		}
			out := completion{}
			for {
			if rawTest, ok := stmt["test"]; ok {
				test, err := evalExpr(asMap(rawTest), env)
				if err != nil {
					return completion{}, err
				}
				if !isTruthy(test) {
						return out, nil
					}
				}
				result, err := evalStmtList(asStmtSlice(stmt["body"]), env)
				if err != nil {
					return completion{}, err
				}
				out.yields = append(out.yields, result.yields...)
				if result.returned {
					result.yields = out.yields
					return result, nil
				}
				if result.broke {
					if result.breakLabel == "" || result.breakLabel == loopLabel {
						return out, nil
					}
					result.yields = out.yields
					return result, nil
				}
				if result.continued && result.continueLabel != "" && result.continueLabel != loopLabel {
					result.yields = out.yields
					return result, nil
				}
				if rawUpdate, ok := stmt["update"]; ok {
				if _, err := evalExpr(asMap(rawUpdate), env); err != nil {
					return completion{}, err
				}
			}
		}
	case "while":
			loopLabel := asString(stmt["__label"])
			out := completion{}
			for {
			test, err := evalExpr(asMap(stmt["test"]), env)
			if err != nil {
				return completion{}, err
			}
			if !isTruthy(test) {
					return out, nil
				}
				result, err := evalStmtList(asStmtSlice(stmt["body"]), env)
				if err != nil {
					return completion{}, err
				}
				out.yields = append(out.yields, result.yields...)
				if result.returned {
					result.yields = out.yields
					return result, nil
				}
				if result.broke {
					if result.breakLabel == "" || result.breakLabel == loopLabel {
						return out, nil
					}
					result.yields = out.yields
					return result, nil
				}
				if result.continued && result.continueLabel != "" && result.continueLabel != loopLabel {
					result.yields = out.yields
					return result, nil
				}
			}
	case "switch":
		discriminant, err := evalExpr(asMap(stmt["discriminant"]), env)
		if err != nil {
			return completion{}, err
		}
		matched := false
		defaultIndex := -1
		cases := asSlice(stmt["cases"])
		for index, rawCase := range cases {
			caseMap := asMap(rawCase)
			if rawTest, ok := caseMap["test"]; ok {
				test, err := evalExpr(asMap(rawTest), env)
				if err != nil {
					return completion{}, err
				}
				if jsSameValue(discriminant, test) {
					matched = true
				}
			} else if defaultIndex < 0 {
				defaultIndex = index
			}
			if matched {
				break
			}
		}
		start := defaultIndex
		if matched {
			for index, rawCase := range cases {
				caseMap := asMap(rawCase)
				if rawTest, ok := caseMap["test"]; ok {
					test, err := evalExpr(asMap(rawTest), env)
					if err != nil {
						return completion{}, err
					}
					if jsSameValue(discriminant, test) {
						start = index
						break
					}
				}
			}
		}
		if start < 0 {
			return completion{}, nil
		}
		for _, rawCase := range cases[start:] {
			for _, child := range asStmtSlice(asMap(rawCase)["consequent"]) {
				result, err := evalStmt(child, env)
				if err != nil {
					return completion{}, err
				}
				if result.returned {
					return result, nil
				}
				if result.broke {
					if result.breakLabel == "" {
						return completion{}, nil
					}
					return result, nil
				}
				if result.continued {
					return result, nil
				}
			}
		}
		return completion{}, nil
	case "try":
		result, err := evalStmtList(asStmtSlice(stmt["body"]), env)
		if err != nil {
			var thrown jsThrow
			if errors.As(err, &thrown) {
				if catchParam := asString(stmt["catchParam"]); catchParam != "" {
					env[catchParam] = thrown.value
				}
				result, err = evalStmtList(asStmtSlice(stmt["catchBody"]), env)
			}
		}
		finallyResult, finallyErr := evalStmtList(asStmtSlice(stmt["finallyBody"]), env)
		if finallyErr != nil {
			return completion{}, finallyErr
		}
		if finallyResult.returned || finallyResult.broke || finallyResult.continued {
			return finallyResult, nil
		}
		if err != nil {
			return completion{}, err
		}
		return result, nil
		case "throw":
		value, err := evalExpr(asMap(stmt["value"]), env)
		if err != nil {
			return completion{}, err
		}
			return completion{}, jsThrow{value: value}
		case "yield":
			value := any(jsUndefined)
			var err error
			if raw, ok := stmt["value"]; ok {
				value, err = evalExpr(asMap(raw), env)
				if err != nil {
					return completion{}, err
				}
			}
			if stmt["delegate"] == true {
				return completion{yields: iterableValues(value)}, nil
			}
			return completion{yields: []any{value}}, nil
	case "break":
		return completion{broke: true, breakLabel: asString(stmt["label"])}, nil
	case "continue":
		return completion{continued: true, continueLabel: asString(stmt["label"])}, nil
	case "return":
		value := any(jsUndefined)
		var err error
		if raw, ok := stmt["value"]; ok {
			value, err = evalExpr(asMap(raw), env)
			if err != nil {
				return completion{}, err
			}
		}
		return completion{value: value, returned: true}, nil
	default:
		return completion{}, fmt.Errorf("unsupported statement %v", stmt["kind"])
	}
}

func evalStmtList(stmts []map[string]any, env Env) (completion, error) {
	hoistFunctionDeclarations(stmts, env)
	out := completion{}
	for _, stmt := range stmts {
		result, err := evalStmt(stmt, env)
		if err != nil {
			return completion{}, err
		}
		out.yields = append(out.yields, result.yields...)
		if result.returned || result.broke || result.continued {
			result.yields = out.yields
			return result, nil
		}
	}
	return out, nil
}

func hoistFunctionDeclarations(stmts []map[string]any, env Env) {
	for _, stmt := range stmts {
		if stmt["kind"] != "function-decl" {
			continue
		}
		env[asString(stmt["name"])] = userFunctionValue(
			asStringSlice(stmt["params"]),
			asString(stmt["restParam"]),
				asStmtSlice(stmt["body"]),
				env,
				false,
				stmt["async"] == true,
				stmt["generator"] == true,
			)
	}
}

func evalExpr(expr map[string]any, env Env) (any, error) {
	switch expr["kind"] {
	case "value":
		return evalValue(asMap(expr["value"]))
	case "ident":
		return lookupEnv(env, asString(expr["name"])), nil
	case "this":
		return lookupEnv(env, "this"), nil
	case "array":
		out := []any{}
		for _, item := range asSlice(expr["items"]) {
			value, err := evalExpr(asMap(item), env)
			if err != nil {
				return nil, err
			}
			out = append(out, value)
		}
		return &ArrayValue{Items: out}, nil
	case "array-spread":
		out := []any{}
		for _, rawItem := range asSlice(expr["items"]) {
			item := asMap(rawItem)
			value, err := evalExpr(asMap(item["value"]), env)
			if err != nil {
				return nil, err
			}
			if item["spread"] == true {
				out = append(out, iterableValues(value)...)
			} else {
				out = append(out, value)
			}
		}
		return &ArrayValue{Items: out}, nil
	case "object":
		out := map[string]any{}
		for _, prop := range asSlice(expr["props"]) {
			propMap := asMap(prop)
			value, err := evalExpr(asMap(propMap["value"]), env)
			if err != nil {
				return nil, err
			}
			if propMap["spread"] == true {
				if source, ok := value.(map[string]any); ok {
					for _, key := range objectKeys(source) {
						if strings.HasPrefix(key, "__") {
							continue
						}
						setObjectProperty(out, key, source[key])
					}
				}
				continue
			}
			key := asString(propMap["key"])
			if rawKeyExpr, ok := propMap["keyExpr"]; ok {
				keyValue, err := evalExpr(asMap(rawKeyExpr), env)
				if err != nil {
					return nil, err
				}
				key = jsPropertyKey(keyValue)
			}
			setObjectProperty(out, key, value)
		}
		return out, nil
	case "object-rest":
		value, err := evalExpr(asMap(expr["object"]), env)
		if err != nil {
			return nil, err
		}
		out := map[string]any{}
		source, ok := value.(map[string]any)
		if !ok {
			return out, nil
		}
		excluded := map[string]bool{}
		for _, key := range asStringSlice(expr["excluded"]) {
			excluded[key] = true
		}
		for _, key := range objectKeys(source) {
			if excluded[key] || strings.HasPrefix(key, "__") {
				continue
			}
			setObjectProperty(out, key, source[key])
		}
		return out, nil
	case "function":
		return userFunctionValue(
			asStringSlice(expr["params"]),
			asString(expr["restParam"]),
				asStmtSlice(expr["body"]),
				env,
				expr["lexicalThis"] == true,
				expr["async"] == true,
				expr["generator"] == true,
			), nil
	case "class":
		return evalClass(optionalExpr(expr, "superClass"), asSlice(expr["methods"]), env)
	case "unary":
		if asString(expr["op"]) == "delete" {
			return evalDelete(asMap(expr["arg"]), env)
		}
		arg, err := evalExpr(asMap(expr["arg"]), env)
		if err != nil {
			return nil, err
		}
		return evalUnary(asString(expr["op"]), arg)
	case "await":
		value, err := evalExpr(asMap(expr["arg"]), env)
		if err != nil {
			return nil, err
		}
		return awaitValue(value)
	case "binary":
		left, err := evalExpr(asMap(expr["left"]), env)
		if err != nil {
			return nil, err
		}
		op := asString(expr["op"])
		if op == "&&" {
			if !isTruthy(left) {
				return left, nil
			}
			return evalExpr(asMap(expr["right"]), env)
		}
		if op == "||" {
			if isTruthy(left) {
				return left, nil
			}
			return evalExpr(asMap(expr["right"]), env)
		}
		if op == "??" {
			if !isNullish(left) {
				return left, nil
			}
			return evalExpr(asMap(expr["right"]), env)
		}
		right, err := evalExpr(asMap(expr["right"]), env)
		if err != nil {
			return nil, err
		}
		return evalBinary(op, left, right)
	case "conditional":
		if truthy, err := evalExpr(asMap(expr["test"]), env); err != nil {
			return nil, err
		} else if isTruthy(truthy) {
			return evalExpr(asMap(expr["consequent"]), env)
		}
		return evalExpr(asMap(expr["alternate"]), env)
	case "assign":
		return evalAssign(expr, env)
	case "update":
		return evalUpdate(expr, env)
	case "call":
		if isConsoleLog(asMap(expr["callee"])) {
			parts := []string{}
			values, err := evalCallArgs(asSlice(expr["args"]), env)
			if err != nil {
				return nil, err
			}
			for _, value := range values {
				parts = append(parts, jsString(value))
			}
			fmt.Fprintln(os.Stdout, strings.Join(parts, " "))
			return jsUndefined, nil
		}
		if callee := asMap(expr["callee"]); callee["kind"] == "ident" {
			name := asString(callee["name"])
			result, err := callFunction(lookupEnv(env, name), asSlice(expr["args"]), env)
			if err != nil {
				return nil, fmt.Errorf("identifier call %s failed: %w", name, err)
			}
			return result, nil
			}
			if callee := asMap(expr["callee"]); callee["kind"] == "member" {
				object, property, value, err := evalMemberAccess(callee, env)
				if err != nil {
					return nil, err
				}
				if callee["optional"] == true && isNullish(value) {
					return jsUndefined, nil
				}
				args, err := evalCallArgs(asSlice(expr["args"]), env)
				if err != nil {
					return nil, err
				}
			result, err := callFunctionWithValues(value, args, env, object)
			if err != nil {
				return nil, fmt.Errorf("member call %s on %s failed: %w", property, jsInspect(object), err)
			}
			return result, nil
		}
		value, err := evalExpr(asMap(expr["callee"]), env)
			if err != nil {
				return nil, err
			}
			if expr["optional"] == true && isNullish(value) {
				return jsUndefined, nil
			}
			return callFunction(value, asSlice(expr["args"]), env)
	case "new":
		callee, err := evalExpr(asMap(expr["callee"]), env)
		if err != nil {
			return nil, err
		}
		result, err := constructValue(callee, asSlice(expr["args"]), env)
		if err != nil {
			return nil, fmt.Errorf("new %s evaluated to %s failed: %w", exprLabel(asMap(expr["callee"])), jsInspect(callee), err)
		}
		return result, nil
		case "spread":
			return evalExpr(asMap(expr["arg"]), env)
		case "member":
			_, _, value, err := evalMemberAccess(expr, env)
			return value, err
	case "template":
		var out strings.Builder
		quasis := asStringSlice(expr["quasis"])
		exprs := asSlice(expr["exprs"])
		for index, quasi := range quasis {
			out.WriteString(quasi)
			if index < len(exprs) {
				value, err := evalExpr(asMap(exprs[index]), env)
				if err != nil {
					return nil, err
				}
				out.WriteString(jsString(value))
			}
		}
		return out.String(), nil
	case "sequence":
		var value any
		for _, entry := range asSlice(expr["exprs"]) {
			next, err := evalExpr(asMap(entry), env)
			if err != nil {
				return nil, err
			}
			value = next
		}
		return value, nil
	default:
		return nil, fmt.Errorf("unsupported expression %v", expr["kind"])
	}
}

func assignTarget(target map[string]any, value any, env Env) error {
	switch target["kind"] {
	case "ident":
		assignEnv(env, asString(target["name"]), value)
		return nil
	case "member":
		objectExpr := asMap(target["object"])
		object, err := evalExpr(objectExpr, env)
		if err != nil {
			return err
		}
		property, err := evalMemberProperty(target, env)
		if err != nil {
			return err
		}
		objectMap, ok := object.(map[string]any)
		if ok {
			if classValue, ok := objectMap["__class"].(*ClassValue); ok {
				if setter, ok := lookupSetter(classValue, property); ok {
					_, err := callFunctionWithThisValues(setter, []any{value}, objectMap)
					return err
				}
			}
			setObjectProperty(objectMap, property, value)
			return nil
		}
		if function, ok := object.(FunctionValue); ok {
			if function.Props == nil {
				function.Props = map[string]any{}
			}
			function.Props[property] = value
			return assignTarget(objectExpr, function, env)
		}
		if function, ok := object.(NativeFunctionValue); ok {
			if function.Props == nil {
				function.Props = map[string]any{}
			}
			function.Props[property] = value
			return assignTarget(objectExpr, function, env)
		}
		if regExpValue, ok := object.(*RegExpValue); ok {
			if property == "lastIndex" {
				regExpValue.LastIndex = jsInteger(value)
				return nil
			}
			if regExpValue.Props == nil {
				regExpValue.Props = map[string]any{}
			}
			regExpValue.Props[property] = value
			return nil
		}
		if classValue, ok := object.(*ClassValue); ok {
			if setter, ok := lookupStaticSetter(classValue, property); ok {
				_, err := callFunctionWithThisValues(setter, []any{value}, classValue)
				return err
			}
			if classValue.Props == nil {
				classValue.Props = map[string]any{}
			}
			classValue.Props[property] = value
			return nil
		}
		if objectArray, ok := object.(*ArrayValue); ok {
			handled := assignArrayMember(objectArray, property, value)
			if !handled {
				return fmt.Errorf("member assignment target array property %s is not assignable", property)
			}
			return nil
		}
		return fmt.Errorf("member assignment target is not object: %T %s", object, jsInspect(object))
	default:
		return fmt.Errorf("unsupported assignment target %v", target["kind"])
	}
}

func evalMemberProperty(member map[string]any, env Env) (string, error) {
	if raw, ok := member["propertyExpr"]; ok {
		value, err := evalExpr(asMap(raw), env)
		if err != nil {
			return "", err
		}
		return jsPropertyKey(value), nil
	}
	return asString(member["property"]), nil
}

func evalMemberAccess(expr map[string]any, env Env) (any, string, any, error) {
	object, err := evalExpr(asMap(expr["object"]), env)
	if err != nil {
		return nil, "", nil, err
	}
	if expr["optional"] == true && isNullish(object) {
		return object, asString(expr["property"]), jsUndefined, nil
	}
	property, err := evalMemberProperty(expr, env)
	if err != nil {
		return nil, "", nil, err
	}
	if objectMap, ok := object.(map[string]any); ok {
		if objectMap["__promise"] == true && property == "constructor" {
			return object, property, lookupEnv(env, "Promise"), nil
		}
		if classValue, ok := objectMap["__class"].(*ClassValue); ok {
			if getter, ok := lookupGetter(classValue, property); ok {
				value, err := callFunctionWithThis(getter, nil, env, objectMap)
				return object, property, value, err
			}
			if method, ok := lookupMethod(classValue, property); ok {
				return object, property, BoundFunctionValue{Function: method, This: objectMap}, nil
			}
		}
		if value, ok := lookupObjectProperty(objectMap, property); ok {
			return object, property, value, nil
		}
		if property == "hasOwnProperty" {
			return object, property, nativeFunction(func(args []any) (any, error) {
				if len(args) == 0 {
					return false, nil
				}
				_, exists := objectMap[jsPropertyKey(args[0])]
				return exists, nil
			}), nil
		}
		return object, property, jsUndefined, nil
	}
	if classValue, ok := object.(*ClassValue); ok {
		if classValue.Props != nil {
			if value, ok := classValue.Props[property]; ok {
				return object, property, value, nil
			}
		}
		if property == "call" && classValue.Callable {
			return object, property, nativeFunction(func(args []any) (any, error) {
				thisValue := any(jsUndefined)
				callArgs := []any{}
				if len(args) > 0 {
					thisValue = args[0]
					callArgs = append(callArgs, args[1:]...)
				}
				if classValue.Constructor == nil {
					return thisValue, nil
				}
				_, err := callFunctionWithThisValues(*classValue.Constructor, callArgs, thisValue)
				if err != nil {
					return nil, err
				}
				return thisValue, nil
			}), nil
		}
		if getter, ok := classValue.StaticGetters[property]; ok {
			value, err := callFunctionWithThis(getter, nil, env, classValue)
			return object, property, value, err
		}
		if method, ok := classValue.Static[property]; ok {
			return object, property, BoundFunctionValue{Function: method, This: classValue}, nil
		}
	}
	if function, ok := object.(NativeFunctionValue); ok {
		if member, ok := nativeFunctionMember(function, property); ok {
			return object, property, member, nil
		}
	}
	if function, ok := object.(FunctionValue); ok {
		if member, ok := functionMember(function, property); ok {
			return object, property, member, nil
		}
	}
	if function, ok := object.(BoundFunctionValue); ok {
		if member, ok := boundFunctionMember(function, property); ok {
			return object, property, member, nil
		}
	}
	if numberValue, ok := object.(float64); ok {
		if member, ok := numberMember(numberValue, property); ok {
			return object, property, member, nil
		}
	}
	if stringValue, ok := object.(string); ok {
		if member, ok := stringMember(stringValue, property, env); ok {
			return object, property, member, nil
		}
	}
	if symbolValue, ok := object.(*SymbolValue); ok {
		if member, ok := symbolMember(symbolValue, property); ok {
			return object, property, member, nil
		}
	}
	if regExpValue, ok := object.(*RegExpValue); ok {
		if member, ok := regexpMember(regExpValue, property); ok {
			return object, property, member, nil
		}
	}
	if dateValue, ok := object.(*DateValue); ok {
		if member, ok := dateMember(dateValue, property); ok {
			return object, property, member, nil
		}
	}
	if mapValue, ok := object.(*MapValue); ok {
		if member, ok := mapMember(mapValue, property); ok {
			return object, property, member, nil
		}
	}
	if setValue, ok := object.(*SetValue); ok {
		if member, ok := setMember(setValue, property); ok {
			return object, property, member, nil
		}
	}
	if iteratorValue, ok := object.(*IteratorValue); ok {
		if member, ok := iteratorMember(iteratorValue, property); ok {
			return object, property, member, nil
		}
	}
	if objectArray, ok := object.(*ArrayValue); ok {
		if member, ok := arrayMember(objectArray, property, env); ok {
			return object, property, member, nil
		}
	}
	return object, property, jsUndefined, nil
}

func lookupObjectProperty(object map[string]any, property string) (any, bool) {
	if value, ok := object[property]; ok {
		return value, true
	}
	if prototype, ok := object["__prototype"]; ok {
		return lookupPrototypeProperty(prototype, property)
	}
	return nil, false
}

func lookupPrototypeProperty(prototype any, property string) (any, bool) {
	switch typed := prototype.(type) {
	case map[string]any:
		return lookupObjectProperty(typed, property)
	case FunctionValue:
		return functionMember(typed, property)
	case NativeFunctionValue:
		return nativeFunctionMember(typed, property)
	default:
		return nil, false
	}
}

func assignArrayMember(array *ArrayValue, property string, value any) bool {
	if property == "length" {
		nextLength := jsInteger(value)
		if nextLength < 0 {
			nextLength = 0
		}
		if nextLength < len(array.Items) {
			array.Items = array.Items[:nextLength]
			return true
		}
		for len(array.Items) < nextLength {
			array.Items = append(array.Items, jsUndefined)
		}
		return true
	}
	index, err := strconv.ParseInt(property, 0, 64)
	if err != nil || index < 0 {
		if array.Props == nil {
			array.Props = map[string]any{}
		}
		array.Props[property] = value
		return true
	}
	for int64(len(array.Items)) <= index {
		array.Items = append(array.Items, jsUndefined)
	}
	array.Items[int(index)] = value
	return true
}

func readTarget(target map[string]any, env Env) (any, error) {
	switch target["kind"] {
	case "ident":
		return lookupEnv(env, asString(target["name"])), nil
	case "member":
		return evalExpr(target, env)
	default:
		return nil, fmt.Errorf("unsupported assignment target %v", target["kind"])
	}
}

func evalAssign(expr map[string]any, env Env) (any, error) {
	op := asString(expr["op"])
	left := asMap(expr["left"])
	rightExpr := asMap(expr["right"])
	var value any
	var err error
	switch op {
	case "=":
		value, err = evalExpr(rightExpr, env)
	case "+=", "-=", "*=", "/=", "%=", "**=", "&=", "|=", "^=", "<<=", ">>=", ">>>=":
		current, readErr := readTarget(left, env)
		if readErr != nil {
			return nil, readErr
		}
		right, evalErr := evalExpr(rightExpr, env)
		if evalErr != nil {
			return nil, evalErr
		}
		value, err = evalBinary(strings.TrimSuffix(op, "="), current, right)
	case "&&=":
		current, readErr := readTarget(left, env)
		if readErr != nil {
			return nil, readErr
		}
		if !isTruthy(current) {
			return current, nil
		}
		value, err = evalExpr(rightExpr, env)
	case "||=":
		current, readErr := readTarget(left, env)
		if readErr != nil {
			return nil, readErr
		}
		if isTruthy(current) {
			return current, nil
		}
		value, err = evalExpr(rightExpr, env)
	case "??=":
		current, readErr := readTarget(left, env)
		if readErr != nil {
			return nil, readErr
		}
		if !isNullish(current) {
			return current, nil
		}
		value, err = evalExpr(rightExpr, env)
	default:
		return nil, errors.New("unsupported assignment operator")
	}
	if err != nil {
		return nil, err
	}
	if err := assignTarget(left, value, env); err != nil {
		return nil, err
	}
	return value, nil
}

func evalDelete(target map[string]any, env Env) (any, error) {
	if target["kind"] != "member" {
		return true, nil
	}
	object, err := evalExpr(asMap(target["object"]), env)
	if err != nil {
		return nil, err
	}
	property, err := evalMemberProperty(target, env)
	if err != nil {
		return nil, err
	}
	deleteDynamicProperty(object, property)
	return true, nil
}

func evalUpdate(expr map[string]any, env Env) (any, error) {
	target := asMap(expr["arg"])
	current, err := readTarget(target, env)
	if err != nil {
		return nil, err
	}
	oldNumber := toNumber(current)
	nextNumber := oldNumber
	switch asString(expr["op"]) {
	case "++":
		nextNumber++
	case "--":
		nextNumber--
	default:
		return nil, errors.New("unsupported update operator")
	}
	if err := assignTarget(target, nextNumber, env); err != nil {
		return nil, err
	}
	if expr["prefix"] == true {
		return nextNumber, nil
	}
	return oldNumber, nil
}

func callArrayPush(callee map[string]any, rawArgs []any, env Env) (any, error) {
	objectExpr := asMap(callee["object"])
	current, err := evalExpr(objectExpr, env)
	if err != nil {
		return nil, err
	}
	array, ok := current.(*ArrayValue)
	if !ok {
		return nil, fmt.Errorf("push receiver is not array: %T %s", current, jsInspect(current))
	}
	for _, arg := range rawArgs {
		value, err := evalExpr(asMap(arg), env)
		if err != nil {
			return nil, err
		}
		array.Items = append(array.Items, value)
	}
	return float64(len(array.Items)), nil
}

func callArrayPop(callee map[string]any, env Env) (any, error) {
	objectExpr := asMap(callee["object"])
	current, err := evalExpr(objectExpr, env)
	if err != nil {
		return nil, err
	}
	array, ok := current.(*ArrayValue)
	if !ok {
		return nil, errors.New("pop receiver is not array")
	}
	if len(array.Items) == 0 {
		return jsUndefined, nil
	}
	value := array.Items[len(array.Items)-1]
	array.Items = array.Items[:len(array.Items)-1]
	return value, nil
}

func nativeFunctionMember(function NativeFunctionValue, property string) (any, bool) {
	if function.Props != nil {
		if value, ok := function.Props[property]; ok {
			return value, true
		}
	}
	switch property {
	case "call":
		return nativeFunction(func(args []any) (any, error) {
			thisValue := any(jsUndefined)
			if len(args) > 0 {
				thisValue = args[0]
				args = args[1:]
			}
			if function.CallWithThis != nil {
				return function.CallWithThis(thisValue, args)
			}
			return function.Call(args)
		}), true
	case "apply":
		return nativeFunction(func(args []any) (any, error) {
			thisValue := any(jsUndefined)
			callArgs := []any{}
			if len(args) > 0 {
				thisValue = args[0]
			}
			if len(args) > 1 {
				callArgs = iterableValues(args[1])
			}
			if function.CallWithThis != nil {
				return function.CallWithThis(thisValue, callArgs)
			}
			return function.Call(callArgs)
		}), true
	case "bind":
		return nativeFunction(func(args []any) (any, error) {
			thisValue := any(jsUndefined)
			boundArgs := []any{}
			if len(args) > 0 {
				thisValue = args[0]
				boundArgs = append(boundArgs, args[1:]...)
			}
			return bindCallable(function, thisValue, boundArgs), nil
		}), true
	}
	return nil, false
}

func functionMember(function FunctionValue, property string) (any, bool) {
	if function.Props != nil {
		if value, ok := function.Props[property]; ok {
			return value, true
		}
	}
	switch property {
	case "call":
		return nativeFunction(func(args []any) (any, error) {
			thisValue := any(jsUndefined)
			if len(args) > 0 {
				thisValue = args[0]
				args = args[1:]
			}
			return callFunctionWithThisValues(function, args, thisValue)
		}), true
	case "apply":
		return nativeFunction(func(args []any) (any, error) {
			thisValue := any(jsUndefined)
			callArgs := []any{}
			if len(args) > 0 {
				thisValue = args[0]
			}
			if len(args) > 1 {
				callArgs = iterableValues(args[1])
			}
			return callFunctionWithThisValues(function, callArgs, thisValue)
		}), true
	case "bind":
		return nativeFunction(func(args []any) (any, error) {
			thisValue := any(jsUndefined)
			boundArgs := []any{}
			if len(args) > 0 {
				thisValue = args[0]
				boundArgs = append(boundArgs, args[1:]...)
			}
			return bindCallable(function, thisValue, boundArgs), nil
		}), true
	}
	return nil, false
}

func boundFunctionMember(function BoundFunctionValue, property string) (any, bool) {
	switch property {
	case "call":
		return nativeFunction(func(args []any) (any, error) {
			if len(args) > 0 {
				args = args[1:]
			}
			callArgs := append([]any{}, function.Args...)
			callArgs = append(callArgs, args...)
			return callFunctionWithThisValues(function.Function, callArgs, function.This)
		}), true
	case "apply":
		return nativeFunction(func(args []any) (any, error) {
			callArgs := []any{}
			if len(args) > 1 {
				callArgs = iterableValues(args[1])
			}
			return callFunctionWithThisValues(function.Function, append(append([]any{}, function.Args...), callArgs...), function.This)
		}), true
	case "bind":
		return nativeFunction(func(args []any) (any, error) {
			boundArgs := append([]any{}, function.Args...)
			if len(args) > 1 {
				boundArgs = append(boundArgs, args[1:]...)
			}
			return BoundFunctionValue{Function: function.Function, This: function.This, Args: boundArgs}, nil
		}), true
	}
	return nil, false
}

func bindCallable(raw any, thisValue any, boundArgs []any) any {
	switch function := raw.(type) {
	case FunctionValue:
		return BoundFunctionValue{Function: function, This: thisValue, Args: boundArgs}
	case BoundFunctionValue:
		args := append([]any{}, function.Args...)
		args = append(args, boundArgs...)
		return BoundFunctionValue{Function: function.Function, This: function.This, Args: args}
	case NativeFunctionValue:
		return nativeFunction(func(args []any) (any, error) {
			callArgs := append([]any{}, boundArgs...)
			callArgs = append(callArgs, args...)
			if function.CallWithThis != nil {
				return function.CallWithThis(thisValue, callArgs)
			}
			return function.Call(callArgs)
		})
	default:
		return raw
	}
}

func numberMember(value float64, property string) (any, bool) {
	switch property {
	case "toString":
		return nativeFunction(func(args []any) (any, error) {
			radix := 10
			if len(args) > 0 && !isNullish(args[0]) {
				radix = jsInteger(args[0])
			}
			if radix < 2 || radix > 36 || math.IsNaN(value) || math.IsInf(value, 0) || math.Trunc(value) != value {
				return jsString(value), nil
			}
			return strconv.FormatInt(int64(value), radix), nil
		}), true
	case "valueOf":
		return nativeFunction(func(args []any) (any, error) {
			return value, nil
		}), true
	}
	return nil, false
}

func symbolMember(value *SymbolValue, property string) (any, bool) {
	switch property {
	case "toString":
		return nativeFunction(func(args []any) (any, error) {
			return jsString(value), nil
		}), true
	case "valueOf":
		return nativeFunction(func(args []any) (any, error) {
			return value, nil
		}), true
	}
	return nil, false
}

func stringMember(value string, property string, env Env) (any, bool) {
	switch property {
	case "length":
		return float64(len([]rune(value))), true
	case "toString", "valueOf":
		return nativeFunction(func(args []any) (any, error) {
			return value, nil
		}), true
	case "trim":
		return nativeFunction(func(args []any) (any, error) {
			return strings.TrimSpace(value), nil
		}), true
	case "toLowerCase":
		return nativeFunction(func(args []any) (any, error) {
			return strings.ToLower(value), nil
		}), true
	case "toUpperCase":
		return nativeFunction(func(args []any) (any, error) {
			return strings.ToUpper(value), nil
		}), true
	case "trimStart", "trimLeft":
		return nativeFunction(func(args []any) (any, error) {
			return strings.TrimLeftFunc(value, isJSWhitespace), nil
		}), true
	case "trimEnd", "trimRight":
		return nativeFunction(func(args []any) (any, error) {
			return strings.TrimRightFunc(value, isJSWhitespace), nil
		}), true
	case "split":
		return nativeFunction(func(args []any) (any, error) {
			if len(args) == 0 || isNullish(args[0]) {
				return &ArrayValue{Items: []any{value}}, nil
			}
			if separator, ok := args[0].(*RegExpValue); ok {
				result := []any{}
				for _, part := range regexpSplit(separator, value) {
					result = append(result, part)
				}
				return &ArrayValue{Items: result}, nil
			}
			separator := jsString(args[0])
			result := []any{}
			if separator == "" {
				for _, char := range value {
					result = append(result, string(char))
				}
				return &ArrayValue{Items: result}, nil
			}
			for _, part := range strings.Split(value, separator) {
				result = append(result, part)
			}
			return &ArrayValue{Items: result}, nil
		}), true
	case "replace":
		return nativeFunction(func(args []any) (any, error) {
			if len(args) < 2 {
				return value, nil
			}
			if search, ok := args[0].(*RegExpValue); ok {
				return replaceRegExp(value, search, args[1], env)
			}
			search := jsString(args[0])
			index := strings.Index(value, search)
			if index < 0 {
				return value, nil
			}
			replacement := jsString(args[1])
			return value[:index] + replacement + value[index+len(search):], nil
		}), true
	case "replaceAll":
		return nativeFunction(func(args []any) (any, error) {
			if len(args) < 2 {
				return value, nil
			}
			if search, ok := args[0].(*RegExpValue); ok {
				search.Global = true
				return replaceRegExp(value, search, args[1], env)
			}
			search := jsString(args[0])
			if search == "" {
				return strings.ReplaceAll(value, "", jsString(args[1])), nil
			}
			return strings.ReplaceAll(value, search, jsString(args[1])), nil
		}), true
	case "match":
		return nativeFunction(func(args []any) (any, error) {
			if len(args) == 0 {
				compiled, err := newRegExp("", "")
				if err != nil {
					return nil, err
				}
				return regexpMatches(compiled, value), nil
			}
			if search, ok := args[0].(*RegExpValue); ok {
				return regexpMatches(search, value), nil
			}
			compiled, err := newRegExp(jsString(args[0]), "")
			if err != nil {
				return nil, err
			}
			return regexpMatches(compiled, value), nil
		}), true
	case "slice":
		return nativeFunction(func(args []any) (any, error) {
			start := 0
			end := len([]rune(value))
			if len(args) > 0 {
				start = jsSliceIndex(args[0], end)
			}
			if len(args) > 1 {
				end = jsSliceIndex(args[1], end)
			}
			return stringRuneSlice(value, start, end), nil
		}), true
	case "substring":
		return nativeFunction(func(args []any) (any, error) {
			length := len([]rune(value))
			start := 0
			end := length
			if len(args) > 0 {
				start = clampIndex(jsInteger(args[0]), length)
			}
			if len(args) > 1 {
				end = clampIndex(jsInteger(args[1]), length)
			}
			if start > end {
				start, end = end, start
			}
			return stringRuneSlice(value, start, end), nil
		}), true
	case "substr":
		return nativeFunction(func(args []any) (any, error) {
			length := len([]rune(value))
			start := 0
			if len(args) > 0 {
				start = jsInteger(args[0])
				if start < 0 {
					start = maxInt(length+start, 0)
				}
			}
			count := length - start
			if len(args) > 1 {
				count = maxInt(jsInteger(args[1]), 0)
			}
			return stringRuneSlice(value, start, start+count), nil
		}), true
	case "charAt":
		return nativeFunction(func(args []any) (any, error) {
			index := 0
			if len(args) > 0 {
				index = jsInteger(args[0])
			}
			runes := []rune(value)
			if index < 0 || index >= len(runes) {
				return "", nil
			}
			return string(runes[index]), nil
		}), true
	case "charCodeAt":
		return nativeFunction(func(args []any) (any, error) {
			index := 0
			if len(args) > 0 {
				index = jsInteger(args[0])
			}
			runes := []rune(value)
			if index < 0 || index >= len(runes) {
				return math.NaN(), nil
			}
			return float64(runes[index]), nil
		}), true
	case "codePointAt":
		return nativeFunction(func(args []any) (any, error) {
			index := 0
			if len(args) > 0 {
				index = jsInteger(args[0])
			}
			runes := []rune(value)
			if index < 0 || index >= len(runes) {
				return jsUndefined, nil
			}
			return float64(runes[index]), nil
		}), true
	case "at":
		return nativeFunction(func(args []any) (any, error) {
			index := 0
			if len(args) > 0 {
				index = jsInteger(args[0])
			}
			runes := []rune(value)
			if index < 0 {
				index = len(runes) + index
			}
			if index < 0 || index >= len(runes) {
				return jsUndefined, nil
			}
			return string(runes[index]), nil
		}), true
	case "indexOf":
		return nativeFunction(func(args []any) (any, error) {
			search := "undefined"
			if len(args) > 0 {
				search = jsString(args[0])
			}
			start := 0
			if len(args) > 1 {
				start = clampIndex(jsInteger(args[1]), len([]rune(value)))
			}
			index := strings.Index(stringRuneSlice(value, start, len([]rune(value))), search)
			if index < 0 {
				return float64(-1), nil
			}
			return float64(len([]rune(value[:byteIndexForRune(value, start)+index]))), nil
		}), true
	case "lastIndexOf":
		return nativeFunction(func(args []any) (any, error) {
			search := "undefined"
			if len(args) > 0 {
				search = jsString(args[0])
			}
			prefix := value
			if len(args) > 1 {
				end := clampIndex(jsInteger(args[1])+len([]rune(search)), len([]rune(value)))
				prefix = stringRuneSlice(value, 0, end)
			}
			index := strings.LastIndex(prefix, search)
			if index < 0 {
				return float64(-1), nil
			}
			return float64(len([]rune(prefix[:index]))), nil
		}), true
	case "includes":
		return nativeFunction(func(args []any) (any, error) {
			search := "undefined"
			if len(args) > 0 {
				search = jsString(args[0])
			}
			start := 0
			if len(args) > 1 {
				start = clampIndex(jsInteger(args[1]), len([]rune(value)))
			}
			return strings.Contains(stringRuneSlice(value, start, len([]rune(value))), search), nil
		}), true
	case "startsWith":
		return nativeFunction(func(args []any) (any, error) {
			search := "undefined"
			if len(args) > 0 {
				search = jsString(args[0])
			}
			start := 0
			if len(args) > 1 {
				start = clampIndex(jsInteger(args[1]), len([]rune(value)))
			}
			return strings.HasPrefix(stringRuneSlice(value, start, len([]rune(value))), search), nil
		}), true
	case "endsWith":
		return nativeFunction(func(args []any) (any, error) {
			search := "undefined"
			if len(args) > 0 {
				search = jsString(args[0])
			}
			end := len([]rune(value))
			if len(args) > 1 {
				end = clampIndex(jsInteger(args[1]), len([]rune(value)))
			}
			return strings.HasSuffix(stringRuneSlice(value, 0, end), search), nil
		}), true
	case "repeat":
		return nativeFunction(func(args []any) (any, error) {
			count := 0
			if len(args) > 0 {
				count = jsInteger(args[0])
			}
			if count < 0 {
				return nil, errors.New("repeat count must be non-negative")
			}
			return strings.Repeat(value, count), nil
		}), true
	}
	index, err := strconv.Atoi(property)
	if err == nil {
		runes := []rune(value)
		if index >= 0 && index < len(runes) {
			return string(runes[index]), true
		}
	}
	return nil, false
}

func regexpMember(value *RegExpValue, property string) (any, bool) {
	if value.Props != nil {
		if prop, ok := value.Props[property]; ok {
			return prop, true
		}
	}
	switch property {
	case "source":
		return value.Pattern, true
	case "flags":
		return value.Flags, true
	case "lastIndex":
		return float64(value.LastIndex), true
	case "test":
		return nativeFunction(func(args []any) (any, error) {
			text := ""
			if len(args) > 0 {
				text = jsString(args[0])
			}
			return regexpTest(value, text)
		}), true
	case "exec":
		return nativeFunction(func(args []any) (any, error) {
			text := ""
			if len(args) > 0 {
				text = jsString(args[0])
			}
			return regexpExec(value, text), nil
		}), true
	}
	return nil, false
}

func dateMember(value *DateValue, property string) (any, bool) {
	switch property {
	case "toISOString", "toJSON":
		return nativeFunction(func(args []any) (any, error) {
			return formatDateISO(value.Time), nil
		}), true
	case "getTime", "valueOf":
		return nativeFunction(func(args []any) (any, error) {
			return float64(value.Time.UnixMilli()), nil
		}), true
	}
	return nil, false
}

func formatDateISO(value time.Time) string {
	return value.UTC().Format("2006-01-02T15:04:05.000Z")
}

func arrayMember(value *ArrayValue, property string, env Env) (any, bool) {
	switch property {
	case "length":
		return float64(len(value.Items)), true
	case "Symbol.iterator":
		return nativeFunction(func(args []any) (any, error) {
			return &IteratorValue{Values: append([]any{}, value.Items...)}, nil
		}), true
	}
	index, err := strconv.Atoi(property)
	if err == nil && index >= 0 && index < len(value.Items) {
		return value.Items[index], true
	}
	if value.Props != nil {
		if member, ok := value.Props[property]; ok {
			return member, true
		}
	}
	switch property {
	case "push":
		return nativeFunction(func(args []any) (any, error) {
			value.Items = append(value.Items, args...)
			return float64(len(value.Items)), nil
		}), true
	case "pop":
		return nativeFunction(func(args []any) (any, error) {
			if len(value.Items) == 0 {
				return jsUndefined, nil
			}
			last := value.Items[len(value.Items)-1]
			value.Items = value.Items[:len(value.Items)-1]
			return last, nil
		}), true
	case "shift":
		return nativeFunction(func(args []any) (any, error) {
			if len(value.Items) == 0 {
				return jsUndefined, nil
			}
			first := value.Items[0]
			value.Items = append([]any{}, value.Items[1:]...)
			return first, nil
		}), true
	case "unshift":
		return nativeFunction(func(args []any) (any, error) {
			next := append([]any{}, args...)
			next = append(next, value.Items...)
			value.Items = next
			return float64(len(value.Items)), nil
		}), true
		case "splice":
			return nativeFunction(func(args []any) (any, error) {
			length := len(value.Items)
			start := length
			if len(args) > 0 {
				start = jsInteger(args[0])
				if start < 0 {
					start = maxInt(length+start, 0)
				} else if start > length {
					start = length
				}
			}
			deleteCount := length - start
			if len(args) > 1 {
				deleteCount = minInt(maxInt(jsInteger(args[1]), 0), length-start)
			}
			insertItems := []any{}
			if len(args) > 2 {
				insertItems = append(insertItems, args[2:]...)
			}
			removed := append([]any{}, value.Items[start:start+deleteCount]...)
			next := append([]any{}, value.Items[:start]...)
			next = append(next, insertItems...)
			next = append(next, value.Items[start+deleteCount:]...)
				value.Items = next
				return &ArrayValue{Items: removed}, nil
			}), true
	case "fill":
		return nativeFunction(func(args []any) (any, error) {
			fillValue := any(jsUndefined)
			if len(args) > 0 {
				fillValue = args[0]
				}
				start := 0
				if len(args) > 1 {
					start = jsInteger(args[1])
					if start < 0 {
						start = maxInt(len(value.Items)+start, 0)
					}
				}
				end := len(value.Items)
				if len(args) > 2 && !isNullish(args[2]) {
					end = jsInteger(args[2])
					if end < 0 {
						end = maxInt(len(value.Items)+end, 0)
					}
				}
				start = minInt(maxInt(start, 0), len(value.Items))
				end = minInt(maxInt(end, 0), len(value.Items))
				for index := start; index < end; index++ {
					value.Items[index] = fillValue
			}
			return value, nil
		}), true
	case "set":
		return nativeFunction(func(args []any) (any, error) {
			if len(args) == 0 {
				return jsUndefined, nil
			}
			offset := 0
			if len(args) > 1 {
				offset = jsInteger(args[1])
			}
			source := iterableValues(args[0])
			for index, item := range source {
				targetIndex := offset + index
				if targetIndex >= 0 && targetIndex < len(value.Items) {
					value.Items[targetIndex] = float64(byte(toUint32(item)))
				}
			}
			return jsUndefined, nil
		}), true
	case "join":
		return nativeFunction(func(args []any) (any, error) {
			separator := ","
			if len(args) > 0 {
				separator = jsString(args[0])
			}
			parts := []string{}
			for _, item := range value.Items {
				if isNullish(item) {
					parts = append(parts, "")
				} else {
					parts = append(parts, jsString(item))
				}
			}
			return strings.Join(parts, separator), nil
		}), true
	case "toString":
		return nativeFunction(func(args []any) (any, error) {
			bytes := bytesFromJSValue(value)
			return string(bytes), nil
		}), true
	case "replace":
		return nativeFunction(func(args []any) (any, error) {
			text := string(bytesFromJSValue(value))
			member, ok := stringMember(text, "replace", env)
			if !ok {
				return text, nil
			}
			return callFunctionWithValues(member, args, env, text)
		}), true
	case "map":
		return nativeFunction(func(args []any) (any, error) {
			if len(args) == 0 {
				return nil, errors.New("map callback is required")
			}
			result := []any{}
			for index, item := range value.Items {
				mapped, err := callFunctionWithValues(args[0], []any{item, float64(index), value}, env, jsUndefined)
				if err != nil {
					return nil, err
				}
				result = append(result, mapped)
			}
			return &ArrayValue{Items: result}, nil
		}), true
	case "flatMap":
		return nativeFunction(func(args []any) (any, error) {
			if len(args) == 0 {
				return nil, errors.New("flatMap callback is required")
			}
			result := []any{}
			for index, item := range value.Items {
				mapped, err := callFunctionWithValues(args[0], []any{item, float64(index), value}, env, jsUndefined)
				if err != nil {
					return nil, err
				}
				if array, ok := mapped.(*ArrayValue); ok {
					result = append(result, array.Items...)
				} else {
					result = append(result, mapped)
				}
			}
			return &ArrayValue{Items: result}, nil
		}), true
	case "filter":
		return nativeFunction(func(args []any) (any, error) {
			if len(args) == 0 {
				return nil, errors.New("filter callback is required")
			}
			result := []any{}
			for index, item := range value.Items {
				keep, err := callFunctionWithValues(args[0], []any{item, float64(index), value}, env, jsUndefined)
				if err != nil {
					return nil, err
				}
				if isTruthy(keep) {
					result = append(result, item)
				}
			}
			return &ArrayValue{Items: result}, nil
		}), true
	case "forEach":
		return nativeFunction(func(args []any) (any, error) {
			if len(args) == 0 {
				return nil, errors.New("forEach callback is required")
			}
			for index, item := range value.Items {
				if _, err := callFunctionWithValues(args[0], []any{item, float64(index), value}, env, jsUndefined); err != nil {
					return nil, err
				}
			}
			return jsUndefined, nil
		}), true
	case "reduce":
		return nativeFunction(func(args []any) (any, error) {
			if len(args) == 0 {
				return nil, errors.New("reduce callback is required")
			}
			if len(value.Items) == 0 && len(args) < 2 {
				return nil, errors.New("reduce of empty array with no initial value")
			}
			start := 0
			accumulator := any(jsUndefined)
			if len(args) > 1 {
				accumulator = args[1]
			} else {
				accumulator = value.Items[0]
				start = 1
			}
			for index := start; index < len(value.Items); index++ {
				next, err := callFunctionWithValues(args[0], []any{accumulator, value.Items[index], float64(index), value}, env, jsUndefined)
				if err != nil {
					return nil, err
				}
				accumulator = next
			}
			return accumulator, nil
		}), true
	case "reduceRight":
		return nativeFunction(func(args []any) (any, error) {
			if len(args) == 0 {
				return nil, errors.New("reduceRight callback is required")
			}
			if len(value.Items) == 0 && len(args) < 2 {
				return nil, errors.New("reduce of empty array with no initial value")
			}
			index := len(value.Items) - 1
			accumulator := any(jsUndefined)
			if len(args) > 1 {
				accumulator = args[1]
			} else {
				accumulator = value.Items[index]
				index--
			}
			for ; index >= 0; index-- {
				next, err := callFunctionWithValues(args[0], []any{accumulator, value.Items[index], float64(index), value}, env, jsUndefined)
				if err != nil {
					return nil, err
				}
				accumulator = next
			}
			return accumulator, nil
		}), true
	case "some":
		return nativeFunction(func(args []any) (any, error) {
			if len(args) == 0 {
				return nil, errors.New("some callback is required")
			}
			for index, item := range value.Items {
				next, err := callFunctionWithValues(args[0], []any{item, float64(index), value}, env, jsUndefined)
				if err != nil {
					return nil, err
				}
				if isTruthy(next) {
					return true, nil
				}
			}
			return false, nil
		}), true
	case "every":
		return nativeFunction(func(args []any) (any, error) {
			if len(args) == 0 {
				return nil, errors.New("every callback is required")
			}
			for index, item := range value.Items {
				next, err := callFunctionWithValues(args[0], []any{item, float64(index), value}, env, jsUndefined)
				if err != nil {
					return nil, err
				}
				if !isTruthy(next) {
					return false, nil
				}
			}
			return true, nil
		}), true
	case "find":
		return nativeFunction(func(args []any) (any, error) {
			if len(args) == 0 {
				return nil, errors.New("find callback is required")
			}
			for index, item := range value.Items {
				next, err := callFunctionWithValues(args[0], []any{item, float64(index), value}, env, jsUndefined)
				if err != nil {
					return nil, err
				}
				if isTruthy(next) {
					return item, nil
				}
			}
			return jsUndefined, nil
		}), true
	case "findIndex":
		return nativeFunction(func(args []any) (any, error) {
			if len(args) == 0 {
				return nil, errors.New("findIndex callback is required")
			}
			for index, item := range value.Items {
				next, err := callFunctionWithValues(args[0], []any{item, float64(index), value}, env, jsUndefined)
				if err != nil {
					return nil, err
				}
				if isTruthy(next) {
					return float64(index), nil
				}
			}
			return float64(-1), nil
		}), true
	case "findLast":
		return nativeFunction(func(args []any) (any, error) {
			if len(args) == 0 {
				return nil, errors.New("findLast callback is required")
			}
			for index := len(value.Items) - 1; index >= 0; index-- {
				item := value.Items[index]
				next, err := callFunctionWithValues(args[0], []any{item, float64(index), value}, env, jsUndefined)
				if err != nil {
					return nil, err
				}
				if isTruthy(next) {
					return item, nil
				}
			}
			return jsUndefined, nil
		}), true
	case "findLastIndex":
		return nativeFunction(func(args []any) (any, error) {
			if len(args) == 0 {
				return nil, errors.New("findLastIndex callback is required")
			}
			for index := len(value.Items) - 1; index >= 0; index-- {
				item := value.Items[index]
				next, err := callFunctionWithValues(args[0], []any{item, float64(index), value}, env, jsUndefined)
				if err != nil {
					return nil, err
				}
				if isTruthy(next) {
					return float64(index), nil
				}
			}
			return float64(-1), nil
		}), true
	case "indexOf":
		return nativeFunction(func(args []any) (any, error) {
			search := any(jsUndefined)
			if len(args) > 0 {
				search = args[0]
			}
			start := 0
			if len(args) > 1 {
				start = jsArrayStartIndex(args[1], len(value.Items))
			}
			return float64(arrayIndexOf(value.Items, search, start)), nil
		}), true
	case "lastIndexOf":
		return nativeFunction(func(args []any) (any, error) {
			search := any(jsUndefined)
			if len(args) > 0 {
				search = args[0]
			}
			start := len(value.Items) - 1
			if len(args) > 1 {
				start = jsSliceIndex(args[1], len(value.Items))
			}
			for index := minInt(start, len(value.Items)-1); index >= 0; index-- {
				if jsSameValue(value.Items[index], search) {
					return float64(index), nil
				}
			}
			return float64(-1), nil
		}), true
	case "includes":
		return nativeFunction(func(args []any) (any, error) {
			search := any(jsUndefined)
			if len(args) > 0 {
				search = args[0]
			}
			start := 0
			if len(args) > 1 {
				start = jsArrayStartIndex(args[1], len(value.Items))
			}
			return arrayIndexOf(value.Items, search, start) >= 0, nil
		}), true
	case "concat":
		return nativeFunction(func(args []any) (any, error) {
			result := append([]any{}, value.Items...)
			for _, arg := range args {
				if next, ok := arg.(*ArrayValue); ok {
					result = append(result, next.Items...)
				} else {
					result = append(result, arg)
				}
			}
			return &ArrayValue{Items: result}, nil
		}), true
	case "slice":
		return nativeFunction(func(args []any) (any, error) {
			start := 0
			end := len(value.Items)
			if len(args) > 0 {
				start = jsSliceIndex(args[0], len(value.Items))
			}
			if len(args) > 1 {
				end = jsSliceIndex(args[1], len(value.Items))
			}
			if end < start {
				end = start
			}
			return &ArrayValue{Items: append([]any{}, value.Items[start:end]...)}, nil
		}), true
	case "flat":
		return nativeFunction(func(args []any) (any, error) {
			depth := 1
			if len(args) > 0 {
				depth = jsInteger(args[0])
			}
			return &ArrayValue{Items: flattenArray(value.Items, depth)}, nil
		}), true
	case "reverse":
		return nativeFunction(func(args []any) (any, error) {
			for left, right := 0, len(value.Items)-1; left < right; left, right = left+1, right-1 {
				value.Items[left], value.Items[right] = value.Items[right], value.Items[left]
			}
			return value, nil
		}), true
	case "sort":
		return nativeFunction(func(args []any) (any, error) {
			var sortErr error
			sort.SliceStable(value.Items, func(leftIndex int, rightIndex int) bool {
				if sortErr != nil {
					return false
				}
				left := value.Items[leftIndex]
				right := value.Items[rightIndex]
				if len(args) > 0 && !isNullish(args[0]) {
					compared, err := callFunctionWithValues(args[0], []any{left, right}, env, jsUndefined)
					if err != nil {
						sortErr = err
						return false
					}
					return toNumber(compared) < 0
				}
				return jsString(left) < jsString(right)
			})
			if sortErr != nil {
				return nil, sortErr
			}
			return value, nil
		}), true
	}
	return nil, false
}

func isJSWhitespace(value rune) bool {
	return value == ' ' || value == '\t' || value == '\n' || value == '\r' || value == '\f' || value == '\v'
}

func jsInteger(value any) int {
	number := toNumber(value)
	if math.IsNaN(number) || math.IsInf(number, 0) || number == 0 {
		return 0
	}
	return int(number)
}

func clampIndex(value int, length int) int {
	if value < 0 {
		return 0
	}
	if value > length {
		return length
	}
	return value
}

func jsSliceIndex(value any, length int) int {
	index := jsInteger(value)
	if index < 0 {
		return maxInt(length+index, 0)
	}
	return minInt(index, length)
}

func jsArrayStartIndex(value any, length int) int {
	index := jsInteger(value)
	if index < 0 {
		return maxInt(length+index, 0)
	}
	return minInt(index, length)
}

func stringRuneSlice(value string, start int, end int) string {
	runes := []rune(value)
	start = clampIndex(start, len(runes))
	end = clampIndex(end, len(runes))
	if end < start {
		end = start
	}
	return string(runes[start:end])
}

func byteIndexForRune(value string, runeIndex int) int {
	if runeIndex <= 0 {
		return 0
	}
	index := 0
	for byteIndex := range value {
		if index == runeIndex {
			return byteIndex
		}
		index++
	}
	return len(value)
}

func arrayIndexOf(value []any, search any, start int) int {
	for index := start; index < len(value); index++ {
		if jsSameValue(value[index], search) {
			return index
		}
	}
	return -1
}

func flattenArray(value []any, depth int) []any {
	if depth < 1 {
		return append([]any{}, value...)
	}
	result := []any{}
	for _, item := range value {
		if nested, ok := item.(*ArrayValue); ok {
			result = append(result, flattenArray(nested.Items, depth-1)...)
		} else {
			result = append(result, item)
		}
	}
	return result
}

func minInt(left int, right int) int {
	if left < right {
		return left
	}
	return right
}

func maxInt(left int, right int) int {
	if left > right {
		return left
	}
	return right
}

func mapMember(value *MapValue, property string) (any, bool) {
	switch property {
	case "size":
		return float64(len(value.Entries)), true
	case "get":
		return nativeFunction(func(args []any) (any, error) {
			if len(args) == 0 {
				return jsUndefined, nil
			}
			if index := mapIndex(value, args[0]); index >= 0 {
				return value.Entries[index].Value, nil
			}
			return jsUndefined, nil
		}), true
	case "set":
		return nativeFunction(func(args []any) (any, error) {
			if len(args) == 0 {
				return value, nil
			}
			next := any(jsUndefined)
			if len(args) > 1 {
				next = args[1]
			}
			mapSet(value, args[0], next)
			return value, nil
		}), true
	case "has":
		return nativeFunction(func(args []any) (any, error) {
			return len(args) > 0 && mapIndex(value, args[0]) >= 0, nil
		}), true
	case "delete":
		return nativeFunction(func(args []any) (any, error) {
			if len(args) == 0 {
				return false, nil
			}
			index := mapIndex(value, args[0])
			if index < 0 {
				return false, nil
			}
			value.Entries = append(value.Entries[:index], value.Entries[index+1:]...)
			return true, nil
		}), true
	case "clear":
		return nativeFunction(func(args []any) (any, error) {
			value.Entries = []MapEntry{}
			return jsUndefined, nil
		}), true
	case "keys":
		return nativeFunction(func(args []any) (any, error) {
			keys := []any{}
			for _, entry := range value.Entries {
				keys = append(keys, entry.Key)
			}
			return &IteratorValue{Values: keys}, nil
		}), true
	case "values":
		return nativeFunction(func(args []any) (any, error) {
			values := []any{}
			for _, entry := range value.Entries {
				values = append(values, entry.Value)
			}
			return &IteratorValue{Values: values}, nil
		}), true
	case "entries":
		return nativeFunction(func(args []any) (any, error) {
			entries := []any{}
			for _, entry := range value.Entries {
				entries = append(entries, &ArrayValue{Items: []any{entry.Key, entry.Value}})
			}
			return &IteratorValue{Values: entries}, nil
		}), true
	}
	return nil, false
}

func setMember(value *SetValue, property string) (any, bool) {
	switch property {
	case "size":
		return float64(len(value.Values)), true
	case "add":
		return nativeFunction(func(args []any) (any, error) {
			if len(args) > 0 {
				setAdd(value, args[0])
			}
			return value, nil
		}), true
	case "has":
		return nativeFunction(func(args []any) (any, error) {
			return len(args) > 0 && setIndex(value, args[0]) >= 0, nil
		}), true
	case "delete":
		return nativeFunction(func(args []any) (any, error) {
			if len(args) == 0 {
				return false, nil
			}
			index := setIndex(value, args[0])
			if index < 0 {
				return false, nil
			}
			value.Values = append(value.Values[:index], value.Values[index+1:]...)
			return true, nil
		}), true
	case "clear":
		return nativeFunction(func(args []any) (any, error) {
			value.Values = []any{}
			return jsUndefined, nil
		}), true
	case "values", "keys":
		return nativeFunction(func(args []any) (any, error) {
			return &IteratorValue{Values: append([]any{}, value.Values...)}, nil
		}), true
	case "entries":
		return nativeFunction(func(args []any) (any, error) {
			entries := []any{}
			for _, item := range value.Values {
				entries = append(entries, &ArrayValue{Items: []any{item, item}})
			}
			return &IteratorValue{Values: entries}, nil
		}), true
	}
	return nil, false
}

func iteratorMember(value *IteratorValue, property string) (any, bool) {
	switch property {
	case "Symbol.iterator", "Symbol.asyncIterator":
		return nativeFunction(func(args []any) (any, error) {
			return value, nil
		}), true
	case "next":
		return nativeFunction(func(args []any) (any, error) {
			if value.Index >= len(value.Values) {
				return map[string]any{"done": true, "value": jsUndefined}, nil
			}
			next := value.Values[value.Index]
			value.Index++
			return map[string]any{"done": false, "value": next}, nil
		}), true
	}
	return nil, false
}

func mapSet(value *MapValue, key any, next any) {
	if index := mapIndex(value, key); index >= 0 {
		value.Entries[index].Value = next
		return
	}
	value.Entries = append(value.Entries, MapEntry{Key: key, Value: next})
}

func mapIndex(value *MapValue, key any) int {
	for index, entry := range value.Entries {
		if jsSameValue(entry.Key, key) {
			return index
		}
	}
	return -1
}

func setAdd(value *SetValue, item any) {
	if setIndex(value, item) < 0 {
		value.Values = append(value.Values, item)
	}
}

func setIndex(value *SetValue, item any) int {
	for index, current := range value.Values {
		if jsSameValue(current, item) {
			return index
		}
	}
	return -1
}

func regexpTest(value *RegExpValue, text string) (bool, error) {
	if value.Regex != nil {
		return value.Regex.MatchString(text), nil
	}
	if value.Regex2 == nil {
		return false, nil
	}
	return value.Regex2.MatchString(text)
}

func regexpSplit(value *RegExpValue, text string) []string {
	if value.Regex != nil {
		return value.Regex.Split(text, -1)
	}
	matches, err := regexpFindAll(value, text)
	if err != nil || len(matches) == 0 {
		return []string{text}
	}
	parts := []string{}
	last := 0
	for _, match := range matches {
		if len(match.Index) < 2 || match.Index[0] < 0 {
			continue
		}
		parts = append(parts, text[last:match.Index[0]])
		last = match.Index[1]
		if match.Index[0] == match.Index[1] && last < len(text) {
			last++
		}
	}
	parts = append(parts, text[last:])
	return parts
}

func replaceRegExp(value string, search *RegExpValue, replacement any, env Env) (any, error) {
	matches, err := regexpFindAll(search, value)
	if err != nil {
		return nil, err
	}
	if len(matches) == 0 {
		return value, nil
	}
	if !search.Global && len(matches) > 1 {
		matches = matches[:1]
	}
	var out strings.Builder
	last := 0
	for _, match := range matches {
		if len(match.Index) < 2 || match.Index[0] < 0 {
			continue
		}
		start := match.Index[0]
		end := match.Index[1]
		out.WriteString(value[last:start])
		if _, ok := replacement.(FunctionValue); ok {
			args := regexpReplacementArgs(value, match.Index)
			next, err := callFunctionWithValues(replacement, args, env, jsUndefined)
			if err != nil {
				return nil, err
			}
			out.WriteString(jsString(next))
		} else if _, ok := replacement.(BoundFunctionValue); ok {
			args := regexpReplacementArgs(value, match.Index)
			next, err := callFunctionWithValues(replacement, args, env, jsUndefined)
			if err != nil {
				return nil, err
			}
			out.WriteString(jsString(next))
		} else if _, ok := replacement.(NativeFunctionValue); ok {
			args := regexpReplacementArgs(value, match.Index)
			next, err := callFunctionWithValues(replacement, args, env, jsUndefined)
			if err != nil {
				return nil, err
			}
			out.WriteString(jsString(next))
		} else {
			out.WriteString(expandRegExpReplacement(jsString(replacement), match.Groups))
		}
		last = end
	}
	out.WriteString(value[last:])
	return out.String(), nil
}

func expandRegExpReplacement(replacement string, groups []string) string {
	var out strings.Builder
	for index := 0; index < len(replacement); index++ {
		if replacement[index] != '$' || index+1 >= len(replacement) {
			out.WriteByte(replacement[index])
			continue
		}
		next := replacement[index+1]
		switch {
		case next == '$':
			out.WriteByte('$')
			index++
		case next == '&':
			if len(groups) > 0 {
				out.WriteString(groups[0])
			}
			index++
		case next >= '0' && next <= '9':
			groupIndex := int(next - '0')
			if index+2 < len(replacement) && replacement[index+2] >= '0' && replacement[index+2] <= '9' {
				candidate := groupIndex*10 + int(replacement[index+2]-'0')
				if candidate < len(groups) {
					groupIndex = candidate
					index++
				}
			}
			if groupIndex > 0 && groupIndex < len(groups) {
				out.WriteString(groups[groupIndex])
			}
			index++
		default:
			out.WriteByte(replacement[index])
		}
	}
	return out.String()
}

func regexpReplacementArgs(value string, match []int) []any {
	args := []any{}
	for index := 0; index < len(match); index += 2 {
		if match[index] < 0 || match[index+1] < 0 {
			args = append(args, jsUndefined)
		} else {
			args = append(args, value[match[index]:match[index+1]])
		}
	}
	return args
}

func evalClass(superExpr map[string]any, rawMethods []any, env Env) (*ClassValue, error) {
	classValue := &ClassValue{
		Methods:       map[string]FunctionValue{},
		Getters:       map[string]FunctionValue{},
		Setters:       map[string]FunctionValue{},
		Static:        map[string]FunctionValue{},
		StaticGetters: map[string]FunctionValue{},
		StaticSetters: map[string]FunctionValue{},
		Props:         map[string]any{},
	}
		if len(superExpr) > 0 {
			superValue, err := evalExpr(superExpr, env)
			if err != nil {
				return nil, err
			}
			if superClass, ok := superValue.(*ClassValue); ok {
				classValue.Super = superClass
			} else if !isConstructable(superValue) {
				return nil, errors.New("class extends target is not constructable")
			} else {
				classValue.SuperCtor = superValue
			}
		}
	for _, rawMethod := range rawMethods {
		method := asMap(rawMethod)
		function := FunctionValue{
			Params:    asStringSlice(method["params"]),
				RestParam: asString(method["restParam"]),
				Body:      asStmtSlice(method["body"]),
				Env:       env,
				Async:     method["async"] == true,
				Generator: method["generator"] == true,
			}
		name := asString(method["name"])
		switch asString(method["kind"]) {
		case "constructor":
			classValue.Constructor = &function
		case "method":
			if method["isStatic"] == true {
				classValue.Static[name] = function
			} else {
				classValue.Methods[name] = function
			}
		case "getter":
			if method["isStatic"] == true {
				classValue.StaticGetters[name] = function
			} else {
				classValue.Getters[name] = function
			}
		case "setter":
			if method["isStatic"] == true {
				classValue.StaticSetters[name] = function
			} else {
				classValue.Setters[name] = function
			}
		default:
			return nil, fmt.Errorf("unsupported class method %s", asString(method["kind"]))
		}
	}
	return classValue, nil
}

func constructValue(raw any, rawArgs []any, callerEnv Env) (any, error) {
	classValue, ok := raw.(*ClassValue)
	if !ok {
		if function, functionOk := raw.(FunctionValue); functionOk {
			instance := objectWithPrototype(function.Props["prototype"])
			result, err := callFunctionWithThis(function, rawArgs, callerEnv, instance)
			if err != nil {
				return nil, err
			}
			if resultMap, ok := result.(map[string]any); ok {
				return resultMap, nil
			}
			return instance, nil
		}
		if bound, boundOk := raw.(BoundFunctionValue); boundOk {
			instance := objectWithPrototype(bound.Function.Props["prototype"])
			args := append([]any{}, bound.Args...)
			callArgs, err := evalCallArgs(rawArgs, callerEnv)
			if err != nil {
				return nil, err
			}
			args = append(args, callArgs...)
			result, err := callFunctionWithThisValues(bound.Function, args, instance)
			if err != nil {
				return nil, err
			}
			if resultMap, ok := result.(map[string]any); ok {
				return resultMap, nil
			}
			return instance, nil
		}
		if _, nativeOk := raw.(NativeFunctionValue); nativeOk {
			return callFunction(raw, rawArgs, callerEnv)
		}
		if object, objectOk := raw.(map[string]any); objectOk {
			if callable, callableOk := object["__call"]; callableOk {
				return callFunction(callable, rawArgs, callerEnv)
			}
		}
		return nil, fmt.Errorf("constructor is not callable: %T %s", raw, jsInspect(raw))
	}
	var instance any = map[string]any{"__class": classValue}
	if classValue.SuperCtor != nil {
		superInstance, err := constructValue(classValue.SuperCtor, rawArgs, callerEnv)
		if err != nil {
			return nil, err
		}
		if superMap, ok := superInstance.(map[string]any); ok {
			superMap["__class"] = classValue
		}
		instance = superInstance
	}
	if classValue.Constructor != nil {
		result, err := callFunctionWithThis(*classValue.Constructor, rawArgs, callerEnv, instance)
		if err != nil {
			return nil, err
		}
		if resultMap, ok := result.(map[string]any); ok {
			return resultMap, nil
		}
		if resultArray, ok := result.(*ArrayValue); ok {
			return resultArray, nil
		}
	} else if classValue.Super != nil && classValue.Super.Constructor != nil {
		result, err := callFunctionWithThis(*classValue.Super.Constructor, rawArgs, callerEnv, instance)
		if err != nil {
			return nil, err
		}
		if resultMap, ok := result.(map[string]any); ok {
			return resultMap, nil
		}
		if resultArray, ok := result.(*ArrayValue); ok {
			return resultArray, nil
		}
	}
	return instance, nil
}

func constructClassWithValues(classValue *ClassValue, args []any) (any, error) {
	var instance any = map[string]any{"__class": classValue}
	if classValue.Constructor != nil {
		result, err := callFunctionWithThisValues(*classValue.Constructor, args, instance)
		if err != nil {
			return nil, err
		}
		if resultMap, ok := result.(map[string]any); ok {
			return resultMap, nil
		}
		if resultArray, ok := result.(*ArrayValue); ok {
			return resultArray, nil
		}
	}
	return instance, nil
}

func isConstructable(raw any) bool {
	switch typed := raw.(type) {
	case *ClassValue, FunctionValue, NativeFunctionValue:
		return true
	case map[string]any:
		_, ok := typed["__call"]
		return ok
	default:
		return false
	}
}

func callFunction(raw any, rawArgs []any, callerEnv Env) (any, error) {
	switch function := raw.(type) {
	case FunctionValue:
		return callFunctionWithThis(function, rawArgs, callerEnv, jsUndefined)
	case BoundFunctionValue:
		args := append([]any{}, function.Args...)
		callArgs, err := evalCallArgs(rawArgs, callerEnv)
		if err != nil {
			return nil, err
		}
		args = append(args, callArgs...)
		return callFunctionWithThisValues(function.Function, args, function.This)
	case NativeFunctionValue:
		args, err := evalCallArgs(rawArgs, callerEnv)
		if err != nil {
			return nil, err
		}
		return function.Call(args)
	case *ClassValue:
		if !function.Callable {
			return nil, fmt.Errorf("class constructor cannot be invoked without new: %s", jsInspect(raw))
		}
		args, err := evalCallArgs(rawArgs, callerEnv)
		if err != nil {
			return nil, err
		}
		return constructClassWithValues(function, args)
	case map[string]any:
		if callable, ok := function["__call"]; ok {
			return callFunction(callable, rawArgs, callerEnv)
		}
		return nil, fmt.Errorf("callee is not callable: %T %s", raw, jsInspect(raw))
	default:
		return nil, fmt.Errorf("callee is not callable: %T %s", raw, jsInspect(raw))
	}
}

func callFunctionWithValues(raw any, args []any, callerEnv Env, thisValue any) (any, error) {
	switch function := raw.(type) {
	case FunctionValue:
		return callFunctionWithThisValues(function, args, thisValue)
	case BoundFunctionValue:
		callArgs := append([]any{}, function.Args...)
		callArgs = append(callArgs, args...)
		return callFunctionWithThisValues(function.Function, callArgs, function.This)
	case NativeFunctionValue:
		if function.CallWithThis != nil {
			return function.CallWithThis(thisValue, args)
		}
		return function.Call(args)
	case *ClassValue:
		if !function.Callable {
			return nil, fmt.Errorf("class constructor cannot be invoked without new: %s", jsInspect(raw))
		}
		return constructClassWithValues(function, args)
	case map[string]any:
		if callable, ok := function["__call"]; ok {
			return callFunctionWithValues(callable, args, callerEnv, thisValue)
		}
		return nil, fmt.Errorf("callee is not callable: %T %s", raw, jsInspect(raw))
	default:
		return nil, fmt.Errorf("callee is not callable: %T %s", raw, jsInspect(raw))
	}
}

func callFunctionWithThis(function FunctionValue, rawArgs []any, callerEnv Env, thisValue any) (any, error) {
	args, err := evalCallArgs(rawArgs, callerEnv)
	if err != nil {
		return nil, err
	}
	return callFunctionWithThisValues(function, args, thisValue)
}

func evalCallArgs(rawArgs []any, env Env) ([]any, error) {
	args := []any{}
	for _, rawArg := range rawArgs {
		argExpr := asMap(rawArg)
		if argExpr["kind"] == "spread" {
			spreadValue, err := evalExpr(asMap(argExpr["arg"]), env)
			if err != nil {
				return nil, err
			}
			args = append(args, iterableValues(spreadValue)...)
			continue
		}
		value, err := evalExpr(argExpr, env)
		if err != nil {
			return nil, err
		}
		args = append(args, value)
	}
	return args, nil
}

func callFunctionWithThisValues(function FunctionValue, args []any, thisValue any) (any, error) {
	effectiveThis := thisValue
	if function.LexicalThis {
		effectiveThis = lookupEnv(function.Env, "this")
	}
	child := Env{"__parent": function.Env, "this": effectiveThis}
	child["arguments"] = &ArrayValue{Items: append([]any{}, args...)}
	for index, param := range function.Params {
		value := any(jsUndefined)
		if index < len(args) {
			value = args[index]
		}
		child[param] = value
	}
	if function.RestParam != "" {
		rest := []any{}
		if len(args) > len(function.Params) {
			rest = append(rest, args[len(function.Params):]...)
		}
		child[function.RestParam] = &ArrayValue{Items: rest}
	}
		result, err := evalStmtList(function.Body, child)
		if err != nil {
			if function.Async {
				var pending pendingAwait
				if errors.As(err, &pending) {
					return pending.promise, nil
				}
				return promiseRejectedFromError(err), nil
			}
			return nil, err
		}
	if function.Generator {
		return &IteratorValue{Values: result.yields}, nil
	}
	if function.Async {
		if result.returned {
			return promiseFulfilled(result.value), nil
		}
		return promiseFulfilled(jsUndefined), nil
	}
	if result.returned {
		return result.value, nil
	}
	return nil, nil
}

func builtinErrorClass(name string) *ClassValue {
	constructor := FunctionValue{
		Params: []string{"message"},
		Body: []map[string]any{
			{
				"kind": "expr",
				"expr": map[string]any{
					"kind": "assign",
					"op":   "=",
					"left": map[string]any{
						"kind":     "member",
						"object":   map[string]any{"kind": "this"},
						"property": "name",
					},
					"right": map[string]any{"kind": "value", "value": map[string]any{"kind": "string", "value": name}},
				},
			},
			{
				"kind": "expr",
				"expr": map[string]any{
					"kind": "assign",
					"op":   "=",
					"left": map[string]any{
						"kind":     "member",
						"object":   map[string]any{"kind": "this"},
						"property": "message",
					},
					"right": map[string]any{"kind": "ident", "name": "message"},
				},
			},
		},
		Env: Env{},
	}
	return &ClassValue{
		Constructor: &constructor,
		Methods:     map[string]FunctionValue{},
		Getters:     map[string]FunctionValue{},
		Setters:     map[string]FunctionValue{},
		Static:      map[string]FunctionValue{},
		StaticGetters: map[string]FunctionValue{},
		StaticSetters: map[string]FunctionValue{},
		Callable:    true,
		Props:       map[string]any{},
	}
}

func lookupMethod(classValue *ClassValue, property string) (FunctionValue, bool) {
	for current := classValue; current != nil; current = current.Super {
		if method, ok := current.Methods[property]; ok {
			return method, true
		}
	}
	return FunctionValue{}, false
}

func lookupGetter(classValue *ClassValue, property string) (FunctionValue, bool) {
	for current := classValue; current != nil; current = current.Super {
		if getter, ok := current.Getters[property]; ok {
			return getter, true
		}
	}
	return FunctionValue{}, false
}

func lookupSetter(classValue *ClassValue, property string) (FunctionValue, bool) {
	for current := classValue; current != nil; current = current.Super {
		if setter, ok := current.Setters[property]; ok {
			return setter, true
		}
	}
	return FunctionValue{}, false
}

func lookupStaticSetter(classValue *ClassValue, property string) (FunctionValue, bool) {
	for current := classValue; current != nil; current = current.Super {
		if setter, ok := current.StaticSetters[property]; ok {
			return setter, true
		}
	}
	return FunctionValue{}, false
}

func lookupEnv(env Env, name string) any {
	if value, ok := env[name]; ok {
		return value
	}
	if parent, ok := env["__parent"].(Env); ok {
		return lookupEnv(parent, name)
	}
	return jsUndefined
}

func assignEnv(env Env, name string, value any) {
	if _, ok := env[name]; ok {
		env[name] = value
		return
	}
	if parent, ok := env["__parent"].(Env); ok && hasEnvBinding(parent, name) {
		assignEnv(parent, name, value)
		return
	}
	env[name] = value
}

func hasEnvBinding(env Env, name string) bool {
	if _, ok := env[name]; ok {
		return true
	}
	if parent, ok := env["__parent"].(Env); ok {
		return hasEnvBinding(parent, name)
	}
	return false
}

func evalValue(value map[string]any) (any, error) {
	switch value["kind"] {
	case "undefined":
		return jsUndefined, nil
	case "null":
		return jsNull, nil
	case "bool":
		return value["value"] == true, nil
	case "number":
		number, err := parseJSNumberLiteral(asString(value["value"]))
		if err != nil {
			return nil, err
		}
		return number, nil
	case "string", "bigint":
		return asString(value["value"]), nil
	case "regexp":
		return newRegExp(asString(value["pattern"]), asString(value["flags"]))
	default:
		return nil, fmt.Errorf("unsupported value %v", value["kind"])
	}
}

func evalUnary(op string, arg any) (any, error) {
	switch op {
	case "!":
		return !isTruthy(arg), nil
	case "+":
		return toNumber(arg), nil
	case "-":
		return -toNumber(arg), nil
	case "~":
		return float64(^toInt32(arg)), nil
	case "typeof":
		return jsTypeof(arg), nil
	case "void":
		return jsUndefined, nil
	default:
		return nil, fmt.Errorf("unsupported unary %s", op)
	}
}

func evalBinary(op string, left any, right any) (any, error) {
	switch op {
	case "+":
		if _, ok := left.(string); ok {
			return jsString(left) + jsString(right), nil
		}
		if _, ok := right.(string); ok {
			return jsString(left) + jsString(right), nil
		}
		return toNumber(left) + toNumber(right), nil
	case "-":
		return toNumber(left) - toNumber(right), nil
	case "*":
		return toNumber(left) * toNumber(right), nil
	case "**":
		return math.Pow(toNumber(left), toNumber(right)), nil
	case "/":
		return toNumber(left) / toNumber(right), nil
	case "%":
		return math.Mod(toNumber(left), toNumber(right)), nil
	case "&":
		return float64(toInt32(left) & toInt32(right)), nil
	case "|":
		return float64(toInt32(left) | toInt32(right)), nil
	case "^":
		return float64(toInt32(left) ^ toInt32(right)), nil
	case "<<":
		return float64(toInt32(left) << (toUint32(right) & 31)), nil
	case ">>":
		return float64(toInt32(left) >> (toUint32(right) & 31)), nil
	case ">>>":
		return float64(toUint32(left) >> (toUint32(right) & 31)), nil
	case "in":
		return hasProperty(right, jsPropertyKey(left)), nil
	case "instanceof":
		return jsInstanceOf(left, right), nil
	case "==":
		return jsLooseEqual(left, right), nil
	case "===":
		return jsSameValue(left, right), nil
	case "!=":
		return !jsLooseEqual(left, right), nil
	case "!==":
		return !jsSameValue(left, right), nil
	case "<":
		return toNumber(left) < toNumber(right), nil
	case "<=":
		return toNumber(left) <= toNumber(right), nil
	case ">":
		return toNumber(left) > toNumber(right), nil
	case ">=":
		return toNumber(left) >= toNumber(right), nil
	default:
		return nil, fmt.Errorf("unsupported binary %s", op)
	}
}

func isConsoleLog(callee map[string]any) bool {
	if callee["kind"] != "member" || asString(callee["property"]) != "log" {
		return false
	}
	object := asMap(callee["object"])
	return object["kind"] == "ident" && asString(object["name"]) == "console"
}

func exprLabel(expr map[string]any) string {
	switch asString(expr["kind"]) {
	case "ident":
		return asString(expr["name"])
	case "member":
		return exprLabel(asMap(expr["object"])) + "." + asString(expr["property"])
	default:
		return asString(expr["kind"])
	}
}

func isArrayPush(callee map[string]any) bool {
	return callee["kind"] == "member" && asString(callee["property"]) == "push"
}

func isArrayPop(callee map[string]any) bool {
	return callee["kind"] == "member" && asString(callee["property"]) == "pop"
}

func iterableValues(value any) []any {
	switch typed := value.(type) {
	case *ArrayValue:
		return append([]any{}, typed.Items...)
	case *IteratorValue:
		if typed.Index >= len(typed.Values) {
			return []any{}
		}
		values := append([]any{}, typed.Values[typed.Index:]...)
		typed.Index = len(typed.Values)
		return values
	case *MapValue:
		values := []any{}
		for _, entry := range typed.Entries {
			values = append(values, &ArrayValue{Items: []any{entry.Key, entry.Value}})
		}
		return values
	case *SetValue:
		return append([]any{}, typed.Values...)
	case map[string]any:
		for _, key := range []string{"Symbol.iterator", "Symbol.asyncIterator"} {
			if iterator, ok := lookupObjectProperty(typed, key); ok && isCallable(iterator) {
				values, err := callFunctionWithValues(iterator, []any{}, Env{}, typed)
				if err == nil {
					if iteratorValue, ok := values.(*IteratorValue); ok {
						return iterableValues(iteratorValue)
					}
					if arrayValue, ok := values.(*ArrayValue); ok {
						return append([]any{}, arrayValue.Items...)
					}
				}
			}
		}
		values := []any{}
		for _, item := range typed {
			values = append(values, item)
		}
		return values
	default:
		return nil
	}
}

func stringsToAny(values []string) []any {
	out := make([]any, 0, len(values))
	for _, value := range values {
		out = append(out, value)
	}
	return out
}

func isTruthy(value any) bool {
	switch typed := value.(type) {
	case nil, UndefinedValue, NullValue:
		return false
	case bool:
		return typed
	case string:
		return typed != ""
	case float64:
		return typed != 0 && !math.IsNaN(typed)
	default:
		return true
	}
}

func jsSameValue(left any, right any) bool {
	switch leftTyped := left.(type) {
	case UndefinedValue:
		_, ok := right.(UndefinedValue)
		return ok
	case NullValue:
		_, ok := right.(NullValue)
		return ok
	case float64:
		rightTyped, ok := right.(float64)
		return ok && leftTyped == rightTyped
	case string:
		rightTyped, ok := right.(string)
		return ok && leftTyped == rightTyped
	case bool:
		rightTyped, ok := right.(bool)
		return ok && leftTyped == rightTyped
	case *MapValue:
		rightTyped, ok := right.(*MapValue)
		return ok && leftTyped == rightTyped
	case *SetValue:
		rightTyped, ok := right.(*SetValue)
		return ok && leftTyped == rightTyped
	case *IteratorValue:
		rightTyped, ok := right.(*IteratorValue)
		return ok && leftTyped == rightTyped
	case *SymbolValue:
		rightTyped, ok := right.(*SymbolValue)
		return ok && leftTyped == rightTyped
	case map[string]any:
		_, ok := right.(map[string]any)
		return ok && referenceIdentity(left) == referenceIdentity(right)
	case *ArrayValue:
		_, ok := right.(*ArrayValue)
		return ok && referenceIdentity(left) == referenceIdentity(right)
	case FunctionValue:
		_, ok := right.(FunctionValue)
		return ok && referenceIdentity(left) == referenceIdentity(right)
	case BoundFunctionValue:
		_, ok := right.(BoundFunctionValue)
		return ok && referenceIdentity(left) == referenceIdentity(right)
	case NativeFunctionValue:
		_, ok := right.(NativeFunctionValue)
		return ok && referenceIdentity(left) == referenceIdentity(right)
	case *ClassValue:
		_, ok := right.(*ClassValue)
		return ok && referenceIdentity(left) == referenceIdentity(right)
	default:
		return fmt.Sprintf("%p", &left) == fmt.Sprintf("%p", &right)
	}
}

func referenceIdentity(value any) uintptr {
	reflectValue := reflect.ValueOf(value)
	switch reflectValue.Kind() {
	case reflect.Map, reflect.Slice, reflect.Func, reflect.Pointer:
		if reflectValue.IsNil() {
			return 0
		}
		return reflectValue.Pointer()
	default:
		return 0
	}
}

func jsLooseEqual(left any, right any) bool {
	if isNullish(left) && isNullish(right) {
		return true
	}
	if jsSameValue(left, right) {
		return true
	}
	switch left.(type) {
	case string, float64, bool:
		switch right.(type) {
		case string, float64, bool:
			return toNumber(left) == toNumber(right)
		}
	}
	return false
}

func isNullish(value any) bool {
	switch value.(type) {
	case nil, UndefinedValue, NullValue:
		return true
	default:
		return false
	}
}

func isUndefined(value any) bool {
	switch value.(type) {
	case nil, UndefinedValue:
		return true
	default:
		return false
	}
}

func jsPropertyKey(value any) string {
	if symbol, ok := value.(*SymbolValue); ok {
		return symbol.Description
	}
	return jsString(value)
}

func hasProperty(value any, key string) bool {
	switch typed := value.(type) {
	case map[string]any:
		_, ok := lookupObjectProperty(typed, key)
		return ok
	case *ArrayValue:
		if key == "length" || key == "Symbol.iterator" {
			return true
		}
		index, err := strconv.Atoi(key)
		if err == nil && index >= 0 && index < len(typed.Items) {
			return true
		}
		_, ok := typed.Props[key]
		return ok
	case FunctionValue:
		if key == "length" || key == "name" || key == "prototype" {
			return true
		}
		_, ok := typed.Props[key]
		return ok
	case BoundFunctionValue:
		if key == "length" || key == "name" {
			return true
		}
		return false
		case NativeFunctionValue:
			if key == "length" || key == "name" {
				return true
			}
			_, ok := typed.Props[key]
			return ok
		case *IteratorValue:
			return key == "next" || key == "Symbol.iterator" || key == "Symbol.asyncIterator"
		case *RegExpValue:
			if key == "lastIndex" {
				return true
			}
		_, ok := typed.Props[key]
		return ok
	default:
		return false
	}
}

func jsInstanceOf(value any, constructor any) bool {
	if classValue, ok := constructor.(*ClassValue); ok {
		object, ok := value.(map[string]any)
		if !ok {
			return false
		}
		instanceClass, ok := object["__class"].(*ClassValue)
		if !ok {
			return false
		}
		for current := instanceClass; current != nil; current = current.Super {
			if current == classValue {
				return true
			}
		}
		return false
	}
	if function, ok := constructor.(FunctionValue); ok {
		return objectHasPrototype(value, function.Props["prototype"])
	}
	return false
}

func objectHasPrototype(value any, prototype any) bool {
	object, ok := value.(map[string]any)
	if !ok || isNullish(prototype) {
		return false
	}
	for current, ok := object["__prototype"]; ok; {
		if jsSameValue(current, prototype) {
			return true
		}
		currentObject, ok := current.(map[string]any)
		if !ok {
			return false
		}
		current, ok = currentObject["__prototype"]
	}
	return false
}

func toNumber(value any) float64 {
	switch typed := value.(type) {
	case nil, NullValue:
		return 0
	case UndefinedValue:
		return math.NaN()
	case float64:
		return typed
	case bool:
		if typed {
			return 1
		}
		return 0
	case string:
		number, err := parseJSNumberLiteral(strings.TrimSpace(typed))
		if err != nil {
			return math.NaN()
		}
		return number
	default:
		return math.NaN()
	}
}

func parseJSNumberLiteral(raw string) (float64, error) {
	text := strings.TrimSpace(raw)
	if text == "" {
		return 0, nil
	}
	sign := 1.0
	if strings.HasPrefix(text, "+") || strings.HasPrefix(text, "-") {
		if text[0] == '-' {
			sign = -1
		}
		text = text[1:]
	}
	lower := strings.ToLower(text)
	for _, prefixed := range []struct {
		prefix string
		base   int
	}{
		{"0b", 2},
		{"0o", 8},
		{"0x", 16},
	} {
		if strings.HasPrefix(lower, prefixed.prefix) {
			if sign < 0 {
				return math.NaN(), nil
			}
			integer, err := strconv.ParseUint(lower[len(prefixed.prefix):], prefixed.base, 64)
			if err != nil {
				return math.NaN(), nil
			}
			return float64(integer), nil
		}
	}
	number, err := strconv.ParseFloat(text, 64)
	if err != nil {
		return math.NaN(), nil
	}
	return sign * number, nil
}

func toInt32(value any) int32 {
	return int32(toUint32(value))
}

func toUint32(value any) uint32 {
	number := toNumber(value)
	if math.IsNaN(number) || math.IsInf(number, 0) || number == 0 {
		return 0
	}
	return uint32(int64(number))
}

func jsString(value any) string {
	switch typed := value.(type) {
	case nil, UndefinedValue:
		return "undefined"
	case NullValue:
		return "null"
	case string:
		return typed
	case *SymbolValue:
		if typed.Description == "" {
			return "Symbol()"
		}
		return "Symbol(" + typed.Description + ")"
	case float64:
		if math.Trunc(typed) == typed {
			return strconv.FormatInt(int64(typed), 10)
		}
		return strconv.FormatFloat(typed, 'f', -1, 64)
	case bool:
		if typed {
			return "true"
		}
		return "false"
	case *ArrayValue:
		parts := []string{}
		for _, item := range typed.Items {
			if isNullish(item) {
				parts = append(parts, "")
			} else {
				parts = append(parts, jsString(item))
			}
		}
		return strings.Join(parts, ",")
	case map[string]any:
		if message, ok := typed["message"]; ok {
			if name, ok := typed["name"]; ok {
				return jsString(name) + ": " + jsString(message)
			}
			return jsString(message)
		}
		return objectTag(typed)
	case *RegExpValue:
		return "/" + typed.Pattern + "/" + typed.Flags
	case *DateValue:
		return formatDateISO(typed.Time)
	case *MapValue:
		return "[object Map]"
	case *SetValue:
		return "[object Set]"
	case *IteratorValue:
		return "[object Iterator]"
	case FunctionValue, BoundFunctionValue, NativeFunctionValue:
		return "function"
	case *ClassValue:
		return "class"
	default:
		return "[object Object]"
	}
}

func jsInspect(value any) string {
	switch typed := value.(type) {
	case string:
		return strconv.Quote(typed)
	case UndefinedValue:
		return "undefined"
	case NullValue:
		return "null"
	default:
		return jsString(typed)
	}
}

func jsFormat(args []any) string {
	format, ok := args[0].(string)
	if !ok {
		parts := []string{}
		for _, arg := range args {
			parts = append(parts, jsInspect(arg))
		}
		return strings.Join(parts, " ")
	}
	out := strings.Builder{}
	argIndex := 1
	for index := 0; index < len(format); index++ {
		if format[index] != '%' || index+1 >= len(format) {
			out.WriteByte(format[index])
			continue
		}
		verb := format[index+1]
		if verb == '%' {
			out.WriteByte('%')
			index++
			continue
		}
		if argIndex >= len(args) {
			out.WriteByte(format[index])
			continue
		}
		arg := args[argIndex]
		argIndex++
		switch verb {
		case 's':
			out.WriteString(jsString(arg))
		case 'd', 'i', 'f':
			out.WriteString(jsString(toNumber(arg)))
		case 'j', 'o', 'O':
			out.WriteString(jsInspect(arg))
		default:
			out.WriteByte(format[index])
			argIndex--
			continue
		}
		index++
	}
	for ; argIndex < len(args); argIndex++ {
		out.WriteByte(' ')
		out.WriteString(jsInspect(args[argIndex]))
	}
	return out.String()
}

func jsTypeof(value any) string {
	switch value.(type) {
	case nil, UndefinedValue:
		return "undefined"
	case NullValue:
		return "object"
	case bool:
		return "boolean"
	case float64:
		return "number"
	case string:
		return "string"
	case *SymbolValue:
		return "symbol"
	case FunctionValue, BoundFunctionValue, NativeFunctionValue, *ClassValue:
		return "function"
	case map[string]any:
		if _, ok := value.(map[string]any)["__call"]; ok {
			return "function"
		}
		return "object"
	default:
		return "object"
	}
}

func asMap(value any) map[string]any {
	if typed, ok := value.(map[string]any); ok {
		return typed
	}
	return map[string]any{}
}

func cloneStmtMap(value map[string]any) map[string]any {
	out := map[string]any{}
	for key, item := range value {
		out[key] = item
	}
	return out
}

func optionalExpr(source map[string]any, key string) map[string]any {
	if raw, ok := source[key]; ok {
		return asMap(raw)
	}
	return map[string]any{}
}

func asSlice(value any) []any {
	if typed, ok := value.([]any); ok {
		return typed
	}
	return nil
}

func asStmtSlice(value any) []map[string]any {
	out := []map[string]any{}
	for _, item := range asSlice(value) {
		out = append(out, asMap(item))
	}
	return out
}

func asString(value any) string {
	if typed, ok := value.(string); ok {
		return typed
	}
	return ""
}

func asStringSlice(value any) []string {
	out := []string{}
	for _, item := range asSlice(value) {
		out = append(out, asString(item))
	}
	return out
}
"#;
    source.replacen(
        ")\n\nfunc FailClosedReport",
        &format!(
            ")\n\n{}\nfunc FailClosedReport",
            render_runtime_contract_go_metadata()
        ),
        1,
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
    let json_literal = go_raw_string_literal(&json);
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

fn go_raw_string_literal(value: &str) -> String {
    format!("`{}`", value.replace('`', "` + \"`\" + `"))
}
