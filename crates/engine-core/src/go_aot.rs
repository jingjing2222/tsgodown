use std::collections::{BTreeMap, BTreeSet};

use crate::contract::{AnalyzeResponse, IrDocument, JsExpr, JsStmt, JsValue, Module};
use crate::emit_go::{go_string_literal, sanitize_go_identifier};

const CJS_DEFAULT_EXPORT_FUNCTION: &str = "__cjs_default_export";
const NODE_LTS_VERSION: &str = "24.15.0";
const NODE_LTS_VERSION_WITH_PREFIX: &str = "v24.15.0";

pub(crate) fn render_aot_executable_program(
    package_name: &str,
    analyzed: &AnalyzeResponse,
) -> Option<String> {
    let module = entry_module(&analyzed.ir)?;
    if !can_aot_module_graph(&analyzed.ir) {
        return None;
    }
    let module_functions = collect_module_functions(&analyzed.ir);
    let module_classes = collect_module_classes(&analyzed.ir);
    let module_default_exports = collect_module_default_exports(&analyzed.ir, &module_functions);
    let module_default_class_exports =
        collect_module_default_class_exports(&analyzed.ir, &module_classes);
    let module_object_exports = collect_module_object_function_exports(
        &analyzed.ir,
        &module_functions,
        &module_default_exports,
    );
    let module_named_exports = collect_module_named_exports(
        &analyzed.ir,
        &module_functions,
        &module_default_exports,
        &module_object_exports,
    );
    let module_slots = collect_module_slots(&analyzed.ir, &module_functions);
    let declarations = render_module_decls(
        &analyzed.ir,
        &module_functions,
        &module_classes,
        &module_slots,
    )?;
    let mut state = module_aot_state(
        module,
        &analyzed.ir,
        &AotModuleContext {
            functions: &module_functions,
            classes: &module_classes,
            default_exports: &module_default_exports,
            default_class_exports: &module_default_class_exports,
            named_exports: &module_named_exports,
            slots: &module_slots,
        },
    )?;
    state.go_imports = collect_aot_imports(&analyzed.ir);
    let mut body = Vec::new();
    for stmt in &module.executable.as_ref()?.stmts {
        if matches!(stmt, JsStmt::FunctionDecl { .. } | JsStmt::ClassDecl { .. })
            || is_function_binding_stmt(stmt)
        {
            continue;
        }
        body.push(render_stmt(stmt, &mut state)?);
    }
    Some(format!(
        r#"package {package_name}

{imports}

{declarations}
{helpers}
func main() {{
{body}
}}
"#,
        imports = render_go_imports(&state.go_imports),
        declarations = declarations.join("\n\n"),
        helpers = render_aot_helpers(&state.go_imports),
        body = indent_lines(&body.join("\n"))
    ))
}

pub(crate) fn aot_unsupported_features(ir: &IrDocument) -> Vec<String> {
    let module_functions = collect_module_functions(ir);
    let module_classes = collect_module_classes(ir);
    let module_default_exports = collect_module_default_exports(ir, &module_functions);
    let module_default_class_exports = collect_module_default_class_exports(ir, &module_classes);
    let module_object_exports =
        collect_module_object_function_exports(ir, &module_functions, &module_default_exports);
    let module_named_exports = collect_module_named_exports(
        ir,
        &module_functions,
        &module_default_exports,
        &module_object_exports,
    );
    let module_slots = collect_module_slots(ir, &module_functions);
    let mut features = BTreeSet::new();
    for module in &ir.modules {
        for import in &module.imports {
            if import.resolved.is_none() && !is_node_builtin_spec(&import.spec) {
                features.insert(format!("aot.module.unresolved_import:{}", import.spec));
            }
        }
        let Some(executable) = &module.executable else {
            features.insert(format!("aot.module.no_executable:{}", module.source_path));
            continue;
        };
        let state = module_aot_state(
            module,
            ir,
            &AotModuleContext {
                functions: &module_functions,
                classes: &module_classes,
                default_exports: &module_default_exports,
                default_class_exports: &module_default_class_exports,
                named_exports: &module_named_exports,
                slots: &module_slots,
            },
        );
        if state.is_none() {
            features.insert(format!(
                "aot.module.unsupported_bindings:{}",
                module.source_path
            ));
        }
        for stmt in &executable.stmts {
            if let JsStmt::ClassDecl { name, .. } = stmt {
                if !module_classes.contains_key(&(module.id.clone(), name.clone())) {
                    features.insert(format!(
                        "aot.class.unsupported:{}:{}",
                        module.source_path, name
                    ));
                }
            }
            if let Some(parts) = function_parts(stmt) {
                if let (Some(function), Some(state)) = (
                    module_functions.get(&(module.id.clone(), parts.name.clone())),
                    state.as_ref(),
                ) {
                    if render_function_decl(function, state).is_none() {
                        features.insert(format!(
                            "aot.function.unsupported_body:{}:{}",
                            module.source_path, parts.name
                        ));
                    }
                }
            }
        }
        collect_builtin_usage_stmt_list_features(
            &executable.stmts,
            &mut features,
            &BTreeSet::new(),
        );
    }
    features.into_iter().collect()
}

fn collect_builtin_usage_stmt_list_features(
    stmts: &[JsStmt],
    features: &mut BTreeSet<String>,
    shadowed: &BTreeSet<String>,
) {
    let mut scoped = shadowed.clone();
    for stmt in stmts {
        collect_builtin_usage_features_with_scope(stmt, features, &scoped);
        if let JsStmt::VarDecl { name, init } = stmt {
            if var_decl_shadows_node_builtin(name, init.as_ref()) {
                scoped.insert(name.clone());
            }
        }
    }
}

fn collect_builtin_usage_features_with_scope(
    stmt: &JsStmt,
    features: &mut BTreeSet<String>,
    shadowed: &BTreeSet<String>,
) {
    match stmt {
        JsStmt::VarDecl {
            init: Some(expr), ..
        }
        | JsStmt::Expr { expr }
        | JsStmt::Return { value: Some(expr) }
        | JsStmt::Throw { value: expr }
        | JsStmt::Yield {
            value: Some(expr), ..
        } => collect_builtin_usage_expr_features(expr, features, shadowed),
        JsStmt::FunctionDecl { params, body, .. } => {
            let scoped = scoped_shadowed(shadowed, params);
            collect_builtin_usage_stmt_list_features(body, features, &scoped);
        }
        JsStmt::ClassDecl { methods, .. } => {
            for method in methods {
                let scoped = scoped_shadowed(shadowed, &method.params);
                collect_builtin_usage_stmt_list_features(&method.body, features, &scoped);
            }
        }
        JsStmt::If {
            test,
            consequent,
            alternate,
        } => {
            collect_builtin_usage_expr_features(test, features, shadowed);
            collect_builtin_usage_stmt_list_features(consequent, features, shadowed);
            collect_builtin_usage_stmt_list_features(alternate, features, shadowed);
        }
        JsStmt::For {
            init,
            test,
            update,
            body,
        } => {
            for stmt in init {
                collect_builtin_usage_features_with_scope(stmt, features, shadowed);
            }
            if let Some(test) = test {
                collect_builtin_usage_expr_features(test, features, shadowed);
            }
            if let Some(update) = update {
                collect_builtin_usage_expr_features(update, features, shadowed);
            }
            collect_builtin_usage_stmt_list_features(body, features, shadowed);
        }
        JsStmt::While { test, body } => {
            collect_builtin_usage_expr_features(test, features, shadowed);
            collect_builtin_usage_stmt_list_features(body, features, shadowed);
        }
        _ => {}
    }
}

fn collect_builtin_usage_expr_features(
    expr: &JsExpr,
    features: &mut BTreeSet<String>,
    shadowed: &BTreeSet<String>,
) {
    if is_process_supported_builtin_expr(expr)
        || is_supported_node_builtin_call_expr(expr)
        || is_node_fs_function_ref(expr).is_some()
    {
        return;
    }
    if is_node_path_static_string_expr(expr) {
        return;
    }
    match expr {
        JsExpr::Call { callee, args, .. } => {
            if let JsExpr::Member {
                object, property, ..
            } = callee.as_ref()
            {
                if let JsExpr::Ident { name } = object.as_ref() {
                    if is_observed_node_builtin_name(name, shadowed) {
                        features.insert(format!("aot.node.builtin_operation:{name}.{property}"));
                    }
                }
            }
            collect_builtin_usage_expr_features(callee, features, shadowed);
            for arg in args {
                collect_builtin_usage_expr_features(arg, features, shadowed);
            }
        }
        JsExpr::Member {
            object, property, ..
        } => {
            if let JsExpr::Ident { name } = object.as_ref() {
                if is_observed_node_builtin_name(name, shadowed) {
                    features.insert(format!("aot.node.builtin_property:{name}.{property}"));
                }
            }
            collect_builtin_usage_expr_features(object, features, shadowed);
        }
        JsExpr::Array { items } => {
            for item in items {
                collect_builtin_usage_expr_features(item, features, shadowed);
            }
        }
        JsExpr::ArraySpread { items } => {
            for item in items {
                collect_builtin_usage_expr_features(&item.value, features, shadowed);
            }
        }
        JsExpr::Object { props } => {
            for prop in props {
                collect_builtin_usage_expr_features(&prop.value, features, shadowed);
            }
        }
        JsExpr::Unary { arg, .. }
        | JsExpr::Await { arg }
        | JsExpr::Update { arg, .. }
        | JsExpr::Spread { arg }
        | JsExpr::ObjectRest { object: arg, .. } => {
            collect_builtin_usage_expr_features(arg, features, shadowed)
        }
        JsExpr::Binary { left, right, .. } | JsExpr::Assign { left, right, .. } => {
            collect_builtin_usage_expr_features(left, features, shadowed);
            collect_builtin_usage_expr_features(right, features, shadowed);
        }
        JsExpr::Conditional {
            test,
            consequent,
            alternate,
        } => {
            collect_builtin_usage_expr_features(test, features, shadowed);
            collect_builtin_usage_expr_features(consequent, features, shadowed);
            collect_builtin_usage_expr_features(alternate, features, shadowed);
        }
        JsExpr::New { callee, args } => {
            collect_builtin_usage_expr_features(callee, features, shadowed);
            for arg in args {
                collect_builtin_usage_expr_features(arg, features, shadowed);
            }
        }
        JsExpr::Template { exprs, .. } | JsExpr::Sequence { exprs } => {
            for expr in exprs {
                collect_builtin_usage_expr_features(expr, features, shadowed);
            }
        }
        JsExpr::Function { params, body, .. } => {
            let scoped = scoped_shadowed(shadowed, params);
            collect_builtin_usage_stmt_list_features(body, features, &scoped);
        }
        JsExpr::Class { methods, .. } => {
            for method in methods {
                let scoped = scoped_shadowed(shadowed, &method.params);
                collect_builtin_usage_stmt_list_features(&method.body, features, &scoped);
            }
        }
        JsExpr::Value { .. } | JsExpr::Ident { .. } | JsExpr::This | JsExpr::Super => {}
    }
}

fn scoped_shadowed(shadowed: &BTreeSet<String>, params: &[String]) -> BTreeSet<String> {
    let mut scoped = shadowed.clone();
    scoped.extend(params.iter().cloned());
    scoped
}

fn var_decl_shadows_node_builtin(name: &str, init: Option<&JsExpr>) -> bool {
    if !is_observed_node_builtin_name(name, &BTreeSet::new()) {
        return false;
    }
    !matches!(
        init.and_then(require_call_spec),
        Some(spec) if node_builtin_spec_matches_binding(spec, name)
    )
}

fn require_call_spec(expr: &JsExpr) -> Option<&str> {
    let JsExpr::Call { callee, args, .. } = expr else {
        return None;
    };
    if !matches!(callee.as_ref(), JsExpr::Ident { name } if name == "require") {
        return None;
    }
    let JsExpr::Value {
        value: JsValue::String { value },
    } = args.first()?
    else {
        return None;
    };
    Some(value)
}

fn node_builtin_spec_matches_binding(spec: &str, binding: &str) -> bool {
    let spec = spec.strip_prefix("node:").unwrap_or(spec);
    spec == binding || (binding == "path" && matches!(spec, "path/posix" | "path/win32"))
}

fn is_observed_node_builtin_name(name: &str, shadowed: &BTreeSet<String>) -> bool {
    !shadowed.contains(name)
        && matches!(name, "fs" | "path" | "os" | "crypto" | "Buffer" | "process")
}

fn collect_aot_imports(ir: &IrDocument) -> BTreeSet<&'static str> {
    let mut imports = BTreeSet::new();
    imports.insert("strconv");
    for module in &ir.modules {
        if let Some(executable) = &module.executable {
            for stmt in &executable.stmts {
                collect_stmt_imports(stmt, &mut imports);
            }
        }
    }
    imports
}

fn collect_stmt_imports(stmt: &JsStmt, imports: &mut BTreeSet<&'static str>) {
    match stmt {
        JsStmt::Expr { expr } => collect_expr_imports(expr, imports),
        JsStmt::VarDecl {
            init: Some(init), ..
        } => collect_expr_imports(init, imports),
        JsStmt::VarDecl { init: None, .. } => {}
        JsStmt::Return { value: Some(expr) } | JsStmt::Throw { value: expr } => {
            collect_expr_imports(expr, imports)
        }
        JsStmt::Return { value: None } => {}
        JsStmt::FunctionDecl { body, .. } => {
            for stmt in body {
                collect_stmt_imports(stmt, imports);
            }
        }
        JsStmt::If {
            test,
            consequent,
            alternate,
        } => {
            collect_expr_imports(test, imports);
            for stmt in consequent {
                collect_stmt_imports(stmt, imports);
            }
            for stmt in alternate {
                collect_stmt_imports(stmt, imports);
            }
        }
        JsStmt::For {
            init,
            test,
            update,
            body,
        } => {
            for stmt in init {
                collect_stmt_imports(stmt, imports);
            }
            if let Some(test) = test {
                collect_expr_imports(test, imports);
            }
            if let Some(update) = update {
                collect_expr_imports(update, imports);
            }
            for stmt in body {
                collect_stmt_imports(stmt, imports);
            }
        }
        _ => {}
    }
}

fn collect_expr_imports(expr: &JsExpr, imports: &mut BTreeSet<&'static str>) {
    match expr {
        JsExpr::Call { callee, args, .. } => {
            if is_process_cwd_call(callee, args) {
                imports.insert("os");
            }
            if is_process_uid_gid_call(callee, args) || is_process_chdir_call(callee, args) {
                imports.insert("os");
            }
            if is_console_log(callee) {
                imports.insert("fmt");
            }
            if is_console_error(callee) {
                imports.insert("fmt");
                imports.insert("os");
            }
            if is_json_stringify(callee) {
                imports.insert("encoding/json");
            }
            if is_string_cast_call(callee, args) {
                imports.insert("strconv");
            }
            if is_array_map_to_string_call(callee, args) {
                imports.insert("strconv");
            }
            if is_regexp_test_call(callee, args) {
                imports.insert("regexp");
                if regexp_test_needs_to_string_helper(args) {
                    imports.insert("strconv");
                }
            }
            if is_string_match_call(callee, args) {
                imports.insert("regexp");
            }
            if is_node_path_string_call(callee, args) {
                imports.insert("path/filepath");
                if is_node_path_basename_call(callee, args) {
                    imports.insert("strings");
                }
            }
            if is_node_path_bool_call(callee, args) {
                imports.insert("path/filepath");
            }
            if is_node_path_parse_call(callee, args) {
                imports.insert("os");
                imports.insert("path/filepath");
                imports.insert("strings");
            }
            if is_node_os_homedir_call(callee, args) {
                imports.insert("os");
            }
            if is_node_fs_exists_sync_call(callee, args) {
                imports.insert("os");
            }
            if is_node_fs_stat_sync_call(callee, args) {
                imports.insert("os");
            }
            if is_node_buffer_from_call(callee, args) {
                imports.insert("encoding/base64");
                imports.insert("encoding/hex");
            }
            if is_node_buffer_is_buffer_call(callee, args) {
                imports.insert("encoding/base64");
                imports.insert("encoding/hex");
            }
            if call_uses_strings_import(callee) {
                imports.insert("strings");
            }
            if is_string_array_join_call(callee, args) {
                imports.insert("strings");
            }
            if string_method_name(callee).is_some() {
                imports.insert("strconv");
            }
            collect_expr_imports(callee, imports);
            for arg in args {
                collect_expr_imports(arg, imports);
            }
        }
        JsExpr::Array { items } => {
            for item in items {
                collect_expr_imports(item, imports);
            }
        }
        JsExpr::Object { props } => {
            for prop in props {
                collect_expr_imports(&prop.value, imports);
            }
        }
        JsExpr::Binary { left, right, .. } => {
            collect_expr_imports(left, imports);
            collect_expr_imports(right, imports);
        }
        JsExpr::Unary { arg, .. }
        | JsExpr::Await { arg }
        | JsExpr::Update { arg, .. }
        | JsExpr::Spread { arg }
        | JsExpr::ObjectRest { object: arg, .. } => collect_expr_imports(arg, imports),
        JsExpr::Conditional {
            test,
            consequent,
            alternate,
        } => {
            collect_expr_imports(test, imports);
            collect_expr_imports(consequent, imports);
            collect_expr_imports(alternate, imports);
        }
        expr if is_process_stdout_is_tty(expr)
            || process_env_lookup_name(expr).is_some()
            || is_process_env_ref(expr)
            || is_process_exec_path_expr(expr)
            || is_process_stdio_ref(expr).is_some()
            || is_process_function_ref(expr).is_some() =>
        {
            imports.insert("os");
        }
        expr if is_process_cwd_ref(expr) => {
            imports.insert("os");
        }
        expr if is_process_platform_expr(expr) || is_process_arch_expr(expr) => {
            imports.insert("runtime");
        }
        expr if is_node_path_sep_expr(expr) => {
            imports.insert("os");
        }
        expr if is_node_path_delimiter_expr(expr) => {
            imports.insert("runtime");
        }
        JsExpr::Member { object, .. } => collect_expr_imports(object, imports),
        _ => {}
    }
}

fn render_go_imports(imports: &BTreeSet<&'static str>) -> String {
    if imports.len() == 1 {
        return format!("import {:?}", imports.iter().next().expect("single import"));
    }
    format!(
        "import (\n{}\n)",
        imports
            .iter()
            .map(|import| format!("\t{import:?}"))
            .collect::<Vec<_>>()
            .join("\n")
    )
}

