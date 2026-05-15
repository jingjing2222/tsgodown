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
    r#"package tsgodownrt

import (
	"encoding/json"
	"errors"
	"fmt"
	"math"
	"os"
	"path/filepath"
	"reflect"
	"regexp"
	"runtime"
	"sort"
	"strconv"
	"strings"
	"time"
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
	Props  map[string]any
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
	Call         func(args []any) (any, error)
	CallWithThis func(thisValue any, args []any) (any, error)
}

type RegExpValue struct {
	Pattern string
	Flags   string
	Regex   *regexp.Regexp
	Global  bool
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
}

type moduleState struct {
	exports   any
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

func executeModule(module Module, program Program, cache map[string]*moduleState) (any, error) {
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
		"Error":   builtinErrorClass("Error"),
		"TypeError": builtinErrorClass("TypeError"),
		"RangeError": builtinErrorClass("RangeError"),
		"SyntaxError": builtinErrorClass("SyntaxError"),
		"ReferenceError": builtinErrorClass("ReferenceError"),
		"Number": nativeFunction(func(args []any) (any, error) {
			if len(args) == 0 {
				return float64(0), nil
			}
			return toNumber(args[0]), nil
		}),
		"String": nativeFunction(func(args []any) (any, error) {
			if len(args) == 0 {
				return "", nil
			}
			return jsString(args[0]), nil
		}),
		"Boolean": nativeFunction(func(args []any) (any, error) {
			if len(args) == 0 {
				return false, nil
			}
			return isTruthy(args[0]), nil
		}),
		"RegExp": nativeFunction(func(args []any) (any, error) {
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
		}),
		"Symbol": symbolGlobal(),
		"Array":  arrayGlobal(),
		"Object": objectGlobal(),
		"Math":   mathGlobal(),
		"Map":    mapGlobal(),
		"Set":    setGlobal(),
		"WeakMap": mapGlobal(),
		"WeakSet": setGlobal(),
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
	return NativeFunctionValue{
		Call: call,
		CallWithThis: func(thisValue any, args []any) (any, error) {
			return call(args)
		},
	}
}

func nativeMethod(call func(thisValue any, args []any) (any, error)) NativeFunctionValue {
	return NativeFunctionValue{
		Call: func(args []any) (any, error) {
			return call(jsUndefined, args)
		},
		CallWithThis: call,
	}
}

func arrayGlobal() map[string]any {
	prototype := map[string]any{
		"push": nativeMethod(func(thisValue any, args []any) (any, error) {
			array, ok := thisValue.([]any)
			if !ok {
				return nil, errors.New("push receiver is not array")
			}
			array = append(array, args...)
			return float64(len(array)), nil
		}),
	}
	return map[string]any{
		"isArray": nativeFunction(func(args []any) (any, error) {
			if len(args) == 0 {
				return false, nil
			}
			_, ok := args[0].([]any)
			return ok, nil
		}),
		"prototype": prototype,
	}
}

func objectGlobal() map[string]any {
	prototype := map[string]any{}
	prototype["hasOwnProperty"] = nativeMethod(func(thisValue any, args []any) (any, error) {
		if len(args) == 0 {
			return false, nil
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
		target, ok := args[0].(map[string]any)
		if !ok {
			target = map[string]any{}
		}
		for _, source := range args[1:] {
			if sourceMap, ok := source.(map[string]any); ok {
				for key, value := range sourceMap {
					if strings.HasPrefix(key, "__") {
						continue
					}
					target[key] = value
				}
			}
		}
		return target, nil
	})
	return map[string]any{
		"assign": assign,
		"create": nativeFunction(func(args []any) (any, error) {
			return map[string]any{}, nil
		}),
		"defineProperty": nativeFunction(func(args []any) (any, error) {
			if len(args) < 3 {
				return jsUndefined, nil
			}
			object, ok := args[0].(map[string]any)
			if !ok {
				return args[0], nil
			}
			descriptor, ok := args[2].(map[string]any)
			if ok {
				object[jsPropertyKey(args[1])] = descriptor["value"]
			}
			return object, nil
		}),
		"entries": nativeFunction(func(args []any) (any, error) {
			if len(args) == 0 {
				return []any{}, nil
			}
			object, ok := args[0].(map[string]any)
			if !ok {
				return []any{}, nil
			}
			keys := objectKeys(object)
			result := []any{}
			for _, key := range keys {
				result = append(result, []any{key, object[key]})
			}
			return result, nil
		}),
		"freeze": nativeFunction(func(args []any) (any, error) {
			if len(args) == 0 {
				return jsUndefined, nil
			}
			return args[0], nil
		}),
		"keys": nativeFunction(func(args []any) (any, error) {
			if len(args) == 0 {
				return []any{}, nil
			}
			object, ok := args[0].(map[string]any)
			if !ok {
				return []any{}, nil
			}
			keys := objectKeys(object)
			result := []any{}
			for _, key := range keys {
				result = append(result, key)
			}
			return result, nil
		}),
		"prototype": prototype,
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
	return nativeFunction(func(args []any) (any, error) {
		value := &MapValue{Entries: []MapEntry{}}
		if len(args) > 0 {
			for _, entry := range iterableValues(args[0]) {
				if pair, ok := entry.([]any); ok && len(pair) >= 2 {
					mapSet(value, pair[0], pair[1])
				}
			}
		}
		return value, nil
	})
}

func setGlobal() NativeFunctionValue {
	return nativeFunction(func(args []any) (any, error) {
		value := &SetValue{Values: []any{}}
		if len(args) > 0 {
			for _, item := range iterableValues(args[0]) {
				setAdd(value, item)
			}
		}
		return value, nil
	})
}

func objectKeys(object map[string]any) []string {
	keys := []string{}
	for key := range object {
		if strings.HasPrefix(key, "__") {
			continue
		}
		keys = append(keys, key)
	}
	sort.Strings(keys)
	return keys
}

func objectTag(value any) string {
	switch value.(type) {
	case []any:
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
	default:
		return "[object Object]"
	}
}

func newRegExp(pattern string, flags string) (*RegExpValue, error) {
	goPattern := pattern
	if strings.Contains(flags, "i") {
		goPattern = "(?i)" + goPattern
	}
	compiled, err := regexp.Compile(goPattern)
	if err != nil {
		return nil, err
	}
	return &RegExpValue{
		Pattern: pattern,
		Flags:   flags,
		Regex:   compiled,
		Global:  strings.Contains(flags, "g"),
	}, nil
}

func regexpMatches(value *RegExpValue, text string) any {
	if value.Global {
		matches := value.Regex.FindAllString(text, -1)
		if matches == nil {
			return jsNull
		}
		result := []any{}
		for _, match := range matches {
			result = append(result, match)
		}
		return result
	}
	matches := value.Regex.FindStringSubmatch(text)
	if matches == nil {
		return jsNull
	}
	result := []any{}
	for _, match := range matches {
		result = append(result, match)
	}
	return result
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

func bindImport(env Env, importDecl Import, importedExports any) {
	importedMap, _ := importedExports.(map[string]any)
	for _, binding := range importDecl.Bindings {
		switch binding.Kind {
		case "named":
			env[binding.Local] = importedMap[binding.Imported]
		case "default":
			if value, ok := importedMap["default"]; ok {
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
			name := asString(callee["name"])
			result, err := callFunction(lookupEnv(env, name), asSlice(expr["args"]), env)
			if err != nil {
				return nil, fmt.Errorf("identifier call %s failed: %w", name, err)
			}
			return result, nil
		}
		if callee := asMap(expr["callee"]); callee["kind"] == "member" {
			property := asString(callee["property"])
			value, err := evalExpr(callee, env)
			if err != nil {
				return nil, err
			}
			result, err := callFunction(value, asSlice(expr["args"]), env)
			if err != nil {
				return nil, fmt.Errorf("member call %s failed: %w", property, err)
			}
			return result, nil
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
		if function, ok := object.(NativeFunctionValue); ok {
			if member, ok := nativeFunctionMember(function, property); ok {
				return member, nil
			}
		}
		if function, ok := object.(FunctionValue); ok {
			if member, ok := functionMember(function, property); ok {
				return member, nil
			}
		}
		if function, ok := object.(BoundFunctionValue); ok {
			if member, ok := boundFunctionMember(function, property); ok {
				return member, nil
			}
		}
		if numberValue, ok := object.(float64); ok {
			if member, ok := numberMember(numberValue, property); ok {
				return member, nil
			}
		}
		if stringValue, ok := object.(string); ok {
			if member, ok := stringMember(stringValue, property, env); ok {
				return member, nil
			}
		}
		if symbolValue, ok := object.(*SymbolValue); ok {
			if member, ok := symbolMember(symbolValue, property); ok {
				return member, nil
			}
		}
		if regExpValue, ok := object.(*RegExpValue); ok {
			if member, ok := regexpMember(regExpValue, property); ok {
				return member, nil
			}
		}
		if mapValue, ok := object.(*MapValue); ok {
			if member, ok := mapMember(mapValue, property); ok {
				return member, nil
			}
		}
		if setValue, ok := object.(*SetValue); ok {
			if member, ok := setMember(setValue, property); ok {
				return member, nil
			}
		}
		if iteratorValue, ok := object.(*IteratorValue); ok {
			if member, ok := iteratorMember(iteratorValue, property); ok {
				return member, nil
			}
		}
		if objectArray, ok := object.([]any); ok {
			if member, ok := arrayMember(objectArray, property, env); ok {
				return member, nil
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
		objectExpr := asMap(target["object"])
		object, err := evalExpr(objectExpr, env)
		if err != nil {
			return err
		}
		property := asString(target["property"])
		objectMap, ok := object.(map[string]any)
		if ok {
			objectMap[property] = value
			return nil
		}
		if function, ok := object.(FunctionValue); ok {
			if function.Props == nil {
				function.Props = map[string]any{}
			}
			function.Props[property] = value
			return assignTarget(objectExpr, function, env)
		}
		if objectArray, ok := object.([]any); ok {
			nextArray, handled := assignArrayMember(objectArray, property, value)
			if !handled {
				return fmt.Errorf("member assignment target array property %s is not assignable", property)
			}
			return assignTarget(objectExpr, nextArray, env)
		}
		return fmt.Errorf("member assignment target is not object: %T %s", object, jsInspect(object))
	default:
		return fmt.Errorf("unsupported assignment target %v", target["kind"])
	}
}

func assignArrayMember(array []any, property string, value any) ([]any, bool) {
	if property == "length" {
		nextLength := jsInteger(value)
		if nextLength < 0 {
			nextLength = 0
		}
		if nextLength < len(array) {
			return array[:nextLength], true
		}
		for len(array) < nextLength {
			array = append(array, jsUndefined)
		}
		return array, true
	}
	index, err := strconv.ParseInt(property, 0, 64)
	if err != nil || index < 0 {
		return array, false
	}
	for int64(len(array)) <= index {
		array = append(array, jsUndefined)
	}
	array[int(index)] = value
	return array, true
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

func nativeFunctionMember(function NativeFunctionValue, property string) (any, bool) {
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
				if arrayArgs, ok := args[1].([]any); ok {
					callArgs = arrayArgs
				}
			}
			if function.CallWithThis != nil {
				return function.CallWithThis(thisValue, callArgs)
			}
			return function.Call(callArgs)
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
			return callFunctionWithThisValues(function.Function, args, function.This)
		}), true
	case "apply":
		return nativeFunction(func(args []any) (any, error) {
			callArgs := []any{}
			if len(args) > 1 {
				callArgs = iterableValues(args[1])
			}
			return callFunctionWithThisValues(function.Function, callArgs, function.This)
		}), true
	}
	return nil, false
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
				return []any{value}, nil
			}
			if separator, ok := args[0].(*RegExpValue); ok {
				parts := separator.Regex.Split(value, -1)
				result := []any{}
				for _, part := range parts {
					result = append(result, part)
				}
				return result, nil
			}
			separator := jsString(args[0])
			result := []any{}
			if separator == "" {
				for _, char := range value {
					result = append(result, string(char))
				}
				return result, nil
			}
			for _, part := range strings.Split(value, separator) {
				result = append(result, part)
			}
			return result, nil
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
				return regexpMatches(&RegExpValue{Regex: regexp.MustCompile("")}, value), nil
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
			return float64(start + len([]rune(value[:byteIndexForRune(value, start)+index]))), nil
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
	switch property {
	case "source":
		return value.Pattern, true
	case "flags":
		return value.Flags, true
	case "test":
		return nativeFunction(func(args []any) (any, error) {
			text := ""
			if len(args) > 0 {
				text = jsString(args[0])
			}
			return value.Regex.MatchString(text), nil
		}), true
	case "exec":
		return nativeFunction(func(args []any) (any, error) {
			text := ""
			if len(args) > 0 {
				text = jsString(args[0])
			}
			return regexpMatches(value, text), nil
		}), true
	}
	return nil, false
}

func arrayMember(value []any, property string, env Env) (any, bool) {
	switch property {
	case "length":
		return float64(len(value)), true
	case "join":
		return nativeFunction(func(args []any) (any, error) {
			separator := ","
			if len(args) > 0 {
				separator = jsString(args[0])
			}
			parts := []string{}
			for _, item := range value {
				if isNullish(item) {
					parts = append(parts, "")
				} else {
					parts = append(parts, jsString(item))
				}
			}
			return strings.Join(parts, separator), nil
		}), true
	case "map":
		return nativeFunction(func(args []any) (any, error) {
			if len(args) == 0 {
				return nil, errors.New("map callback is required")
			}
			result := []any{}
			for index, item := range value {
				mapped, err := callFunctionWithValues(args[0], []any{item, float64(index), value}, env, jsUndefined)
				if err != nil {
					return nil, err
				}
				result = append(result, mapped)
			}
			return result, nil
		}), true
	case "filter":
		return nativeFunction(func(args []any) (any, error) {
			if len(args) == 0 {
				return nil, errors.New("filter callback is required")
			}
			result := []any{}
			for index, item := range value {
				keep, err := callFunctionWithValues(args[0], []any{item, float64(index), value}, env, jsUndefined)
				if err != nil {
					return nil, err
				}
				if isTruthy(keep) {
					result = append(result, item)
				}
			}
			return result, nil
		}), true
	case "forEach":
		return nativeFunction(func(args []any) (any, error) {
			if len(args) == 0 {
				return nil, errors.New("forEach callback is required")
			}
			for index, item := range value {
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
			if len(value) == 0 && len(args) < 2 {
				return nil, errors.New("reduce of empty array with no initial value")
			}
			start := 0
			accumulator := any(jsUndefined)
			if len(args) > 1 {
				accumulator = args[1]
			} else {
				accumulator = value[0]
				start = 1
			}
			for index := start; index < len(value); index++ {
				next, err := callFunctionWithValues(args[0], []any{accumulator, value[index], float64(index), value}, env, jsUndefined)
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
			if len(value) == 0 && len(args) < 2 {
				return nil, errors.New("reduce of empty array with no initial value")
			}
			index := len(value) - 1
			accumulator := any(jsUndefined)
			if len(args) > 1 {
				accumulator = args[1]
			} else {
				accumulator = value[index]
				index--
			}
			for ; index >= 0; index-- {
				next, err := callFunctionWithValues(args[0], []any{accumulator, value[index], float64(index), value}, env, jsUndefined)
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
			for index, item := range value {
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
			for index, item := range value {
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
			for index, item := range value {
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
			for index, item := range value {
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
				start = jsArrayStartIndex(args[1], len(value))
			}
			return float64(arrayIndexOf(value, search, start)), nil
		}), true
	case "lastIndexOf":
		return nativeFunction(func(args []any) (any, error) {
			search := any(jsUndefined)
			if len(args) > 0 {
				search = args[0]
			}
			start := len(value) - 1
			if len(args) > 1 {
				start = jsSliceIndex(args[1], len(value))
			}
			for index := minInt(start, len(value)-1); index >= 0; index-- {
				if jsSameValue(value[index], search) {
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
				start = jsArrayStartIndex(args[1], len(value))
			}
			return arrayIndexOf(value, search, start) >= 0, nil
		}), true
	case "concat":
		return nativeFunction(func(args []any) (any, error) {
			result := append([]any{}, value...)
			for _, arg := range args {
				if next, ok := arg.([]any); ok {
					result = append(result, next...)
				} else {
					result = append(result, arg)
				}
			}
			return result, nil
		}), true
	case "slice":
		return nativeFunction(func(args []any) (any, error) {
			start := 0
			end := len(value)
			if len(args) > 0 {
				start = jsSliceIndex(args[0], len(value))
			}
			if len(args) > 1 {
				end = jsSliceIndex(args[1], len(value))
			}
			if end < start {
				end = start
			}
			return append([]any{}, value[start:end]...), nil
		}), true
	case "flat":
		return nativeFunction(func(args []any) (any, error) {
			depth := 1
			if len(args) > 0 {
				depth = jsInteger(args[0])
			}
			return flattenArray(value, depth), nil
		}), true
	case "sort":
		return nativeFunction(func(args []any) (any, error) {
			var sortErr error
			sort.SliceStable(value, func(leftIndex int, rightIndex int) bool {
				if sortErr != nil {
					return false
				}
				left := value[leftIndex]
				right := value[rightIndex]
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
	index, err := strconv.Atoi(property)
	if err == nil && index >= 0 && index < len(value) {
		return value[index], true
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
		if nested, ok := item.([]any); ok {
			result = append(result, flattenArray(nested, depth-1)...)
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
				entries = append(entries, []any{entry.Key, entry.Value})
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
				entries = append(entries, []any{item, item})
			}
			return &IteratorValue{Values: entries}, nil
		}), true
	}
	return nil, false
}

func iteratorMember(value *IteratorValue, property string) (any, bool) {
	switch property {
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

func replaceRegExp(value string, search *RegExpValue, replacement any, env Env) (any, error) {
	matches := search.Regex.FindAllStringSubmatchIndex(value, -1)
	if matches == nil {
		return value, nil
	}
	if !search.Global && len(matches) > 1 {
		matches = matches[:1]
	}
	var out strings.Builder
	last := 0
	for _, match := range matches {
		start := match[0]
		end := match[1]
		out.WriteString(value[last:start])
		if _, ok := replacement.(FunctionValue); ok {
			args := regexpReplacementArgs(value, match)
			next, err := callFunctionWithValues(replacement, args, env, jsUndefined)
			if err != nil {
				return nil, err
			}
			out.WriteString(jsString(next))
		} else if _, ok := replacement.(BoundFunctionValue); ok {
			args := regexpReplacementArgs(value, match)
			next, err := callFunctionWithValues(replacement, args, env, jsUndefined)
			if err != nil {
				return nil, err
			}
			out.WriteString(jsString(next))
		} else if _, ok := replacement.(NativeFunctionValue); ok {
			args := regexpReplacementArgs(value, match)
			next, err := callFunctionWithValues(replacement, args, env, jsUndefined)
			if err != nil {
				return nil, err
			}
			out.WriteString(jsString(next))
		} else {
			out.WriteString(string(search.Regex.ExpandString(nil, jsString(replacement), value, match)))
		}
		last = end
	}
	out.WriteString(value[last:])
	return out.String(), nil
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
		if function, functionOk := raw.(FunctionValue); functionOk {
			instance := map[string]any{}
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
			instance := map[string]any{}
			result, err := callFunctionWithThis(bound.Function, rawArgs, callerEnv, instance)
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
		return nil, fmt.Errorf("constructor is not callable: %T %s", raw, jsInspect(raw))
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
		return callFunctionWithThisValues(function.Function, args, function.This)
	case NativeFunctionValue:
		return function.Call(args)
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
	args := []any{}
	for _, rawArg := range rawArgs {
		value, err := evalExpr(asMap(rawArg), callerEnv)
		if err != nil {
			return nil, err
		}
		args = append(args, value)
	}
	return callFunctionWithThisValues(function, args, thisValue)
}

func callFunctionWithThisValues(function FunctionValue, args []any, thisValue any) (any, error) {
	child := Env{"__parent": function.Env, "this": thisValue}
	for index, param := range function.Params {
		value := any(jsUndefined)
		if index < len(args) {
			value = args[index]
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
			values = append(values, []any{entry.Key, entry.Value})
		}
		return values
	case *SetValue:
		return append([]any{}, typed.Values...)
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
	case map[string]any, []any, FunctionValue, BoundFunctionValue, NativeFunctionValue, *ClassValue:
		return referenceIdentity(left) == referenceIdentity(right)
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
	case []any:
		parts := []string{}
		for _, item := range typed {
			if isNullish(item) {
				parts = append(parts, "")
			} else {
				parts = append(parts, jsString(item))
			}
		}
		return strings.Join(parts, ",")
	case map[string]any:
		return objectTag(typed)
	case *RegExpValue:
		return "/" + typed.Pattern + "/" + typed.Flags
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
