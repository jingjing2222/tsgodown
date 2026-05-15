use serde::{Deserialize, Serialize};

use crate::analyze;
use crate::contract::{AnalyzeRequest, AnalyzeResponse, Diagnostic};
use crate::runtime_contract::{
    fail_closed_report_version, unsupported_codegen_diagnostic, unsupported_executable_features,
    ProgramPurpose,
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
    let analyzed = analyze(request.analyze);
    let mut diagnostics = analyzed.diagnostics.clone();
    let package_name = sanitize_package_name(request.package_name.as_deref().unwrap_or("main"));
    let module_path = sanitize_module_path(
        request
            .module_path
            .as_deref()
            .unwrap_or("example.com/tsgodown-generated"),
    );
    let unsupported_features = unsupported_executable_features(&analyzed.ir);
    let can_emit_executable = diagnostics.is_empty() && unsupported_features.is_empty();
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

    EmitGoResponse {
        version: "engine-core.emit-go.v1".to_string(),
        target_backend: "go".to_string(),
        files,
        diagnostics,
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
    format!(
        r#"package {package_name}

import (
	"fmt"
	"os"

	"{module_path}/tsgodownrt"
)

func main() {{
	if err := tsgodownrt.RunProgram({program_json:?}); err != nil {{
		fmt.Fprintln(os.Stderr, err)
		os.Exit(1)
	}}
}}
"#
    )
}

fn render_runtime_package() -> String {
    r#"package tsgodownrt

import (
	"encoding/json"
	"errors"
	"fmt"
	"math"
	"os"
	"path/filepath"
	"runtime"
	"strconv"
	"strings"
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
	Body   []map[string]any
	Env    Env
}

type ClassValue struct {
	Constructor *FunctionValue
	Methods     map[string]FunctionValue
	Getters     map[string]FunctionValue
	Static      map[string]FunctionValue
	StaticGetters map[string]FunctionValue
	Super       *ClassValue
}

type BoundFunctionValue struct {
	Function FunctionValue
	This     any
}

type NativeFunctionValue struct {
	Call func(args []any) (any, error)
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
}

type moduleState struct {
	exports   map[string]any
	evaluated bool
	evaluating bool
}

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
	module, ok := entryModule(program)
	if !ok {
		return errors.New("entry module not found")
	}
	cache := map[string]*moduleState{}
	_, err := executeModule(module, program, cache)
	return err
}

func executeModule(module Module, program Program, cache map[string]*moduleState) (map[string]any, error) {
	if state, ok := cache[module.SourcePath]; ok {
		if state.evaluated || state.evaluating {
			return state.exports, nil
		}
	}
	exports := map[string]any{}
	state := &moduleState{exports: exports, evaluating: true}
	cache[module.SourcePath] = state
	env := Env{
		"exports": exports,
		"Error":   builtinErrorClass(),
		"Number": nativeFunction(func(args []any) (any, error) {
			if len(args) == 0 {
				return float64(0), nil
			}
			return toNumber(args[0]), nil
		}),
		"process": processObject(),
		"module": map[string]any{
			"exports": exports,
		},
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
		return nil, fmt.Errorf("module import %s is not resolved", spec)
	}}
	env["import"] = NativeFunctionValue{Call: func(args []any) (any, error) {
		return dynamicImportThenable(), nil
	}}
	for _, importDecl := range module.Imports {
		importedExports, ok := builtinModuleExports(importDecl.Spec)
		if !ok {
			importedModule, moduleOk := moduleByID(program, importDecl.Resolved)
			if !moduleOk {
				return nil, fmt.Errorf("module import %s is not resolved", importDecl.Spec)
			}
			var err error
			importedExports, err = executeModule(importedModule, program, cache)
			if err != nil {
				return nil, err
			}
		}
		bindImport(env, importDecl, importedExports)
	}
	for _, stmt := range module.Executable.Stmts {
		if result, err := evalStmt(stmt, env); err != nil {
			return nil, err
		} else if result.returned {
			return nil, errors.New("top-level return is not supported")
		} else if result.broke || result.continued {
			return nil, errors.New("break/continue outside loop")
		}
	}
	if moduleObject, ok := env["module"].(map[string]any); ok {
		if moduleExports, ok := moduleObject["exports"].(map[string]any); ok {
			exports = moduleExports
			state.exports = exports
		}
	}
	for _, name := range module.Exports {
		if value, ok := env[name]; ok {
			exports[name] = value
		}
	}
	state.evaluating = false
	state.evaluated = true
	return exports, nil
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

func builtinModuleExports(spec string) (map[string]any, bool) {
	switch spec {
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
		exports["default"] = exports
		return exports, true
	case "path", "node:path":
		return pathModuleExports(), true
	case "os", "node:os":
		return osModuleExports(), true
	case "node:diagnostics_channel":
		return diagnosticsChannelModuleExports(), true
	case "fs", "node:fs":
		return fsModuleExports(), true
	case "node:module":
		return moduleModuleExports(), true
	default:
		return nil, false
	}
}

func nativeFunction(call func(args []any) (any, error)) NativeFunctionValue {
	return NativeFunctionValue{Call: call}
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
	exports["constants"] = map[string]any{}
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
	exports["default"] = exports
	return exports
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

func processObject() map[string]any {
	return map[string]any{
		"env":      map[string]any{},
		"version":  "v20.0.0",
		"versions": map[string]any{"node": "20.0.0"},
		"platform": runtime.GOOS,
		"cwd": nativeFunction(func(args []any) (any, error) {
			cwd, err := os.Getwd()
			if err != nil {
				return "", nil
			}
			return cwd, nil
		}),
		"emitWarning": nativeFunction(func(args []any) (any, error) {
			return jsUndefined, nil
		}),
	}
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

func bindImport(env Env, importDecl Import, importedExports map[string]any) {
	for _, binding := range importDecl.Bindings {
		switch binding.Kind {
		case "named":
			env[binding.Local] = importedExports[binding.Imported]
		case "default":
			if value, ok := importedExports["default"]; ok {
				env[binding.Local] = value
			} else {
				env[binding.Local] = importedExports
			}
		case "namespace":
			env[binding.Local] = importedExports
		case "require":
			env[binding.Local] = importedExports
		case "destructure":
			env[binding.Local] = importedExports[binding.Imported]
		}
	}
}

func evalStmt(stmt map[string]any, env Env) (completion, error) {
	switch stmt["kind"] {
	case "expr":
		_, err := evalExpr(asMap(stmt["expr"]), env)
		return completion{}, err
	case "function-decl":
		env[asString(stmt["name"])] = FunctionValue{
			Params: asStringSlice(stmt["params"]),
			Body:   asStmtSlice(stmt["body"]),
			Env:    env,
		}
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
		for _, child := range branch {
			result, err := evalStmt(child, env)
			if err != nil {
				return completion{}, err
			}
			if result.returned || result.broke || result.continued {
				return result, nil
			}
		}
		return completion{}, nil
	case "for-of":
		iterable, err := evalExpr(asMap(stmt["right"]), env)
		if err != nil {
			return completion{}, err
		}
		for _, value := range iterableValues(iterable) {
			env[asString(stmt["left"])] = value
			for _, child := range asStmtSlice(stmt["body"]) {
				result, err := evalStmt(child, env)
				if err != nil {
					return completion{}, err
				}
				if result.returned {
					return result, nil
				}
				if result.broke {
					return completion{}, nil
				}
				if result.continued {
					break
				}
			}
		}
		return completion{}, nil
	case "for":
		for _, init := range asStmtSlice(stmt["init"]) {
			result, err := evalStmt(init, env)
			if err != nil {
				return completion{}, err
			}
			if result.returned || result.broke || result.continued {
				return completion{}, errors.New("invalid for initializer completion")
			}
		}
		for {
			if rawTest, ok := stmt["test"]; ok {
				test, err := evalExpr(asMap(rawTest), env)
				if err != nil {
					return completion{}, err
				}
				if !isTruthy(test) {
					return completion{}, nil
				}
			}
			for _, child := range asStmtSlice(stmt["body"]) {
				result, err := evalStmt(child, env)
				if err != nil {
					return completion{}, err
				}
				if result.returned {
					return result, nil
				}
				if result.broke {
					return completion{}, nil
				}
				if result.continued {
					break
				}
			}
			if rawUpdate, ok := stmt["update"]; ok {
				if _, err := evalExpr(asMap(rawUpdate), env); err != nil {
					return completion{}, err
				}
			}
		}
	case "while":
		for {
			test, err := evalExpr(asMap(stmt["test"]), env)
			if err != nil {
				return completion{}, err
			}
			if !isTruthy(test) {
				return completion{}, nil
			}
			for _, child := range asStmtSlice(stmt["body"]) {
				result, err := evalStmt(child, env)
				if err != nil {
					return completion{}, err
				}
				if result.returned {
					return result, nil
				}
				if result.broke {
					return completion{}, nil
				}
				if result.continued {
					break
				}
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
					return completion{}, nil
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
			if thrown, ok := err.(jsThrow); ok {
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
	case "break":
		return completion{broke: true}, nil
	case "continue":
		return completion{continued: true}, nil
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
	for _, stmt := range stmts {
		result, err := evalStmt(stmt, env)
		if err != nil {
			return completion{}, err
		}
		if result.returned || result.broke || result.continued {
			return result, nil
		}
	}
	return completion{}, nil
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
		return out, nil
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
		return out, nil
	case "object":
		out := map[string]any{}
		for _, prop := range asSlice(expr["props"]) {
			propMap := asMap(prop)
			value, err := evalExpr(asMap(propMap["value"]), env)
			if err != nil {
				return nil, err
			}
			out[asString(propMap["key"])] = value
		}
		return out, nil
	case "function":
		return FunctionValue{
			Params: asStringSlice(expr["params"]),
			Body:   asStmtSlice(expr["body"]),
			Env:    env,
		}, nil
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
		return evalExpr(asMap(expr["arg"]), env)
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
			for _, arg := range asSlice(expr["args"]) {
				value, err := evalExpr(asMap(arg), env)
				if err != nil {
					return nil, err
				}
				parts = append(parts, jsString(value))
			}
			fmt.Fprintln(os.Stdout, strings.Join(parts, " "))
			return jsUndefined, nil
		}
		if isArrayPush(asMap(expr["callee"])) {
			return callArrayPush(asMap(expr["callee"]), asSlice(expr["args"]), env)
		}
		if callee := asMap(expr["callee"]); callee["kind"] == "ident" {
			return callFunction(lookupEnv(env, asString(callee["name"])), asSlice(expr["args"]), env)
		}
		if callee := asMap(expr["callee"]); callee["kind"] == "member" {
			value, err := evalExpr(callee, env)
			if err != nil {
				return nil, err
			}
			return callFunction(value, asSlice(expr["args"]), env)
		}
		value, err := evalExpr(asMap(expr["callee"]), env)
		if err != nil {
			return nil, err
		}
		return callFunction(value, asSlice(expr["args"]), env)
	case "new":
		callee, err := evalExpr(asMap(expr["callee"]), env)
		if err != nil {
			return nil, err
		}
		return constructValue(callee, asSlice(expr["args"]), env)
	case "member":
		object, err := evalExpr(asMap(expr["object"]), env)
		if err != nil {
			return nil, err
		}
		property := asString(expr["property"])
		if objectMap, ok := object.(map[string]any); ok {
			if classValue, ok := objectMap["__class"].(*ClassValue); ok {
				if getter, ok := lookupGetter(classValue, property); ok {
					return callFunctionWithThis(getter, nil, env, objectMap)
				}
				if method, ok := lookupMethod(classValue, property); ok {
					return BoundFunctionValue{Function: method, This: objectMap}, nil
				}
			}
			return objectMap[property], nil
		}
		if classValue, ok := object.(*ClassValue); ok {
			if getter, ok := classValue.StaticGetters[property]; ok {
				return callFunctionWithThis(getter, nil, env, classValue)
			}
			if method, ok := classValue.Static[property]; ok {
				return BoundFunctionValue{Function: method, This: classValue}, nil
			}
		}
		if objectArray, ok := object.([]any); ok {
			if property == "length" {
				return float64(len(objectArray)), nil
			}
			index, err := strconv.Atoi(property)
			if err == nil && index >= 0 && index < len(objectArray) {
				return objectArray[index], nil
			}
		}
		return jsUndefined, nil
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
		object, err := evalExpr(asMap(target["object"]), env)
		if err != nil {
			return err
		}
		objectMap, ok := object.(map[string]any)
		if !ok {
			return errors.New("member assignment target is not object")
		}
		objectMap[asString(target["property"])] = value
		return nil
	default:
		return fmt.Errorf("unsupported assignment target %v", target["kind"])
	}
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
	case "+=":
		current, readErr := readTarget(left, env)
		if readErr != nil {
			return nil, readErr
		}
		right, evalErr := evalExpr(rightExpr, env)
		if evalErr != nil {
			return nil, evalErr
		}
		value, err = evalBinary("+", current, right)
	case "-=":
		current, readErr := readTarget(left, env)
		if readErr != nil {
			return nil, readErr
		}
		right, evalErr := evalExpr(rightExpr, env)
		if evalErr != nil {
			return nil, evalErr
		}
		value, err = evalBinary("-", current, right)
	case "*=":
		current, readErr := readTarget(left, env)
		if readErr != nil {
			return nil, readErr
		}
		right, evalErr := evalExpr(rightExpr, env)
		if evalErr != nil {
			return nil, evalErr
		}
		value, err = evalBinary("*", current, right)
	case "|=":
		current, readErr := readTarget(left, env)
		if readErr != nil {
			return nil, readErr
		}
		right, evalErr := evalExpr(rightExpr, env)
		if evalErr != nil {
			return nil, evalErr
		}
		value, err = evalBinary("|", current, right)
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
	if objectMap, ok := object.(map[string]any); ok {
		delete(objectMap, asString(target["property"]))
	}
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
	array, ok := current.([]any)
	if !ok {
		return nil, errors.New("push receiver is not array")
	}
	for _, arg := range rawArgs {
		value, err := evalExpr(asMap(arg), env)
		if err != nil {
			return nil, err
		}
		array = append(array, value)
	}
	if err := assignTarget(objectExpr, array, env); err != nil {
		return nil, err
	}
	return float64(len(array)), nil
}

func evalClass(superExpr map[string]any, rawMethods []any, env Env) (*ClassValue, error) {
	classValue := &ClassValue{
		Methods:       map[string]FunctionValue{},
		Getters:       map[string]FunctionValue{},
		Static:        map[string]FunctionValue{},
		StaticGetters: map[string]FunctionValue{},
	}
	if len(superExpr) > 0 {
		superValue, err := evalExpr(superExpr, env)
		if err != nil {
			return nil, err
		}
		superClass, ok := superValue.(*ClassValue)
		if !ok {
			return nil, errors.New("class extends target is not constructable")
		}
		classValue.Super = superClass
	}
	for _, rawMethod := range rawMethods {
		method := asMap(rawMethod)
		function := FunctionValue{
			Params: asStringSlice(method["params"]),
			Body:   asStmtSlice(method["body"]),
			Env:    env,
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
		default:
			return nil, fmt.Errorf("unsupported class method %s", asString(method["kind"]))
		}
	}
	return classValue, nil
}

func constructValue(raw any, rawArgs []any, callerEnv Env) (any, error) {
	classValue, ok := raw.(*ClassValue)
	if !ok {
		return nil, errors.New("constructor is not callable")
	}
	instance := map[string]any{"__class": classValue}
	if classValue.Constructor != nil {
		if _, err := callFunctionWithThis(*classValue.Constructor, rawArgs, callerEnv, instance); err != nil {
			return nil, err
		}
	} else if classValue.Super != nil && classValue.Super.Constructor != nil {
		if _, err := callFunctionWithThis(*classValue.Super.Constructor, rawArgs, callerEnv, instance); err != nil {
			return nil, err
		}
	}
	return instance, nil
}

func callFunction(raw any, rawArgs []any, callerEnv Env) (any, error) {
	switch function := raw.(type) {
	case FunctionValue:
		return callFunctionWithThis(function, rawArgs, callerEnv, jsUndefined)
	case BoundFunctionValue:
		return callFunctionWithThis(function.Function, rawArgs, callerEnv, function.This)
	case NativeFunctionValue:
		args := []any{}
		for _, rawArg := range rawArgs {
			value, err := evalExpr(asMap(rawArg), callerEnv)
			if err != nil {
				return nil, err
			}
			args = append(args, value)
		}
		return function.Call(args)
	default:
		return nil, errors.New("callee is not callable")
	}
}

func callFunctionWithThis(function FunctionValue, rawArgs []any, callerEnv Env, thisValue any) (any, error) {
	child := Env{"__parent": function.Env, "this": thisValue}
	for index, param := range function.Params {
		value := any(jsUndefined)
		if index < len(rawArgs) {
			evaluated, err := evalExpr(asMap(rawArgs[index]), callerEnv)
			if err != nil {
				return nil, err
			}
			value = evaluated
		}
		child[param] = value
	}
	for _, stmt := range function.Body {
		result, err := evalStmt(stmt, child)
		if err != nil {
			return nil, err
		}
		if result.returned {
			return result.value, nil
		}
	}
	return nil, nil
}

func builtinErrorClass() *ClassValue {
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
		Static:      map[string]FunctionValue{},
		StaticGetters: map[string]FunctionValue{},
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
		number, err := strconv.ParseFloat(asString(value["value"]), 64)
		if err != nil {
			return nil, err
		}
		return number, nil
	case "string", "bigint":
		return asString(value["value"]), nil
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
	case "==", "===":
		return jsSameValue(left, right), nil
	case "!=", "!==":
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

func isArrayPush(callee map[string]any) bool {
	return callee["kind"] == "member" && asString(callee["property"]) == "push"
}

func iterableValues(value any) []any {
	switch typed := value.(type) {
	case []any:
		return typed
	case map[string]any:
		values := []any{}
		for _, item := range typed {
			values = append(values, item)
		}
		return values
	default:
		return nil
	}
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
	default:
		return fmt.Sprintf("%p", &left) == fmt.Sprintf("%p", &right)
	}
}

func isNullish(value any) bool {
	switch value.(type) {
	case nil, UndefinedValue, NullValue:
		return true
	default:
		return false
	}
}

func jsPropertyKey(value any) string {
	return jsString(value)
}

func hasProperty(value any, key string) bool {
	switch typed := value.(type) {
	case map[string]any:
		_, ok := typed[key]
		return ok
	case []any:
		if key == "length" {
			return true
		}
		index, err := strconv.Atoi(key)
		return err == nil && index >= 0 && index < len(typed)
	default:
		return false
	}
}

func jsInstanceOf(value any, constructor any) bool {
	classValue, ok := constructor.(*ClassValue)
	if !ok {
		return false
	}
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
		number, err := strconv.ParseFloat(strings.TrimSpace(typed), 64)
		if err != nil {
			return math.NaN()
		}
		return number
	default:
		return math.NaN()
	}
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
	default:
		bytes, err := json.Marshal(typed)
		if err != nil {
			return fmt.Sprint(typed)
		}
		return string(bytes)
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
	case FunctionValue, BoundFunctionValue, NativeFunctionValue, *ClassValue:
		return "function"
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
"#
    .to_string()
}

fn render_go_mod(module_path: &str) -> String {
    format!("module {module_path}\n\ngo 1.22\n")
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