fn render_aot_helpers(imports: &BTreeSet<&'static str>) -> String {
    let mut helpers = vec![
        r#"func tsgodownStringArrayAt(values []string, index float64) string {
	offset := int(index)
	if values == nil || offset < 0 || offset >= len(values) {
		return ""
	}
	return values[offset]
}

func tsgodownStringArraySet(values []string, index float64, value string) []string {
	offset := int(index)
	if offset < 0 {
		return values
	}
	for len(values) <= offset {
		values = append(values, "")
	}
	values[offset] = value
	return values
}

func tsgodownStringArrayAdd(values []string, index float64, value string) []string {
	current := tsgodownStringArrayAt(values, index)
	return tsgodownStringArraySet(values, index, current+value)
}

func tsgodownStringCharAt(value string, index float64) string {
	chars := []rune(value)
	offset := int(index)
	if offset < 0 || offset >= len(chars) {
		return ""
	}
	return string(chars[offset])
}

func tsgodownStringSlice(value string, start float64, endValues ...float64) string {
	chars := []rune(value)
	length := len(chars)
	from := int(start)
	if from < 0 {
		from = length + from
	}
	if from < 0 {
		from = 0
	}
	if from > length {
		from = length
	}
	to := length
	if len(endValues) > 0 {
		to = int(endValues[0])
		if to < 0 {
			to = length + to
		}
		if to < 0 {
			to = 0
		}
		if to > length {
			to = length
		}
	}
	if to < from {
		to = from
	}
	return string(chars[from:to])
}
"#
        .to_string(),
    ];
    if imports.contains("encoding/json") {
        helpers.push(
            r#"func tsgodownJSONStringify(value any) string {
	bytes, err := json.MarshalIndent(value, "", "  ")
	if err != nil {
		return ""
	}
	return string(bytes)
}
"#
            .to_string(),
        );
    }
    if imports.contains("encoding/base64") || imports.contains("encoding/hex") {
        helpers.push(
            r#"func tsgodownBufferFromString(value string, encoding string) []byte {
	switch encoding {
	case "", "utf8", "utf-8":
		return []byte(value)
	case "hex":
		decoded, err := hex.DecodeString(value)
		if err != nil {
			return []byte{}
		}
		return decoded
	case "base64":
		decoded, err := base64.StdEncoding.DecodeString(value)
		if err != nil {
			return []byte{}
		}
		return decoded
	default:
		return []byte(value)
	}
}

func tsgodownBufferIsBuffer(value any) bool {
	_, ok := value.([]byte)
	return ok
}
"#
            .to_string(),
        );
    }
    if imports.contains("strconv") {
        helpers.push(
            r#"func tsgodownToString(value any) string {
	switch value := value.(type) {
	case nil:
		return "undefined"
	case bool:
		if value {
			return "true"
		}
		return "false"
	case float64:
		return strconv.FormatFloat(value, 'f', -1, 64)
	case int:
		return strconv.Itoa(value)
	case int64:
		return strconv.FormatInt(value, 10)
	case string:
		return value
	default:
		return "[object Object]"
	}
}

func tsgodownStringArrayFromAny(value any) []string {
	switch value := value.(type) {
	case []string:
		out := make([]string, len(value))
		copy(out, value)
		return out
	case []any:
		out := make([]string, len(value))
		for index, item := range value {
			out[index] = tsgodownToString(item)
		}
		return out
	default:
		return []string{}
	}
}
"#
            .to_string(),
        );
    }
    if imports.contains("regexp") {
        helpers.push(
            r#"func tsgodownStringMatch(value string, pattern string) []string {
	matches := regexp.MustCompile(pattern).FindStringSubmatch(value)
	if matches == nil {
		return nil
	}
	return matches
}
"#
            .to_string(),
        );
    }
    if imports.contains("os") {
        helpers.push(
            r#"func tsgodownStdoutIsTTY() bool {
	info, err := os.Stdout.Stat()
	return err == nil && (info.Mode()&os.ModeCharDevice) != 0
}

func tsgodownProcessCwd() string {
	cwd, err := os.Getwd()
	if err != nil {
		return ""
	}
	return cwd
}

func tsgodownProcessExecPath() string {
	path, err := os.Executable()
	if err != nil {
		return ""
	}
	return path
}

func tsgodownProcessChdir(path string) any {
	_ = os.Chdir(path)
	return nil
}

func tsgodownProcessEnv() map[string]any {
	env := map[string]any{}
	for _, pair := range os.Environ() {
		for index, char := range pair {
			if char == '=' {
				env[pair[:index]] = pair[index+1:]
				break
			}
		}
	}
	return env
}

func tsgodownOsHomedir() string {
	home, err := os.UserHomeDir()
	if err != nil {
		return ""
	}
	return home
}

func tsgodownFsExistsSync(path string) bool {
	_, err := os.Stat(path)
	return err == nil
}
"#
            .to_string(),
        );
    }
    if imports.contains("runtime") {
        helpers.push(
            r#"func tsgodownProcessPlatform() string {
	if runtime.GOOS == "windows" {
		return "win32"
	}
	return runtime.GOOS
}

func tsgodownProcessArch() string {
	switch runtime.GOARCH {
	case "amd64":
		return "x64"
	case "386":
		return "ia32"
	default:
		return runtime.GOARCH
	}
}

func tsgodownPathDelimiter() string {
	if runtime.GOOS == "windows" {
		return ";"
	}
	return ":"
}
"#
            .to_string(),
        );
    }
    helpers.join("\n")
}

fn can_aot_module_graph(ir: &IrDocument) -> bool {
    ir.modules.iter().all(|module| {
        module.executable.is_some()
            && module.imports.iter().all(|import| {
                matches!(import.kind.as_str(), "esm" | "cjs")
                    && (import.resolved.is_some() || is_node_builtin_spec(&import.spec))
            })
    })
}

fn collect_module_functions(ir: &IrDocument) -> BTreeMap<(String, String), AotFunction> {
    let mut functions = BTreeMap::new();
    let Some(entry) = entry_module(ir) else {
        return functions;
    };
    for module in &ir.modules {
        let Some(executable) = &module.executable else {
            continue;
        };
        for stmt in &executable.stmts {
            if let Some(parts) = function_parts(stmt) {
                let go_name = function_go_name(module, entry, parts.name);
                functions.insert(
                    (module.id.clone(), parts.name.clone()),
                    AotFunction {
                        params: parts.params.clone(),
                        param_kinds: infer_function_param_kinds(parts.params, parts.body),
                        rest_param: parts.rest_param.clone(),
                        r#async: *parts.r#async,
                        generator: *parts.generator,
                        body: parts.body.clone(),
                        go_name,
                    },
                );
            }
            if let Some(parts) = cjs_default_function_expr(stmt) {
                let go_name = function_go_name(module, entry, CJS_DEFAULT_EXPORT_FUNCTION);
                functions.insert(
                    (module.id.clone(), CJS_DEFAULT_EXPORT_FUNCTION.to_string()),
                    AotFunction {
                        params: parts.params.clone(),
                        param_kinds: infer_function_param_kinds(parts.params, parts.body),
                        rest_param: parts.rest_param.clone(),
                        r#async: *parts.r#async,
                        generator: *parts.generator,
                        body: parts.body.clone(),
                        go_name,
                    },
                );
            }
        }
    }
    functions
}

fn collect_module_classes(ir: &IrDocument) -> BTreeMap<(String, String), AotClass> {
    let mut classes = BTreeMap::new();
    let Some(entry) = entry_module(ir) else {
        return classes;
    };
    for module in &ir.modules {
        let Some(executable) = &module.executable else {
            continue;
        };
        for stmt in &executable.stmts {
            if let Some(class) = collect_class(module, entry, stmt) {
                classes.insert((module.id.clone(), class.name.clone()), class);
            }
        }
    }
    classes
}

fn collect_class(module: &Module, entry: &Module, stmt: &JsStmt) -> Option<AotClass> {
    let JsStmt::ClassDecl {
        name,
        super_class,
        methods,
    } = stmt
    else {
        return None;
    };
    if super_class.is_some() {
        return None;
    }
    let go_name = if module.id == entry.id {
        sanitize_go_identifier(name)
    } else {
        module_member_go_name(module, name)
    };
    let mut fields = BTreeMap::new();
    let mut constructor_params = Vec::new();
    let mut constructor_values = Vec::new();
    let mut class_methods = BTreeMap::new();
    for method in methods {
        if method.r#async || method.generator || method.rest_param.is_some() || method.is_static {
            return None;
        }
        if method.kind == "constructor" {
            for param in &method.params {
                constructor_params.push(param.clone());
            }
            for stmt in &method.body {
                let JsStmt::Expr { expr } = stmt else {
                    return None;
                };
                let JsExpr::Assign { op, left, right } = expr else {
                    return None;
                };
                if op != "=" {
                    return None;
                }
                let property = this_member_property(left)?;
                let kind = match right.as_ref() {
                    JsExpr::Value {
                        value: JsValue::String { .. },
                    } => AotSlotKind::String,
                    JsExpr::Value {
                        value: JsValue::Number { .. },
                    } => AotSlotKind::Number,
                    JsExpr::Value {
                        value: JsValue::Bool { .. },
                    } => AotSlotKind::Bool,
                    JsExpr::Ident { name } if method.params.contains(name) => AotSlotKind::Any,
                    _ => return None,
                };
                fields.insert(property.clone(), kind);
                constructor_values.push((property, right.as_ref().clone()));
            }
            continue;
        }
        if method.kind != "method" || method.body.len() != 1 {
            return None;
        }
        let JsStmt::Return { value: Some(value) } = &method.body[0] else {
            return None;
        };
        class_methods.insert(
            method.name.clone(),
            AotMethod {
                params: method.params.clone(),
                return_expr: value.clone(),
            },
        );
    }
    Some(AotClass {
        name: name.clone(),
        go_name,
        fields,
        constructor_params,
        constructor_values,
        methods: class_methods,
    })
}

fn collect_module_default_exports(
    ir: &IrDocument,
    module_functions: &BTreeMap<(String, String), AotFunction>,
) -> BTreeMap<String, AotFunction> {
    let mut exports = BTreeMap::new();
    for module in &ir.modules {
        let Some(executable) = &module.executable else {
            continue;
        };
        for stmt in &executable.stmts {
            let JsStmt::Expr { expr } = stmt else {
                continue;
            };
            let JsExpr::Assign { op, left, right } = expr else {
                continue;
            };
            if op != "=" || !is_module_exports_member(left) {
                continue;
            }
            let function = match right.as_ref() {
                JsExpr::Ident { name } => module_functions.get(&(module.id.clone(), name.clone())),
                JsExpr::Function { .. } => module_functions
                    .get(&(module.id.clone(), CJS_DEFAULT_EXPORT_FUNCTION.to_string())),
                _ => None,
            };
            if let Some(function) = function {
                exports.insert(module.id.clone(), function.clone());
            }
        }
    }
    exports
}

fn collect_module_default_class_exports(
    ir: &IrDocument,
    module_classes: &BTreeMap<(String, String), AotClass>,
) -> BTreeMap<String, AotClass> {
    let mut exports = BTreeMap::new();
    for module in &ir.modules {
        let Some(executable) = &module.executable else {
            continue;
        };
        for stmt in &executable.stmts {
            let JsStmt::Expr { expr } = stmt else {
                continue;
            };
            let JsExpr::Assign { op, left, right } = expr else {
                continue;
            };
            if op != "=" || !is_module_exports_member(left) {
                continue;
            }
            let JsExpr::Ident { name } = right.as_ref() else {
                continue;
            };
            if let Some(class) = module_classes.get(&(module.id.clone(), name.clone())) {
                exports.insert(module.id.clone(), class.clone());
            }
        }
    }
    exports
}

fn collect_module_named_exports(
    ir: &IrDocument,
    module_functions: &BTreeMap<(String, String), AotFunction>,
    module_default_exports: &BTreeMap<String, AotFunction>,
    module_object_exports: &BTreeMap<(String, String), BTreeMap<String, AotFunction>>,
) -> BTreeMap<String, BTreeMap<String, AotFunction>> {
    let mut exports = BTreeMap::new();
    for module in &ir.modules {
        let Some(executable) = &module.executable else {
            continue;
        };
        for stmt in &executable.stmts {
            let JsStmt::Expr { expr } = stmt else {
                continue;
            };
            let JsExpr::Assign { op, left, right } = expr else {
                continue;
            };
            if op != "=" {
                continue;
            }
            if is_module_exports_member(left) {
                if let Some(object_exports) = module_exported_object_functions(
                    ir,
                    module,
                    right,
                    module_functions,
                    module_default_exports,
                    module_object_exports,
                ) {
                    exports
                        .entry(module.id.clone())
                        .or_insert_with(BTreeMap::new)
                        .extend(object_exports);
                }
                continue;
            }
            let Some(exported_name) = cjs_named_export_property(left) else {
                continue;
            };
            let function = match right.as_ref() {
                JsExpr::Ident { name } => module_functions
                    .get(&(module.id.clone(), name.clone()))
                    .cloned(),
                JsExpr::Member {
                    object,
                    property,
                    property_expr: None,
                    ..
                } => {
                    let JsExpr::Ident { name } = object.as_ref() else {
                        continue;
                    };
                    module_object_exports
                        .get(&(module.id.clone(), name.clone()))
                        .and_then(|object_exports| object_exports.get(property))
                        .cloned()
                }
                _ => None,
            };
            let Some(function) = function else {
                continue;
            };
            exports
                .entry(module.id.clone())
                .or_insert_with(BTreeMap::new)
                .insert(exported_name, function.clone());
        }
    }
    exports
}

fn module_exported_object_functions(
    ir: &IrDocument,
    module: &Module,
    expr: &JsExpr,
    module_functions: &BTreeMap<(String, String), AotFunction>,
    module_default_exports: &BTreeMap<String, AotFunction>,
    module_object_exports: &BTreeMap<(String, String), BTreeMap<String, AotFunction>>,
) -> Option<BTreeMap<String, AotFunction>> {
    match expr {
        JsExpr::Ident { name } => module_object_exports
            .get(&(module.id.clone(), name.clone()))
            .cloned(),
        JsExpr::Object { props } => Some(collect_object_function_props(
            ir,
            module,
            props,
            module_functions,
            module_default_exports,
        )),
        _ => None,
    }
}

fn collect_module_object_function_exports(
    ir: &IrDocument,
    module_functions: &BTreeMap<(String, String), AotFunction>,
    module_default_exports: &BTreeMap<String, AotFunction>,
) -> BTreeMap<(String, String), BTreeMap<String, AotFunction>> {
    let mut objects = BTreeMap::new();
    for module in &ir.modules {
        let Some(executable) = &module.executable else {
            continue;
        };
        for stmt in &executable.stmts {
            let JsStmt::VarDecl {
                name,
                init: Some(JsExpr::Object { props }),
            } = stmt
            else {
                continue;
            };
            let functions = collect_object_function_props(
                ir,
                module,
                props,
                module_functions,
                module_default_exports,
            );
            if !functions.is_empty() {
                objects.insert((module.id.clone(), name.clone()), functions);
            }
        }
    }
    objects
}

fn collect_object_function_props(
    ir: &IrDocument,
    module: &Module,
    props: &[crate::contract::JsObjectProp],
    module_functions: &BTreeMap<(String, String), AotFunction>,
    module_default_exports: &BTreeMap<String, AotFunction>,
) -> BTreeMap<String, AotFunction> {
    let mut functions = BTreeMap::new();
    for prop in props {
        if prop.spread || prop.key_expr.is_some() {
            continue;
        }
        if let Some(function) = resolve_module_function_binding(
            ir,
            module,
            &prop.value,
            module_functions,
            module_default_exports,
        ) {
            functions.insert(prop.key.clone(), function);
        }
    }
    functions
}

fn resolve_module_function_binding(
    ir: &IrDocument,
    module: &Module,
    expr: &JsExpr,
    module_functions: &BTreeMap<(String, String), AotFunction>,
    module_default_exports: &BTreeMap<String, AotFunction>,
) -> Option<AotFunction> {
    let JsExpr::Ident { name } = expr else {
        return None;
    };
    if let Some(function) = module_functions.get(&(module.id.clone(), name.clone())) {
        return Some(function.clone());
    }
    for import in &module.imports {
        if import.kind != "cjs" {
            continue;
        }
        if !import.bindings.iter().any(|binding| binding.local == *name) {
            continue;
        }
        let resolved = import.resolved.as_ref()?;
        let imported_module = ir
            .modules
            .iter()
            .find(|candidate| &candidate.id == resolved)?;
        if let Some(function) = module_default_exports.get(&imported_module.id) {
            return Some(function.clone());
        }
    }
    None
}

fn collect_module_slots(
    ir: &IrDocument,
    module_functions: &BTreeMap<(String, String), AotFunction>,
) -> BTreeMap<(String, String), AotModuleSlot> {
    let mut slots = BTreeMap::new();
    for module in &ir.modules {
        let Some(executable) = &module.executable else {
            continue;
        };
        let Some(state) = module_aot_state(
            module,
            ir,
            &AotModuleContext {
                functions: module_functions,
                classes: &BTreeMap::new(),
                default_exports: &BTreeMap::new(),
                default_class_exports: &BTreeMap::new(),
                named_exports: &BTreeMap::new(),
                slots: &BTreeMap::new(),
            },
        ) else {
            continue;
        };
        for stmt in &executable.stmts {
            let JsStmt::VarDecl {
                name,
                init: Some(init),
            } = stmt
            else {
                continue;
            };
            let Some((kind, rendered, go_type)) = render_typed_slot_expr(init, &state) else {
                continue;
            };
            slots.insert(
                (module.id.clone(), name.clone()),
                AotModuleSlot {
                    kind,
                    go_name: module_member_go_name(module, name),
                    go_type,
                    rendered,
                },
            );
        }
    }
    slots
}

fn render_module_decls(
    ir: &IrDocument,
    module_functions: &BTreeMap<(String, String), AotFunction>,
    module_classes: &BTreeMap<(String, String), AotClass>,
    module_slots: &BTreeMap<(String, String), AotModuleSlot>,
) -> Option<Vec<String>> {
    let mut declarations = Vec::new();
    let module_default_exports = collect_module_default_exports(ir, module_functions);
    let module_default_class_exports = collect_module_default_class_exports(ir, module_classes);
    let module_object_exports =
        collect_module_object_function_exports(ir, module_functions, &module_default_exports);
    let module_named_exports = collect_module_named_exports(
        ir,
        module_functions,
        &module_default_exports,
        &module_object_exports,
    );
    for module in &ir.modules {
        let state = module_aot_state(
            module,
            ir,
            &AotModuleContext {
                functions: module_functions,
                classes: module_classes,
                default_exports: &module_default_exports,
                default_class_exports: &module_default_class_exports,
                named_exports: &module_named_exports,
                slots: module_slots,
            },
        )?;
        for stmt in &module.executable.as_ref()?.stmts {
            if let JsStmt::ClassDecl { name, .. } = stmt {
                let class = module_classes.get(&(module.id.clone(), name.clone()))?;
                declarations.push(render_class_decl(class)?);
            }
        }
        for stmt in &module.executable.as_ref()?.stmts {
            if let JsStmt::VarDecl { name, .. } = stmt {
                if module_functions.contains_key(&(module.id.clone(), name.clone())) {
                    continue;
                }
                if !is_exported_name(module, name) {
                    continue;
                }
                let Some(slot) = module_slots.get(&(module.id.clone(), name.clone())) else {
                    continue;
                };
                declarations.push(format!(
                    "var {} {} = {}",
                    slot.go_name, slot.go_type, slot.rendered
                ));
            }
        }
        for stmt in &module.executable.as_ref()?.stmts {
            if let Some(parts) = function_parts(stmt) {
                let function = module_functions.get(&(module.id.clone(), parts.name.clone()))?;
                declarations.push(render_function_decl(function, &state)?);
            }
            if cjs_default_function_expr(stmt).is_some() {
                let function = module_functions
                    .get(&(module.id.clone(), CJS_DEFAULT_EXPORT_FUNCTION.to_string()))?;
                declarations.push(render_function_decl(function, &state)?);
            }
        }
    }
    Some(declarations)
}

fn module_aot_state(
    module: &Module,
    ir: &IrDocument,
    context: &AotModuleContext<'_>,
) -> Option<AotState> {
    let mut state = AotState::default();
    for stmt in &module.executable.as_ref()?.stmts {
        if let Some(parts) = function_parts(stmt) {
            let function = context
                .functions
                .get(&(module.id.clone(), parts.name.clone()))?;
            state.functions.insert(parts.name.clone(), function.clone());
        }
        if let JsStmt::VarDecl { name, .. } = stmt {
            if let Some(slot) = context.slots.get(&(module.id.clone(), name.clone())) {
                state.bind_slot(name, slot.go_name.clone(), slot.kind);
            }
        }
        if let JsStmt::ClassDecl { name, .. } = stmt {
            let class = context.classes.get(&(module.id.clone(), name.clone()))?;
            state.classes.insert(name.clone(), class.clone());
        }
    }
    for import in &module.imports {
        if import.resolved.is_none() && is_node_builtin_spec(&import.spec) {
            for binding in &import.bindings {
                state.builtin_bindings.insert(binding.local.clone());
            }
            continue;
        }
        let resolved = import.resolved.as_ref()?;
        let imported_module = ir
            .modules
            .iter()
            .find(|candidate| &candidate.id == resolved)?;
        for binding in &import.bindings {
            if import.kind == "cjs" {
                if let Some(function) = context.default_exports.get(&imported_module.id) {
                    state
                        .functions
                        .insert(binding.local.clone(), function.clone());
                    continue;
                }
                if let Some(class) = context.default_class_exports.get(&imported_module.id) {
                    state.classes.insert(binding.local.clone(), class.clone());
                    continue;
                }
                let Some(named) = context.named_exports.get(&imported_module.id) else {
                    continue;
                };
                for (property, function) in named {
                    state
                        .namespace_functions
                        .insert((binding.local.clone(), property.clone()), function.clone());
                }
                continue;
            }
            let imported = binding.imported.as_deref().unwrap_or(&binding.local);
            if let Some(function) = context
                .functions
                .get(&(imported_module.id.clone(), imported.to_string()))
            {
                state
                    .functions
                    .insert(binding.local.clone(), function.clone());
                continue;
            }
            if let Some(class) = context
                .classes
                .get(&(imported_module.id.clone(), imported.to_string()))
            {
                state.classes.insert(binding.local.clone(), class.clone());
                continue;
            }
            let slot = context
                .slots
                .get(&(imported_module.id.clone(), imported.to_string()))?;
            state.bind_slot(&binding.local, slot.go_name.clone(), slot.kind);
        }
    }
    Some(state)
}

fn is_exported_name(module: &Module, name: &str) -> bool {
    module.exports.iter().any(|exported| exported == name)
}

fn is_module_exports_member(expr: &JsExpr) -> bool {
    matches!(
        expr,
        JsExpr::Member {
            object,
            property,
            property_expr: None,
            ..
        } if matches!(object.as_ref(), JsExpr::Ident { name } if name == "module") && property == "exports"
    )
}

fn is_cjs_export_target(expr: &JsExpr) -> bool {
    is_module_exports_member(expr) || cjs_named_export_property(expr).is_some()
}

fn cjs_named_export_property(expr: &JsExpr) -> Option<String> {
    let JsExpr::Member {
        object,
        property,
        property_expr: None,
        ..
    } = expr
    else {
        return None;
    };
    match object.as_ref() {
        JsExpr::Ident { name } if name == "exports" => Some(property.clone()),
        object if is_module_exports_member(object) => Some(property.clone()),
        _ => None,
    }
}

fn this_member_property(expr: &JsExpr) -> Option<String> {
    let JsExpr::Member {
        object,
        property,
        property_expr: None,
        ..
    } = expr
    else {
        return None;
    };
    if matches!(object.as_ref(), JsExpr::This) {
        Some(property.clone())
    } else {
        None
    }
}

fn module_go_prefix(module: &Module) -> String {
    let raw = module
        .source_path
        .replace(['/', '.', '-'], "_")
        .trim_matches('_')
        .to_string();
    let sanitized = sanitize_go_identifier(&raw);
    if sanitized == "irSnapshotJSON" {
        "module".to_string()
    } else {
        sanitized
    }
}

fn function_go_name(module: &Module, entry: &Module, name: &str) -> String {
    if module.id == entry.id {
        sanitize_go_identifier(name)
    } else {
        module_member_go_name(module, name)
    }
}

fn module_member_go_name(module: &Module, name: &str) -> String {
    format!(
        "{}_{}",
        module_go_prefix(module),
        sanitize_go_identifier(name)
    )
}

#[derive(Default)]
struct AotState {
    go_imports: BTreeSet<&'static str>,
    bindings: BTreeSet<String>,
    binding_refs: BTreeMap<String, String>,
    numeric_bindings: BTreeSet<String>,
    string_bindings: BTreeSet<String>,
    bool_bindings: BTreeSet<String>,
    bytes_bindings: BTreeSet<String>,
    string_array_bindings: BTreeSet<String>,
    string_function_bindings: BTreeSet<String>,
    object_bindings: BTreeMap<String, AotObject>,
    class_instance_bindings: BTreeMap<String, String>,
    current_receiver: Option<String>,
    current_fields: BTreeMap<String, AotSlotKind>,
    functions: BTreeMap<String, AotFunction>,
    classes: BTreeMap<String, AotClass>,
    namespace_functions: BTreeMap<(String, String), AotFunction>,
    builtin_bindings: BTreeSet<String>,
}

impl AotState {
    fn bind_slot(&mut self, name: &str, go_ref: String, kind: AotSlotKind) {
        self.bindings.insert(name.to_string());
        self.binding_refs.insert(name.to_string(), go_ref);
        match kind {
            AotSlotKind::Any => {}
            AotSlotKind::Bool => {
                self.bool_bindings.insert(name.to_string());
            }
            AotSlotKind::Number => {
                self.numeric_bindings.insert(name.to_string());
            }
            AotSlotKind::String => {
                self.string_bindings.insert(name.to_string());
            }
            AotSlotKind::Bytes => {
                self.bytes_bindings.insert(name.to_string());
            }
            AotSlotKind::StringArray => {
                self.string_array_bindings.insert(name.to_string());
            }
            AotSlotKind::BoolFunction => {}
            AotSlotKind::StringFunction => {
                self.string_function_bindings.insert(name.to_string());
            }
        }
    }
}

fn clone_aot_state(state: &AotState) -> AotState {
    AotState {
        go_imports: state.go_imports.clone(),
        bindings: state.bindings.clone(),
        binding_refs: state.binding_refs.clone(),
        numeric_bindings: state.numeric_bindings.clone(),
        string_bindings: state.string_bindings.clone(),
        bool_bindings: state.bool_bindings.clone(),
        bytes_bindings: state.bytes_bindings.clone(),
        string_array_bindings: state.string_array_bindings.clone(),
        string_function_bindings: state.string_function_bindings.clone(),
        object_bindings: state.object_bindings.clone(),
        class_instance_bindings: state.class_instance_bindings.clone(),
        current_receiver: state.current_receiver.clone(),
        current_fields: state.current_fields.clone(),
        functions: state.functions.clone(),
        classes: state.classes.clone(),
        namespace_functions: state.namespace_functions.clone(),
        builtin_bindings: state.builtin_bindings.clone(),
    }
}

#[derive(Clone)]
struct AotFunction {
    params: Vec<String>,
    param_kinds: Vec<AotSlotKind>,
    rest_param: Option<String>,
    r#async: bool,
    generator: bool,
    body: Vec<JsStmt>,
    go_name: String,
}

struct AotFunctionParts<'a> {
    name: &'a String,
    params: &'a Vec<String>,
    rest_param: &'a Option<String>,
    r#async: &'a bool,
    generator: &'a bool,
    body: &'a Vec<JsStmt>,
}

struct AotInlineFunctionParts<'a> {
    params: &'a Vec<String>,
    rest_param: &'a Option<String>,
    r#async: &'a bool,
    generator: &'a bool,
    body: &'a Vec<JsStmt>,
}

struct AotModuleContext<'a> {
    functions: &'a BTreeMap<(String, String), AotFunction>,
    classes: &'a BTreeMap<(String, String), AotClass>,
    default_exports: &'a BTreeMap<String, AotFunction>,
    default_class_exports: &'a BTreeMap<String, AotClass>,
    named_exports: &'a BTreeMap<String, BTreeMap<String, AotFunction>>,
    slots: &'a BTreeMap<(String, String), AotModuleSlot>,
}

#[derive(Clone)]
struct AotClass {
    name: String,
    go_name: String,
    fields: BTreeMap<String, AotSlotKind>,
    constructor_params: Vec<String>,
    constructor_values: Vec<(String, JsExpr)>,
    methods: BTreeMap<String, AotMethod>,
}

#[derive(Clone)]
struct AotMethod {
    params: Vec<String>,
    return_expr: JsExpr,
}

#[derive(Clone)]
struct AotModuleSlot {
    kind: AotSlotKind,
    go_name: String,
    go_type: &'static str,
    rendered: String,
}

#[derive(Clone)]
struct AotObject {
    fields: BTreeMap<String, AotSlotKind>,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum AotSlotKind {
    Any,
    Bool,
    Bytes,
    Number,
    String,
    StringArray,
    BoolFunction,
    StringFunction,
}

fn go_type_for_slot(kind: AotSlotKind) -> &'static str {
    match kind {
        AotSlotKind::Any => "any",
        AotSlotKind::Bool => "bool",
        AotSlotKind::Bytes => "[]byte",
        AotSlotKind::Number => "float64",
        AotSlotKind::String => "string",
        AotSlotKind::StringArray => "[]string",
        AotSlotKind::BoolFunction => "func() bool",
        AotSlotKind::StringFunction => "func() string",
    }
}

fn entry_module(ir: &IrDocument) -> Option<&Module> {
    ir.modules
        .iter()
        .find(|module| module.source_path == ir.entry || module.id == ir.entry)
        .or_else(|| ir.modules.first())
}

fn render_stmt(stmt: &JsStmt, state: &mut AotState) -> Option<String> {
    match stmt {
        JsStmt::VarDecl { name, init } => {
            let ident = sanitize_go_identifier(name);
            if let Some(expr) = init {
                if matches!(expr, JsExpr::Function { .. }) && state.functions.contains_key(name) {
                    return Some(String::new());
                }
                if is_require_call(expr)
                    && (state.functions.contains_key(name)
                        || state.bindings.contains(name)
                        || state.classes.contains_key(name)
                        || state.builtin_bindings.contains(name)
                        || state
                            .namespace_functions
                            .keys()
                            .any(|(namespace, _)| namespace == name))
                {
                    return Some(String::new());
                }
                if is_require_call(expr) {
                    return Some(String::new());
                }
                if let Some(value) = render_numeric_expr(expr, state) {
                    state.bind_slot(name, ident.clone(), AotSlotKind::Number);
                    return Some(format!("var {ident} float64 = {value}"));
                }
                if let Some(value) = render_string_expr(expr, state) {
                    state.bind_slot(name, ident.clone(), AotSlotKind::String);
                    return Some(format!("var {ident} string = {value}"));
                }
                if let Some(value) = render_bool_expr(expr, state) {
                    state.bind_slot(name, ident.clone(), AotSlotKind::Bool);
                    return Some(format!("var {ident} bool = {value}"));
                }
                if let Some(value) = render_string_array_expr(expr, state) {
                    state.bind_slot(name, ident.clone(), AotSlotKind::StringArray);
                    return Some(format!("var {ident} []string = {value}"));
                }
                if let Some(value) = render_bytes_expr(expr, state) {
                    state.bind_slot(name, ident.clone(), AotSlotKind::Bytes);
                    return Some(format!("var {ident} []byte = {value}"));
                }
                if let Some(value) = render_string_function_expr(expr) {
                    state.bind_slot(name, ident.clone(), AotSlotKind::StringFunction);
                    return Some(format!("var {ident} func() string = {value}"));
                }
                if let Some((value, object)) = render_object_literal(expr, state) {
                    state.bindings.insert(name.clone());
                    state.binding_refs.insert(name.clone(), ident.clone());
                    state.object_bindings.insert(name.clone(), object);
                    return Some(format!("var {ident} = {value}"));
                }
                if let Some((value, object)) = render_node_path_parse_object(expr, state) {
                    state.bindings.insert(name.clone());
                    state.binding_refs.insert(name.clone(), ident.clone());
                    state.object_bindings.insert(name.clone(), object);
                    return Some(format!("var {ident} = {value}"));
                }
                if let Some((value, object)) = render_node_fs_stat_sync_object(expr, state) {
                    state.bindings.insert(name.clone());
                    state.binding_refs.insert(name.clone(), ident.clone());
                    state.object_bindings.insert(name.clone(), object);
                    return Some(format!("var {ident} = {value}"));
                }
                if let Some((class_name, value)) = render_new_class_expr(expr, state) {
                    state.bindings.insert(name.clone());
                    state.binding_refs.insert(name.clone(), ident.clone());
                    state
                        .class_instance_bindings
                        .insert(name.clone(), class_name);
                    return Some(format!("var {ident} = {value}"));
                }
                if let Some(value) = render_json_value_expr(expr, state) {
                    state.bindings.insert(name.clone());
                    state.binding_refs.insert(name.clone(), ident.clone());
                    return Some(format!("var {ident} any = {value}"));
                }
                let value = render_expr(expr, state)?;
                state.bindings.insert(name.clone());
                state.binding_refs.insert(name.clone(), ident.clone());
                return Some(format!("var {ident} any = {value}"));
            }
            state.bindings.insert(name.clone());
            state.binding_refs.insert(name.clone(), ident.clone());
            Some(format!("var {ident} any = nil"))
        }
        JsStmt::Expr { expr } => render_expr_stmt(expr, state),
        JsStmt::If {
            test,
            consequent,
            alternate,
        } => {
            let test_expr = test;
            let test = render_bool_expr(test_expr, state)?;
            let consequent_state = narrowed_typeof_state(test_expr, state);
            let consequent = indent_lines(&render_stmt_block(consequent, &consequent_state)?);
            if alternate.is_empty() {
                return Some(format!("if {test} {{\n{consequent}\n}}"));
            }
            let alternate = indent_lines(&render_stmt_block(alternate, state)?);
            Some(format!(
                "if {test} {{\n{consequent}\n}} else {{\n{alternate}\n}}"
            ))
        }
        JsStmt::For {
            init,
            test,
            update,
            body,
        } => render_for_stmt(init, test.as_ref(), update.as_ref(), body, state),
        JsStmt::While { test, body } => render_while_stmt(test, body, state),
        JsStmt::Break { label: None } => Some("break".to_string()),
        JsStmt::Continue { label: None } => Some("continue".to_string()),
        _ => None,
    }
}

fn render_for_stmt(
    init: &[JsStmt],
    test: Option<&JsExpr>,
    update: Option<&JsExpr>,
    body: &[JsStmt],
    state: &AotState,
) -> Option<String> {
    if init.len() > 1 {
        return None;
    }
    let mut loop_state = AotState {
        go_imports: state.go_imports.clone(),
        bindings: state.bindings.clone(),
        binding_refs: state.binding_refs.clone(),
        numeric_bindings: state.numeric_bindings.clone(),
        string_bindings: state.string_bindings.clone(),
        bool_bindings: state.bool_bindings.clone(),
        bytes_bindings: state.bytes_bindings.clone(),
        string_array_bindings: state.string_array_bindings.clone(),
        string_function_bindings: state.string_function_bindings.clone(),
        object_bindings: state.object_bindings.clone(),
        class_instance_bindings: state.class_instance_bindings.clone(),
        current_receiver: state.current_receiver.clone(),
        current_fields: state.current_fields.clone(),
        functions: state.functions.clone(),
        classes: state.classes.clone(),
        namespace_functions: state.namespace_functions.clone(),
        builtin_bindings: state.builtin_bindings.clone(),
    };
    let init = init
        .first()
        .map(|stmt| render_for_init(stmt, &mut loop_state))
        .unwrap_or_else(|| Some(String::new()))?;
    let test = test
        .map(|expr| render_bool_expr(expr, &loop_state))
        .unwrap_or_else(|| Some(String::new()))?;
    let update = update
        .map(|expr| render_for_update(expr, &loop_state))
        .unwrap_or_else(|| Some(String::new()))?;
    let body = indent_lines(&render_stmt_block_with_state(body, &loop_state)?);
    Some(format!("for {init}; {test}; {update} {{\n{body}\n}}"))
}

fn render_while_stmt(test: &JsExpr, body: &[JsStmt], state: &AotState) -> Option<String> {
    let loop_state = clone_aot_state(state);
    let test = render_bool_expr(test, &loop_state)?;
    let body = indent_lines(&render_stmt_block_with_state(body, &loop_state)?);
    Some(format!("for {test} {{\n{body}\n}}"))
}

fn render_for_init(stmt: &JsStmt, state: &mut AotState) -> Option<String> {
    let JsStmt::VarDecl {
        name,
        init: Some(init),
    } = stmt
    else {
        return None;
    };
    let value = render_numeric_expr(init, state)?;
    state.bindings.insert(name.clone());
    state
        .binding_refs
        .insert(name.clone(), sanitize_go_identifier(name));
    state.numeric_bindings.insert(name.clone());
    Some(format!(
        "{} := float64({value})",
        sanitize_go_identifier(name)
    ))
}

fn render_for_update(expr: &JsExpr, state: &AotState) -> Option<String> {
    match expr {
        JsExpr::Update { op, arg, .. } if matches!(op.as_str(), "++" | "--") => {
            let JsExpr::Ident { name } = arg.as_ref() else {
                return None;
            };
            if !state.numeric_bindings.contains(name) {
                return None;
            }
            Some(format!("{}{}", sanitize_go_identifier(name), op))
        }
        JsExpr::Assign { op, left, right } if matches!(op.as_str(), "+=" | "-=") => {
            let JsExpr::Ident { name } = left.as_ref() else {
                return None;
            };
            if !state.numeric_bindings.contains(name) {
                return None;
            }
            let right = render_numeric_expr(right, state)?;
            Some(format!("{} {} {right}", sanitize_go_identifier(name), op))
        }
        _ => None,
    }
}

fn render_stmt_block(stmts: &[JsStmt], state: &AotState) -> Option<String> {
    let block_state = AotState {
        go_imports: state.go_imports.clone(),
        bindings: state.bindings.clone(),
        binding_refs: state.binding_refs.clone(),
        numeric_bindings: state.numeric_bindings.clone(),
        string_bindings: state.string_bindings.clone(),
        bool_bindings: state.bool_bindings.clone(),
        bytes_bindings: state.bytes_bindings.clone(),
        string_array_bindings: state.string_array_bindings.clone(),
        string_function_bindings: state.string_function_bindings.clone(),
        object_bindings: state.object_bindings.clone(),
        class_instance_bindings: state.class_instance_bindings.clone(),
        current_receiver: state.current_receiver.clone(),
        current_fields: state.current_fields.clone(),
        functions: state.functions.clone(),
        classes: state.classes.clone(),
        namespace_functions: state.namespace_functions.clone(),
        builtin_bindings: state.builtin_bindings.clone(),
    };
    render_stmt_block_with_state(stmts, &block_state)
}

fn render_stmt_block_with_state(stmts: &[JsStmt], state: &AotState) -> Option<String> {
    let mut block_state = AotState {
        go_imports: state.go_imports.clone(),
        bindings: state.bindings.clone(),
        binding_refs: state.binding_refs.clone(),
        numeric_bindings: state.numeric_bindings.clone(),
        string_bindings: state.string_bindings.clone(),
        bool_bindings: state.bool_bindings.clone(),
        bytes_bindings: state.bytes_bindings.clone(),
        string_array_bindings: state.string_array_bindings.clone(),
        string_function_bindings: state.string_function_bindings.clone(),
        object_bindings: state.object_bindings.clone(),
        class_instance_bindings: state.class_instance_bindings.clone(),
        current_receiver: state.current_receiver.clone(),
        current_fields: state.current_fields.clone(),
        functions: state.functions.clone(),
        classes: state.classes.clone(),
        namespace_functions: state.namespace_functions.clone(),
        builtin_bindings: state.builtin_bindings.clone(),
    };
    stmts
        .iter()
        .map(|stmt| render_stmt(stmt, &mut block_state))
        .collect::<Option<Vec<_>>>()
        .map(|stmts| stmts.join("\n"))
}

fn render_class_decl(class: &AotClass) -> Option<String> {
    let fields = class
        .fields
        .iter()
        .map(|(name, kind)| {
            format!(
                "{} {}",
                sanitize_go_identifier(name),
                go_type_for_slot(*kind)
            )
        })
        .collect::<Vec<_>>()
        .join("\n");
    let params = class
        .constructor_params
        .iter()
        .map(|param| {
            let kind = class
                .constructor_values
                .iter()
                .find_map(|(_, expr)| match expr {
                    JsExpr::Ident { name } if name == param => Some(AotSlotKind::Any),
                    _ => None,
                })
                .unwrap_or(AotSlotKind::Any);
            format!(
                "{} {}",
                sanitize_go_identifier(param),
                go_type_for_slot(kind)
            )
        })
        .collect::<Vec<_>>()
        .join(", ");
    let constructor_fields = class
        .constructor_values
        .iter()
        .map(|(field, value)| {
            let rendered = render_class_constructor_value(value)?;
            Some(format!("{}: {rendered}", sanitize_go_identifier(field)))
        })
        .collect::<Option<Vec<_>>>()?
        .join(", ");
    let mut out = vec![format!(
        "type {} struct {{\n{}\n}}\n\nfunc new_{}({params}) *{} {{\n\treturn &{}{{{constructor_fields}}}\n}}",
        class.go_name,
        indent_lines(&fields),
        class.go_name,
        class.go_name,
        class.go_name
    )];
    for (method_name, method) in &class.methods {
        out.push(render_class_method_decl(class, method_name, method)?);
    }
    Some(out.join("\n\n"))
}

fn render_class_constructor_value(expr: &JsExpr) -> Option<String> {
    match expr {
        JsExpr::Ident { name } => Some(sanitize_go_identifier(name)),
        JsExpr::Value { value } => render_value(value),
        _ => None,
    }
}

fn render_class_method_decl(
    class: &AotClass,
    method_name: &str,
    method: &AotMethod,
) -> Option<String> {
    let mut state = AotState {
        current_receiver: Some("self".to_string()),
        current_fields: class.fields.clone(),
        ..AotState::default()
    };
    for param in &method.params {
        state.bind_slot(param, sanitize_go_identifier(param), AotSlotKind::Any);
    }
    let params = method
        .params
        .iter()
        .map(|param| format!("{} any", sanitize_go_identifier(param)))
        .collect::<Vec<_>>()
        .join(", ");
    let returned = render_expr(&method.return_expr, &state)?;
    Some(format!(
        "func (self *{}) {}({params}) any {{\n\treturn {returned}\n}}",
        class.go_name,
        sanitize_go_identifier(method_name)
    ))
}

fn infer_function_param_kinds(params: &[String], body: &[JsStmt]) -> Vec<AotSlotKind> {
    let mut kinds = params
        .iter()
        .map(|_| AotSlotKind::Number)
        .collect::<Vec<_>>();
    let param_index = params
        .iter()
        .enumerate()
        .map(|(index, param)| (param.clone(), index))
        .collect::<BTreeMap<_, _>>();
    for stmt in body {
        infer_stmt_param_kinds(stmt, &param_index, &mut kinds);
    }
    kinds
}

fn infer_stmt_param_kinds(
    stmt: &JsStmt,
    param_index: &BTreeMap<String, usize>,
    kinds: &mut [AotSlotKind],
) {
    match stmt {
        JsStmt::VarDecl {
            init: Some(expr), ..
        }
        | JsStmt::Expr { expr }
        | JsStmt::Return { value: Some(expr) }
        | JsStmt::Throw { value: expr }
        | JsStmt::Yield {
            value: Some(expr), ..
        } => infer_expr_param_kinds(expr, param_index, kinds),
        JsStmt::If {
            test,
            consequent,
            alternate,
        } => {
            infer_bool_context_param_kinds(test, param_index, kinds);
            for stmt in consequent {
                infer_stmt_param_kinds(stmt, param_index, kinds);
            }
            for stmt in alternate {
                infer_stmt_param_kinds(stmt, param_index, kinds);
            }
        }
        JsStmt::For {
            init,
            test,
            update,
            body,
        } => {
            for stmt in init {
                infer_stmt_param_kinds(stmt, param_index, kinds);
            }
            if let Some(test) = test {
                infer_bool_context_param_kinds(test, param_index, kinds);
            }
            if let Some(update) = update {
                infer_expr_param_kinds(update, param_index, kinds);
            }
            for stmt in body {
                infer_stmt_param_kinds(stmt, param_index, kinds);
            }
        }
        JsStmt::While { test, body } | JsStmt::DoWhile { test, body } => {
            infer_bool_context_param_kinds(test, param_index, kinds);
            for stmt in body {
                infer_stmt_param_kinds(stmt, param_index, kinds);
            }
        }
        _ => {}
    }
}

fn infer_expr_param_kinds(
    expr: &JsExpr,
    param_index: &BTreeMap<String, usize>,
    kinds: &mut [AotSlotKind],
) {
    match expr {
        JsExpr::Binary { op, left, right } if op == "+" => {
            if is_string_literal_like(right) {
                mark_ident_param_kind(left, param_index, kinds, AotSlotKind::String);
            }
            if is_string_literal_like(left) {
                mark_ident_param_kind(right, param_index, kinds, AotSlotKind::String);
            }
            infer_expr_param_kinds(left, param_index, kinds);
            infer_expr_param_kinds(right, param_index, kinds);
        }
        JsExpr::Binary { op, left, right } if op == "||" => {
            if is_string_literal_like(right) {
                mark_ident_param_kind(left, param_index, kinds, AotSlotKind::String);
            }
            if is_string_literal_like(left) {
                mark_ident_param_kind(right, param_index, kinds, AotSlotKind::String);
            }
            infer_expr_param_kinds(left, param_index, kinds);
            infer_expr_param_kinds(right, param_index, kinds);
        }
        JsExpr::Binary { op, left, right } if go_comparison_op(op).is_some() => {
            infer_comparison_param_kind(left, right, param_index, kinds);
            infer_comparison_param_kind(right, left, param_index, kinds);
            infer_expr_param_kinds(left, param_index, kinds);
            infer_expr_param_kinds(right, param_index, kinds);
        }
        JsExpr::Assign { left, right, .. } | JsExpr::Binary { left, right, .. } => {
            infer_expr_param_kinds(left, param_index, kinds);
            infer_expr_param_kinds(right, param_index, kinds);
        }
        JsExpr::Call { callee, args, .. } if string_method_name(callee).is_some() => {
            if let JsExpr::Member { object, .. } = callee.as_ref() {
                mark_ident_param_kind(object, param_index, kinds, AotSlotKind::String);
                infer_expr_param_kinds(object, param_index, kinds);
            }
            for arg in args {
                infer_expr_param_kinds(arg, param_index, kinds);
            }
        }
        JsExpr::Call { callee, args, .. }
            if is_string_cast_call(callee, args) || is_boolean_cast_call(callee, args) =>
        {
            mark_ident_param_kind(&args[0], param_index, kinds, AotSlotKind::Any);
            infer_expr_param_kinds(&args[0], param_index, kinds);
        }
        JsExpr::Call { callee, args, .. } if is_array_is_array_call(callee, args) => {
            mark_ident_param_kind(&args[0], param_index, kinds, AotSlotKind::Any);
            infer_expr_param_kinds(&args[0], param_index, kinds);
        }
        JsExpr::Call { callee, args, .. } if is_regexp_test_call(callee, args) => {
            mark_ident_param_kind(&args[0], param_index, kinds, AotSlotKind::Any);
            infer_expr_param_kinds(&args[0], param_index, kinds);
        }
        JsExpr::Call { callee, args, .. } => {
            infer_expr_param_kinds(callee, param_index, kinds);
            for arg in args {
                infer_expr_param_kinds(arg, param_index, kinds);
            }
        }
        JsExpr::Member { object, .. } => infer_expr_param_kinds(object, param_index, kinds),
        JsExpr::Array { items } => {
            for item in items {
                infer_expr_param_kinds(item, param_index, kinds);
            }
        }
        JsExpr::ArraySpread { items } => {
            for item in items {
                infer_expr_param_kinds(&item.value, param_index, kinds);
            }
        }
        JsExpr::Object { props } => {
            for prop in props {
                infer_expr_param_kinds(&prop.value, param_index, kinds);
            }
        }
        JsExpr::Unary { arg, .. }
        | JsExpr::Await { arg }
        | JsExpr::Update { arg, .. }
        | JsExpr::Spread { arg }
        | JsExpr::ObjectRest { object: arg, .. } => infer_expr_param_kinds(arg, param_index, kinds),
        JsExpr::Conditional {
            test,
            consequent,
            alternate,
        } => {
            infer_bool_context_param_kinds(test, param_index, kinds);
            infer_expr_param_kinds(consequent, param_index, kinds);
            infer_expr_param_kinds(alternate, param_index, kinds);
        }
        JsExpr::New { callee, args } => {
            infer_expr_param_kinds(callee, param_index, kinds);
            for arg in args {
                infer_expr_param_kinds(arg, param_index, kinds);
            }
        }
        JsExpr::Template { exprs, .. } | JsExpr::Sequence { exprs } => {
            if let JsExpr::Template { exprs, .. } = expr {
                for item in exprs {
                    mark_ident_param_kind(item, param_index, kinds, AotSlotKind::String);
                }
            }
            for expr in exprs {
                infer_expr_param_kinds(expr, param_index, kinds);
            }
        }
        _ => {}
    }
}

fn infer_comparison_param_kind(
    candidate: &JsExpr,
    other: &JsExpr,
    param_index: &BTreeMap<String, usize>,
    kinds: &mut [AotSlotKind],
) {
    match other {
        expr if is_nullish_expr(expr) => {
            mark_ident_param_kind(candidate, param_index, kinds, AotSlotKind::Any);
        }
        expr if is_string_literal_like(expr) => {
            if let JsExpr::Unary { op, arg } = candidate {
                if op == "typeof" {
                    mark_ident_param_kind(arg, param_index, kinds, AotSlotKind::Any);
                    return;
                }
            }
            mark_ident_param_kind(candidate, param_index, kinds, AotSlotKind::String);
        }
        JsExpr::Value {
            value: JsValue::Bool { .. },
        } => {
            mark_ident_param_kind(candidate, param_index, kinds, AotSlotKind::Bool);
        }
        JsExpr::Value {
            value: JsValue::Number { .. },
        } => {
            mark_ident_param_kind(candidate, param_index, kinds, AotSlotKind::Number);
        }
        _ => {}
    }
}

fn infer_bool_context_param_kinds(
    expr: &JsExpr,
    param_index: &BTreeMap<String, usize>,
    kinds: &mut [AotSlotKind],
) {
    match expr {
        JsExpr::Ident { .. } => mark_ident_param_kind(expr, param_index, kinds, AotSlotKind::Bool),
        JsExpr::Unary { op, arg } if op == "!" => {
            infer_bool_context_param_kinds(arg, param_index, kinds);
        }
        JsExpr::Binary { op, left, right } if matches!(op.as_str(), "&&" | "||") => {
            infer_bool_context_param_kinds(left, param_index, kinds);
            infer_bool_context_param_kinds(right, param_index, kinds);
        }
        _ => infer_expr_param_kinds(expr, param_index, kinds),
    }
}

fn is_string_literal_like(expr: &JsExpr) -> bool {
    match expr {
        JsExpr::Value {
            value: JsValue::String { .. },
        } => true,
        JsExpr::Template { quasis, exprs } => exprs.is_empty() && quasis.len() == 1,
        _ => false,
    }
}

fn mark_ident_param_kind(
    expr: &JsExpr,
    param_index: &BTreeMap<String, usize>,
    kinds: &mut [AotSlotKind],
    kind: AotSlotKind,
) {
    let JsExpr::Ident { name } = expr else {
        return;
    };
    let Some(index) = param_index.get(name) else {
        return;
    };
    if kinds[*index] == AotSlotKind::Any {
        return;
    }
    kinds[*index] = kind;
}

fn render_function_decl(function: &AotFunction, state: &AotState) -> Option<String> {
    if function.rest_param.is_some() || function.r#async || function.generator {
        return None;
    }
    let mut function_state = AotState {
        functions: state.functions.clone(),
        classes: state.classes.clone(),
        namespace_functions: state.namespace_functions.clone(),
        builtin_bindings: state.builtin_bindings.clone(),
        ..AotState::default()
    };
    for (param, kind) in function.params.iter().zip(function.param_kinds.iter()) {
        function_state.bind_slot(param, sanitize_go_identifier(param), *kind);
    }
    let rendered_params = function
        .params
        .iter()
        .zip(function.param_kinds.iter())
        .map(|(param, kind)| {
            format!(
                "{} {}",
                sanitize_go_identifier(param),
                go_type_for_slot(*kind)
            )
        })
        .collect::<Vec<_>>()
        .join(", ");
    let function_body = render_function_body(&function.body, &function_state)?;
    let function_body = if function_body.trim_end().ends_with("return nil") {
        function_body
    } else {
        format!("{function_body}\nreturn nil")
    };
    Some(format!(
        "func {}({rendered_params}) any {{\n{}\n}}",
        function.go_name,
        indent_lines(&function_body)
    ))
}

fn aot_function_state(function: &AotFunction, state: &AotState) -> AotState {
    let mut function_state = AotState {
        functions: state.functions.clone(),
        classes: state.classes.clone(),
        namespace_functions: state.namespace_functions.clone(),
        builtin_bindings: state.builtin_bindings.clone(),
        ..AotState::default()
    };
    for (param, kind) in function.params.iter().zip(function.param_kinds.iter()) {
        function_state.bind_slot(param, sanitize_go_identifier(param), *kind);
    }
    function_state
}

fn is_function_binding_stmt(stmt: &JsStmt) -> bool {
    matches!(
        stmt,
        JsStmt::VarDecl {
            init: Some(JsExpr::Function { .. }),
            ..
        }
    )
}

fn function_parts(stmt: &JsStmt) -> Option<AotFunctionParts<'_>> {
    match stmt {
        JsStmt::FunctionDecl {
            name,
            params,
            rest_param,
            r#async,
            generator,
            body,
        } => Some(AotFunctionParts {
            name,
            params,
            rest_param,
            r#async,
            generator,
            body,
        }),
        JsStmt::VarDecl {
            name,
            init:
                Some(JsExpr::Function {
                    params,
                    rest_param,
                    r#async,
                    generator,
                    body,
                    ..
                }),
        } => Some(AotFunctionParts {
            name,
            params,
            rest_param,
            r#async,
            generator,
            body,
        }),
        _ => None,
    }
}

fn cjs_default_function_expr(stmt: &JsStmt) -> Option<AotInlineFunctionParts<'_>> {
    let JsStmt::Expr { expr } = stmt else {
        return None;
    };
    let JsExpr::Assign { op, left, right } = expr else {
        return None;
    };
    if op != "=" || !is_module_exports_member(left) {
        return None;
    }
    let JsExpr::Function {
        params,
        rest_param,
        r#async,
        generator,
        body,
        ..
    } = right.as_ref()
    else {
        return None;
    };
    Some(AotInlineFunctionParts {
        params,
        rest_param,
        r#async,
        generator,
        body,
    })
}

fn render_function_body(body: &[JsStmt], state: &AotState) -> Option<String> {
    let mut function_state = clone_aot_state(state);
    body.iter()
        .map(|stmt| render_function_stmt(stmt, &mut function_state))
        .collect::<Option<Vec<_>>>()
        .map(|stmts| stmts.join("\n"))
}

fn function_returns_string_array(function: &AotFunction, state: &AotState) -> bool {
    let mut function_state = aot_function_state(function, state);
    let mut saw_return = false;
    collect_string_array_returns(&function.body, &mut function_state, &mut saw_return)
        .unwrap_or(false)
        && saw_return
}

fn collect_string_array_returns(
    body: &[JsStmt],
    state: &mut AotState,
    saw_return: &mut bool,
) -> Option<bool> {
    for stmt in body {
        match stmt {
            JsStmt::Return { value: Some(value) } => {
                *saw_return = true;
                if render_string_array_expr(value, state).is_none() {
                    return Some(false);
                }
            }
            JsStmt::If {
                test,
                consequent,
                alternate,
            } => {
                let consequent_state = narrowed_typeof_state(test, state);
                let mut consequent_state = clone_aot_state(&consequent_state);
                if !collect_string_array_returns(consequent, &mut consequent_state, saw_return)? {
                    return Some(false);
                }
                let mut alternate_state = clone_aot_state(state);
                if !collect_string_array_returns(alternate, &mut alternate_state, saw_return)? {
                    return Some(false);
                }
                render_function_stmt(stmt, state)?;
            }
            JsStmt::For { .. } | JsStmt::While { .. } => {
                render_function_stmt(stmt, state)?;
            }
            _ => {
                render_function_stmt(stmt, state)?;
            }
        }
    }
    Some(true)
}

fn render_function_stmt(stmt: &JsStmt, state: &mut AotState) -> Option<String> {
    match stmt {
        JsStmt::VarDecl { .. } => render_stmt(stmt, state),
        JsStmt::Expr { expr } => render_expr_stmt(expr, state),
        JsStmt::Return { value: Some(value) } => {
            let returned = render_expr(value, state)?;
            Some(format!("return {returned}"))
        }
        JsStmt::If {
            test,
            consequent,
            alternate,
        } => {
            let test_expr = test;
            let test = render_bool_expr(test_expr, state)?;
            let consequent_state = narrowed_typeof_state(test_expr, state);
            let consequent = indent_lines(&render_function_body(consequent, &consequent_state)?);
            if alternate.is_empty() {
                return Some(format!("if {test} {{\n{consequent}\n}}"));
            }
            let alternate = indent_lines(&render_function_body(alternate, state)?);
            Some(format!(
                "if {test} {{\n{consequent}\n}} else {{\n{alternate}\n}}"
            ))
        }
        JsStmt::For {
            init,
            test,
            update,
            body,
        } => render_for_stmt(init, test.as_ref(), update.as_ref(), body, state),
        JsStmt::While { test, body } => render_while_stmt(test, body, state),
        JsStmt::Break { label: None } => Some("break".to_string()),
        JsStmt::Continue { label: None } => Some("continue".to_string()),
        _ => None,
    }
}

fn render_bool_expr(expr: &JsExpr, state: &AotState) -> Option<String> {
    match expr {
        JsExpr::Value {
            value: JsValue::Bool { value },
        } => Some(value.to_string()),
        expr if is_process_stdout_is_tty(expr) => Some("tsgodownStdoutIsTTY()".to_string()),
        JsExpr::Call { callee, args, .. } if is_node_fs_exists_sync_call(callee, args) => {
            render_node_fs_exists_sync_call(callee, args, state)
        }
        JsExpr::Call { callee, args, .. } if is_node_buffer_is_buffer_call(callee, args) => {
            render_node_buffer_is_buffer_call(callee, args, state)
        }
        JsExpr::Call { callee, args, .. } if is_node_path_bool_call(callee, args) => {
            render_node_path_bool_call(callee, args, state)
        }
        JsExpr::Call { callee, args, .. } if is_array_is_array_call(callee, args) => {
            let value = render_expr(args.first()?, state)?;
            Some(format!(
                "func() bool {{ switch any({value}).(type) {{ case []string, []any: return true; default: return false }} }}()"
            ))
        }
        expr if process_env_lookup_name(expr).is_some() => {
            let value = render_process_env_lookup(expr)?;
            Some(format!("({value} != \"\")"))
        }
        JsExpr::Ident { name } if name == "process" => Some("true".to_string()),
        expr if is_process_env_ref(expr) || is_process_versions_ref(expr) => {
            Some("true".to_string())
        }
        expr if is_process_stdio_ref(expr).is_some() => render_process_stdio_bool_expr(expr),
        expr if is_process_function_ref(expr).is_some() => Some("true".to_string()),
        expr if is_node_fs_function_ref(expr).is_some() => Some("true".to_string()),
        JsExpr::Ident { name } if state.bool_bindings.contains(name) => {
            Some(go_binding_ref(name, state))
        }
        JsExpr::Ident { name } if state.numeric_bindings.contains(name) => {
            let value = go_binding_ref(name, state);
            Some(format!("({value} != 0)"))
        }
        JsExpr::Ident { name } if state.string_bindings.contains(name) => {
            let value = go_binding_ref(name, state);
            Some(format!("({value} != \"\")"))
        }
        JsExpr::Ident { name } if state.string_array_bindings.contains(name) => {
            let value = go_binding_ref(name, state);
            Some(format!("({value} != nil)"))
        }
        expr if render_string_array_index_expr(expr, state).is_some() => {
            let value = render_string_array_index_expr(expr, state)?;
            Some(format!("({value} != \"\")"))
        }
        JsExpr::Ident { name } if state.bindings.contains(name) => {
            let value = go_binding_ref(name, state);
            Some(format!(
                "func() bool {{ switch value := any({value}).(type) {{ case nil: return false; case bool: return value; case float64: return value != 0; case int: return value != 0; case int64: return value != 0; case string: return value != \"\"; default: return true }} }}()"
            ))
        }
        JsExpr::Member {
            object,
            property,
            property_expr: None,
            optional: false,
        } if static_member_kind(object, property, state) == Some(AotSlotKind::Bool) => {
            render_static_member_expr(object, property, state)
        }
        JsExpr::Binary { op, left, right } if go_comparison_op(op).is_some() => {
            render_comparison_expr(op, left, right, state)
        }
        JsExpr::Binary { op, left, right } if matches!(op.as_str(), "&&" | "||") => {
            let left = render_bool_expr(left, state)?;
            let right = render_bool_expr(right, state)?;
            Some(format!("({left} {op} {right})"))
        }
        JsExpr::Unary { op, arg } if op == "!" => {
            let arg = render_bool_expr(arg, state)?;
            Some(format!("(!{arg})"))
        }
        JsExpr::Conditional {
            test,
            consequent,
            alternate,
        } => render_conditional_expr(test, consequent, alternate, state, render_bool_expr, "bool"),
        JsExpr::Call { callee, args, .. } if is_boolean_cast_call(callee, args) => {
            render_js_to_bool_expr(args.first()?, state)
        }
        JsExpr::Call { callee, args, .. } if is_regexp_test_call(callee, args) => {
            render_regexp_test_call(callee, args, state)
        }
        JsExpr::Call { callee, args, .. } => render_bool_call_expr(callee, args, state)
            .or_else(|| render_bool_function_call(callee, args, state))
            .or_else(|| render_string_bool_method_call(callee, args, state))
            .or_else(|| render_array_bool_method_call(callee, args, state)),
        _ => None,
    }
}

fn render_expr_stmt(expr: &JsExpr, state: &mut AotState) -> Option<String> {
    match expr {
        JsExpr::Call { callee, args, .. } if is_console_log(callee) => {
            let args = args
                .iter()
                .map(|arg| render_console_arg_expr(arg, state))
                .collect::<Option<Vec<_>>>()?;
            Some(format!("fmt.Println({})", args.join(", ")))
        }
        JsExpr::Call { callee, args, .. } if is_console_error(callee) => {
            let args = args
                .iter()
                .map(|arg| render_console_arg_expr(arg, state))
                .collect::<Option<Vec<_>>>()?;
            if args.is_empty() {
                return Some("fmt.Fprintln(os.Stderr)".to_string());
            }
            Some(format!("fmt.Fprintln(os.Stderr, {})", args.join(", ")))
        }
        JsExpr::Assign { op, left, .. } if op == "=" && is_cjs_export_target(left) => {
            Some(String::new())
        }
        JsExpr::Assign { op, left, right }
            if render_string_array_assignment_stmt(op, left, right, state).is_some() =>
        {
            render_string_array_assignment_stmt(op, left, right, state)
        }
        JsExpr::Assign { op, left, right } => render_assignment_stmt(op, left, right, state),
        JsExpr::Update { op, arg, .. } => render_update_stmt(op, arg, state),
        JsExpr::Call { callee, args, .. } => {
            let call = render_call_expr(callee, args, state)?;
            Some(format!("_ = {call}"))
        }
        _ => None,
    }
}

fn render_assignment_stmt(
    op: &str,
    left: &JsExpr,
    right: &JsExpr,
    state: &AotState,
) -> Option<String> {
    let JsExpr::Ident { name } = left else {
        return None;
    };
    if !state.numeric_bindings.contains(name) {
        if state.string_bindings.contains(name) && matches!(op, "=" | "+=") {
            let right = render_string_expr(right, state)?;
            return Some(format!("{} {op} {right}", sanitize_go_identifier(name)));
        }
        if state.bool_bindings.contains(name) && op == "=" {
            let right = render_bool_expr(right, state)?;
            return Some(format!("{} = {right}", sanitize_go_identifier(name)));
        }
        if state.bindings.contains(name) && op == "=" {
            let right = render_expr(right, state)?;
            return Some(format!("{} = {right}", sanitize_go_identifier(name)));
        }
        return None;
    }
    let right = render_numeric_expr(right, state)?;
    match op {
        "=" | "+=" | "-=" | "*=" | "/=" | "%=" => {
            Some(format!("{} {op} {right}", sanitize_go_identifier(name)))
        }
        _ => None,
    }
}

fn render_string_array_assignment_stmt(
    op: &str,
    left: &JsExpr,
    right: &JsExpr,
    state: &AotState,
) -> Option<String> {
    let JsExpr::Member {
        object,
        property,
        property_expr,
        optional: false,
    } = left
    else {
        return None;
    };
    let JsExpr::Ident { name } = object.as_ref() else {
        return None;
    };
    if !state.string_array_bindings.contains(name) {
        return None;
    }
    let target = go_binding_ref(name, state);
    let index = if let Some(property_expr) = property_expr {
        render_numeric_expr(property_expr, state)?
    } else {
        number_literal(property)?
    };
    let value = render_string_expr(right, state)?;
    match op {
        "=" => Some(format!(
            "{target} = tsgodownStringArraySet({target}, {index}, {value})"
        )),
        "+=" => Some(format!(
            "{target} = tsgodownStringArrayAdd({target}, {index}, {value})"
        )),
        _ => None,
    }
}

fn render_update_stmt(op: &str, arg: &JsExpr, state: &AotState) -> Option<String> {
    let JsExpr::Ident { name } = arg else {
        return None;
    };
    if !state.numeric_bindings.contains(name) || !matches!(op, "++" | "--") {
        return None;
    }
    Some(format!("{}{}", sanitize_go_identifier(name), op))
}

fn render_new_class_expr(expr: &JsExpr, state: &AotState) -> Option<(String, String)> {
    let JsExpr::New { callee, args } = expr else {
        return None;
    };
    let JsExpr::Ident { name } = callee.as_ref() else {
        return None;
    };
    let class = state.classes.get(name)?;
    if class.constructor_params.len() != args.len() {
        return None;
    }
    let rendered_args = args
        .iter()
        .map(|arg| render_expr(arg, state))
        .collect::<Option<Vec<_>>>()?;
    Some((
        name.clone(),
        format!("new_{}({})", class.go_name, rendered_args.join(", ")),
    ))
}

fn render_console_arg_expr(expr: &JsExpr, state: &AotState) -> Option<String> {
    match expr {
        JsExpr::Value {
            value: JsValue::Null,
        } => Some("\"null\"".to_string()),
        JsExpr::Value {
            value: JsValue::Undefined,
        } => Some("\"undefined\"".to_string()),
        JsExpr::Ident { name } if name == "undefined" => Some("\"undefined\"".to_string()),
        _ => render_expr(expr, state),
    }
}

fn is_console_log(expr: &JsExpr) -> bool {
    matches!(
        expr,
        JsExpr::Member {
            object,
            property,
            property_expr: None,
            ..
        } if matches!(object.as_ref(), JsExpr::Ident { name } if name == "console") && property == "log"
    )
}

fn is_console_error(expr: &JsExpr) -> bool {
    matches!(
        expr,
        JsExpr::Member {
            object,
            property,
            property_expr: None,
            ..
        } if matches!(object.as_ref(), JsExpr::Ident { name } if name == "console") && property == "error"
    )
}

fn is_require_call(expr: &JsExpr) -> bool {
    matches!(
        expr,
        JsExpr::Call { callee, .. } if matches!(callee.as_ref(), JsExpr::Ident { name } if name == "require")
    )
}

fn is_node_builtin_spec(spec: &str) -> bool {
    let spec = spec.strip_prefix("node:").unwrap_or(spec);
    matches!(
        spec,
        "assert"
            | "assert/strict"
            | "async_hooks"
            | "buffer"
            | "child_process"
            | "constants"
            | "crypto"
            | "diagnostics_channel"
            | "events"
            | "fs"
            | "fs/promises"
            | "module"
            | "os"
            | "path"
            | "path/posix"
            | "path/win32"
            | "perf_hooks"
            | "process"
            | "querystring"
            | "stream"
            | "stream/promises"
            | "string_decoder"
            | "timers"
            | "timers/promises"
            | "tty"
            | "url"
            | "util"
            | "v8"
            | "zlib"
    )
}

fn render_expr(expr: &JsExpr, state: &AotState) -> Option<String> {
    match expr {
        JsExpr::Value { value } => render_value(value),
        JsExpr::Ident { name } if name == "undefined" => Some("nil".to_string()),
        JsExpr::Ident { name } if state.bindings.contains(name) => {
            Some(go_binding_ref(name, state))
        }
        JsExpr::Array { .. } => {
            render_string_array_expr(expr, state).or_else(|| render_json_value_expr(expr, state))
        }
        JsExpr::Binary { op, .. } if op == "+" => render_string_expr(expr, state).or_else(|| {
            let JsExpr::Binary { left, right, .. } = expr else {
                return None;
            };
            let left = render_numeric_expr(left, state)?;
            let right = render_numeric_expr(right, state)?;
            Some(format!("({left} + {right})"))
        }),
        JsExpr::Binary { op, left, right } if is_numeric_binary_op(op) => {
            let left = render_numeric_expr(left, state)?;
            let right = render_numeric_expr(right, state)?;
            Some(format!("({left} {op} {right})"))
        }
        JsExpr::Binary { op, .. } if go_comparison_op(op).is_some() => {
            render_bool_expr(expr, state)
        }
        JsExpr::Binary { op, .. } if matches!(op.as_str(), "&&" | "||") => {
            render_bool_expr(expr, state)
        }
        JsExpr::Unary { .. } => render_string_expr(expr, state)
            .or_else(|| render_numeric_expr(expr, state))
            .or_else(|| render_bool_expr(expr, state)),
        JsExpr::Conditional {
            test,
            consequent,
            alternate,
        } => render_conditional_expr(test, consequent, alternate, state, render_expr, "any"),
        JsExpr::Call { callee, args, .. } => render_string_expr(expr, state)
            .or_else(|| render_numeric_expr(expr, state))
            .or_else(|| render_bool_expr(expr, state))
            .or_else(|| render_bytes_expr(expr, state))
            .or_else(|| render_string_array_expr(expr, state))
            .or_else(|| render_call_expr(callee, args, state)),
        JsExpr::New { .. } => render_new_class_expr(expr, state).map(|(_, value)| value),
        expr if is_process_version_expr(expr) => render_process_version_expr(expr),
        expr if is_process_platform_expr(expr) => Some("tsgodownProcessPlatform()".to_string()),
        expr if is_process_arch_expr(expr) => Some("tsgodownProcessArch()".to_string()),
        expr if is_process_exec_path_expr(expr) => Some("tsgodownProcessExecPath()".to_string()),
        expr if is_process_env_ref(expr) => Some("tsgodownProcessEnv()".to_string()),
        expr if is_process_versions_ref(expr) => Some(render_process_versions_expr()),
        expr if is_process_cwd_ref(expr) => render_string_function_expr(expr),
        expr if is_process_stdio_ref(expr).is_some() => render_process_stdio_expr(expr),
        expr if is_process_function_ref(expr).is_some() => render_process_function_ref(expr),
        JsExpr::Member {
            object,
            property,
            property_expr: None,
            optional: false,
        } => render_string_expr(expr, state)
            .or_else(|| render_numeric_expr(expr, state))
            .or_else(|| render_bool_expr(expr, state))
            .or_else(|| render_static_member_expr(object, property, state)),
        JsExpr::Template { quasis, exprs } => render_template_string_expr(quasis, exprs, state),
        _ => None,
    }
}

fn render_numeric_expr(expr: &JsExpr, state: &AotState) -> Option<String> {
    match expr {
        JsExpr::Value {
            value: JsValue::Number { value },
        } => number_literal(value),
        JsExpr::Ident { name } if state.numeric_bindings.contains(name) => {
            Some(go_binding_ref(name, state))
        }
        JsExpr::Member {
            object,
            property,
            property_expr: None,
            optional: false,
        } if property == "length" && render_string_array_index_expr(object, state).is_some() => {
            let value = render_string_array_index_expr(object, state)?;
            Some(format!("float64(len([]rune({value})))"))
        }
        JsExpr::Member {
            object,
            property,
            property_expr: None,
            optional: false,
        } if static_member_kind(object, property, state) == Some(AotSlotKind::Number) => {
            render_static_member_expr(object, property, state)
        }
        JsExpr::Member {
            object,
            property,
            property_expr: None,
            optional: false,
        } if property == "length" && render_bytes_expr(object, state).is_some() => {
            let object = render_bytes_expr(object, state)?;
            Some(format!("float64(len({object}))"))
        }
        JsExpr::Member {
            object,
            property,
            property_expr: None,
            optional: false,
        } if property == "length" => {
            let object = render_string_expr(object, state)?;
            Some(format!("float64(len({object}))"))
        }
        JsExpr::Member {
            object,
            property,
            property_expr: None,
            optional: false,
        } if property.parse::<usize>().is_ok() && render_bytes_expr(object, state).is_some() => {
            let object = render_bytes_expr(object, state)?;
            let index = property.parse::<usize>().ok()?;
            Some(format!("float64({object}[{index}])"))
        }
        JsExpr::Unary { op, arg } if op == "-" => {
            let arg = render_numeric_expr(arg, state)?;
            Some(format!("(-{arg})"))
        }
        JsExpr::Binary { op, left, right } if is_numeric_binary_op(op) => {
            let left = render_numeric_expr(left, state)?;
            let right = render_numeric_expr(right, state)?;
            Some(format!("({left} {op} {right})"))
        }
        JsExpr::Conditional {
            test,
            consequent,
            alternate,
        } => render_conditional_expr(
            test,
            consequent,
            alternate,
            state,
            render_numeric_expr,
            "float64",
        ),
        JsExpr::Call { callee, args, .. } if is_process_uid_gid_call(callee, args) => {
            render_process_uid_gid_call(callee, args)
        }
        JsExpr::Call { callee, args, .. } => render_string_numeric_method_call(callee, args, state),
        _ => None,
    }
}

fn render_string_expr(expr: &JsExpr, state: &AotState) -> Option<String> {
    match expr {
        JsExpr::Value {
            value: JsValue::String { value },
        } => Some(go_string_literal(value)),
        JsExpr::Ident { name } if state.string_bindings.contains(name) => {
            Some(go_binding_ref(name, state))
        }
        expr if render_string_index_expr(expr, state).is_some() => {
            render_string_index_expr(expr, state)
        }
        JsExpr::Ident { name } if is_any_binding(name, state) => {
            let value = go_binding_ref(name, state);
            Some(format!("tsgodownToString({value})"))
        }
        expr if render_string_array_index_expr(expr, state).is_some() => {
            render_string_array_index_expr(expr, state)
        }
        expr if process_env_lookup_name(expr).is_some() => render_process_env_lookup(expr),
        expr if is_node_path_static_string_expr(expr) => render_node_path_static_string_expr(expr),
        expr if is_process_exec_path_expr(expr) => Some("tsgodownProcessExecPath()".to_string()),
        JsExpr::Member {
            object,
            property,
            property_expr: None,
            optional: false,
        } if static_member_kind(object, property, state) == Some(AotSlotKind::String) => {
            render_static_member_expr(object, property, state)
        }
        JsExpr::Binary { op, left, right } if op == "+" => {
            let left = render_string_expr(left, state)?;
            let right = render_string_expr(right, state)?;
            Some(format!("({left} + {right})"))
        }
        JsExpr::Binary { op, left, right } if op == "||" => {
            let left = render_string_expr(left, state)?;
            let right = render_string_expr(right, state)?;
            Some(format!(
                "func() string {{ value := {left}; if value != \"\" {{ return value }}; return {right} }}()"
            ))
        }
        JsExpr::Template { quasis, exprs } => render_template_string_expr(quasis, exprs, state),
        JsExpr::Unary { op, arg } if op == "typeof" => render_typeof_expr(arg, state),
        JsExpr::Call { callee, args, .. } if is_string_cast_call(callee, args) => {
            render_js_to_string_expr(args.first()?, state)
        }
        JsExpr::Call { callee, args, .. } if is_string_array_join_call(callee, args) => {
            render_string_array_join_call(callee, args, state)
        }
        JsExpr::Call { callee, args, .. } => render_node_path_string_call(callee, args, state)
            .or_else(|| render_node_os_homedir_call(callee, args, state))
            .or_else(|| {
                if is_process_cwd_call(callee, args) {
                    Some("tsgodownProcessCwd()".to_string())
                } else {
                    None
                }
            })
            .or_else(|| {
                if is_string_function_call(callee, args, state) {
                    render_call_expr(callee, args, state)
                } else {
                    None
                }
            })
            .or_else(|| render_string_string_method_call(callee, args, state)),
        expr if is_process_version_expr(expr) => render_process_version_expr(expr),
        expr if is_process_platform_expr(expr) => Some("tsgodownProcessPlatform()".to_string()),
        expr if is_process_arch_expr(expr) => Some("tsgodownProcessArch()".to_string()),
        expr if is_process_exec_path_expr(expr) => Some("tsgodownProcessExecPath()".to_string()),
        JsExpr::Conditional {
            test,
            consequent,
            alternate,
        } => render_conditional_expr(
            test,
            consequent,
            alternate,
            state,
            render_string_expr,
            "string",
        ),
        _ => None,
    }
}

fn render_template_string_expr(
    quasis: &[String],
    exprs: &[JsExpr],
    state: &AotState,
) -> Option<String> {
    if quasis.len() != exprs.len() + 1 {
        return None;
    }
    let mut parts = Vec::new();
    for (index, quasi) in quasis.iter().enumerate() {
        if !quasi.is_empty() {
            parts.push(go_string_literal(quasi));
        }
        if let Some(expr) = exprs.get(index) {
            parts.push(render_template_part_string_expr(expr, state)?);
        }
    }
    if parts.is_empty() {
        return Some("\"\"".to_string());
    }
    Some(parts.join(" + "))
}

fn render_template_part_string_expr(expr: &JsExpr, state: &AotState) -> Option<String> {
    match expr {
        JsExpr::Value {
            value: JsValue::String { value },
        } => Some(go_string_literal(value)),
        JsExpr::Ident { name } if state.string_bindings.contains(name) => {
            Some(go_binding_ref(name, state))
        }
        expr if is_process_version_expr(expr) => render_process_version_expr(expr),
        JsExpr::Member {
            object,
            property,
            property_expr: None,
            optional: false,
        } if static_member_kind(object, property, state) == Some(AotSlotKind::String) => {
            render_static_member_expr(object, property, state)
        }
        JsExpr::Call { callee, args, .. } => render_string_string_method_call(callee, args, state),
        _ => None,
    }
}

fn render_js_to_bool_expr(expr: &JsExpr, state: &AotState) -> Option<String> {
    match expr {
        JsExpr::Value {
            value: JsValue::Undefined | JsValue::Null,
        } => Some("false".to_string()),
        JsExpr::Ident { name } if name == "undefined" => Some("false".to_string()),
        JsExpr::Value {
            value: JsValue::Bool { value },
        } => Some(value.to_string()),
        JsExpr::Value {
            value: JsValue::Number { value },
        } => {
            let value = number_literal(value)?;
            Some(format!("({value} != 0)"))
        }
        JsExpr::Value {
            value: JsValue::String { value },
        } => Some((!value.is_empty()).to_string()),
        JsExpr::Ident { name } if name == "process" => Some("true".to_string()),
        expr if is_process_env_ref(expr) || is_process_versions_ref(expr) => {
            Some("true".to_string())
        }
        expr if is_process_stdio_ref(expr).is_some() => render_process_stdio_bool_expr(expr),
        expr if is_process_function_ref(expr).is_some() => Some("true".to_string()),
        expr if is_node_fs_function_ref(expr).is_some() => Some("true".to_string()),
        JsExpr::Ident { name } if state.bool_bindings.contains(name) => {
            Some(go_binding_ref(name, state))
        }
        JsExpr::Ident { name } if state.numeric_bindings.contains(name) => {
            let value = go_binding_ref(name, state);
            Some(format!("({value} != 0)"))
        }
        JsExpr::Ident { name } if state.string_bindings.contains(name) => {
            let value = go_binding_ref(name, state);
            Some(format!("({value} != \"\")"))
        }
        JsExpr::Ident { name } if state.bindings.contains(name) => {
            let value = go_binding_ref(name, state);
            Some(format!(
                "func() bool {{ switch value := any({value}).(type) {{ case nil: return false; case bool: return value; case float64: return value != 0; case int: return value != 0; case int64: return value != 0; case string: return value != \"\"; default: return true }} }}()"
            ))
        }
        _ => None,
    }
}

fn render_bool_call_expr(callee: &JsExpr, args: &[JsExpr], state: &AotState) -> Option<String> {
    if !args.is_empty() {
        return None;
    }
    let JsExpr::Ident { name } = callee else {
        return None;
    };
    let function = state.functions.get(name)?;
    if !function_returns_bool(function, state) {
        return None;
    }
    let call = render_call_expr(callee, args, state)?;
    Some(format!("({call}).(bool)"))
}

fn function_returns_bool(function: &AotFunction, state: &AotState) -> bool {
    let [JsStmt::Return { value: Some(value) }] = function.body.as_slice() else {
        return false;
    };
    matches!(
        value,
        JsExpr::Value {
            value: JsValue::Bool { .. },
        }
    ) || is_process_stdout_is_tty(value)
        || matches!(value, JsExpr::Ident { name } if state.bool_bindings.contains(name))
}

fn render_js_to_string_expr(expr: &JsExpr, state: &AotState) -> Option<String> {
    match expr {
        JsExpr::Value {
            value: JsValue::Undefined,
        } => Some("\"undefined\"".to_string()),
        JsExpr::Ident { name } if name == "undefined" => Some("\"undefined\"".to_string()),
        JsExpr::Value {
            value: JsValue::Null,
        } => Some("\"null\"".to_string()),
        JsExpr::Value {
            value: JsValue::String { value },
        } => Some(go_string_literal(value)),
        JsExpr::Ident { name } if state.string_bindings.contains(name) => {
            Some(go_binding_ref(name, state))
        }
        _ => {
            let value = render_expr(expr, state)?;
            Some(format!("tsgodownToString({value})"))
        }
    }
}

fn render_typeof_expr(expr: &JsExpr, state: &AotState) -> Option<String> {
    match expr {
        JsExpr::Value {
            value: JsValue::Undefined,
        } => Some("\"undefined\"".to_string()),
        JsExpr::Ident { name } if name == "undefined" => Some("\"undefined\"".to_string()),
        JsExpr::Value {
            value: JsValue::Null,
        } => Some("\"object\"".to_string()),
        JsExpr::Value {
            value: JsValue::Bool { .. },
        } => Some("\"boolean\"".to_string()),
        JsExpr::Value {
            value: JsValue::Number { .. },
        } => Some("\"number\"".to_string()),
        JsExpr::Value {
            value: JsValue::String { .. },
        } => Some("\"string\"".to_string()),
        JsExpr::Ident { name } if state.string_bindings.contains(name) => {
            Some("\"string\"".to_string())
        }
        JsExpr::Ident { name } if state.numeric_bindings.contains(name) => {
            Some("\"number\"".to_string())
        }
        JsExpr::Ident { name } if state.bool_bindings.contains(name) => {
            Some("\"boolean\"".to_string())
        }
        JsExpr::Ident { name } if state.bindings.contains(name) => {
            let value = go_binding_ref(name, state);
            Some(format!(
                "func() string {{ switch any({value}).(type) {{ case nil: return \"undefined\"; case bool: return \"boolean\"; case float64, int, int64: return \"number\"; case string: return \"string\"; default: return \"object\" }} }}()"
            ))
        }
        expr if is_process_function_ref(expr).is_some() => Some("\"function\"".to_string()),
        expr if is_node_fs_function_ref(expr).is_some() => Some("\"function\"".to_string()),
        _ => None,
    }
}

fn render_conditional_expr(
    test: &JsExpr,
    consequent: &JsExpr,
    alternate: &JsExpr,
    state: &AotState,
    render_branch: fn(&JsExpr, &AotState) -> Option<String>,
    go_type: &str,
) -> Option<String> {
    let test = render_bool_expr(test, state)?;
    let consequent = render_branch(consequent, state)?;
    let alternate = render_branch(alternate, state)?;
    Some(format!(
        "func() {go_type} {{ if {test} {{ return {consequent} }}; return {alternate} }}()"
    ))
}

fn render_comparison_expr(
    op: &str,
    left: &JsExpr,
    right: &JsExpr,
    state: &AotState,
) -> Option<String> {
    let op = go_comparison_op(op)?;
    if let Some(value) = render_nullish_comparison_expr(op, left, right, state) {
        return Some(value);
    }
    if let (Some(left), Some(right)) = (
        render_numeric_expr(left, state),
        render_numeric_expr(right, state),
    ) {
        return Some(format!("({left} {op} {right})"));
    }
    if let (Some(left), Some(right)) = (
        render_string_expr(left, state),
        render_string_expr(right, state),
    ) {
        return Some(format!("({left} {op} {right})"));
    }
    if matches!(op, "==" | "!=") {
        if let (Some(left), Some(right)) = (
            render_bool_expr(left, state),
            render_bool_expr(right, state),
        ) {
            return Some(format!("({left} {op} {right})"));
        }
    }
    None
}

fn render_nullish_comparison_expr(
    op: &str,
    left: &JsExpr,
    right: &JsExpr,
    state: &AotState,
) -> Option<String> {
    let go_op = match op {
        "==" | "===" => "==",
        "!=" | "!==" => "!=",
        _ => return None,
    };
    if is_nullish_expr(left) {
        let right = render_expr(right, state)?;
        return Some(format!("({right} {go_op} nil)"));
    }
    if is_nullish_expr(right) {
        let left = render_expr(left, state)?;
        return Some(format!("({left} {go_op} nil)"));
    }
    None
}

fn is_nullish_expr(expr: &JsExpr) -> bool {
    match expr {
        JsExpr::Value {
            value: JsValue::Null | JsValue::Undefined,
        } => true,
        JsExpr::Ident { name } if name == "undefined" => true,
        _ => false,
    }
}

fn render_object_literal(expr: &JsExpr, state: &AotState) -> Option<(String, AotObject)> {
    let JsExpr::Object { props } = expr else {
        return None;
    };
    let mut fields = BTreeMap::new();
    let mut type_fields = Vec::new();
    let mut value_fields = Vec::new();
    for prop in props {
        if prop.spread || prop.key_expr.is_some() {
            return None;
        }
        let field_name = sanitize_go_identifier(&prop.key);
        let (kind, rendered, go_type) = render_typed_slot_expr(&prop.value, state)?;
        fields.insert(prop.key.clone(), kind);
        type_fields.push(format!("{field_name} {go_type}"));
        value_fields.push(format!("{field_name}: {rendered}"));
    }
    Some((
        format!(
            "struct {{\n{}\n}}{{\n{},\n}}",
            indent_lines(&type_fields.join("\n")),
            indent_lines(&value_fields.join(",\n"))
        ),
        AotObject { fields },
    ))
}

fn render_typed_slot_expr(
    expr: &JsExpr,
    state: &AotState,
) -> Option<(AotSlotKind, String, &'static str)> {
    if let Some(value) = render_numeric_expr(expr, state) {
        return Some((AotSlotKind::Number, value, "float64"));
    }
    if let Some(value) = render_string_expr(expr, state) {
        return Some((AotSlotKind::String, value, "string"));
    }
    if let Some(value) = render_bool_expr(expr, state) {
        return Some((AotSlotKind::Bool, value, "bool"));
    }
    if let Some(value) = render_bytes_expr(expr, state) {
        return Some((AotSlotKind::Bytes, value, "[]byte"));
    }
    if let Some(value) = render_bool_function_expr(expr, state) {
        return Some((AotSlotKind::BoolFunction, value, "func() bool"));
    }
    if let Some(value) = render_string_function_expr(expr) {
        return Some((AotSlotKind::StringFunction, value, "func() string"));
    }
    None
}

fn render_static_member_expr(object: &JsExpr, property: &str, state: &AotState) -> Option<String> {
    static_member_kind(object, property, state)?;
    if matches!(object, JsExpr::This) {
        let receiver = state.current_receiver.as_ref()?;
        return Some(format!("{}.{}", receiver, sanitize_go_identifier(property)));
    }
    let JsExpr::Ident { name } = object else {
        return None;
    };
    Some(format!(
        "{}.{}",
        sanitize_go_identifier(name),
        sanitize_go_identifier(property)
    ))
}

fn static_member_kind(object: &JsExpr, property: &str, state: &AotState) -> Option<AotSlotKind> {
    if matches!(object, JsExpr::This) {
        return state.current_fields.get(property).copied();
    }
    let JsExpr::Ident { name } = object else {
        return None;
    };
    let object = state.object_bindings.get(name)?;
    object.fields.get(property).copied()
}

fn render_bytes_expr(expr: &JsExpr, state: &AotState) -> Option<String> {
    match expr {
        JsExpr::Ident { name } if state.bytes_bindings.contains(name) => {
            Some(go_binding_ref(name, state))
        }
        JsExpr::Array { items } => {
            let bytes = items
                .iter()
                .map(|item| render_numeric_expr(item, state).map(|value| format!("byte({value})")))
                .collect::<Option<Vec<_>>>()?;
            Some(format!("[]byte{{{}}}", bytes.join(", ")))
        }
        JsExpr::Call { callee, args, .. } if is_node_buffer_from_call(callee, args) => {
            render_node_buffer_from_call(callee, args, state)
        }
        JsExpr::Call { callee, args, .. } if is_node_buffer_alloc_call(callee, args) => {
            render_node_buffer_alloc_call(callee, args, state)
        }
        _ => None,
    }
}

fn render_string_array_expr(expr: &JsExpr, state: &AotState) -> Option<String> {
    match expr {
        JsExpr::Ident { name } if state.string_array_bindings.contains(name) => {
            Some(go_binding_ref(name, state))
        }
        JsExpr::Array { items } => {
            let items = items
                .iter()
                .map(|item| render_string_expr(item, state))
                .collect::<Option<Vec<_>>>()?;
            Some(format!("[]string{{{}}}", items.join(", ")))
        }
        JsExpr::Call { callee, args, .. } if is_array_map_to_string_call(callee, args) => {
            render_array_map_to_string_call(callee, args, state)
        }
        JsExpr::Call { callee, args, .. } if is_string_match_call(callee, args) => {
            render_string_match_call(callee, args, state)
        }
        JsExpr::Call { callee, args, .. } => {
            let JsExpr::Ident { name } = callee.as_ref() else {
                return None;
            };
            let function = state.functions.get(name)?;
            if !function_returns_string_array(function, state) {
                return None;
            }
            let call = render_call_expr(callee, args, state)?;
            Some(format!("({call}).([]string)"))
        }
        _ => None,
    }
}

fn render_string_array_index_expr(expr: &JsExpr, state: &AotState) -> Option<String> {
    let JsExpr::Member {
        object,
        property,
        property_expr,
        optional: false,
    } = expr
    else {
        return None;
    };
    let values = render_string_array_expr(object, state)?;
    let index = if let Some(property_expr) = property_expr {
        render_numeric_expr(property_expr, state)?
    } else {
        number_literal(property)?
    };
    Some(format!("tsgodownStringArrayAt({values}, {index})"))
}

fn render_string_index_expr(expr: &JsExpr, state: &AotState) -> Option<String> {
    let JsExpr::Member {
        object,
        property,
        property_expr: Some(property_expr),
        optional: false,
    } = expr
    else {
        return None;
    };
    if !property.is_empty() {
        return None;
    }
    let value = render_string_expr(object, state)?;
    let index = render_numeric_expr(property_expr, state)?;
    Some(format!("tsgodownStringCharAt({value}, {index})"))
}

fn call_uses_strings_import(callee: &JsExpr) -> bool {
    matches!(
        string_method_name(callee),
        Some("toLowerCase" | "toUpperCase" | "trim" | "includes" | "indexOf")
    )
}

fn string_method_name(callee: &JsExpr) -> Option<&str> {
    let JsExpr::Member {
        object: _,
        property,
        property_expr: None,
        optional: false,
    } = callee
    else {
        return None;
    };
    match property.as_str() {
        "toLowerCase" | "toUpperCase" | "trim" | "includes" | "indexOf" | "charAt" | "slice" => {
            Some(property.as_str())
        }
        _ => None,
    }
}

fn string_method_receiver<'a>(
    callee: &'a JsExpr,
    method: &str,
    args: &[JsExpr],
    state: &AotState,
) -> Option<&'a JsExpr> {
    if string_method_name(callee) != Some(method) {
        return None;
    }
    let JsExpr::Member { object, .. } = callee else {
        return None;
    };
    match method {
        "toLowerCase" | "toUpperCase" | "trim" if args.is_empty() => {}
        "includes" | "indexOf" if args.len() == 1 => {}
        "charAt" if args.len() == 1 => {}
        "slice" if matches!(args.len(), 1 | 2) => {}
        _ => return None,
    }
    render_string_expr(object, state)?;
    Some(object)
}

fn render_string_string_method_call(
    callee: &JsExpr,
    args: &[JsExpr],
    state: &AotState,
) -> Option<String> {
    if let Some(object) = string_method_receiver(callee, "toLowerCase", args, state) {
        let object = render_string_expr(object, state)?;
        return Some(format!("strings.ToLower({object})"));
    }
    if let Some(object) = string_method_receiver(callee, "toUpperCase", args, state) {
        let object = render_string_expr(object, state)?;
        return Some(format!("strings.ToUpper({object})"));
    }
    if let Some(object) = string_method_receiver(callee, "trim", args, state) {
        let object = render_string_expr(object, state)?;
        return Some(format!("strings.TrimSpace({object})"));
    }
    if let Some(object) = string_method_receiver(callee, "charAt", args, state) {
        let object = render_string_expr(object, state)?;
        let index = render_numeric_expr(args.first()?, state)?;
        return Some(format!("tsgodownStringCharAt({object}, {index})"));
    }
    if let Some(object) = string_method_receiver(callee, "slice", args, state) {
        let object = render_string_expr(object, state)?;
        let start = render_numeric_expr(args.first()?, state)?;
        if let Some(end) = args.get(1) {
            let end = render_numeric_expr(end, state)?;
            return Some(format!("tsgodownStringSlice({object}, {start}, {end})"));
        }
        return Some(format!("tsgodownStringSlice({object}, {start})"));
    }
    None
}

fn render_string_bool_method_call(
    callee: &JsExpr,
    args: &[JsExpr],
    state: &AotState,
) -> Option<String> {
    let object = string_method_receiver(callee, "includes", args, state)?;
    let object = render_string_expr(object, state)?;
    let needle = render_string_expr(args.first()?, state)?;
    Some(format!("strings.Contains({object}, {needle})"))
}

fn render_array_bool_method_call(
    callee: &JsExpr,
    args: &[JsExpr],
    state: &AotState,
) -> Option<String> {
    let JsExpr::Member {
        object,
        property,
        property_expr: None,
        optional: false,
    } = callee
    else {
        return None;
    };
    if property != "includes" || args.len() != 1 {
        return None;
    }
    let JsExpr::Array { items } = object.as_ref() else {
        return None;
    };
    let needle = render_string_expr(args.first()?, state)?;
    let comparisons = items
        .iter()
        .map(|item| render_string_expr(item, state).map(|item| format!("{item} == {needle}")))
        .collect::<Option<Vec<_>>>()?;
    if comparisons.is_empty() {
        return Some("false".to_string());
    }
    Some(format!("({})", comparisons.join(" || ")))
}

fn is_array_is_array_call(callee: &JsExpr, args: &[JsExpr]) -> bool {
    args.len() == 1
        && matches!(
            callee,
            JsExpr::Member {
                object,
                property,
                property_expr: None,
                optional: false,
            } if matches!(object.as_ref(), JsExpr::Ident { name } if name == "Array")
                && property == "isArray"
        )
}

fn is_array_map_to_string_call(callee: &JsExpr, args: &[JsExpr]) -> bool {
    if args.len() != 1 {
        return false;
    }
    let JsExpr::Member {
        property,
        property_expr: None,
        optional: false,
        ..
    } = callee
    else {
        return false;
    };
    property == "map" && is_string_coercion_map_callback(args.first().expect("one arg"))
}

fn is_string_coercion_map_callback(expr: &JsExpr) -> bool {
    let JsExpr::Function {
        params,
        rest_param: None,
        r#async: false,
        generator: false,
        body,
        ..
    } = expr
    else {
        return false;
    };
    let [param] = params.as_slice() else {
        return false;
    };
    let [JsStmt::Return { value: Some(value) }] = body.as_slice() else {
        return false;
    };
    is_string_coercion_expr(value, param)
}

fn is_string_coercion_expr(expr: &JsExpr, param: &str) -> bool {
    match expr {
        JsExpr::Ident { name } => name == param,
        JsExpr::Binary { op, left, right } if op == "+" => {
            (matches!(left.as_ref(), JsExpr::Ident { name } if name == param)
                && is_string_literal_like(right))
                || (matches!(right.as_ref(), JsExpr::Ident { name } if name == param)
                    && is_string_literal_like(left))
        }
        JsExpr::Conditional {
            consequent,
            alternate,
            ..
        } => {
            is_string_coercion_expr(consequent, param) && is_string_coercion_expr(alternate, param)
        }
        _ => false,
    }
}

fn render_array_map_to_string_call(
    callee: &JsExpr,
    args: &[JsExpr],
    state: &AotState,
) -> Option<String> {
    if !is_array_map_to_string_call(callee, args) {
        return None;
    }
    let JsExpr::Member { object, .. } = callee else {
        return None;
    };
    let value = render_expr(object, state)?;
    Some(format!("tsgodownStringArrayFromAny({value})"))
}

fn is_string_array_join_call(callee: &JsExpr, args: &[JsExpr]) -> bool {
    matches!(args.len(), 0 | 1)
        && matches!(
            callee,
            JsExpr::Member {
                property,
                property_expr: None,
                optional: false,
                ..
            } if property == "join"
        )
}

fn render_string_array_join_call(
    callee: &JsExpr,
    args: &[JsExpr],
    state: &AotState,
) -> Option<String> {
    if !is_string_array_join_call(callee, args) {
        return None;
    }
    let JsExpr::Member { object, .. } = callee else {
        return None;
    };
    let values = render_string_array_expr(object, state)?;
    let separator = args
        .first()
        .map(|expr| render_string_expr(expr, state))
        .unwrap_or_else(|| Some("\",\"".to_string()))?;
    Some(format!("strings.Join({values}, {separator})"))
}

fn is_string_match_call(callee: &JsExpr, args: &[JsExpr]) -> bool {
    if args.len() != 1 {
        return false;
    }
    let JsExpr::Member {
        object,
        property,
        property_expr: None,
        optional: false,
    } = callee
    else {
        return false;
    };
    property == "match"
        && matches!(args.first(), Some(JsExpr::Value { value: JsValue::RegExp { flags, .. } }) if flags.chars().all(|flag| flag == 'i'))
        && matches!(
            object.as_ref(),
            JsExpr::Ident { .. }
                | JsExpr::Value {
                    value: JsValue::String { .. },
                }
                | JsExpr::Template { .. }
        )
}

fn render_string_match_call(callee: &JsExpr, args: &[JsExpr], state: &AotState) -> Option<String> {
    let JsExpr::Member { object, .. } = callee else {
        return None;
    };
    let value = render_string_expr(object, state)?;
    let pattern = render_supported_regexp_pattern(args.first()?)?;
    Some(format!(
        "tsgodownStringMatch({value}, {})",
        go_string_literal(&pattern)
    ))
}

fn is_regexp_test_call(callee: &JsExpr, args: &[JsExpr]) -> bool {
    if args.len() != 1 {
        return false;
    }
    let JsExpr::Member {
        object,
        property,
        property_expr: None,
        optional: false,
    } = callee
    else {
        return false;
    };
    if property != "test" {
        return false;
    }
    matches!(
        object.as_ref(),
        JsExpr::Value {
            value: JsValue::RegExp { flags, .. },
        } if flags.chars().all(|flag| flag == 'i')
    )
}

fn render_regexp_test_call(callee: &JsExpr, args: &[JsExpr], state: &AotState) -> Option<String> {
    let JsExpr::Member { object, .. } = callee else {
        return None;
    };
    let pattern = render_supported_regexp_pattern(object)?;
    let value = render_regexp_test_value_expr(args.first()?, state)?;
    Some(format!(
        "regexp.MustCompile({}).MatchString({value})",
        go_string_literal(&pattern)
    ))
}

fn render_supported_regexp_pattern(expr: &JsExpr) -> Option<String> {
    let JsExpr::Value {
        value: JsValue::RegExp { pattern, flags },
    } = expr
    else {
        return None;
    };
    if !flags.chars().all(|flag| flag == 'i') {
        return None;
    }
    Some(if flags.contains('i') {
        format!("(?i){pattern}")
    } else {
        pattern.clone()
    })
}

fn regexp_test_needs_to_string_helper(args: &[JsExpr]) -> bool {
    match args.first() {
        Some(JsExpr::Value {
            value: JsValue::String { .. },
        }) => false,
        Some(JsExpr::Template { quasis, exprs }) if exprs.is_empty() && quasis.len() == 1 => false,
        _ => true,
    }
}

fn render_regexp_test_value_expr(expr: &JsExpr, state: &AotState) -> Option<String> {
    match expr {
        JsExpr::Value {
            value: JsValue::String { value },
        } => Some(go_string_literal(value)),
        JsExpr::Template { quasis, exprs } if exprs.is_empty() && quasis.len() == 1 => {
            Some(go_string_literal(&quasis[0]))
        }
        _ => {
            let value = render_expr(expr, state)?;
            Some(format!("tsgodownToString({value})"))
        }
    }
}

fn is_supported_node_builtin_call_expr(expr: &JsExpr) -> bool {
    matches!(
        expr,
        JsExpr::Call { callee, args, .. }
            if is_node_path_string_call(callee, args)
                || is_node_os_homedir_call(callee, args)
                || is_node_fs_exists_sync_call(callee, args)
                || is_node_fs_stat_sync_call(callee, args)
                || is_node_buffer_from_call(callee, args)
                || is_node_buffer_alloc_call(callee, args)
                || is_node_buffer_is_buffer_call(callee, args)
                || is_node_path_bool_call(callee, args)
                || is_node_path_parse_call(callee, args)
    )
}

fn is_node_path_string_call(callee: &JsExpr, args: &[JsExpr]) -> bool {
    if args.is_empty() {
        return false;
    }
    matches!(
        callee,
        JsExpr::Member {
            object,
            property,
            property_expr: None,
            optional: false,
            } if matches!(object.as_ref(), JsExpr::Ident { name } if name == "path")
            && matches!(
                property.as_str(),
                "basename" | "dirname" | "join" | "normalize" | "relative" | "resolve"
            )
    )
}

fn is_node_path_basename_call(callee: &JsExpr, args: &[JsExpr]) -> bool {
    matches!(args.len(), 1 | 2)
        && matches!(
            callee,
            JsExpr::Member {
                object,
                property,
                property_expr: None,
                optional: false,
            } if matches!(object.as_ref(), JsExpr::Ident { name } if name == "path")
                && property == "basename"
        )
}

fn render_node_path_string_call(
    callee: &JsExpr,
    args: &[JsExpr],
    state: &AotState,
) -> Option<String> {
    let JsExpr::Member {
        object,
        property,
        property_expr: None,
        optional: false,
    } = callee
    else {
        return None;
    };
    let JsExpr::Ident { name } = object.as_ref() else {
        return None;
    };
    if name != "path" || !state.builtin_bindings.contains(name) {
        return None;
    }
    let rendered_args = args
        .iter()
        .map(|arg| render_string_expr(arg, state))
        .collect::<Option<Vec<_>>>()?;
    match property.as_str() {
        "basename" if rendered_args.len() == 1 => Some(format!("filepath.Base({})", rendered_args[0])),
        "basename" if rendered_args.len() == 2 => Some(format!(
            "func() string {{ base := filepath.Base({}); ext := {}; if strings.HasSuffix(base, ext) {{ return strings.TrimSuffix(base, ext) }}; return base }}()",
            rendered_args[0], rendered_args[1]
        )),
        "dirname" if rendered_args.len() == 1 => Some(format!("filepath.Dir({})", rendered_args[0])),
        "join" => Some(format!("filepath.Join({})", rendered_args.join(", "))),
        "normalize" if rendered_args.len() == 1 => Some(format!("filepath.Clean({})", rendered_args[0])),
        "relative" if rendered_args.len() == 2 => Some(format!(
            "func() string {{ value, err := filepath.Rel({}, {}); if err != nil {{ return {} }}; return value }}()",
            rendered_args[0], rendered_args[1], rendered_args[1]
        )),
        "resolve" => Some(format!(
            "func() string {{ value, err := filepath.Abs(filepath.Join({})); if err != nil {{ return \"\" }}; return value }}()",
            rendered_args.join(", ")
        )),
        _ => None,
    }
}

fn is_node_path_bool_call(callee: &JsExpr, args: &[JsExpr]) -> bool {
    args.len() == 1
        && matches!(
            callee,
            JsExpr::Member {
                object,
                property,
                property_expr: None,
                optional: false,
            } if matches!(object.as_ref(), JsExpr::Ident { name } if name == "path")
                && property == "isAbsolute"
        )
}

fn render_node_path_bool_call(
    callee: &JsExpr,
    args: &[JsExpr],
    state: &AotState,
) -> Option<String> {
    let JsExpr::Member { object, .. } = callee else {
        return None;
    };
    let JsExpr::Ident { name } = object.as_ref() else {
        return None;
    };
    if !state.builtin_bindings.contains(name) || !is_node_path_bool_call(callee, args) {
        return None;
    }
    let value = render_string_expr(args.first()?, state)?;
    Some(format!("filepath.IsAbs({value})"))
}

fn is_node_path_parse_call(callee: &JsExpr, args: &[JsExpr]) -> bool {
    args.len() == 1
        && matches!(
            callee,
            JsExpr::Member {
                object,
                property,
                property_expr: None,
                optional: false,
            } if matches!(object.as_ref(), JsExpr::Ident { name } if name == "path")
                && property == "parse"
        )
}

fn render_node_path_parse_object(expr: &JsExpr, state: &AotState) -> Option<(String, AotObject)> {
    let JsExpr::Call { callee, args, .. } = expr else {
        return None;
    };
    let JsExpr::Member { object, .. } = callee.as_ref() else {
        return None;
    };
    let JsExpr::Ident { name } = object.as_ref() else {
        return None;
    };
    if !state.builtin_bindings.contains(name) || !is_node_path_parse_call(callee, args) {
        return None;
    }
    let value = render_string_expr(args.first()?, state)?;
    let fields = ["root", "dir", "base", "ext", "name"]
        .into_iter()
        .map(|field| (field.to_string(), AotSlotKind::String))
        .collect::<BTreeMap<_, _>>();
    Some((
        format!(
            "func() struct {{ root string; dir string; base string; ext string; name string }} {{ input := {value}; clean := filepath.Clean(input); dir, base := filepath.Split(clean); if dir != \"\" {{ dir = strings.TrimSuffix(dir, string(os.PathSeparator)) }}; ext := filepath.Ext(base); name := strings.TrimSuffix(base, ext); root := \"\"; if filepath.IsAbs(input) {{ root = string(os.PathSeparator) }}; return struct {{ root string; dir string; base string; ext string; name string }}{{root: root, dir: dir, base: base, ext: ext, name: name}} }}()"
        ),
        AotObject { fields },
    ))
}

fn is_node_path_static_string_expr(expr: &JsExpr) -> bool {
    is_node_path_sep_expr(expr)
        || is_node_path_delimiter_expr(expr)
        || matches!(expr, JsExpr::Member { object, property, property_expr: None, optional: false }
        if property == "sep" && matches!(
            object.as_ref(),
            JsExpr::Member { object, property, property_expr: None, optional: false }
                if matches!(object.as_ref(), JsExpr::Ident { name } if name == "path")
                    && matches!(property.as_str(), "posix" | "win32")
        ))
}

fn render_node_path_static_string_expr(expr: &JsExpr) -> Option<String> {
    if is_node_path_sep_expr(expr) {
        return Some("string(os.PathSeparator)".to_string());
    }
    if is_node_path_delimiter_expr(expr) {
        return Some("tsgodownPathDelimiter()".to_string());
    }
    match expr {
        JsExpr::Member {
            object,
            property,
            property_expr: None,
            optional: false,
        } if property == "sep" => match object.as_ref() {
            JsExpr::Member {
                object,
                property,
                property_expr: None,
                optional: false,
            } if matches!(object.as_ref(), JsExpr::Ident { name } if name == "path") => {
                match property.as_str() {
                    "posix" => Some(go_string_literal("/")),
                    "win32" => Some(go_string_literal("\\")),
                    _ => None,
                }
            }
            _ => None,
        },
        _ => None,
    }
}

fn is_node_path_sep_expr(expr: &JsExpr) -> bool {
    matches!(
        expr,
        JsExpr::Member {
            object,
            property,
            property_expr: None,
            optional: false,
        } if matches!(object.as_ref(), JsExpr::Ident { name } if name == "path")
            && property == "sep"
    )
}

fn is_node_path_delimiter_expr(expr: &JsExpr) -> bool {
    matches!(
        expr,
        JsExpr::Member {
            object,
            property,
            property_expr: None,
            optional: false,
        } if matches!(object.as_ref(), JsExpr::Ident { name } if name == "path")
            && property == "delimiter"
    )
}

fn is_node_os_homedir_call(callee: &JsExpr, args: &[JsExpr]) -> bool {
    args.is_empty()
        && matches!(
            callee,
            JsExpr::Member {
                object,
                property,
                property_expr: None,
                optional: false,
            } if matches!(object.as_ref(), JsExpr::Ident { name } if name == "os")
                && property == "homedir"
        )
}

fn render_node_os_homedir_call(
    callee: &JsExpr,
    args: &[JsExpr],
    state: &AotState,
) -> Option<String> {
    let JsExpr::Member { object, .. } = callee else {
        return None;
    };
    let JsExpr::Ident { name } = object.as_ref() else {
        return None;
    };
    if state.builtin_bindings.contains(name) && is_node_os_homedir_call(callee, args) {
        return Some("tsgodownOsHomedir()".to_string());
    }
    None
}

fn is_node_fs_exists_sync_call(callee: &JsExpr, args: &[JsExpr]) -> bool {
    args.len() == 1
        && matches!(
            callee,
            JsExpr::Member {
                object,
                property,
                property_expr: None,
                optional: false,
            } if matches!(object.as_ref(), JsExpr::Ident { name } if name == "fs")
                && property == "existsSync"
        )
}

fn render_node_fs_exists_sync_call(
    callee: &JsExpr,
    args: &[JsExpr],
    state: &AotState,
) -> Option<String> {
    let JsExpr::Member { object, .. } = callee else {
        return None;
    };
    let JsExpr::Ident { name } = object.as_ref() else {
        return None;
    };
    if !state.builtin_bindings.contains(name) || !is_node_fs_exists_sync_call(callee, args) {
        return None;
    }
    let path = render_string_expr(args.first()?, state)?;
    Some(format!("tsgodownFsExistsSync({path})"))
}

fn is_node_fs_stat_sync_call(callee: &JsExpr, args: &[JsExpr]) -> bool {
    matches!(args.len(), 1 | 2)
        && matches!(
            callee,
            JsExpr::Member {
                object,
                property,
                property_expr: None,
                optional: false,
            } if matches!(object.as_ref(), JsExpr::Ident { name } if name == "fs")
                && property == "statSync"
        )
}

fn is_node_fs_function_ref(expr: &JsExpr) -> Option<&str> {
    let JsExpr::Member {
        object,
        property,
        property_expr: None,
        optional: _,
    } = expr
    else {
        return None;
    };
    if matches!(object.as_ref(), JsExpr::Ident { name } if name == "fs")
        && matches!(
            property.as_str(),
            "close" | "closeSync" | "existsSync" | "stat" | "statSync"
        )
    {
        return Some(property);
    }
    None
}

fn render_node_fs_stat_sync_object(expr: &JsExpr, state: &AotState) -> Option<(String, AotObject)> {
    let JsExpr::Call { callee, args, .. } = expr else {
        return None;
    };
    let JsExpr::Member { object, .. } = callee.as_ref() else {
        return None;
    };
    let JsExpr::Ident { name } = object.as_ref() else {
        return None;
    };
    if !state.builtin_bindings.contains(name) || !is_node_fs_stat_sync_call(callee, args) {
        return None;
    }
    let path = render_string_expr(args.first()?, state)?;
    let fields = [
        ("mode".to_string(), AotSlotKind::Number),
        ("isFile".to_string(), AotSlotKind::BoolFunction),
        ("isDirectory".to_string(), AotSlotKind::BoolFunction),
        ("isSymbolicLink".to_string(), AotSlotKind::BoolFunction),
    ]
    .into_iter()
    .collect::<BTreeMap<_, _>>();
    Some((
        format!(
            "func() struct {{ mode float64; isFile func() bool; isDirectory func() bool; isSymbolicLink func() bool }} {{ info, err := os.Stat({path}); if err != nil {{ return struct {{ mode float64; isFile func() bool; isDirectory func() bool; isSymbolicLink func() bool }}{{mode: 0, isFile: func() bool {{ return false }}, isDirectory: func() bool {{ return false }}, isSymbolicLink: func() bool {{ return false }}}} }}; mode := float64(info.Mode().Perm()); return struct {{ mode float64; isFile func() bool; isDirectory func() bool; isSymbolicLink func() bool }}{{mode: mode, isFile: func() bool {{ return info.Mode().IsRegular() }}, isDirectory: func() bool {{ return info.IsDir() }}, isSymbolicLink: func() bool {{ return info.Mode()&os.ModeSymlink != 0 }}}} }}()"
        ),
        AotObject { fields },
    ))
}

fn is_node_buffer_from_call(callee: &JsExpr, args: &[JsExpr]) -> bool {
    matches!(args.len(), 1 | 2)
        && matches!(
            callee,
            JsExpr::Member {
                object,
                property,
                property_expr: None,
                optional: false,
            } if matches!(object.as_ref(), JsExpr::Ident { name } if name == "Buffer")
                && property == "from"
        )
}

fn is_node_buffer_alloc_call(callee: &JsExpr, args: &[JsExpr]) -> bool {
    matches!(args.len(), 1 | 2)
        && matches!(
            callee,
            JsExpr::Member {
                object,
                property,
                property_expr: None,
                optional: false,
            } if matches!(object.as_ref(), JsExpr::Ident { name } if name == "Buffer")
                && property == "alloc"
        )
}

fn render_node_buffer_alloc_call(
    callee: &JsExpr,
    args: &[JsExpr],
    state: &AotState,
) -> Option<String> {
    if !is_node_buffer_alloc_call(callee, args) {
        return None;
    }
    let size = render_numeric_expr(args.first()?, state)?;
    if let Some(fill) = args.get(1) {
        let fill = render_numeric_expr(fill, state)?;
        return Some(format!(
            "func() []byte {{ value := make([]byte, int({size})); for index := range value {{ value[index] = byte({fill}) }}; return value }}()"
        ));
    }
    Some(format!("make([]byte, int({size}))"))
}

fn is_node_buffer_is_buffer_call(callee: &JsExpr, args: &[JsExpr]) -> bool {
    args.len() == 1
        && matches!(
            callee,
            JsExpr::Member {
                object,
                property,
                property_expr: None,
                optional: false,
            } if matches!(object.as_ref(), JsExpr::Ident { name } if name == "Buffer")
                && property == "isBuffer"
        )
}

fn render_node_buffer_is_buffer_call(
    callee: &JsExpr,
    args: &[JsExpr],
    state: &AotState,
) -> Option<String> {
    if !is_node_buffer_is_buffer_call(callee, args) {
        return None;
    }
    if render_bytes_expr(args.first()?, state).is_some() {
        return Some("true".to_string());
    }
    let value = render_expr(args.first()?, state)?;
    Some(format!("tsgodownBufferIsBuffer({value})"))
}

fn render_node_buffer_from_call(
    callee: &JsExpr,
    args: &[JsExpr],
    state: &AotState,
) -> Option<String> {
    if !is_node_buffer_from_call(callee, args) {
        return None;
    }
    match args.first()? {
        JsExpr::Array { .. } => render_bytes_expr(args.first()?, state),
        expr if render_bytes_expr(expr, state).is_some() && args.len() == 1 => {
            let value = render_bytes_expr(expr, state)?;
            Some(format!("append([]byte(nil), {value}...)"))
        }
        expr => {
            let value = render_string_expr(expr, state)?;
            let encoding = args
                .get(1)
                .and_then(string_literal_value)
                .unwrap_or_else(|| "utf8".to_string());
            Some(format!(
                "tsgodownBufferFromString({value}, {})",
                go_string_literal(&encoding)
            ))
        }
    }
}

fn render_string_numeric_method_call(
    callee: &JsExpr,
    args: &[JsExpr],
    state: &AotState,
) -> Option<String> {
    let object = string_method_receiver(callee, "indexOf", args, state)?;
    let object = render_string_expr(object, state)?;
    let needle = render_string_expr(args.first()?, state)?;
    Some(format!("float64(strings.Index({object}, {needle}))"))
}

fn is_string_cast_call(callee: &JsExpr, args: &[JsExpr]) -> bool {
    args.len() == 1 && matches!(callee, JsExpr::Ident { name } if name == "String")
}

fn is_boolean_cast_call(callee: &JsExpr, args: &[JsExpr]) -> bool {
    args.len() == 1 && matches!(callee, JsExpr::Ident { name } if name == "Boolean")
}

fn is_process_supported_builtin_expr(expr: &JsExpr) -> bool {
    is_process_stdout_is_tty(expr)
        || process_env_lookup_name(expr).is_some()
        || is_process_env_ref(expr)
        || is_process_versions_ref(expr)
        || is_process_platform_expr(expr)
        || is_process_arch_expr(expr)
        || is_process_exec_path_expr(expr)
        || is_process_cwd_ref(expr)
        || is_process_cwd_call_expr(expr)
        || is_process_version_expr(expr)
        || is_process_stdio_ref(expr).is_some()
        || is_process_function_ref(expr).is_some()
        || is_process_uid_gid_call_expr(expr)
        || is_process_chdir_call_expr(expr)
        || is_process_noop_call_expr(expr)
}

fn is_process_stdout_is_tty(expr: &JsExpr) -> bool {
    let JsExpr::Member {
        object,
        property,
        property_expr: None,
        optional: false,
    } = expr
    else {
        return false;
    };
    if property != "isTTY" {
        return false;
    }
    matches!(
        object.as_ref(),
        JsExpr::Member {
            object,
            property,
            property_expr: None,
            optional: false,
        } if matches!(object.as_ref(), JsExpr::Ident { name } if name == "process") && property == "stdout"
    )
}

fn is_process_cwd_ref(expr: &JsExpr) -> bool {
    let JsExpr::Member {
        object,
        property,
        property_expr: None,
        optional: _,
    } = expr
    else {
        return false;
    };
    property == "cwd" && matches!(object.as_ref(), JsExpr::Ident { name } if name == "process")
}

fn is_process_env_ref(expr: &JsExpr) -> bool {
    let JsExpr::Member {
        object,
        property,
        property_expr: None,
        optional: _,
    } = expr
    else {
        return false;
    };
    property == "env" && matches!(object.as_ref(), JsExpr::Ident { name } if name == "process")
}

fn is_process_versions_ref(expr: &JsExpr) -> bool {
    let JsExpr::Member {
        object,
        property,
        property_expr: None,
        optional: _,
    } = expr
    else {
        return false;
    };
    property == "versions" && matches!(object.as_ref(), JsExpr::Ident { name } if name == "process")
}

fn render_process_versions_expr() -> String {
    format!(
        "map[string]any{{\"node\": {}}}",
        go_string_literal(NODE_LTS_VERSION)
    )
}

fn is_process_platform_expr(expr: &JsExpr) -> bool {
    let JsExpr::Member {
        object,
        property,
        property_expr: None,
        optional: _,
    } = expr
    else {
        return false;
    };
    property == "platform" && matches!(object.as_ref(), JsExpr::Ident { name } if name == "process")
}

fn is_process_arch_expr(expr: &JsExpr) -> bool {
    let JsExpr::Member {
        object,
        property,
        property_expr: None,
        optional: _,
    } = expr
    else {
        return false;
    };
    property == "arch" && matches!(object.as_ref(), JsExpr::Ident { name } if name == "process")
}

fn is_process_exec_path_expr(expr: &JsExpr) -> bool {
    let JsExpr::Member {
        object,
        property,
        property_expr: None,
        optional: _,
    } = expr
    else {
        return false;
    };
    property == "execPath" && matches!(object.as_ref(), JsExpr::Ident { name } if name == "process")
}

fn is_process_stdio_ref(expr: &JsExpr) -> Option<&str> {
    let JsExpr::Member {
        object,
        property,
        property_expr: None,
        optional: _,
    } = expr
    else {
        return None;
    };
    if matches!(object.as_ref(), JsExpr::Ident { name } if name == "process")
        && matches!(property.as_str(), "stdin" | "stdout" | "stderr" | "channel")
    {
        return Some(property);
    }
    None
}

fn render_process_stdio_expr(expr: &JsExpr) -> Option<String> {
    match is_process_stdio_ref(expr)? {
        "stdin" => Some("os.Stdin".to_string()),
        "stdout" => Some("os.Stdout".to_string()),
        "stderr" => Some("os.Stderr".to_string()),
        "channel" => Some("nil".to_string()),
        _ => None,
    }
}

fn render_process_stdio_bool_expr(expr: &JsExpr) -> Option<String> {
    match is_process_stdio_ref(expr)? {
        "stdin" | "stdout" | "stderr" => Some("true".to_string()),
        "channel" => Some("false".to_string()),
        _ => None,
    }
}

fn is_process_function_ref(expr: &JsExpr) -> Option<&str> {
    let JsExpr::Member {
        object,
        property,
        property_expr: None,
        optional: _,
    } = expr
    else {
        return None;
    };
    if matches!(object.as_ref(), JsExpr::Ident { name } if name == "process")
        && matches!(
            property.as_str(),
            "chdir" | "emitWarning" | "getgid" | "getuid" | "nextTick" | "on"
        )
    {
        return Some(property);
    }
    None
}

fn render_process_function_ref(expr: &JsExpr) -> Option<String> {
    match is_process_function_ref(expr)? {
        "chdir" => Some("tsgodownProcessChdir".to_string()),
        "getgid" => Some("func() float64 { return float64(os.Getgid()) }".to_string()),
        "getuid" => Some("func() float64 { return float64(os.Getuid()) }".to_string()),
        "emitWarning" | "nextTick" | "on" => Some("func(...any) any { return nil }".to_string()),
        _ => None,
    }
}

fn is_process_cwd_call(callee: &JsExpr, args: &[JsExpr]) -> bool {
    args.is_empty() && is_process_cwd_ref(callee)
}

fn is_process_cwd_call_expr(expr: &JsExpr) -> bool {
    matches!(expr, JsExpr::Call { callee, args, .. } if is_process_cwd_call(callee, args))
}

fn is_process_uid_gid_call(callee: &JsExpr, args: &[JsExpr]) -> bool {
    args.is_empty()
        && matches!(
            callee,
            JsExpr::Member {
                object,
                property,
                property_expr: None,
                optional: false,
            } if matches!(object.as_ref(), JsExpr::Ident { name } if name == "process")
                && matches!(property.as_str(), "getuid" | "getgid")
        )
}

fn is_process_uid_gid_call_expr(expr: &JsExpr) -> bool {
    matches!(expr, JsExpr::Call { callee, args, .. } if is_process_uid_gid_call(callee, args))
}

fn is_process_noop_call(callee: &JsExpr, _args: &[JsExpr]) -> bool {
    matches!(is_process_function_ref(callee), Some("emitWarning" | "on"))
}

fn is_process_noop_call_expr(expr: &JsExpr) -> bool {
    matches!(expr, JsExpr::Call { callee, args, .. } if is_process_noop_call(callee, args))
}

fn render_process_uid_gid_call(callee: &JsExpr, args: &[JsExpr]) -> Option<String> {
    if !is_process_uid_gid_call(callee, args) {
        return None;
    }
    match callee {
        JsExpr::Member { property, .. } if property == "getuid" => {
            Some("float64(os.Getuid())".to_string())
        }
        JsExpr::Member { property, .. } if property == "getgid" => {
            Some("float64(os.Getgid())".to_string())
        }
        _ => None,
    }
}

fn is_process_chdir_call(callee: &JsExpr, args: &[JsExpr]) -> bool {
    args.len() == 1
        && matches!(
            callee,
            JsExpr::Member {
                object,
                property,
                property_expr: None,
                optional: false,
            } if matches!(object.as_ref(), JsExpr::Ident { name } if name == "process")
                && property == "chdir"
        )
}

fn is_process_chdir_call_expr(expr: &JsExpr) -> bool {
    matches!(expr, JsExpr::Call { callee, args, .. } if is_process_chdir_call(callee, args))
}

fn is_string_function_call(callee: &JsExpr, args: &[JsExpr], state: &AotState) -> bool {
    if !args.is_empty() {
        return false;
    }
    match callee {
        JsExpr::Ident { name } => state.string_function_bindings.contains(name),
        JsExpr::Member {
            object,
            property,
            property_expr: None,
            optional: false,
        } => static_member_kind(object, property, state) == Some(AotSlotKind::StringFunction),
        _ => false,
    }
}

fn render_bool_function_call(callee: &JsExpr, args: &[JsExpr], state: &AotState) -> Option<String> {
    if !args.is_empty() {
        return None;
    }
    match callee {
        JsExpr::Member {
            object,
            property,
            property_expr: None,
            optional: false,
        } if static_member_kind(object, property, state) == Some(AotSlotKind::BoolFunction) => {
            let function = render_static_member_expr(object, property, state)?;
            Some(format!("{function}()"))
        }
        _ => None,
    }
}

fn render_bool_function_expr(expr: &JsExpr, state: &AotState) -> Option<String> {
    match expr {
        JsExpr::Member {
            object,
            property,
            property_expr: None,
            optional: false,
        } if static_member_kind(object, property, state) == Some(AotSlotKind::BoolFunction) => {
            render_static_member_expr(object, property, state)
        }
        _ => None,
    }
}

fn render_string_function_expr(expr: &JsExpr) -> Option<String> {
    if is_process_cwd_ref(expr) {
        return Some("tsgodownProcessCwd".to_string());
    }
    None
}

fn is_process_version_expr(expr: &JsExpr) -> bool {
    match expr {
        JsExpr::Member {
            object,
            property,
            property_expr: None,
            optional: _,
        } if property == "version"
            && matches!(object.as_ref(), JsExpr::Ident { name } if name == "process") =>
        {
            true
        }
        JsExpr::Member {
            object,
            property,
            property_expr: None,
            optional: _,
        } if property == "node" => matches!(
            object.as_ref(),
            JsExpr::Member {
                object,
                property,
                property_expr: None,
                optional: _,
            } if property == "versions"
                && matches!(object.as_ref(), JsExpr::Ident { name } if name == "process")
        ),
        _ => false,
    }
}

fn render_process_version_expr(expr: &JsExpr) -> Option<String> {
    match expr {
        JsExpr::Member {
            object,
            property,
            property_expr: None,
            optional: _,
        } if property == "version"
            && matches!(object.as_ref(), JsExpr::Ident { name } if name == "process") =>
        {
            Some(go_string_literal(NODE_LTS_VERSION_WITH_PREFIX))
        }
        JsExpr::Member {
            property,
            property_expr: None,
            optional: _,
            ..
        } if property == "node" && is_process_version_expr(expr) => {
            Some(go_string_literal(NODE_LTS_VERSION))
        }
        _ => None,
    }
}

fn process_env_lookup_name(expr: &JsExpr) -> Option<&str> {
    let JsExpr::Member {
        object,
        property,
        property_expr: None,
        optional: false,
    } = expr
    else {
        return None;
    };
    match object.as_ref() {
        JsExpr::Member {
            object,
            property: env_property,
            property_expr: None,
            optional: false,
        } if matches!(object.as_ref(), JsExpr::Ident { name } if name == "process")
            && env_property == "env" =>
        {
            Some(property)
        }
        _ => None,
    }
}

fn render_process_env_lookup(expr: &JsExpr) -> Option<String> {
    let name = process_env_lookup_name(expr)?;
    Some(format!("os.Getenv({})", go_string_literal(name)))
}

fn narrowed_typeof_state(test: &JsExpr, state: &AotState) -> AotState {
    let mut narrowed = clone_aot_state(state);
    let Some((name, kind)) = typeof_narrowing(test) else {
        return narrowed;
    };
    if !state.bindings.contains(&name) {
        return narrowed;
    }
    let go_ref = if state.string_bindings.contains(&name)
        || state.numeric_bindings.contains(&name)
        || state.bool_bindings.contains(&name)
    {
        go_binding_ref(&name, state)
    } else {
        format!(
            "{}.({})",
            go_binding_ref(&name, state),
            go_type_for_slot(kind)
        )
    };
    narrowed.bind_slot(&name, go_ref, kind);
    narrowed
}

fn typeof_narrowing(test: &JsExpr) -> Option<(String, AotSlotKind)> {
    let JsExpr::Binary { op, left, right } = test else {
        return None;
    };
    if !matches!(op.as_str(), "===" | "==") {
        return None;
    }
    typeof_comparison_narrowing(left, right).or_else(|| typeof_comparison_narrowing(right, left))
}

fn typeof_comparison_narrowing(
    candidate: &JsExpr,
    other: &JsExpr,
) -> Option<(String, AotSlotKind)> {
    let JsExpr::Unary { op, arg } = candidate else {
        return None;
    };
    if op != "typeof" {
        return None;
    }
    let JsExpr::Ident { name } = arg.as_ref() else {
        return None;
    };
    let kind = match string_literal_value(other)?.as_str() {
        "boolean" => AotSlotKind::Bool,
        "number" => AotSlotKind::Number,
        "string" => AotSlotKind::String,
        _ => return None,
    };
    Some((name.clone(), kind))
}

fn string_literal_value(expr: &JsExpr) -> Option<String> {
    match expr {
        JsExpr::Value {
            value: JsValue::String { value },
        } => Some(value.clone()),
        JsExpr::Template { quasis, exprs } if exprs.is_empty() && quasis.len() == 1 => {
            Some(quasis[0].clone())
        }
        _ => None,
    }
}

fn render_call_expr(callee: &JsExpr, args: &[JsExpr], state: &AotState) -> Option<String> {
    if is_json_stringify(callee) {
        let value = render_json_value_expr(args.first()?, state)?;
        return Some(format!("tsgodownJSONStringify({value})"));
    }
    if is_process_noop_call(callee, args) {
        return Some("func() any { return nil }()".to_string());
    }
    if is_process_chdir_call(callee, args) {
        let path = render_string_expr(args.first()?, state)?;
        return Some(format!("tsgodownProcessChdir({path})"));
    }
    if let JsExpr::Member {
        object,
        property,
        property_expr: None,
        optional: false,
    } = callee
    {
        let JsExpr::Ident { name } = object.as_ref() else {
            return None;
        };
        if let Some(class_name) = state.class_instance_bindings.get(name) {
            let class = state.classes.get(class_name)?;
            let method = class.methods.get(property)?;
            if method.params.len() != args.len() {
                return None;
            }
            let rendered_args = args
                .iter()
                .map(|arg| render_expr(arg, state))
                .collect::<Option<Vec<_>>>()?;
            return Some(format!(
                "{}.{}({})",
                go_binding_ref(name, state),
                sanitize_go_identifier(property),
                rendered_args.join(", ")
            ));
        }
        if let Some(function) = state
            .namespace_functions
            .get(&(name.clone(), property.clone()))
        {
            if function.params.len() != args.len() {
                return None;
            }
            let rendered_args = args
                .iter()
                .zip(function.param_kinds.iter())
                .map(|(arg, kind)| render_arg_for_kind(arg, *kind, state))
                .collect::<Option<Vec<_>>>()?;
            return Some(format!(
                "{}({})",
                function.go_name,
                rendered_args.join(", ")
            ));
        }
        if args.is_empty()
            && static_member_kind(object, property, state) == Some(AotSlotKind::StringFunction)
        {
            let function = render_static_member_expr(object, property, state)?;
            return Some(format!("{function}()"));
        }
    }
    let JsExpr::Ident { name } = callee else {
        return None;
    };
    if args.is_empty() && state.string_function_bindings.contains(name) {
        return Some(format!("{}()", go_binding_ref(name, state)));
    }
    let function = state.functions.get(name)?;
    if function.params.len() != args.len() {
        return None;
    }
    let rendered_args = args
        .iter()
        .zip(function.param_kinds.iter())
        .map(|(arg, kind)| render_arg_for_kind(arg, *kind, state))
        .collect::<Option<Vec<_>>>()?;
    Some(format!(
        "{}({})",
        function.go_name,
        rendered_args.join(", ")
    ))
}

fn render_arg_for_kind(expr: &JsExpr, kind: AotSlotKind, state: &AotState) -> Option<String> {
    match kind {
        AotSlotKind::Any => render_expr(expr, state),
        AotSlotKind::Bool => render_bool_expr(expr, state),
        AotSlotKind::Bytes => render_bytes_expr(expr, state),
        AotSlotKind::Number => render_numeric_expr(expr, state),
        AotSlotKind::String => render_string_expr(expr, state),
        AotSlotKind::StringArray => render_string_array_expr(expr, state),
        AotSlotKind::BoolFunction => render_bool_function_expr(expr, state),
        AotSlotKind::StringFunction => render_string_function_expr(expr),
    }
}

fn is_json_stringify(expr: &JsExpr) -> bool {
    matches!(
        expr,
        JsExpr::Member {
            object,
            property,
            property_expr: None,
            ..
        } if matches!(object.as_ref(), JsExpr::Ident { name } if name == "JSON") && property == "stringify"
    )
}

fn render_json_value_expr(expr: &JsExpr, state: &AotState) -> Option<String> {
    match expr {
        JsExpr::Value { value } => render_value(value),
        JsExpr::Ident { name } if state.bindings.contains(name) => {
            Some(go_binding_ref(name, state))
        }
        JsExpr::Array { items } => {
            let items = items
                .iter()
                .map(|item| render_json_value_expr(item, state))
                .collect::<Option<Vec<_>>>()?;
            Some(format!("[]any{{{}}}", items.join(", ")))
        }
        JsExpr::Object { props } => {
            let mut fields = Vec::new();
            for prop in props {
                if prop.spread || prop.key_expr.is_some() {
                    return None;
                }
                let value = render_json_value_expr(&prop.value, state)?;
                fields.push(format!("{:?}: {value}", prop.key));
            }
            Some(format!("map[string]any{{{}}}", fields.join(", ")))
        }
        JsExpr::Binary { op, .. } if op == "+" => render_expr(expr, state),
        expr if is_process_version_expr(expr) => render_process_version_expr(expr),
        expr if is_process_platform_expr(expr) => Some("tsgodownProcessPlatform()".to_string()),
        expr if is_process_env_ref(expr) => Some("tsgodownProcessEnv()".to_string()),
        expr if is_process_versions_ref(expr) => Some(render_process_versions_expr()),
        expr if is_process_cwd_ref(expr) => render_string_function_expr(expr),
        JsExpr::Call { callee, args, .. } => render_call_expr(callee, args, state),
        JsExpr::Member {
            object,
            property,
            property_expr: None,
            optional: false,
        } => render_static_member_expr(object, property, state),
        _ => None,
    }
}

fn render_value(value: &JsValue) -> Option<String> {
    match value {
        JsValue::String { value } => Some(go_string_literal(value)),
        JsValue::Number { value } => number_literal(value),
        JsValue::Bool { value } => Some(value.to_string()),
        JsValue::Null | JsValue::Undefined => Some("nil".to_string()),
        JsValue::BigInt { .. } | JsValue::RegExp { .. } => None,
    }
}

fn is_any_binding(name: &str, state: &AotState) -> bool {
    state.bindings.contains(name)
        && !state.numeric_bindings.contains(name)
        && !state.string_bindings.contains(name)
        && !state.bool_bindings.contains(name)
        && !state.bytes_bindings.contains(name)
        && !state.string_array_bindings.contains(name)
        && !state.string_function_bindings.contains(name)
        && !state.object_bindings.contains_key(name)
        && !state.class_instance_bindings.contains_key(name)
}

fn go_binding_ref(name: &str, state: &AotState) -> String {
    state
        .binding_refs
        .get(name)
        .cloned()
        .unwrap_or_else(|| sanitize_go_identifier(name))
}

fn number_literal(value: &str) -> Option<String> {
    let lower = value.to_ascii_lowercase();
    if lower.contains("nan") || lower.contains("infinity") {
        return None;
    }
    Some(value.to_string())
}

fn is_numeric_binary_op(op: &str) -> bool {
    matches!(op, "+" | "-" | "*" | "/" | "%")
}

fn go_comparison_op(op: &str) -> Option<&'static str> {
    match op {
        ">" => Some(">"),
        ">=" => Some(">="),
        "<" => Some("<"),
        "<=" => Some("<="),
        "==" | "===" => Some("=="),
        "!=" | "!==" => Some("!="),
        _ => None,
    }
}

fn indent_lines(value: &str) -> String {
    value
        .lines()
        .map(|line| format!("\t{line}"))
        .collect::<Vec<_>>()
        .join("\n")
}
