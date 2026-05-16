use std::collections::{BTreeMap, BTreeSet};

use crate::contract::{AnalyzeResponse, IrDocument, JsExpr, JsStmt, JsValue, Module};
use crate::emit_go::{go_string_literal, sanitize_go_identifier};

const CJS_DEFAULT_EXPORT_FUNCTION: &str = "__cjs_default_export";
const NODE_LTS_VERSION: &str = "24.15.0";
const NODE_LTS_VERSION_WITH_PREFIX: &str = "v24.15.0";
const AOT_FUNCTION_RENDER_LIMIT: usize = 256;

pub(crate) fn render_aot_executable_program(
    package_name: &str,
    analyzed: &AnalyzeResponse,
) -> Option<String> {
    let module = entry_module(&analyzed.ir)?;
    if !can_aot_module_graph(&analyzed.ir) {
        return None;
    }
    let module_functions = collect_module_functions(&analyzed.ir);
    if aot_function_render_limit_feature(module_functions.len()).is_some() {
        return None;
    }
    let module_classes = collect_module_classes(&analyzed.ir);
    let module_export_aliases = collect_module_export_aliases(&analyzed.ir);
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
            export_aliases: &module_export_aliases,
            default_exports: &module_default_exports,
            default_class_exports: &module_default_class_exports,
            named_exports: &module_named_exports,
            slots: &module_slots,
        },
    )?;
    state.go_imports = collect_aot_imports(&analyzed.ir);
    mark_dynamic_object_locals(&module.executable.as_ref()?.stmts, &mut state);
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
    if let Some(feature) = aot_function_render_limit_feature(module_functions.len()) {
        return vec![feature];
    }
    let module_classes = collect_module_classes(ir);
    let module_export_aliases = collect_module_export_aliases(ir);
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
                export_aliases: &module_export_aliases,
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
        if let Some(state) = state.as_ref() {
            let mut render_state = clone_aot_state(state);
            mark_dynamic_object_locals(&executable.stmts, &mut render_state);
            for stmt in &executable.stmts {
                if matches!(stmt, JsStmt::FunctionDecl { .. } | JsStmt::ClassDecl { .. })
                    || is_function_binding_stmt(stmt)
                {
                    continue;
                }
                let mut next_state = clone_aot_state(&render_state);
                if render_stmt(stmt, &mut next_state).is_none() {
                    collect_aot_render_unsupported_stmt_features(
                        stmt,
                        &module.source_path,
                        &mut features,
                    );
                } else {
                    render_state = next_state;
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

fn collect_aot_render_unsupported_stmt_features(
    stmt: &JsStmt,
    source_path: &str,
    features: &mut BTreeSet<String>,
) {
    features.insert(format!(
        "aot.statement.unsupported:{source_path}:{}",
        js_stmt_kind(stmt)
    ));
    match stmt {
        JsStmt::Expr { expr } => {
            collect_aot_render_unsupported_expr_features(expr, source_path, features);
        }
        JsStmt::FunctionDecl { body, .. } => {
            for stmt in body {
                collect_aot_render_unsupported_stmt_features(stmt, source_path, features);
            }
        }
        JsStmt::ClassDecl {
            super_class,
            methods,
            ..
        } => {
            if let Some(super_class) = super_class {
                collect_aot_render_unsupported_expr_features(super_class, source_path, features);
            }
            for method in methods {
                if !matches!(method.kind.as_str(), "constructor" | "method" | "getter") {
                    features.insert(format!(
                        "aot.class_method.unsupported:{source_path}:{}",
                        method.kind
                    ));
                }
                for stmt in &method.body {
                    collect_aot_render_unsupported_stmt_features(stmt, source_path, features);
                }
            }
        }
        JsStmt::If {
            test,
            consequent,
            alternate,
        } => {
            collect_aot_render_unsupported_expr_features(test, source_path, features);
            for stmt in consequent {
                collect_aot_render_unsupported_stmt_features(stmt, source_path, features);
            }
            for stmt in alternate {
                collect_aot_render_unsupported_stmt_features(stmt, source_path, features);
            }
        }
        JsStmt::For {
            init,
            test,
            update,
            body,
        } => {
            for stmt in init {
                collect_aot_render_unsupported_stmt_features(stmt, source_path, features);
            }
            if let Some(test) = test {
                collect_aot_render_unsupported_expr_features(test, source_path, features);
            }
            if let Some(update) = update {
                collect_aot_render_unsupported_expr_features(update, source_path, features);
            }
            for stmt in body {
                collect_aot_render_unsupported_stmt_features(stmt, source_path, features);
            }
        }
        JsStmt::ForOf { right, body, .. } => {
            collect_aot_render_unsupported_expr_features(right, source_path, features);
            for stmt in body {
                collect_aot_render_unsupported_stmt_features(stmt, source_path, features);
            }
        }
        JsStmt::While { test, body } | JsStmt::DoWhile { test, body } => {
            collect_aot_render_unsupported_expr_features(test, source_path, features);
            for stmt in body {
                collect_aot_render_unsupported_stmt_features(stmt, source_path, features);
            }
        }
        JsStmt::Switch {
            discriminant,
            cases,
        } => {
            collect_aot_render_unsupported_expr_features(discriminant, source_path, features);
            for case in cases {
                if let Some(test) = &case.test {
                    collect_aot_render_unsupported_expr_features(test, source_path, features);
                }
                for stmt in &case.consequent {
                    collect_aot_render_unsupported_stmt_features(stmt, source_path, features);
                }
            }
        }
        JsStmt::Try {
            body,
            catch_body,
            finally_body,
            ..
        } => {
            for stmt in body {
                collect_aot_render_unsupported_stmt_features(stmt, source_path, features);
            }
            for stmt in catch_body {
                collect_aot_render_unsupported_stmt_features(stmt, source_path, features);
            }
            for stmt in finally_body {
                collect_aot_render_unsupported_stmt_features(stmt, source_path, features);
            }
        }
        JsStmt::Label { body, .. } => {
            for stmt in body {
                collect_aot_render_unsupported_stmt_features(stmt, source_path, features);
            }
        }
        JsStmt::Return { value: Some(expr) }
        | JsStmt::Throw { value: expr }
        | JsStmt::Yield {
            value: Some(expr), ..
        }
        | JsStmt::VarDecl {
            init: Some(expr), ..
        } => {
            collect_aot_render_unsupported_expr_features(expr, source_path, features);
        }
        JsStmt::Return { value: None }
        | JsStmt::Yield { value: None, .. }
        | JsStmt::VarDecl { init: None, .. }
        | JsStmt::Break { .. }
        | JsStmt::Continue { .. } => {}
    }
}

fn aot_function_render_limit_feature(function_count: usize) -> Option<String> {
    if function_count <= AOT_FUNCTION_RENDER_LIMIT {
        return None;
    }
    Some(format!(
        "aot.program.function_count_limit:{function_count}>{AOT_FUNCTION_RENDER_LIMIT}"
    ))
}

fn collect_aot_render_unsupported_expr_features(
    expr: &JsExpr,
    source_path: &str,
    features: &mut BTreeSet<String>,
) {
    let expr_kind = js_expr_kind(expr);
    if is_diagnostic_expr_kind(expr_kind) {
        features.insert(format!(
            "aot.expression.unsupported:{source_path}:{expr_kind}"
        ));
    }
    match expr {
        JsExpr::Array { items } => {
            for item in items {
                collect_aot_render_unsupported_expr_features(item, source_path, features);
            }
        }
        JsExpr::ArraySpread { items } => {
            features.insert(format!("aot.expression.unsupported:{source_path}:spread"));
            for item in items {
                collect_aot_render_unsupported_expr_features(&item.value, source_path, features);
            }
        }
        JsExpr::Object { props } => {
            for prop in props {
                if let Some(key_expr) = &prop.key_expr {
                    features.insert(format!(
                        "aot.expression.unsupported:{source_path}:computed-key"
                    ));
                    collect_aot_render_unsupported_expr_features(key_expr, source_path, features);
                }
                collect_aot_render_unsupported_expr_features(&prop.value, source_path, features);
            }
        }
        JsExpr::ObjectRest { object, .. }
        | JsExpr::Unary { arg: object, .. }
        | JsExpr::Await { arg: object }
        | JsExpr::Update { arg: object, .. }
        | JsExpr::Spread { arg: object } => {
            collect_aot_render_unsupported_expr_features(object, source_path, features);
        }
        JsExpr::Function { body, .. } => {
            for stmt in body {
                collect_aot_render_unsupported_stmt_features(stmt, source_path, features);
            }
        }
        JsExpr::Class {
            super_class,
            methods,
        } => {
            if let Some(super_class) = super_class {
                collect_aot_render_unsupported_expr_features(super_class, source_path, features);
            }
            for method in methods {
                if !matches!(method.kind.as_str(), "constructor" | "method" | "getter") {
                    features.insert(format!(
                        "aot.class_method.unsupported:{source_path}:{}",
                        method.kind
                    ));
                }
                for stmt in &method.body {
                    collect_aot_render_unsupported_stmt_features(stmt, source_path, features);
                }
            }
        }
        JsExpr::Binary { left, right, .. } | JsExpr::Assign { left, right, .. } => {
            collect_aot_render_unsupported_expr_features(left, source_path, features);
            collect_aot_render_unsupported_expr_features(right, source_path, features);
        }
        JsExpr::Conditional {
            test,
            consequent,
            alternate,
        } => {
            collect_aot_render_unsupported_expr_features(test, source_path, features);
            collect_aot_render_unsupported_expr_features(consequent, source_path, features);
            collect_aot_render_unsupported_expr_features(alternate, source_path, features);
        }
        JsExpr::Call { callee, args, .. } => {
            collect_aot_render_unsupported_expr_features(callee, source_path, features);
            for arg in args {
                collect_aot_render_unsupported_expr_features(arg, source_path, features);
            }
        }
        JsExpr::New { callee, args } => {
            collect_aot_render_unsupported_expr_features(callee, source_path, features);
            for arg in args {
                collect_aot_render_unsupported_expr_features(arg, source_path, features);
            }
        }
        JsExpr::Member {
            object,
            property_expr,
            ..
        } => {
            collect_aot_render_unsupported_expr_features(object, source_path, features);
            if let Some(property_expr) = property_expr {
                collect_aot_render_unsupported_expr_features(property_expr, source_path, features);
            }
        }
        JsExpr::Template { exprs, .. } | JsExpr::Sequence { exprs } => {
            for expr in exprs {
                collect_aot_render_unsupported_expr_features(expr, source_path, features);
            }
        }
        JsExpr::Value { .. } | JsExpr::Ident { .. } | JsExpr::This | JsExpr::Super => {}
    }
}

fn is_diagnostic_expr_kind(kind: &str) -> bool {
    !matches!(kind, "value" | "ident" | "this" | "super")
}

fn js_stmt_kind(stmt: &JsStmt) -> &'static str {
    match stmt {
        JsStmt::Expr { .. } => "expr",
        JsStmt::FunctionDecl { .. } => "function-decl",
        JsStmt::ClassDecl { .. } => "class-decl",
        JsStmt::If { .. } => "if",
        JsStmt::For { .. } => "for",
        JsStmt::ForOf { .. } => "for-of",
        JsStmt::While { .. } => "while",
        JsStmt::DoWhile { .. } => "do-while",
        JsStmt::Switch { .. } => "switch",
        JsStmt::Try { .. } => "try",
        JsStmt::Label { .. } => "label",
        JsStmt::Break { .. } => "break",
        JsStmt::Continue { .. } => "continue",
        JsStmt::Return { .. } => "return",
        JsStmt::Throw { .. } => "throw",
        JsStmt::Yield { .. } => "yield",
        JsStmt::VarDecl { .. } => "var-decl",
    }
}

fn js_expr_kind(expr: &JsExpr) -> &'static str {
    match expr {
        JsExpr::Value { .. } => "value",
        JsExpr::Ident { .. } => "ident",
        JsExpr::This => "this",
        JsExpr::Super => "super",
        JsExpr::Array { .. } => "array",
        JsExpr::ArraySpread { .. } => "array-spread",
        JsExpr::Object { .. } => "object",
        JsExpr::ObjectRest { .. } => "object-rest",
        JsExpr::Function { .. } => "function",
        JsExpr::Class { .. } => "class",
        JsExpr::Unary { .. } => "unary",
        JsExpr::Await { .. } => "await",
        JsExpr::Binary { .. } => "binary",
        JsExpr::Conditional { .. } => "conditional",
        JsExpr::Assign { .. } => "assign",
        JsExpr::Update { .. } => "update",
        JsExpr::Call { .. } => "call",
        JsExpr::Spread { .. } => "spread",
        JsExpr::New { .. } => "new",
        JsExpr::Member { .. } => "member",
        JsExpr::Template { .. } => "template",
        JsExpr::Sequence { .. } => "sequence",
    }
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

fn awaited_dynamic_import_spec(expr: &JsExpr) -> Option<&str> {
    let JsExpr::Await { arg } = expr else {
        return None;
    };
    dynamic_import_call_spec(arg)
}

fn awaited_dynamic_import_default_spec(expr: &JsExpr) -> Option<&str> {
    let JsExpr::Member {
        object,
        property,
        property_expr: None,
        optional: false,
    } = expr
    else {
        return None;
    };
    if property != "default" {
        return None;
    }
    awaited_dynamic_import_spec(object)
}

fn dynamic_import_namespace_member<'a>(
    expr: &'a JsExpr,
    state: &'a AotState,
) -> Option<(&'a str, &'a str)> {
    let JsExpr::Member {
        object,
        property,
        property_expr: None,
        optional: false,
    } = expr
    else {
        return None;
    };
    let JsExpr::Ident { name } = object.as_ref() else {
        return None;
    };
    Some((
        state.dynamic_import_namespaces.get(name)?.as_str(),
        property,
    ))
}

fn dynamic_import_call_spec(expr: &JsExpr) -> Option<&str> {
    let JsExpr::Call { callee, args, .. } = expr else {
        return None;
    };
    if !matches!(callee.as_ref(), JsExpr::Ident { name } if name == "import") {
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
        && matches!(
            name,
            "fs" | "path"
                | "os"
                | "crypto"
                | "Buffer"
                | "URL"
                | "EventEmitter"
                | "querystring"
                | "process"
        )
}

fn collect_aot_imports(ir: &IrDocument) -> BTreeSet<&'static str> {
    let mut imports = BTreeSet::new();
    imports.insert("strconv");
    imports.insert("strings");
    for module in &ir.modules {
        if let Some(executable) = &module.executable {
            for stmt in &executable.stmts {
                collect_stmt_imports(stmt, &mut imports);
            }
        }
    }
    if imports.contains("regexp") {
        imports.insert("strings");
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
        JsStmt::While { test, body } => {
            collect_expr_imports(test, imports);
            for stmt in body {
                collect_stmt_imports(stmt, imports);
            }
        }
        JsStmt::DoWhile { body, test } => {
            for stmt in body {
                collect_stmt_imports(stmt, imports);
            }
            collect_expr_imports(test, imports);
        }
        _ => {}
    }
}

fn collect_expr_imports(expr: &JsExpr, imports: &mut BTreeSet<&'static str>) {
    match expr {
        JsExpr::Call { callee, args, .. } => {
            if is_json_parse_call(callee, args) {
                imports.insert("encoding/json");
            }
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
            if is_object_keys_call(callee, args) {
                imports.insert("sort");
                imports.insert("strconv");
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
            if is_string_replace_call(callee, args) {
                match args.first() {
                    Some(JsExpr::Value {
                        value: JsValue::RegExp { .. },
                    }) => {
                        imports.insert("regexp");
                    }
                    Some(JsExpr::Value {
                        value: JsValue::String { .. },
                    }) => {
                        imports.insert("strings");
                    }
                    _ => {}
                }
            }
            if is_node_path_string_call(callee, args) {
                if is_node_path_posix_call(callee) {
                    imports.insert("path");
                } else {
                    imports.insert("path/filepath");
                }
                if is_node_path_basename_call(callee, args) {
                    imports.insert("strings");
                }
            }
            if is_node_fs_mkdtemp_sync_call(callee, args)
                || is_node_fs_write_file_sync_call(callee, args)
                || is_node_fs_read_file_sync_call(callee, args)
                || is_node_fs_rm_sync_call(callee, args)
            {
                imports.insert("os");
                imports.insert("path/filepath");
            }
            if is_node_os_tmpdir_call(callee, args) {
                imports.insert("os");
            }
            if is_querystring_parse_call(callee, args) {
                imports.insert("net/url");
                imports.insert("strings");
            }
            if is_crypto_sha256_hex_slice_call(callee) || is_crypto_sha256_hex_digest_call(callee) {
                imports.insert("encoding/hex");
            }
            if is_url_search_params_get_call(callee, args) {
                imports.insert("net/url");
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
            if is_buffer_to_string_call(callee, args) {
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
        JsExpr::New { callee, args } => {
            if is_new_url_expr(callee, args) {
                imports.insert("net/url");
            }
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
                if let Some(key_expr) = &prop.key_expr {
                    collect_expr_imports(key_expr, imports);
                }
                collect_expr_imports(&prop.value, imports);
            }
        }
        JsExpr::Function { body, .. } => {
            for stmt in body {
                collect_stmt_imports(stmt, imports);
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
            || is_process_argv_ref(expr)
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

func tsgodownStringArraySlice(values []string, start float64, endValues ...float64) []string {
	length := len(values)
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
	out := make([]string, to-from)
	copy(out, values[from:to])
	return out
}

func tsgodownNumberArrayAt(values []float64, index float64) float64 {
	offset := int(index)
	if values == nil || offset < 0 || offset >= len(values) {
		return 0
	}
	return values[offset]
}

func tsgodownArrayAt(value any, index float64) any {
	offset := int(index)
	switch values := value.(type) {
	case []any:
		if offset < 0 {
			offset = len(values) + offset
		}
		if offset < 0 || offset >= len(values) {
			return nil
		}
		return values[offset]
	case []string:
		if offset < 0 {
			offset = len(values) + offset
		}
		if offset < 0 || offset >= len(values) {
			return nil
		}
		return values[offset]
	case []float64:
		if offset < 0 {
			offset = len(values) + offset
		}
		if offset < 0 || offset >= len(values) {
			return nil
		}
		return values[offset]
	default:
		return nil
	}
}

func tsgodownNumberArraySet(values []float64, index float64, value float64) []float64 {
	offset := int(index)
	if offset < 0 {
		return values
	}
	for len(values) <= offset {
		values = append(values, 0)
	}
	values[offset] = value
	return values
}

func tsgodownNumberArrayPop(values *[]float64) float64 {
	if values == nil || len(*values) == 0 {
		return 0
	}
	offset := len(*values) - 1
	value := (*values)[offset]
	*values = (*values)[:offset]
	return value
}

func tsgodownStringCharAt(value string, index float64) string {
	chars := []rune(value)
	offset := int(index)
	if offset < 0 || offset >= len(chars) {
		return ""
	}
	return string(chars[offset])
}

func tsgodownStringCharCodeAt(value string, index float64) float64 {
	chars := []rune(value)
	offset := int(index)
	if offset < 0 || offset >= len(chars) {
		return 0
	}
	return float64(chars[offset])
}

func tsgodownStringFromCharCode(values ...float64) string {
	runes := make([]rune, len(values))
	for index, value := range values {
		runes[index] = rune(int(value))
	}
	return string(runes)
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

func tsgodownStrictEqual(left any, right any) bool {
	switch leftValue := left.(type) {
	case nil:
		return right == nil
	case bool:
		rightValue, ok := right.(bool)
		return ok && leftValue == rightValue
	case float64:
		return leftValue == tsgodownToFloat64(right)
	case int:
		return float64(leftValue) == tsgodownToFloat64(right)
	case int64:
		return float64(leftValue) == tsgodownToFloat64(right)
	case string:
		rightValue, ok := right.(string)
		return ok && leftValue == rightValue
	default:
				return left == right
	}
}

func tsgodownObjectFromAny(value any) map[string]any {
	switch value := value.(type) {
	case nil:
		return map[string]any{}
	case map[string]any:
		return value
	default:
		return map[string]any{}
	}
}

func tsgodownObjectProp(value any, key string) any {
	object := tsgodownObjectFromAny(value)
	return object[key]
}

func tsgodownObjectSetProp(value any, key string, propertyValue any) {
	object := tsgodownObjectFromAny(value)
	object[key] = propertyValue
}

type tsgodownJSMap struct {
	order []string
	items map[string]any
}

func tsgodownNewMap() *tsgodownJSMap {
	return &tsgodownJSMap{order: []string{}, items: map[string]any{}}
}

func tsgodownMapSet(target *tsgodownJSMap, key string, value any) *tsgodownJSMap {
	if target == nil {
		target = tsgodownNewMap()
	}
	if _, ok := target.items[key]; !ok {
		target.order = append(target.order, key)
	}
	target.items[key] = value
	return target
}

func tsgodownMapGet(target *tsgodownJSMap, key string) any {
	if target == nil {
		return nil
	}
	return target.items[key]
}

func tsgodownMapHas(target *tsgodownJSMap, key string) bool {
	if target == nil {
		return false
	}
	_, ok := target.items[key]
	return ok
}

func tsgodownMapDelete(target *tsgodownJSMap, key string) bool {
	if target == nil {
		return false
	}
	if _, ok := target.items[key]; !ok {
		return false
	}
	delete(target.items, key)
	for index, existing := range target.order {
		if existing == key {
			target.order = append(target.order[:index], target.order[index+1:]...)
			break
		}
	}
	return true
}

func tsgodownMapSize(target *tsgodownJSMap) float64 {
	if target == nil {
		return 0
	}
	return float64(len(target.items))
}

func tsgodownCall(value any, args ...any) any {
	switch fn := value.(type) {
	case func() any:
		if len(args) == 0 {
			return fn()
		}
	case func(any) any:
		if len(args) == 1 {
			return fn(args[0])
		}
	case func(any, any) any:
		if len(args) == 2 {
			return fn(args[0], args[1])
		}
	}
	return nil
}
"#
        .to_string(),
    ];
    if imports.contains("encoding/json") {
        helpers.push(
            r#"func tsgodownJSONStringify(value any) string {
	bytes, err := json.Marshal(value)
	if err != nil {
		return ""
	}
	return string(bytes)
}

func tsgodownJSONStringifyIndent(value any, indent string) string {
	bytes, err := json.MarshalIndent(value, "", indent)
	if err != nil {
		return ""
	}
	return string(bytes)
}

func tsgodownJSONParseObject(value string) map[string]any {
	var out map[string]any
	if err := json.Unmarshal([]byte(value), &out); err != nil {
		return map[string]any{}
	}
	return out
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

func tsgodownBytesToString(value []byte, encoding string) string {
	switch encoding {
	case "base64":
		return base64.StdEncoding.EncodeToString(value)
	case "hex":
		return hex.EncodeToString(value)
	default:
		return string(value)
	}
}
"#
            .to_string(),
        );
    }
    if imports.contains("net/url") {
        helpers.push(
            r#"type tsgodownURL struct {
	value *url.URL
}

func tsgodownNewURL(input string, base string) *tsgodownURL {
	baseURL, err := url.Parse(base)
	if err != nil {
		return &tsgodownURL{value: &url.URL{}}
	}
	value, err := baseURL.Parse(input)
	if err != nil {
		return &tsgodownURL{value: &url.URL{}}
	}
	return &tsgodownURL{value: value}
}

func tsgodownURLPathname(value *tsgodownURL) string {
	if value == nil || value.value == nil {
		return ""
	}
	return value.value.Path
}

func tsgodownURLSearchParam(value *tsgodownURL, key string) string {
	if value == nil || value.value == nil {
		return ""
	}
	return value.value.Query().Get(key)
}

func tsgodownQuerystringParse(value string) map[string]any {
	parsed, err := url.ParseQuery(value)
	if err != nil {
		return map[string]any{}
	}
	out := map[string]any{}
	for key, values := range parsed {
		if len(values) == 1 {
			out[key] = values[0]
			continue
		}
		items := make([]any, len(values))
		for index, value := range values {
			items[index] = value
		}
		out[key] = items
	}
	return out
}
"#
            .to_string(),
        );
    }
    if imports.contains("encoding/hex") {
        helpers.push(
            r#"func tsgodownSHA256Hex(value string) string {
	hashes := [8]uint32{0x6a09e667, 0xbb67ae85, 0x3c6ef372, 0xa54ff53a, 0x510e527f, 0x9b05688c, 0x1f83d9ab, 0x5be0cd19}
	rounds := [64]uint32{
		0x428a2f98, 0x71374491, 0xb5c0fbcf, 0xe9b5dba5, 0x3956c25b, 0x59f111f1, 0x923f82a4, 0xab1c5ed5,
		0xd807aa98, 0x12835b01, 0x243185be, 0x550c7dc3, 0x72be5d74, 0x80deb1fe, 0x9bdc06a7, 0xc19bf174,
		0xe49b69c1, 0xefbe4786, 0x0fc19dc6, 0x240ca1cc, 0x2de92c6f, 0x4a7484aa, 0x5cb0a9dc, 0x76f988da,
		0x983e5152, 0xa831c66d, 0xb00327c8, 0xbf597fc7, 0xc6e00bf3, 0xd5a79147, 0x06ca6351, 0x14292967,
		0x27b70a85, 0x2e1b2138, 0x4d2c6dfc, 0x53380d13, 0x650a7354, 0x766a0abb, 0x81c2c92e, 0x92722c85,
		0xa2bfe8a1, 0xa81a664b, 0xc24b8b70, 0xc76c51a3, 0xd192e819, 0xd6990624, 0xf40e3585, 0x106aa070,
		0x19a4c116, 0x1e376c08, 0x2748774c, 0x34b0bcb5, 0x391c0cb3, 0x4ed8aa4a, 0x5b9cca4f, 0x682e6ff3,
		0x748f82ee, 0x78a5636f, 0x84c87814, 0x8cc70208, 0x90befffa, 0xa4506ceb, 0xbef9a3f7, 0xc67178f2,
	}
	rotateRight := func(value uint32, shift uint) uint32 {
		return value>>shift | value<<(32-shift)
	}
	message := append([]byte(value), 0x80)
	for (len(message)+8)%64 != 0 {
		message = append(message, 0)
	}
	bitLength := uint64(len(value)) * 8
	for shift := 56; shift >= 0; shift -= 8 {
		message = append(message, byte(bitLength>>uint(shift)))
		if shift == 0 {
			break
		}
	}
	for offset := 0; offset < len(message); offset += 64 {
		chunk := message[offset : offset+64]
		var words [64]uint32
		for index := 0; index < 16; index++ {
			base := index * 4
			words[index] = uint32(chunk[base])<<24 | uint32(chunk[base+1])<<16 | uint32(chunk[base+2])<<8 | uint32(chunk[base+3])
		}
		for index := 16; index < 64; index++ {
			s0 := rotateRight(words[index-15], 7) ^ rotateRight(words[index-15], 18) ^ (words[index-15] >> 3)
			s1 := rotateRight(words[index-2], 17) ^ rotateRight(words[index-2], 19) ^ (words[index-2] >> 10)
			words[index] = words[index-16] + s0 + words[index-7] + s1
		}
		a, b, c, d := hashes[0], hashes[1], hashes[2], hashes[3]
		e, f, g, h := hashes[4], hashes[5], hashes[6], hashes[7]
		for index := 0; index < 64; index++ {
			s1 := rotateRight(e, 6) ^ rotateRight(e, 11) ^ rotateRight(e, 25)
			ch := (e & f) ^ (^e & g)
			temp1 := h + s1 + ch + rounds[index] + words[index]
			s0 := rotateRight(a, 2) ^ rotateRight(a, 13) ^ rotateRight(a, 22)
			maj := (a & b) ^ (a & c) ^ (b & c)
			temp2 := s0 + maj
			h, g, f, e = g, f, e, d+temp1
			d, c, b, a = c, b, a, temp1+temp2
		}
		hashes[0] += a
		hashes[1] += b
		hashes[2] += c
		hashes[3] += d
		hashes[4] += e
		hashes[5] += f
		hashes[6] += g
		hashes[7] += h
	}
	out := make([]byte, 32)
	for index, value := range hashes {
		base := index * 4
		out[base] = byte(value >> 24)
		out[base+1] = byte(value >> 16)
		out[base+2] = byte(value >> 8)
		out[base+3] = byte(value)
	}
	return hex.EncodeToString(out)
}
"#
            .to_string(),
        );
    }
    if imports.contains("os") && imports.contains("path/filepath") {
        helpers.push(
            r#"func tsgodownFsMkdtempSync(prefix string) string {
	dir := filepath.Dir(prefix)
	pattern := filepath.Base(prefix)
	if dir == "." {
		dir = ""
	}
	value, err := os.MkdirTemp(dir, pattern)
	if err != nil {
		return ""
	}
	return value
}

func tsgodownFsWriteFileSync(path string, data string) {
	_ = os.WriteFile(path, []byte(data), 0o666)
}

func tsgodownFsReadFileSync(path string, encoding string) string {
	bytes, err := os.ReadFile(path)
	if err != nil {
		return ""
	}
	return string(bytes)
}

func tsgodownFsRmSync(path string) {
	_ = os.RemoveAll(path)
}

func tsgodownOsTmpdir() string {
	return os.TempDir()
}
"#
            .to_string(),
        );
    }
    helpers.push(
        r#"type tsgodownEventEmitter struct {
	listeners map[string][]func(any) any
}

func tsgodownNewEventEmitter() *tsgodownEventEmitter {
	return &tsgodownEventEmitter{listeners: map[string][]func(any) any{}}
}

func tsgodownEventEmitterOn(target *tsgodownEventEmitter, name string, listener func(any) any) any {
	if target == nil {
		return nil
	}
	target.listeners[name] = append(target.listeners[name], listener)
	return target
}

func tsgodownEventEmitterEmit(target *tsgodownEventEmitter, name string, payload any) bool {
	if target == nil {
		return false
	}
	listeners := target.listeners[name]
	for _, listener := range listeners {
		_ = listener(payload)
	}
	return len(listeners) > 0
}
"#
        .to_string(),
    );
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

func tsgodownToFloat64(value any) float64 {
	switch value := value.(type) {
	case nil:
		return 0
	case bool:
		if value {
			return 1
		}
		return 0
	case float64:
		return value
	case int:
		return float64(value)
	case int64:
		return float64(value)
	case string:
		number, err := strconv.ParseFloat(value, 64)
		if err != nil {
			return 0
		}
		return number
	default:
		return 0
	}
}

func tsgodownToBool(value any) bool {
	switch value := value.(type) {
	case nil:
		return false
	case bool:
		return value
	case float64:
		return value != 0
	case int:
		return value != 0
	case int64:
		return value != 0
	case string:
		return value != ""
	default:
		return true
	}
}

func tsgodownIsNaN(value any) bool {
	text := tsgodownToString(value)
	if text == "" {
		return false
	}
	_, err := strconv.ParseFloat(text, 64)
	return err != nil
}

func tsgodownParseInt(value any, radix float64) float64 {
	base := int(radix)
	if base == 0 {
		base = 10
	}
	parsed, err := strconv.ParseInt(strings.TrimSpace(tsgodownToString(value)), base, 64)
	if err != nil {
		return 0
	}
	return float64(parsed)
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
    if imports.contains("sort") {
        helpers.push(
            r#"func tsgodownObjectKeys(keys []string) []string {
	seen := map[string]bool{}
	integerKeys := []string{}
	stringKeys := []string{}
	for _, key := range keys {
		if seen[key] {
			continue
		}
		seen[key] = true
		if tsgodownIsArrayIndexKey(key) {
			integerKeys = append(integerKeys, key)
			continue
		}
		stringKeys = append(stringKeys, key)
	}
	sort.Slice(integerKeys, func(left int, right int) bool {
		leftValue, _ := strconv.ParseUint(integerKeys[left], 10, 32)
		rightValue, _ := strconv.ParseUint(integerKeys[right], 10, 32)
		return leftValue < rightValue
	})
	return append(integerKeys, stringKeys...)
}

func tsgodownIsArrayIndexKey(key string) bool {
	if key == "" {
		return false
	}
	value, err := strconv.ParseUint(key, 10, 32)
	if err != nil || value == 4294967295 {
		return false
	}
	return strconv.FormatUint(value, 10) == key
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

func tsgodownRegexpReplace(value string, pattern string, replacement string, global bool) string {
	re := regexp.MustCompile(pattern)
	replacement = strings.ReplaceAll(replacement, "$&", "$0")
	if global {
		return re.ReplaceAllString(value, replacement)
	}
	match := re.FindStringSubmatchIndex(value)
	if match == nil {
		return value
	}
	var expanded []byte
	expanded = re.ExpandString(expanded, replacement, value, match)
	return value[:match[0]] + string(expanded) + value[match[1]:]
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

func tsgodownProcessArgv(entry string) []string {
	argv := []string{tsgodownProcessExecPath(), entry}
	if len(os.Args) > 1 {
		argv = append(argv, os.Args[1:]...)
	}
	return argv
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
        collect_classes_from_stmts(module, entry, &executable.stmts, &mut classes);
    }
    classes
}

fn collect_classes_from_stmts(
    module: &Module,
    entry: &Module,
    stmts: &[JsStmt],
    classes: &mut BTreeMap<(String, String), AotClass>,
) {
    for stmt in stmts {
        if let Some(class) = collect_class(module, entry, stmt) {
            classes.insert((module.id.clone(), class.name.clone()), class);
        }
        match stmt {
            JsStmt::FunctionDecl { body, .. } => {
                collect_classes_from_stmts(module, entry, body, classes);
            }
            JsStmt::If {
                consequent,
                alternate,
                ..
            } => {
                collect_classes_from_stmts(module, entry, consequent, classes);
                collect_classes_from_stmts(module, entry, alternate, classes);
            }
            JsStmt::For { init, body, .. } => {
                collect_classes_from_stmts(module, entry, init, classes);
                collect_classes_from_stmts(module, entry, body, classes);
            }
            JsStmt::ForOf { body, .. }
            | JsStmt::While { body, .. }
            | JsStmt::DoWhile { body, .. }
            | JsStmt::Label { body, .. } => {
                collect_classes_from_stmts(module, entry, body, classes);
            }
            JsStmt::Switch { cases, .. } => {
                for case in cases {
                    collect_classes_from_stmts(module, entry, &case.consequent, classes);
                }
            }
            JsStmt::Try {
                body,
                catch_body,
                finally_body,
                ..
            } => {
                collect_classes_from_stmts(module, entry, body, classes);
                collect_classes_from_stmts(module, entry, catch_body, classes);
                collect_classes_from_stmts(module, entry, finally_body, classes);
            }
            JsStmt::Expr { expr }
            | JsStmt::Return { value: Some(expr) }
            | JsStmt::Throw { value: expr }
            | JsStmt::Yield {
                value: Some(expr), ..
            }
            | JsStmt::VarDecl {
                init: Some(expr), ..
            } => {
                collect_classes_from_expr(module, entry, expr, classes);
            }
            JsStmt::ClassDecl { .. }
            | JsStmt::Return { value: None }
            | JsStmt::Yield { value: None, .. }
            | JsStmt::VarDecl { init: None, .. }
            | JsStmt::Break { .. }
            | JsStmt::Continue { .. } => {}
        }
    }
}

fn collect_classes_from_expr(
    module: &Module,
    entry: &Module,
    expr: &JsExpr,
    classes: &mut BTreeMap<(String, String), AotClass>,
) {
    match expr {
        JsExpr::Function { body, .. } => {
            collect_classes_from_stmts(module, entry, body, classes);
        }
        JsExpr::Array { items } => {
            for item in items {
                collect_classes_from_expr(module, entry, item, classes);
            }
        }
        JsExpr::ArraySpread { items } => {
            for item in items {
                collect_classes_from_expr(module, entry, &item.value, classes);
            }
        }
        JsExpr::Object { props } => {
            for prop in props {
                if let Some(key_expr) = &prop.key_expr {
                    collect_classes_from_expr(module, entry, key_expr, classes);
                }
                collect_classes_from_expr(module, entry, &prop.value, classes);
            }
        }
        JsExpr::ObjectRest { object, .. }
        | JsExpr::Unary { arg: object, .. }
        | JsExpr::Await { arg: object }
        | JsExpr::Update { arg: object, .. }
        | JsExpr::Spread { arg: object } => {
            collect_classes_from_expr(module, entry, object, classes);
        }
        JsExpr::Class { methods, .. } => {
            for method in methods {
                collect_classes_from_stmts(module, entry, &method.body, classes);
            }
        }
        JsExpr::Binary { left, right, .. } | JsExpr::Assign { left, right, .. } => {
            collect_classes_from_expr(module, entry, left, classes);
            collect_classes_from_expr(module, entry, right, classes);
        }
        JsExpr::Conditional {
            test,
            consequent,
            alternate,
        } => {
            collect_classes_from_expr(module, entry, test, classes);
            collect_classes_from_expr(module, entry, consequent, classes);
            collect_classes_from_expr(module, entry, alternate, classes);
        }
        JsExpr::Call { callee, args, .. } | JsExpr::New { callee, args } => {
            collect_classes_from_expr(module, entry, callee, classes);
            for arg in args {
                collect_classes_from_expr(module, entry, arg, classes);
            }
        }
        JsExpr::Member {
            object,
            property_expr,
            ..
        } => {
            collect_classes_from_expr(module, entry, object, classes);
            if let Some(property_expr) = property_expr {
                collect_classes_from_expr(module, entry, property_expr, classes);
            }
        }
        JsExpr::Template { exprs, .. } | JsExpr::Sequence { exprs } => {
            for expr in exprs {
                collect_classes_from_expr(module, entry, expr, classes);
            }
        }
        JsExpr::Value { .. } | JsExpr::Ident { .. } | JsExpr::This | JsExpr::Super => {}
    }
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
    let mut class_getters = BTreeMap::new();
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
        if method.kind == "getter" {
            if !method.params.is_empty() || method.body.len() != 1 {
                return None;
            }
            let JsStmt::Return { value: Some(value) } = &method.body[0] else {
                return None;
            };
            class_getters.insert(
                method.name.clone(),
                AotMethod {
                    params: Vec::new(),
                    return_expr: value.clone(),
                },
            );
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
        getters: class_getters,
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

fn collect_module_export_aliases(ir: &IrDocument) -> BTreeMap<(String, String), String> {
    let mut aliases = BTreeMap::new();
    for module in &ir.modules {
        let Some(executable) = &module.executable else {
            continue;
        };
        for stmt in &executable.stmts {
            let JsStmt::VarDecl {
                name,
                init: Some(JsExpr::Ident { name: local }),
            } = stmt
            else {
                continue;
            };
            if !is_exported_name(module, name) || name == local {
                continue;
            }
            aliases.insert((module.id.clone(), name.clone()), local.clone());
        }
    }
    aliases
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
    let module_export_aliases = collect_module_export_aliases(ir);
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
                export_aliases: &module_export_aliases,
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
    let module_export_aliases = collect_module_export_aliases(ir);
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
                export_aliases: &module_export_aliases,
                default_exports: &module_default_exports,
                default_class_exports: &module_default_class_exports,
                named_exports: &module_named_exports,
                slots: module_slots,
            },
        )?;
        let mut rendered_classes = BTreeSet::new();
        for stmt in &module.executable.as_ref()?.stmts {
            if let JsStmt::ClassDecl { name, .. } = stmt {
                let class = module_classes.get(&(module.id.clone(), name.clone()))?;
                declarations.push(render_class_decl(class)?);
                rendered_classes.insert(name.clone());
            }
        }
        for ((module_id, name), class) in module_classes {
            if module_id == &module.id && !rendered_classes.contains(name) {
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
    let mut state = AotState {
        entry_source_path: entry_module(ir).map(|module| module.source_path.clone()),
        ..AotState::default()
    };
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
    for ((module_id, name), class) in context.classes {
        if module_id == &module.id {
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
            if let Some(local) = context
                .export_aliases
                .get(&(imported_module.id.clone(), imported.to_string()))
            {
                if let Some(function) = context
                    .functions
                    .get(&(imported_module.id.clone(), local.clone()))
                {
                    state
                        .functions
                        .insert(binding.local.clone(), function.clone());
                    continue;
                }
                if let Some(class) = context
                    .classes
                    .get(&(imported_module.id.clone(), local.clone()))
                {
                    state.classes.insert(binding.local.clone(), class.clone());
                    continue;
                }
                if let Some(slot) = context
                    .slots
                    .get(&(imported_module.id.clone(), local.clone()))
                {
                    state.bind_slot(&binding.local, slot.go_name.clone(), slot.kind);
                    continue;
                }
            }
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

fn is_shadowed_cjs_export_target(expr: &JsExpr, state: &AotState) -> bool {
    matches!(
        expr,
        JsExpr::Member {
            object,
            property_expr: None,
            ..
        } if matches!(object.as_ref(), JsExpr::Ident { name } if name == "exports" && state.bindings.contains(name))
    )
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
    number_array_bindings: BTreeSet<String>,
    string_array_bindings: BTreeSet<String>,
    any_array_bindings: BTreeSet<String>,
    map_bindings: BTreeSet<String>,
    url_bindings: BTreeSet<String>,
    event_emitter_bindings: BTreeSet<String>,
    number_closure_bindings: BTreeSet<String>,
    string_function_bindings: BTreeSet<String>,
    dynamic_object_bindings: BTreeSet<String>,
    object_bindings: BTreeMap<String, AotObject>,
    class_instance_bindings: BTreeMap<String, String>,
    current_receiver: Option<String>,
    current_fields: BTreeMap<String, AotSlotKind>,
    functions: BTreeMap<String, AotFunction>,
    classes: BTreeMap<String, AotClass>,
    namespace_functions: BTreeMap<(String, String), AotFunction>,
    builtin_bindings: BTreeSet<String>,
    dynamic_import_namespaces: BTreeMap<String, String>,
    entry_source_path: Option<String>,
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
            AotSlotKind::NumberArray => {
                self.number_array_bindings.insert(name.to_string());
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
        number_array_bindings: state.number_array_bindings.clone(),
        string_array_bindings: state.string_array_bindings.clone(),
        any_array_bindings: state.any_array_bindings.clone(),
        map_bindings: state.map_bindings.clone(),
        url_bindings: state.url_bindings.clone(),
        event_emitter_bindings: state.event_emitter_bindings.clone(),
        number_closure_bindings: state.number_closure_bindings.clone(),
        string_function_bindings: state.string_function_bindings.clone(),
        dynamic_object_bindings: state.dynamic_object_bindings.clone(),
        object_bindings: state.object_bindings.clone(),
        class_instance_bindings: state.class_instance_bindings.clone(),
        current_receiver: state.current_receiver.clone(),
        current_fields: state.current_fields.clone(),
        functions: state.functions.clone(),
        classes: state.classes.clone(),
        namespace_functions: state.namespace_functions.clone(),
        builtin_bindings: state.builtin_bindings.clone(),
        dynamic_import_namespaces: state.dynamic_import_namespaces.clone(),
        entry_source_path: state.entry_source_path.clone(),
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
    export_aliases: &'a BTreeMap<(String, String), String>,
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
    getters: BTreeMap<String, AotMethod>,
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
    NumberArray,
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
        AotSlotKind::NumberArray => "[]float64",
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
                if let Some(spec) = awaited_dynamic_import_spec(expr) {
                    state.bindings.insert(name.clone());
                    state.binding_refs.insert(name.clone(), ident.clone());
                    state
                        .dynamic_import_namespaces
                        .insert(name.clone(), spec.to_string());
                    if is_node_builtin_spec(spec) && !name.starts_with("__tsgodown_destructure_") {
                        state.builtin_bindings.insert(name.clone());
                    }
                    return Some(String::new());
                }
                if let Some(spec) = awaited_dynamic_import_default_spec(expr) {
                    if is_node_builtin_spec(spec) {
                        state.bindings.insert(name.clone());
                        state.binding_refs.insert(name.clone(), ident.clone());
                        state.builtin_bindings.insert(name.clone());
                        return Some(String::new());
                    }
                }
                if let Some((spec, _property)) = dynamic_import_namespace_member(expr, state) {
                    if is_node_builtin_spec(spec) {
                        state.bindings.insert(name.clone());
                        state.binding_refs.insert(name.clone(), ident.clone());
                        state.builtin_bindings.insert(name.clone());
                        return Some(String::new());
                    }
                }
                if matches!(expr, JsExpr::Function { .. }) && state.functions.contains_key(name) {
                    return Some(String::new());
                }
                if state.number_array_bindings.contains(name) && is_nullish_expr(expr) {
                    return Some(format!("var {ident} []float64 = nil"));
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
                if state.any_array_bindings.contains(name)
                    && matches!(expr, JsExpr::Array { items } if items.is_empty())
                {
                    state.bindings.insert(name.clone());
                    state.binding_refs.insert(name.clone(), ident.clone());
                    return Some(format!("var {ident} []any = []any{{}}"));
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
                if let Some(value) = render_number_array_expr(expr, state) {
                    state.bind_slot(name, ident.clone(), AotSlotKind::NumberArray);
                    return Some(format!("var {ident} []float64 = {value}"));
                }
                if let Some(value) = render_string_array_expr(expr, state) {
                    state.bind_slot(name, ident.clone(), AotSlotKind::StringArray);
                    return Some(format!("var {ident} []string = {value}"));
                }
                if let Some(value) = render_number_closure_expr(expr, state) {
                    state.bindings.insert(name.clone());
                    state.binding_refs.insert(name.clone(), ident.clone());
                    state.number_closure_bindings.insert(name.clone());
                    return Some(format!("var {ident} func(float64) any = {value}"));
                }
                if let Some(value) = render_any_array_expr(expr, state) {
                    state.bindings.insert(name.clone());
                    state.binding_refs.insert(name.clone(), ident.clone());
                    state.any_array_bindings.insert(name.clone());
                    return Some(format!("var {ident} []any = {value}"));
                }
                if let Some(value) = render_js_map_expr(expr) {
                    state.bindings.insert(name.clone());
                    state.binding_refs.insert(name.clone(), ident.clone());
                    state.map_bindings.insert(name.clone());
                    return Some(format!("var {ident} *tsgodownJSMap = {value}"));
                }
                if let Some(value) = render_url_new_expr(expr, state) {
                    state.bindings.insert(name.clone());
                    state.binding_refs.insert(name.clone(), ident.clone());
                    state.url_bindings.insert(name.clone());
                    return Some(format!("var {ident} *tsgodownURL = {value}"));
                }
                if let Some(value) = render_event_emitter_new_expr(expr, state) {
                    state.bindings.insert(name.clone());
                    state.binding_refs.insert(name.clone(), ident.clone());
                    state.event_emitter_bindings.insert(name.clone());
                    return Some(format!("var {ident} *tsgodownEventEmitter = {value}"));
                }
                if state.dynamic_object_bindings.contains(name) {
                    if let Some(value) = render_dynamic_object_init_expr(expr, state) {
                        state.bindings.insert(name.clone());
                        state.binding_refs.insert(name.clone(), ident.clone());
                        return Some(format!("var {ident} map[string]any = {value}"));
                    }
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
                if let Some(value) = render_object_map_expr(expr, state) {
                    state.bindings.insert(name.clone());
                    state.binding_refs.insert(name.clone(), ident.clone());
                    state.dynamic_object_bindings.insert(name.clone());
                    return Some(format!("var {ident} map[string]any = {value}"));
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
                if let Some(value) = render_async_iife_expr(expr, state) {
                    state.bindings.insert(name.clone());
                    state.binding_refs.insert(name.clone(), ident.clone());
                    return Some(format!("var {ident} any = {value}"));
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
            if state.number_array_bindings.contains(name) {
                return Some(format!("var {ident} []float64 = nil"));
            }
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
        JsStmt::Try {
            body,
            catch_body,
            finally_body,
            ..
        } => render_try_finally_stmt(body, catch_body, finally_body, state),
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
        number_array_bindings: state.number_array_bindings.clone(),
        string_array_bindings: state.string_array_bindings.clone(),
        any_array_bindings: state.any_array_bindings.clone(),
        map_bindings: state.map_bindings.clone(),
        url_bindings: state.url_bindings.clone(),
        event_emitter_bindings: state.event_emitter_bindings.clone(),
        number_closure_bindings: state.number_closure_bindings.clone(),
        string_function_bindings: state.string_function_bindings.clone(),
        dynamic_object_bindings: state.dynamic_object_bindings.clone(),
        object_bindings: state.object_bindings.clone(),
        class_instance_bindings: state.class_instance_bindings.clone(),
        current_receiver: state.current_receiver.clone(),
        current_fields: state.current_fields.clone(),
        functions: state.functions.clone(),
        classes: state.classes.clone(),
        namespace_functions: state.namespace_functions.clone(),
        builtin_bindings: state.builtin_bindings.clone(),
        dynamic_import_namespaces: state.dynamic_import_namespaces.clone(),
        entry_source_path: state.entry_source_path.clone(),
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

fn render_try_finally_stmt(
    body: &[JsStmt],
    catch_body: &[JsStmt],
    finally_body: &[JsStmt],
    state: &mut AotState,
) -> Option<String> {
    if !catch_body.is_empty() {
        return None;
    }
    let mut rendered = Vec::new();
    if !body.is_empty() {
        rendered.push(render_stmt_sequence(body, state)?);
    }
    if !finally_body.is_empty() {
        rendered.push(render_stmt_sequence(finally_body, state)?);
    }
    Some(rendered.join("\n"))
}

fn render_stmt_sequence(stmts: &[JsStmt], state: &mut AotState) -> Option<String> {
    stmts
        .iter()
        .map(|stmt| render_stmt(stmt, state))
        .collect::<Option<Vec<_>>>()
        .map(|stmts| stmts.join("\n"))
}

fn render_for_init(stmt: &JsStmt, state: &mut AotState) -> Option<String> {
    match stmt {
        JsStmt::VarDecl {
            name,
            init: Some(init),
        } => {
            let value = render_numeric_expr(init, state)?;
            state.bind_slot(name, sanitize_go_identifier(name), AotSlotKind::Number);
            Some(format!(
                "{} := float64({value})",
                sanitize_go_identifier(name)
            ))
        }
        JsStmt::Expr {
            expr: JsExpr::Assign { op, left, right },
        } if op == "=" => {
            let JsExpr::Ident { name } = left.as_ref() else {
                return None;
            };
            if !state.bindings.contains(name) {
                return None;
            }
            let value = render_numeric_expr(right, state)?;
            Some(format!(
                "{} = float64({value})",
                go_binding_ref(name, state)
            ))
        }
        _ => None,
    }
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
            if state.numeric_bindings.contains(name) {
                let right = render_numeric_expr(right, state)?;
                return Some(format!("{} {} {right}", go_binding_ref(name, state), op));
            }
            if !is_any_binding(name, state) {
                return None;
            }
            let right = render_numeric_expr(right, state)?;
            let value = go_binding_ref(name, state);
            let operator = op.trim_end_matches('=');
            Some(format!(
                "{value} = tsgodownToFloat64({value}) {operator} {right}"
            ))
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
        number_array_bindings: state.number_array_bindings.clone(),
        string_array_bindings: state.string_array_bindings.clone(),
        any_array_bindings: state.any_array_bindings.clone(),
        map_bindings: state.map_bindings.clone(),
        url_bindings: state.url_bindings.clone(),
        event_emitter_bindings: state.event_emitter_bindings.clone(),
        number_closure_bindings: state.number_closure_bindings.clone(),
        string_function_bindings: state.string_function_bindings.clone(),
        dynamic_object_bindings: state.dynamic_object_bindings.clone(),
        object_bindings: state.object_bindings.clone(),
        class_instance_bindings: state.class_instance_bindings.clone(),
        current_receiver: state.current_receiver.clone(),
        current_fields: state.current_fields.clone(),
        functions: state.functions.clone(),
        classes: state.classes.clone(),
        namespace_functions: state.namespace_functions.clone(),
        builtin_bindings: state.builtin_bindings.clone(),
        dynamic_import_namespaces: state.dynamic_import_namespaces.clone(),
        entry_source_path: state.entry_source_path.clone(),
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
        number_array_bindings: state.number_array_bindings.clone(),
        string_array_bindings: state.string_array_bindings.clone(),
        any_array_bindings: state.any_array_bindings.clone(),
        map_bindings: state.map_bindings.clone(),
        url_bindings: state.url_bindings.clone(),
        event_emitter_bindings: state.event_emitter_bindings.clone(),
        number_closure_bindings: state.number_closure_bindings.clone(),
        string_function_bindings: state.string_function_bindings.clone(),
        dynamic_object_bindings: state.dynamic_object_bindings.clone(),
        object_bindings: state.object_bindings.clone(),
        class_instance_bindings: state.class_instance_bindings.clone(),
        current_receiver: state.current_receiver.clone(),
        current_fields: state.current_fields.clone(),
        functions: state.functions.clone(),
        classes: state.classes.clone(),
        namespace_functions: state.namespace_functions.clone(),
        builtin_bindings: state.builtin_bindings.clone(),
        dynamic_import_namespaces: state.dynamic_import_namespaces.clone(),
        entry_source_path: state.entry_source_path.clone(),
    };
    mark_number_array_locals(stmts, &mut block_state);
    mark_any_array_locals(stmts, &mut block_state);
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
    for (getter_name, getter) in &class.getters {
        out.push(render_class_method_decl(class, getter_name, getter)?);
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
    mark_string_accumulator_params(body, &param_index, &mut kinds, &mut BTreeSet::new());
    kinds
}

fn mark_string_accumulator_params(
    body: &[JsStmt],
    param_index: &BTreeMap<String, usize>,
    kinds: &mut [AotSlotKind],
    string_locals: &mut BTreeSet<String>,
) {
    for stmt in body {
        match stmt {
            JsStmt::VarDecl {
                name,
                init: Some(expr),
            } if is_string_literal_like(expr) => {
                string_locals.insert(name.clone());
            }
            JsStmt::Expr {
                expr: JsExpr::Assign { op, left, right },
            } if op == "+=" => {
                if matches!(left.as_ref(), JsExpr::Ident { name } if string_locals.contains(name)) {
                    mark_ident_param_kind(right, param_index, kinds, AotSlotKind::String);
                }
            }
            JsStmt::If {
                consequent,
                alternate,
                ..
            } => {
                let mut consequent_locals = string_locals.clone();
                mark_string_accumulator_params(
                    consequent,
                    param_index,
                    kinds,
                    &mut consequent_locals,
                );
                let mut alternate_locals = string_locals.clone();
                mark_string_accumulator_params(
                    alternate,
                    param_index,
                    kinds,
                    &mut alternate_locals,
                );
            }
            JsStmt::For { body, .. }
            | JsStmt::While { body, .. }
            | JsStmt::DoWhile { body, .. } => {
                let mut scoped = string_locals.clone();
                mark_string_accumulator_params(body, param_index, kinds, &mut scoped);
            }
            _ => {}
        }
    }
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
            if matches!(
                string_method_name(callee),
                Some("includes" | "indexOf" | "replace")
            ) {
                if let Some(arg) = args.first() {
                    mark_ident_param_kind(arg, param_index, kinds, AotSlotKind::String);
                }
            }
            if string_method_name(callee) == Some("replace") {
                if let Some(arg) = args.get(1) {
                    mark_ident_param_kind(arg, param_index, kinds, AotSlotKind::String);
                }
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
        JsExpr::Call { callee, args, .. } if is_map_method_call_shape(callee, args) => {
            if matches!(
                map_method_name(callee),
                Some("set" | "get" | "has" | "delete")
            ) {
                if let Some(arg) = args.first() {
                    mark_ident_param_kind(arg, param_index, kinds, AotSlotKind::String);
                }
            }
            if matches!(map_method_name(callee), Some("set")) {
                if let Some(arg) = args.get(1) {
                    mark_ident_param_kind(arg, param_index, kinds, AotSlotKind::Any);
                }
            }
            infer_expr_param_kinds(callee, param_index, kinds);
            for arg in args {
                infer_expr_param_kinds(arg, param_index, kinds);
            }
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
                mark_ident_param_kind(&prop.value, param_index, kinds, AotSlotKind::Any);
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
    if let Some(rendered) = render_number_closure_function_decl(function) {
        return Some(rendered);
    }
    let mut function_state = clone_aot_state(state);
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

fn render_number_closure_function_decl(function: &AotFunction) -> Option<String> {
    let value = render_number_closure_function_value(function)?;
    let function_name = &function.go_name;
    Some(format!(
        "func {function_name}{}",
        value.trim_start_matches("func")
    ))
}

fn render_number_closure_function_value(function: &AotFunction) -> Option<String> {
    if function.params.len() != 1 {
        return None;
    }
    let [JsStmt::VarDecl {
        name: captured,
        init: Some(init),
    }, JsStmt::Return {
        value:
            Some(JsExpr::Function {
                params,
                rest_param: None,
                r#async: false,
                generator: false,
                body,
                ..
            }),
    }] = function.body.as_slice()
    else {
        return None;
    };
    if params.len() != 1 {
        return None;
    }
    let seed_param = function.params.first()?;
    let init = match init {
        JsExpr::Ident { name } if name == seed_param => sanitize_go_identifier(seed_param),
        JsExpr::Value {
            value: JsValue::Number { value },
        } => number_literal(value)?,
        _ => return None,
    };
    let delta_param = params.first()?;
    let [JsStmt::Expr {
        expr: JsExpr::Assign { op, left, right },
    }, JsStmt::Return {
        value: Some(returned),
    }] = body.as_slice()
    else {
        return None;
    };
    if op != "+="
        || !matches!(left.as_ref(), JsExpr::Ident { name } if name == captured)
        || !matches!(right.as_ref(), JsExpr::Ident { name } if name == delta_param)
        || !matches!(returned, JsExpr::Ident { name } if name == captured)
    {
        return None;
    }
    let seed = sanitize_go_identifier(seed_param);
    let captured = sanitize_go_identifier(captured);
    let delta = sanitize_go_identifier(delta_param);
    Some(format!(
        "func({seed} float64) any {{\n\t{captured} := {init}\n\treturn func({delta} float64) any {{\n\t\t{captured} += {delta}\n\t\treturn {captured}\n\t}}\n}}"
    ))
}

fn function_returns_number_closure(function: &AotFunction) -> bool {
    render_number_closure_function_decl(function).is_some()
}

fn aot_function_state(function: &AotFunction, state: &AotState) -> AotState {
    let mut function_state = clone_aot_state(state);
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

fn aot_function_from_stmt(stmt: &JsStmt, name: &str) -> Option<AotFunction> {
    let parts = function_parts(stmt)?;
    Some(AotFunction {
        params: parts.params.clone(),
        param_kinds: infer_function_param_kinds(parts.params, parts.body),
        rest_param: parts.rest_param.clone(),
        r#async: *parts.r#async,
        generator: *parts.generator,
        body: parts.body.clone(),
        go_name: sanitize_go_identifier(name),
    })
}

fn render_local_function_decl(function: &AotFunction, state: &AotState) -> Option<String> {
    if let Some(value) = render_number_closure_function_value(function) {
        return Some(format!("{} := {value}", function.go_name));
    }
    if function.rest_param.is_some() || function.r#async || function.generator {
        return None;
    }
    let mut function_state = clone_aot_state(state);
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
    let function_type = format!(
        "func({}) any",
        function
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
            .join(", ")
    );
    Some(format!(
        "var {} {function_type}\n{} = func({rendered_params}) any {{\n{}\n}}",
        function.go_name,
        function.go_name,
        indent_lines(&function_body)
    ))
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
    mark_number_array_locals(body, &mut function_state);
    mark_any_array_locals(body, &mut function_state);
    mark_dynamic_object_locals(body, &mut function_state);
    body.iter()
        .map(|stmt| render_function_stmt(stmt, &mut function_state))
        .collect::<Option<Vec<_>>>()
        .map(|stmts| stmts.join("\n"))
}

fn mark_dynamic_object_locals(stmts: &[JsStmt], state: &mut AotState) {
    let mut write_candidates = BTreeSet::new();
    collect_dynamic_object_candidates(stmts, &mut write_candidates);
    let mut candidates = write_candidates.clone();
    let mut read_candidates = BTreeSet::new();
    collect_dynamic_object_member_read_candidates(stmts, &mut read_candidates);
    let mut call_initialized = BTreeSet::new();
    collect_call_initialized_bindings(stmts, &mut call_initialized);
    candidates.extend(
        read_candidates
            .into_iter()
            .filter(|name| call_initialized.contains(name)),
    );
    for name in candidates {
        if state.builtin_bindings.contains(&name)
            || state.functions.contains_key(&name)
            || state
                .namespace_functions
                .keys()
                .any(|(namespace, _)| namespace == &name)
            || state.object_bindings.contains_key(&name) && !write_candidates.contains(&name)
        {
            continue;
        }
        if state.object_bindings.contains_key(&name) {
            state.object_bindings.remove(&name);
        }
        state.bindings.insert(name.clone());
        state
            .binding_refs
            .insert(name.clone(), sanitize_go_identifier(&name));
        state.dynamic_object_bindings.insert(name);
    }
}

fn collect_dynamic_object_candidates(stmts: &[JsStmt], candidates: &mut BTreeSet<String>) {
    for stmt in stmts {
        match stmt {
            JsStmt::Expr { expr } | JsStmt::Return { value: Some(expr) } => {
                collect_dynamic_object_candidates_expr(expr, candidates);
            }
            JsStmt::VarDecl {
                init: Some(expr), ..
            } => collect_dynamic_object_candidates_expr(expr, candidates),
            JsStmt::FunctionDecl { body, .. } => {
                collect_dynamic_object_candidates(body, candidates);
            }
            JsStmt::If {
                test,
                consequent,
                alternate,
            } => {
                collect_dynamic_object_candidates_expr(test, candidates);
                collect_dynamic_object_candidates(consequent, candidates);
                collect_dynamic_object_candidates(alternate, candidates);
            }
            JsStmt::For {
                init,
                test,
                update,
                body,
            } => {
                collect_dynamic_object_candidates(init, candidates);
                if let Some(test) = test {
                    collect_dynamic_object_candidates_expr(test, candidates);
                }
                if let Some(update) = update {
                    collect_dynamic_object_candidates_expr(update, candidates);
                }
                collect_dynamic_object_candidates(body, candidates);
            }
            JsStmt::While { test, body } | JsStmt::DoWhile { test, body } => {
                collect_dynamic_object_candidates_expr(test, candidates);
                collect_dynamic_object_candidates(body, candidates);
            }
            JsStmt::Try {
                body,
                catch_body,
                finally_body,
                ..
            } => {
                collect_dynamic_object_candidates(body, candidates);
                collect_dynamic_object_candidates(catch_body, candidates);
                collect_dynamic_object_candidates(finally_body, candidates);
            }
            _ => {}
        }
    }
}

fn collect_dynamic_object_candidates_expr(expr: &JsExpr, candidates: &mut BTreeSet<String>) {
    match expr {
        JsExpr::Assign { op, left, right } => {
            if op == "=" {
                if let Some(name) = dynamic_object_assignment_target(left) {
                    candidates.insert(name.to_string());
                }
            }
            collect_dynamic_object_candidates_expr(left, candidates);
            collect_dynamic_object_candidates_expr(right, candidates);
        }
        JsExpr::Call { callee, args, .. } => {
            collect_dynamic_object_candidates_expr(callee, candidates);
            for arg in args {
                collect_dynamic_object_candidates_expr(arg, candidates);
            }
        }
        JsExpr::Array { items } => {
            for item in items {
                collect_dynamic_object_candidates_expr(item, candidates);
            }
        }
        JsExpr::ArraySpread { items } => {
            for item in items {
                collect_dynamic_object_candidates_expr(&item.value, candidates);
            }
        }
        JsExpr::Object { props } => {
            for prop in props {
                if let Some(key_expr) = &prop.key_expr {
                    collect_dynamic_object_candidates_expr(key_expr, candidates);
                }
                collect_dynamic_object_candidates_expr(&prop.value, candidates);
            }
        }
        JsExpr::Function { body, .. } => {
            collect_dynamic_object_candidates(body, candidates);
        }
        JsExpr::Member {
            object,
            property,
            property_expr,
            ..
        } => {
            let _ = property;
            collect_dynamic_object_candidates_expr(object, candidates);
            if let Some(property_expr) = property_expr {
                collect_dynamic_object_candidates_expr(property_expr, candidates);
            }
        }
        JsExpr::Binary { left, right, .. } => {
            collect_dynamic_object_candidates_expr(left, candidates);
            collect_dynamic_object_candidates_expr(right, candidates);
        }
        JsExpr::Unary { arg, .. }
        | JsExpr::Await { arg }
        | JsExpr::Update { arg, .. }
        | JsExpr::Spread { arg }
        | JsExpr::ObjectRest { object: arg, .. } => {
            collect_dynamic_object_candidates_expr(arg, candidates);
        }
        JsExpr::Conditional {
            test,
            consequent,
            alternate,
        } => {
            collect_dynamic_object_candidates_expr(test, candidates);
            collect_dynamic_object_candidates_expr(consequent, candidates);
            collect_dynamic_object_candidates_expr(alternate, candidates);
        }
        JsExpr::Template { exprs, .. } | JsExpr::Sequence { exprs } => {
            for expr in exprs {
                collect_dynamic_object_candidates_expr(expr, candidates);
            }
        }
        _ => {}
    }
}

fn collect_dynamic_object_member_read_candidates(
    stmts: &[JsStmt],
    candidates: &mut BTreeSet<String>,
) {
    for stmt in stmts {
        match stmt {
            JsStmt::Expr { expr } | JsStmt::Return { value: Some(expr) } => {
                collect_dynamic_object_member_read_candidates_expr(expr, candidates);
            }
            JsStmt::VarDecl {
                init: Some(expr), ..
            } => collect_dynamic_object_member_read_candidates_expr(expr, candidates),
            JsStmt::FunctionDecl { body, .. } => {
                collect_dynamic_object_member_read_candidates(body, candidates);
            }
            JsStmt::If {
                test,
                consequent,
                alternate,
            } => {
                collect_dynamic_object_member_read_candidates_expr(test, candidates);
                collect_dynamic_object_member_read_candidates(consequent, candidates);
                collect_dynamic_object_member_read_candidates(alternate, candidates);
            }
            JsStmt::For {
                init,
                test,
                update,
                body,
            } => {
                collect_dynamic_object_member_read_candidates(init, candidates);
                if let Some(test) = test {
                    collect_dynamic_object_member_read_candidates_expr(test, candidates);
                }
                if let Some(update) = update {
                    collect_dynamic_object_member_read_candidates_expr(update, candidates);
                }
                collect_dynamic_object_member_read_candidates(body, candidates);
            }
            JsStmt::While { test, body } | JsStmt::DoWhile { test, body } => {
                collect_dynamic_object_member_read_candidates_expr(test, candidates);
                collect_dynamic_object_member_read_candidates(body, candidates);
            }
            JsStmt::Try {
                body,
                catch_body,
                finally_body,
                ..
            } => {
                collect_dynamic_object_member_read_candidates(body, candidates);
                collect_dynamic_object_member_read_candidates(catch_body, candidates);
                collect_dynamic_object_member_read_candidates(finally_body, candidates);
            }
            _ => {}
        }
    }
}

fn collect_dynamic_object_member_read_candidates_expr(
    expr: &JsExpr,
    candidates: &mut BTreeSet<String>,
) {
    match expr {
        JsExpr::Member {
            object,
            property,
            property_expr,
            ..
        } => {
            if property_expr.is_none() && !is_collection_member_property(property) {
                if let JsExpr::Ident { name } = object.as_ref() {
                    if !is_builtin_object_name(name) {
                        candidates.insert(name.clone());
                    }
                }
            }
            collect_dynamic_object_member_read_candidates_expr(object, candidates);
            if let Some(property_expr) = property_expr {
                collect_dynamic_object_member_read_candidates_expr(property_expr, candidates);
            }
        }
        JsExpr::Assign { left, right, .. } | JsExpr::Binary { left, right, .. } => {
            collect_dynamic_object_member_read_candidates_expr(left, candidates);
            collect_dynamic_object_member_read_candidates_expr(right, candidates);
        }
        JsExpr::Call { callee, args, .. } => {
            collect_dynamic_object_member_read_candidates_expr(callee, candidates);
            for arg in args {
                collect_dynamic_object_member_read_candidates_expr(arg, candidates);
            }
        }
        JsExpr::Array { items } => {
            for item in items {
                collect_dynamic_object_member_read_candidates_expr(item, candidates);
            }
        }
        JsExpr::ArraySpread { items } => {
            for item in items {
                collect_dynamic_object_member_read_candidates_expr(&item.value, candidates);
            }
        }
        JsExpr::Object { props } => {
            for prop in props {
                if let Some(key_expr) = &prop.key_expr {
                    collect_dynamic_object_member_read_candidates_expr(key_expr, candidates);
                }
                collect_dynamic_object_member_read_candidates_expr(&prop.value, candidates);
            }
        }
        JsExpr::Function { body, .. } => {
            collect_dynamic_object_member_read_candidates(body, candidates);
        }
        JsExpr::Unary { arg, .. }
        | JsExpr::Await { arg }
        | JsExpr::Update { arg, .. }
        | JsExpr::Spread { arg }
        | JsExpr::ObjectRest { object: arg, .. } => {
            collect_dynamic_object_member_read_candidates_expr(arg, candidates);
        }
        JsExpr::Conditional {
            test,
            consequent,
            alternate,
        } => {
            collect_dynamic_object_member_read_candidates_expr(test, candidates);
            collect_dynamic_object_member_read_candidates_expr(consequent, candidates);
            collect_dynamic_object_member_read_candidates_expr(alternate, candidates);
        }
        JsExpr::Template { exprs, .. } | JsExpr::Sequence { exprs } => {
            for expr in exprs {
                collect_dynamic_object_member_read_candidates_expr(expr, candidates);
            }
        }
        _ => {}
    }
}

fn collect_call_initialized_bindings(stmts: &[JsStmt], candidates: &mut BTreeSet<String>) {
    for stmt in stmts {
        match stmt {
            JsStmt::VarDecl {
                name,
                init: Some(expr @ JsExpr::Call { .. }),
            } if !is_require_call(expr) => {
                candidates.insert(name.clone());
            }
            JsStmt::FunctionDecl { body, .. } => {
                collect_call_initialized_bindings(body, candidates)
            }
            JsStmt::If {
                consequent,
                alternate,
                ..
            } => {
                collect_call_initialized_bindings(consequent, candidates);
                collect_call_initialized_bindings(alternate, candidates);
            }
            JsStmt::For { init, body, .. } => {
                collect_call_initialized_bindings(init, candidates);
                collect_call_initialized_bindings(body, candidates);
            }
            JsStmt::While { body, .. } | JsStmt::DoWhile { body, .. } => {
                collect_call_initialized_bindings(body, candidates);
            }
            JsStmt::Try {
                body,
                catch_body,
                finally_body,
                ..
            } => {
                collect_call_initialized_bindings(body, candidates);
                collect_call_initialized_bindings(catch_body, candidates);
                collect_call_initialized_bindings(finally_body, candidates);
            }
            _ => {}
        }
    }
}

fn dynamic_object_assignment_target(expr: &JsExpr) -> Option<&str> {
    let JsExpr::Member {
        object,
        property_expr: None,
        optional: false,
        ..
    } = expr
    else {
        return None;
    };
    let JsExpr::Ident { name } = object.as_ref() else {
        return None;
    };
    Some(name)
}

fn is_collection_member_property(property: &str) -> bool {
    matches!(
        property,
        "set" | "get" | "has" | "delete" | "size" | "length"
    )
}

fn is_builtin_object_name(name: &str) -> bool {
    matches!(
        name,
        "Array"
            | "Boolean"
            | "JSON"
            | "Map"
            | "Math"
            | "Number"
            | "Object"
            | "Promise"
            | "String"
            | "console"
            | "process"
    )
}

fn mark_number_array_locals(stmts: &[JsStmt], state: &mut AotState) {
    let mut candidates = BTreeSet::new();
    collect_number_array_candidates(stmts, &mut candidates);
    for name in candidates {
        state.bind_slot(
            &name,
            sanitize_go_identifier(&name),
            AotSlotKind::NumberArray,
        );
    }
}

fn mark_any_array_locals(stmts: &[JsStmt], state: &mut AotState) {
    let mut candidates = BTreeSet::new();
    collect_any_array_candidates(stmts, &mut candidates);
    for name in candidates {
        if state.number_array_bindings.contains(&name)
            || state.string_array_bindings.contains(&name)
        {
            continue;
        }
        state.bindings.insert(name.clone());
        state
            .binding_refs
            .insert(name.clone(), sanitize_go_identifier(&name));
        state.any_array_bindings.insert(name);
    }
}

fn collect_any_array_candidates(stmts: &[JsStmt], candidates: &mut BTreeSet<String>) {
    for stmt in stmts {
        match stmt {
            JsStmt::Expr { expr } | JsStmt::Return { value: Some(expr) } => {
                collect_any_array_candidates_expr(expr, candidates);
            }
            JsStmt::VarDecl {
                init: Some(expr), ..
            } => collect_any_array_candidates_expr(expr, candidates),
            JsStmt::If {
                test,
                consequent,
                alternate,
            } => {
                collect_any_array_candidates_expr(test, candidates);
                collect_any_array_candidates(consequent, candidates);
                collect_any_array_candidates(alternate, candidates);
            }
            JsStmt::For {
                init,
                test,
                update,
                body,
            } => {
                collect_any_array_candidates(init, candidates);
                if let Some(test) = test {
                    collect_any_array_candidates_expr(test, candidates);
                }
                if let Some(update) = update {
                    collect_any_array_candidates_expr(update, candidates);
                }
                collect_any_array_candidates(body, candidates);
            }
            JsStmt::While { test, body } | JsStmt::DoWhile { test, body } => {
                collect_any_array_candidates_expr(test, candidates);
                collect_any_array_candidates(body, candidates);
            }
            JsStmt::Try {
                body,
                catch_body,
                finally_body,
                ..
            } => {
                collect_any_array_candidates(body, candidates);
                collect_any_array_candidates(catch_body, candidates);
                collect_any_array_candidates(finally_body, candidates);
            }
            _ => {}
        }
    }
}

fn collect_any_array_candidates_expr(expr: &JsExpr, candidates: &mut BTreeSet<String>) {
    match expr {
        JsExpr::Call { callee, args, .. } => {
            if let Some(name) = any_array_candidate_push_target(callee, args) {
                candidates.insert(name.to_string());
            }
            collect_any_array_candidates_expr(callee, candidates);
            for arg in args {
                collect_any_array_candidates_expr(arg, candidates);
            }
        }
        JsExpr::Assign { left, right, .. } | JsExpr::Binary { left, right, .. } => {
            collect_any_array_candidates_expr(left, candidates);
            collect_any_array_candidates_expr(right, candidates);
        }
        JsExpr::Member { object, .. }
        | JsExpr::Unary { arg: object, .. }
        | JsExpr::Update { arg: object, .. }
        | JsExpr::Await { arg: object }
        | JsExpr::Spread { arg: object }
        | JsExpr::ObjectRest { object, .. } => {
            collect_any_array_candidates_expr(object, candidates);
        }
        JsExpr::Array { items } => {
            for item in items {
                collect_any_array_candidates_expr(item, candidates);
            }
        }
        JsExpr::ArraySpread { items } => {
            for item in items {
                collect_any_array_candidates_expr(&item.value, candidates);
            }
        }
        JsExpr::Object { props } => {
            for prop in props {
                collect_any_array_candidates_expr(&prop.value, candidates);
            }
        }
        JsExpr::Function { body, .. } => {
            collect_any_array_candidates(body, candidates);
        }
        JsExpr::Conditional {
            test,
            consequent,
            alternate,
        } => {
            collect_any_array_candidates_expr(test, candidates);
            collect_any_array_candidates_expr(consequent, candidates);
            collect_any_array_candidates_expr(alternate, candidates);
        }
        JsExpr::Template { exprs, .. } | JsExpr::Sequence { exprs } => {
            for expr in exprs {
                collect_any_array_candidates_expr(expr, candidates);
            }
        }
        _ => {}
    }
}

fn collect_number_array_candidates(stmts: &[JsStmt], candidates: &mut BTreeSet<String>) {
    for stmt in stmts {
        match stmt {
            JsStmt::VarDecl {
                name,
                init: Some(JsExpr::Array { items }),
            } if !items.is_empty() && items.iter().all(is_numeric_array_candidate_item) => {
                candidates.insert(name.clone());
            }
            JsStmt::Expr {
                expr: JsExpr::Assign { op, left, right },
            } if op == "="
                && matches!(right.as_ref(), JsExpr::Array { items } if !items.is_empty() && items.iter().all(is_numeric_array_candidate_item)) =>
            {
                if let JsExpr::Ident { name } = left.as_ref() {
                    candidates.insert(name.clone());
                }
            }
            JsStmt::Expr {
                expr: JsExpr::Call { callee, args, .. },
            } if number_array_candidate_push_target(callee, args).is_some() => {
                if let Some(name) = number_array_candidate_push_target(callee, args) {
                    candidates.insert(name.to_string());
                }
            }
            JsStmt::Expr { expr } | JsStmt::Return { value: Some(expr) } => {
                collect_number_array_candidates_expr(expr, candidates);
            }
            JsStmt::If {
                test,
                consequent,
                alternate,
            } => {
                collect_number_array_candidates_expr(test, candidates);
                collect_number_array_candidates(consequent, candidates);
                collect_number_array_candidates(alternate, candidates);
            }
            JsStmt::For {
                init,
                test,
                update,
                body,
            } => {
                collect_number_array_candidates(init, candidates);
                if let Some(test) = test {
                    collect_number_array_candidates_expr(test, candidates);
                }
                if let Some(update) = update {
                    collect_number_array_candidates_expr(update, candidates);
                }
                collect_number_array_candidates(body, candidates);
            }
            JsStmt::While { test, body } | JsStmt::DoWhile { test, body } => {
                collect_number_array_candidates_expr(test, candidates);
                collect_number_array_candidates(body, candidates);
            }
            _ => {}
        }
    }
}

fn collect_number_array_candidates_expr(expr: &JsExpr, candidates: &mut BTreeSet<String>) {
    match expr {
        JsExpr::Call { callee, args, .. } => {
            if let Some(name) = number_array_candidate_push_target(callee, args) {
                candidates.insert(name.to_string());
            }
            if let Some(name) = number_array_candidate_pop_target(callee, args) {
                candidates.insert(name.to_string());
            }
            collect_number_array_candidates_expr(callee, candidates);
            for arg in args {
                collect_number_array_candidates_expr(arg, candidates);
            }
        }
        JsExpr::Assign { left, right, .. } | JsExpr::Binary { left, right, .. } => {
            collect_number_array_candidates_expr(left, candidates);
            collect_number_array_candidates_expr(right, candidates);
        }
        JsExpr::Member { object, .. }
        | JsExpr::Unary { arg: object, .. }
        | JsExpr::Update { arg: object, .. }
        | JsExpr::Await { arg: object }
        | JsExpr::Spread { arg: object }
        | JsExpr::ObjectRest { object, .. } => {
            collect_number_array_candidates_expr(object, candidates);
        }
        JsExpr::Array { items } => {
            for item in items {
                collect_number_array_candidates_expr(item, candidates);
            }
        }
        JsExpr::Object { props } => {
            for prop in props {
                collect_number_array_candidates_expr(&prop.value, candidates);
            }
        }
        JsExpr::Conditional {
            test,
            consequent,
            alternate,
        } => {
            collect_number_array_candidates_expr(test, candidates);
            collect_number_array_candidates_expr(consequent, candidates);
            collect_number_array_candidates_expr(alternate, candidates);
        }
        JsExpr::Template { exprs, .. } | JsExpr::Sequence { exprs } => {
            for expr in exprs {
                collect_number_array_candidates_expr(expr, candidates);
            }
        }
        _ => {}
    }
}

fn number_array_candidate_push_target<'a>(callee: &'a JsExpr, args: &[JsExpr]) -> Option<&'a str> {
    if args.len() != 1 || !is_numeric_array_candidate_item(args.first()?) {
        return None;
    }
    number_array_candidate_method_target(callee, "push")
}

fn any_array_candidate_push_target<'a>(callee: &'a JsExpr, args: &[JsExpr]) -> Option<&'a str> {
    if args.len() != 1 || is_numeric_array_candidate_item(args.first()?) {
        return None;
    }
    number_array_candidate_method_target(callee, "push")
}

fn number_array_candidate_pop_target<'a>(callee: &'a JsExpr, args: &[JsExpr]) -> Option<&'a str> {
    if !args.is_empty() {
        return None;
    }
    number_array_candidate_method_target(callee, "pop")
}

fn number_array_candidate_method_target<'a>(callee: &'a JsExpr, method: &str) -> Option<&'a str> {
    let JsExpr::Member {
        object,
        property,
        property_expr: None,
        optional: false,
    } = callee
    else {
        return None;
    };
    if property != method {
        return None;
    }
    let JsExpr::Ident { name } = object.as_ref() else {
        return None;
    };
    Some(name.as_str())
}

fn is_numeric_array_candidate_item(expr: &JsExpr) -> bool {
    matches!(
        expr,
        JsExpr::Value {
            value: JsValue::Number { .. },
        } | JsExpr::Binary { .. }
            | JsExpr::Call { .. }
            | JsExpr::Member { .. }
    )
}

fn function_returns_string_array(function: &AotFunction, state: &AotState) -> bool {
    let mut function_state = aot_function_state(function, state);
    let mut saw_return = false;
    collect_string_array_returns(&function.body, &mut function_state, &mut saw_return)
        .unwrap_or(false)
        && saw_return
}

fn function_returns_number_array(function: &AotFunction, state: &AotState) -> bool {
    let mut function_state = aot_function_state(function, state);
    mark_number_array_locals(&function.body, &mut function_state);
    let mut saw_return = false;
    collect_number_array_returns(&function.body, &mut function_state, &mut saw_return)
        .unwrap_or(false)
        && saw_return
}

fn collect_number_array_returns(
    body: &[JsStmt],
    state: &mut AotState,
    saw_return: &mut bool,
) -> Option<bool> {
    for stmt in body {
        match stmt {
            JsStmt::Return { value: Some(value) } => {
                *saw_return = true;
                if render_number_array_expr(value, state).is_none() {
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
                mark_number_array_locals(consequent, &mut consequent_state);
                if !collect_number_array_returns(consequent, &mut consequent_state, saw_return)? {
                    return Some(false);
                }
                let mut alternate_state = clone_aot_state(state);
                mark_number_array_locals(alternate, &mut alternate_state);
                if !collect_number_array_returns(alternate, &mut alternate_state, saw_return)? {
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
        JsStmt::Try {
            body,
            catch_body,
            finally_body,
            ..
        } => render_try_finally_stmt(body, catch_body, finally_body, state),
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
        expr if is_process_argv_ref(expr) => Some("true".to_string()),
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
        JsExpr::Ident { name } if state.number_array_bindings.contains(name) => {
            let value = go_binding_ref(name, state);
            Some(format!("(len({value}) > 0)"))
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
        JsExpr::Member {
            object,
            property,
            property_expr: None,
            optional: false,
        } if render_dynamic_object_member_expr(object, property, state).is_some() => {
            let value = render_dynamic_object_member_expr(object, property, state)?;
            Some(format!("tsgodownToBool({value})"))
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
        JsExpr::Call { callee, args, .. } if is_global_is_nan_call(callee, args) => {
            let value = render_expr(args.first()?, state)?;
            Some(format!("tsgodownIsNaN({value})"))
        }
        JsExpr::Call { callee, args, .. } if is_map_has_call(callee, args, state) => {
            render_map_bool_call(callee, args, state)
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
        JsExpr::Await { arg } => render_await_promise_then_stmt(arg, state),
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
        JsExpr::Assign { op, left, .. }
            if op == "="
                && is_cjs_export_target(left)
                && !is_shadowed_cjs_export_target(left, state) =>
        {
            Some(String::new())
        }
        JsExpr::Assign { op, left, right }
            if render_string_array_assignment_stmt(op, left, right, state).is_some() =>
        {
            render_string_array_assignment_stmt(op, left, right, state)
        }
        JsExpr::Assign { op, left, right }
            if render_number_array_assignment_stmt(op, left, right, state).is_some() =>
        {
            render_number_array_assignment_stmt(op, left, right, state)
        }
        JsExpr::Assign { op, left, right }
            if render_dynamic_object_assignment_stmt(op, left, right, state).is_some() =>
        {
            render_dynamic_object_assignment_stmt(op, left, right, state)
        }
        JsExpr::Call { callee, args, .. }
            if render_any_array_call_stmt(callee, args, state).is_some() =>
        {
            render_any_array_call_stmt(callee, args, state)
        }
        JsExpr::Call { callee, args, .. }
            if render_number_array_call_stmt(callee, args, state).is_some() =>
        {
            render_number_array_call_stmt(callee, args, state)
        }
        JsExpr::Call { callee, args, .. }
            if render_node_fs_write_file_sync_stmt(callee, args, state).is_some() =>
        {
            render_node_fs_write_file_sync_stmt(callee, args, state)
        }
        JsExpr::Call { callee, args, .. }
            if render_node_fs_rm_sync_stmt(callee, args, state).is_some() =>
        {
            render_node_fs_rm_sync_stmt(callee, args, state)
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

fn render_await_promise_then_stmt(expr: &JsExpr, state: &mut AotState) -> Option<String> {
    let JsExpr::Call { callee, args, .. } = expr else {
        return None;
    };
    if args.len() != 1 {
        return None;
    }
    let JsExpr::Member {
        object,
        property,
        property_expr: None,
        optional: false,
    } = callee.as_ref()
    else {
        return None;
    };
    if property != "then" || !is_promise_resolve_call(object) {
        return None;
    }
    let JsExpr::Function {
        params,
        rest_param: None,
        r#async: false,
        generator: false,
        body,
        ..
    } = args.first()?
    else {
        return None;
    };
    if !params.is_empty() {
        return None;
    }
    render_promise_then_body(body, state)
}

fn is_promise_resolve_call(expr: &JsExpr) -> bool {
    matches!(
        expr,
        JsExpr::Call { callee, .. }
            if matches!(
                callee.as_ref(),
                JsExpr::Member {
                    object,
                    property,
                    property_expr: None,
                    optional: false,
                } if matches!(object.as_ref(), JsExpr::Ident { name } if name == "Promise")
                    && property == "resolve"
            )
    )
}

fn render_promise_then_body(body: &[JsStmt], state: &mut AotState) -> Option<String> {
    body.iter()
        .map(|stmt| match stmt {
            JsStmt::Return { value: Some(expr) } => render_expr_stmt(expr, state),
            JsStmt::Return { value: None } => Some(String::new()),
            stmt => render_stmt(stmt, state),
        })
        .collect::<Option<Vec<_>>>()
        .map(|stmts| stmts.join("\n"))
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
        if state.number_array_bindings.contains(name) && op == "=" {
            if matches!(right, JsExpr::Array { items } if items.is_empty()) {
                return Some(format!("{} = []float64{{}}", go_binding_ref(name, state)));
            }
            let right = render_number_array_expr(right, state)?;
            return Some(format!("{} = {right}", go_binding_ref(name, state)));
        }
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

fn render_number_array_assignment_stmt(
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
    if !state.number_array_bindings.contains(name) {
        return None;
    }
    let target = go_binding_ref(name, state);
    let index = if let Some(property_expr) = property_expr {
        render_numeric_expr(property_expr, state)?
    } else {
        number_literal(property)?
    };
    let value = render_numeric_expr(right, state)?;
    match op {
        "=" => Some(format!(
            "{target} = tsgodownNumberArraySet({target}, {index}, {value})"
        )),
        _ => None,
    }
}

fn render_dynamic_object_assignment_stmt(
    op: &str,
    left: &JsExpr,
    right: &JsExpr,
    state: &AotState,
) -> Option<String> {
    if op != "=" {
        return None;
    }
    let JsExpr::Member {
        object,
        property,
        property_expr: None,
        optional: false,
    } = left
    else {
        return None;
    };
    let object = render_dynamic_object_source_expr(object, state)?;
    let value = render_json_value_expr(right, state)?;
    Some(format!(
        "tsgodownObjectSetProp({object}, {}, {value})",
        go_string_literal(property)
    ))
}

fn render_number_array_call_stmt(
    callee: &JsExpr,
    args: &[JsExpr],
    state: &AotState,
) -> Option<String> {
    let (name, method) = number_array_method_target(callee, state)?;
    if method != "push" || args.len() != 1 {
        return None;
    }
    let value = render_numeric_expr(args.first()?, state)?;
    let target = go_binding_ref(name, state);
    Some(format!("{target} = append({target}, {value})"))
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
        JsExpr::Ident { name } if state.functions.contains_key(name) => {
            render_function_reference_expr(name, state)
        }
        JsExpr::Ident { name } if state.bindings.contains(name) => {
            Some(go_binding_ref(name, state))
        }
        JsExpr::Array { .. } => {
            render_string_array_expr(expr, state).or_else(|| render_json_value_expr(expr, state))
        }
        JsExpr::Object { .. } => render_object_map_expr(expr, state),
        JsExpr::Binary { op, .. } if op == "+" => render_string_expr(expr, state).or_else(|| {
            let JsExpr::Binary { left, right, .. } = expr else {
                return None;
            };
            let left = render_numeric_expr(left, state)?;
            let right = render_numeric_expr(right, state)?;
            Some(format!("({left} + {right})"))
        }),
        JsExpr::Binary { op, left, right } if is_bitwise_binary_op(op) => {
            let left = render_numeric_expr(left, state)?;
            let right = render_numeric_expr(right, state)?;
            let expr = match op.as_str() {
                ">>" => format!("(int({left}) >> int({right}))"),
                "<<" => format!("(int({left}) << int({right}))"),
                "&" => format!("(int({left}) & int({right}))"),
                "|" => format!("(int({left}) | int({right}))"),
                "^" => format!("(int({left}) ^ int({right}))"),
                _ => return None,
            };
            Some(format!("float64({expr})"))
        }
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
            .or_else(|| render_map_call_expr(callee, args, state))
            .or_else(|| render_call_expr(callee, args, state)),
        JsExpr::Await { arg } => {
            render_async_iife_expr(arg, state).or_else(|| render_expr(arg, state))
        }
        JsExpr::New { .. } => render_url_new_expr(expr, state)
            .or_else(|| render_event_emitter_new_expr(expr, state))
            .or_else(|| render_new_class_expr(expr, state).map(|(_, value)| value)),
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
        } => render_class_getter_member_expr(object, property, state)
            .or_else(|| render_static_member_expr(object, property, state))
            .or_else(|| {
                if property == "length" || property.parse::<usize>().is_ok() {
                    render_numeric_expr(expr, state).or_else(|| render_string_expr(expr, state))
                } else {
                    None
                }
            })
            .or_else(|| render_dynamic_object_member_expr(object, property, state))
            .or_else(|| render_string_expr(expr, state))
            .or_else(|| render_numeric_expr(expr, state))
            .or_else(|| render_bool_expr(expr, state)),
        JsExpr::Template { quasis, exprs } => render_template_string_expr(quasis, exprs, state),
        _ => None,
    }
}

fn render_async_iife_expr(expr: &JsExpr, state: &AotState) -> Option<String> {
    match expr {
        JsExpr::Await { arg } => render_async_iife_expr(arg, state),
        JsExpr::Call { callee, args, .. } if args.is_empty() => {
            let JsExpr::Function {
                params,
                body,
                r#async: true,
                generator: false,
                rest_param: None,
                ..
            } = callee.as_ref()
            else {
                return None;
            };
            if !params.is_empty() {
                return None;
            }
            render_iife_block_expr(body, state)
        }
        _ => None,
    }
}

fn render_iife_block_expr(body: &[JsStmt], state: &AotState) -> Option<String> {
    let mut block_state = clone_aot_state(state);
    mark_number_array_locals(body, &mut block_state);
    mark_any_array_locals(body, &mut block_state);
    mark_dynamic_object_locals(body, &mut block_state);
    let mut rendered = Vec::new();
    for stmt in body {
        match stmt {
            JsStmt::FunctionDecl { name, .. } => {
                let function = aot_function_from_stmt(stmt, name)?;
                block_state.functions.insert(name.clone(), function);
                let function = block_state.functions.get(name)?;
                let rendered_function = render_local_function_decl(function, &block_state)?;
                rendered.push(rendered_function);
            }
            JsStmt::Break { .. } | JsStmt::Continue { .. } => {
                return None;
            }
            JsStmt::ClassDecl { name, .. } if block_state.classes.contains_key(name) => {}
            JsStmt::ClassDecl { .. } => {
                return None;
            }
            JsStmt::Return { value: Some(value) } => {
                let value = render_json_value_expr(value, &block_state)
                    .or_else(|| render_expr(value, &block_state))?;
                rendered.push(format!("return {value}"));
            }
            JsStmt::Return { value: None } => {
                rendered.push("return nil".to_string());
            }
            other => {
                rendered.push(render_stmt(other, &mut block_state)?);
            }
        }
    }
    if !matches!(body.last(), Some(JsStmt::Return { .. })) {
        rendered.push("return nil".to_string());
    }
    Some(format!(
        "func() any {{\n{}\n}}()",
        indent_lines(&rendered.join("\n"))
    ))
}

fn render_numeric_expr(expr: &JsExpr, state: &AotState) -> Option<String> {
    match expr {
        JsExpr::Value {
            value: JsValue::Number { value },
        } => number_literal(value),
        JsExpr::Ident { name } if state.numeric_bindings.contains(name) => {
            Some(go_binding_ref(name, state))
        }
        JsExpr::Ident { name } if is_any_binding(name, state) => {
            let value = go_binding_ref(name, state);
            Some(format!("tsgodownToFloat64({value})"))
        }
        expr if render_number_array_index_expr(expr, state).is_some() => {
            render_number_array_index_expr(expr, state)
        }
        JsExpr::Member {
            object,
            property,
            property_expr,
            optional: false,
        } if is_map_size_member(object, property, property_expr.as_deref(), state) => {
            let object = render_map_expr(object, state)?;
            Some(format!("tsgodownMapSize({object})"))
        }
        JsExpr::Member {
            object,
            property,
            property_expr,
            optional: false,
        } if is_length_member_property(property, property_expr.as_deref())
            && render_number_array_expr(object, state).is_some() =>
        {
            let object = render_number_array_expr(object, state)?;
            Some(format!("float64(len({object}))"))
        }
        JsExpr::Member {
            object,
            property,
            property_expr,
            optional: false,
        } if is_length_member_property(property, property_expr.as_deref())
            && render_string_array_expr(object, state).is_some() =>
        {
            let object = render_string_array_expr(object, state)?;
            Some(format!("float64(len({object}))"))
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
        } if static_member_kind(object, property, state) == Some(AotSlotKind::Any) => {
            let value = render_static_member_expr(object, property, state)?;
            Some(format!("tsgodownToFloat64({value})"))
        }
        JsExpr::Member {
            object,
            property,
            property_expr,
            optional: false,
        } if is_length_member_property(property, property_expr.as_deref())
            && render_bytes_expr(object, state).is_some() =>
        {
            let object = render_bytes_expr(object, state)?;
            Some(format!("float64(len({object}))"))
        }
        JsExpr::Member {
            object,
            property,
            property_expr,
            optional: false,
        } if is_length_member_property(property, property_expr.as_deref()) => {
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
        JsExpr::Member {
            object,
            property,
            property_expr: None,
            optional: false,
        } if render_dynamic_object_member_expr(object, property, state).is_some() => {
            let value = render_dynamic_object_member_expr(object, property, state)?;
            Some(format!("tsgodownToFloat64({value})"))
        }
        JsExpr::Unary { op, arg } if op == "-" => {
            let arg = render_numeric_expr(arg, state)?;
            Some(format!("(-{arg})"))
        }
        JsExpr::Binary { op, left, right } if is_bitwise_binary_op(op) => {
            let left = render_numeric_expr(left, state)?;
            let right = render_numeric_expr(right, state)?;
            let expr = match op.as_str() {
                ">>" => format!("(int({left}) >> int({right}))"),
                "<<" => format!("(int({left}) << int({right}))"),
                "&" => format!("(int({left}) & int({right}))"),
                "|" => format!("(int({left}) | int({right}))"),
                "^" => format!("(int({left}) ^ int({right}))"),
                _ => return None,
            };
            Some(format!("float64({expr})"))
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
        JsExpr::Call { callee, args, .. } if is_parse_int_call(callee, args) => {
            let value = render_expr(args.first()?, state)?;
            let radix = args
                .get(1)
                .map(|arg| render_numeric_expr(arg, state))
                .unwrap_or_else(|| Some("10".to_string()))?;
            Some(format!("tsgodownParseInt({value}, {radix})"))
        }
        JsExpr::Call { callee, args, .. } if is_number_array_pop_call(callee, args, state) => {
            let (name, _) = number_array_method_target(callee, state)?;
            let target = go_binding_ref(name, state);
            Some(format!("tsgodownNumberArrayPop(&{target})"))
        }
        JsExpr::Call { callee, args, .. } if is_string_from_char_code_call(callee, args) => {
            let args = args
                .iter()
                .map(|arg| render_numeric_expr(arg, state))
                .collect::<Option<Vec<_>>>()?;
            Some(format!(
                "tsgodownToFloat64(tsgodownStringFromCharCode({}))",
                args.join(", ")
            ))
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
        JsExpr::Member {
            object,
            property,
            property_expr: None,
            optional: false,
        } if render_dynamic_object_member_expr(object, property, state).is_some() => {
            let value = render_dynamic_object_member_expr(object, property, state)?;
            Some(format!("tsgodownToString({value})"))
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
        JsExpr::Call { callee, args, .. } if is_string_from_char_code_call(callee, args) => {
            let args = args
                .iter()
                .map(|arg| render_numeric_expr(arg, state))
                .collect::<Option<Vec<_>>>()?;
            Some(format!("tsgodownStringFromCharCode({})", args.join(", ")))
        }
        JsExpr::Call { callee, args, .. } if is_json_stringify(callee) => {
            let value = render_json_value_expr(args.first()?, state)?;
            if let Some(space) = args.get(2).and_then(render_json_stringify_space_expr) {
                return Some(format!("tsgodownJSONStringifyIndent({value}, {space})"));
            }
            Some(format!("tsgodownJSONStringify({value})"))
        }
        JsExpr::Call { callee, args, .. } if is_string_array_join_call(callee, args) => {
            render_string_array_join_call(callee, args, state)
        }
        JsExpr::Call { callee, args, .. } => render_node_path_string_call(callee, args, state)
            .or_else(|| render_node_os_homedir_call(callee, args, state))
            .or_else(|| render_node_os_tmpdir_call(callee, args, state))
            .or_else(|| render_node_fs_mkdtemp_sync_call(callee, args, state))
            .or_else(|| render_node_fs_read_file_sync_call(callee, args, state))
            .or_else(|| render_buffer_to_string_call(callee, args, state))
            .or_else(|| render_url_search_params_get_call(callee, args, state))
            .or_else(|| render_crypto_sha256_hex_call(callee, args, state))
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
        JsExpr::Member {
            object,
            property,
            property_expr: None,
            optional: false,
        } if render_url_string_member_expr(object, property, state).is_some() => {
            render_url_string_member_expr(object, property, state)
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
        expr if is_process_argv_ref(expr) => Some("true".to_string()),
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
        expr if is_process_cwd_call_expr(expr) => Some("\"string\"".to_string()),
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
    let go_op = go_comparison_op(op)?;
    if let Some(value) = render_nullish_comparison_expr(go_op, left, right, state) {
        return Some(value);
    }
    if let Some(value) = render_any_equality_expr(go_op, left, right, state) {
        return Some(value);
    }
    if let Some(value) = render_mixed_number_string_equality_expr(op, left, right, state) {
        return Some(value);
    }
    if let (Some(left), Some(right)) = (
        render_numeric_expr(left, state),
        render_numeric_expr(right, state),
    ) {
        return Some(format!("({left} {go_op} {right})"));
    }
    if let (Some(left), Some(right)) = (
        render_string_expr(left, state),
        render_string_expr(right, state),
    ) {
        return Some(format!("({left} {go_op} {right})"));
    }
    if matches!(go_op, "==" | "!=") {
        if let (Some(left), Some(right)) = (
            render_bool_expr(left, state),
            render_bool_expr(right, state),
        ) {
            return Some(format!("({left} {go_op} {right})"));
        }
    }
    None
}

fn render_mixed_number_string_equality_expr(
    op: &str,
    left: &JsExpr,
    right: &JsExpr,
    state: &AotState,
) -> Option<String> {
    if !matches!(op, "==" | "!=" | "===" | "!==") {
        return None;
    }
    let mixed = render_numeric_expr(left, state)
        .zip(render_string_expr(right, state))
        .or_else(|| render_numeric_expr(right, state).zip(render_string_expr(left, state)))?;
    if matches!(op, "===" | "!==") {
        return Some((op == "!==").to_string());
    }
    let comparison = format!("({} == tsgodownToFloat64({}))", mixed.0, mixed.1);
    if op == "!=" {
        Some(format!("(!{comparison})"))
    } else {
        Some(comparison)
    }
}

fn render_any_equality_expr(
    op: &str,
    left: &JsExpr,
    right: &JsExpr,
    state: &AotState,
) -> Option<String> {
    if !matches!(op, "==" | "!=")
        || !expr_uses_any_binding(left, state) && !expr_uses_any_binding(right, state)
    {
        return None;
    }
    let left = render_expr(left, state)?;
    let right = render_expr(right, state)?;
    let comparison = format!("tsgodownStrictEqual({left}, {right})");
    if op == "!=" {
        Some(format!("(!{comparison})"))
    } else {
        Some(comparison)
    }
}

fn expr_uses_any_binding(expr: &JsExpr, state: &AotState) -> bool {
    matches!(expr, JsExpr::Ident { name } if is_any_binding(name, state))
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

fn render_object_map_expr(expr: &JsExpr, state: &AotState) -> Option<String> {
    match expr {
        JsExpr::Object { props } => {
            if props.iter().any(|prop| prop.spread) {
                let mut statements = vec!["out := map[string]any{}".to_string()];
                for prop in props {
                    if prop.key_expr.is_some() {
                        return None;
                    }
                    if prop.spread {
                        let value = render_object_map_expr(&prop.value, state)?;
                        statements.push(format!(
                            "for key, value := range tsgodownObjectFromAny({value}) {{ out[key] = value }}"
                        ));
                        continue;
                    }
                    let value = render_json_value_expr(&prop.value, state)?;
                    statements.push(format!("out[{}] = {value}", go_string_literal(&prop.key)));
                }
                statements.push("return out".to_string());
                return Some(format!(
                    "func() map[string]any {{ {} }}()",
                    statements.join("; ")
                ));
            }
            let mut fields = Vec::new();
            for prop in props {
                let key = match &prop.key_expr {
                    Some(key_expr) => render_string_expr(key_expr, state)?,
                    None => go_string_literal(&prop.key),
                };
                let value = render_json_value_expr(&prop.value, state)?;
                fields.push(format!("{key}: {value}"));
            }
            Some(format!("map[string]any{{{}}}", fields.join(", ")))
        }
        JsExpr::Ident { name } if state.dynamic_object_bindings.contains(name) => {
            Some(go_binding_ref(name, state))
        }
        JsExpr::Ident { name } if state.bindings.contains(name) => {
            let value = go_binding_ref(name, state);
            Some(format!("tsgodownObjectFromAny({value})"))
        }
        JsExpr::Call { callee, args, .. }
            if render_map_call_expr(callee, args, state).is_some() =>
        {
            let value = render_map_call_expr(callee, args, state)?;
            Some(format!("tsgodownObjectFromAny({value})"))
        }
        JsExpr::Conditional {
            test,
            consequent,
            alternate,
        } => {
            let test = render_bool_expr(test, state)?;
            let consequent = render_object_map_expr(consequent, state)?;
            let alternate = render_object_map_expr(alternate, state)?;
            Some(format!(
                "func() map[string]any {{ if {test} {{ return {consequent} }}; return {alternate} }}()"
            ))
        }
        _ => None,
    }
}

fn render_dynamic_object_member_expr(
    object: &JsExpr,
    property: &str,
    state: &AotState,
) -> Option<String> {
    let object = render_dynamic_object_source_expr(object, state)?;
    Some(format!(
        "tsgodownObjectProp({object}, {})",
        go_string_literal(property)
    ))
}

fn render_dynamic_object_init_expr(expr: &JsExpr, state: &AotState) -> Option<String> {
    render_object_map_expr(expr, state).or_else(|| {
        if let JsExpr::Call { callee, args, .. } = expr {
            let value = render_call_expr(callee, args, state)?;
            Some(format!("tsgodownObjectFromAny({value})"))
        } else {
            None
        }
    })
}

fn render_dynamic_object_source_expr(object: &JsExpr, state: &AotState) -> Option<String> {
    match object {
        JsExpr::Ident { name } if state.dynamic_object_bindings.contains(name) => {
            Some(go_binding_ref(name, state))
        }
        JsExpr::Member {
            object,
            property,
            property_expr: None,
            optional: false,
        } => render_dynamic_object_member_expr(object, property, state),
        JsExpr::Call { callee, args, .. } => {
            let value = render_call_expr(callee, args, state)?;
            Some(format!("tsgodownObjectFromAny({value})"))
        }
        _ => None,
    }
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
    if let Some(value) = render_number_array_expr(expr, state) {
        return Some((AotSlotKind::NumberArray, value, "[]float64"));
    }
    if let Some(value) = render_string_array_expr(expr, state) {
        return Some((AotSlotKind::StringArray, value, "[]string"));
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

fn render_class_getter_member_expr(
    object: &JsExpr,
    property: &str,
    state: &AotState,
) -> Option<String> {
    match object {
        JsExpr::Ident { name } => {
            let class_name = state.class_instance_bindings.get(name)?;
            let class = state.classes.get(class_name)?;
            class.getters.get(property)?;
            Some(format!(
                "{}.{}()",
                go_binding_ref(name, state),
                sanitize_go_identifier(property)
            ))
        }
        JsExpr::New { .. } => {
            let (class_name, value) = render_new_class_expr(object, state)?;
            let class = state.classes.get(&class_name)?;
            class.getters.get(property)?;
            Some(format!("{}.{}()", value, sanitize_go_identifier(property)))
        }
        _ => None,
    }
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

fn render_number_array_expr(expr: &JsExpr, state: &AotState) -> Option<String> {
    match expr {
        JsExpr::Ident { name } if state.number_array_bindings.contains(name) => {
            Some(go_binding_ref(name, state))
        }
        JsExpr::Array { items } => {
            if items.is_empty() {
                return None;
            }
            let items = items
                .iter()
                .map(|item| render_numeric_expr(item, state))
                .collect::<Option<Vec<_>>>()?;
            Some(format!("[]float64{{{}}}", items.join(", ")))
        }
        JsExpr::Call { callee, args, .. } => {
            let JsExpr::Ident { name } = callee.as_ref() else {
                return None;
            };
            let function = state.functions.get(name)?;
            if !function_returns_number_array(function, state) {
                return None;
            }
            let call = render_call_expr(callee, args, state)?;
            Some(format!("({call}).([]float64)"))
        }
        _ => None,
    }
}

fn render_any_array_expr(expr: &JsExpr, state: &AotState) -> Option<String> {
    match expr {
        JsExpr::Ident { name } if state.any_array_bindings.contains(name) => {
            Some(go_binding_ref(name, state))
        }
        JsExpr::Array { items } => {
            let items = items
                .iter()
                .map(|item| render_json_value_expr(item, state))
                .collect::<Option<Vec<_>>>()?;
            Some(format!("[]any{{{}}}", items.join(", ")))
        }
        _ => None,
    }
}

fn render_number_closure_expr(expr: &JsExpr, state: &AotState) -> Option<String> {
    match expr {
        JsExpr::Ident { name } if state.number_closure_bindings.contains(name) => {
            Some(go_binding_ref(name, state))
        }
        JsExpr::Call { callee, args, .. } => {
            let JsExpr::Ident { name } = callee.as_ref() else {
                return None;
            };
            let function = state.functions.get(name)?;
            if !function_returns_number_closure(function) {
                return None;
            }
            let call = render_call_expr(callee, args, state)?;
            Some(format!("({call}).(func(float64) any)"))
        }
        _ => None,
    }
}

fn render_js_map_expr(expr: &JsExpr) -> Option<String> {
    matches!(
        expr,
        JsExpr::New { callee, args }
            if args.is_empty()
                && matches!(callee.as_ref(), JsExpr::Ident { name } if name == "Map")
    )
    .then(|| "tsgodownNewMap()".to_string())
}

fn is_new_url_expr(callee: &JsExpr, args: &[JsExpr]) -> bool {
    args.len() == 2 && matches!(callee, JsExpr::Ident { name } if name == "URL")
}

fn render_url_new_expr(expr: &JsExpr, state: &AotState) -> Option<String> {
    let JsExpr::New { callee, args } = expr else {
        return None;
    };
    if !is_new_url_expr(callee, args) || !state.builtin_bindings.contains("URL") {
        return None;
    }
    let input = render_string_expr(args.first()?, state)?;
    let base = render_string_expr(args.get(1)?, state)?;
    Some(format!("tsgodownNewURL({input}, {base})"))
}

fn render_url_string_member_expr(
    object: &JsExpr,
    property: &str,
    state: &AotState,
) -> Option<String> {
    let JsExpr::Ident { name } = object else {
        return None;
    };
    if !state.url_bindings.contains(name) || property != "pathname" {
        return None;
    }
    Some(format!(
        "tsgodownURLPathname({})",
        go_binding_ref(name, state)
    ))
}

fn is_url_search_params_get_call(callee: &JsExpr, args: &[JsExpr]) -> bool {
    args.len() == 1
        && matches!(
            callee,
            JsExpr::Member {
                object,
                property,
                property_expr: None,
                optional: false,
            } if property == "get" && matches!(
                object.as_ref(),
                JsExpr::Member {
                    property,
                    property_expr: None,
                    optional: false,
                    ..
                } if property == "searchParams"
            )
        )
}

fn render_url_search_params_get_call(
    callee: &JsExpr,
    args: &[JsExpr],
    state: &AotState,
) -> Option<String> {
    if !is_url_search_params_get_call(callee, args) {
        return None;
    }
    let JsExpr::Member { object, .. } = callee else {
        return None;
    };
    let JsExpr::Member { object, .. } = object.as_ref() else {
        return None;
    };
    let JsExpr::Ident { name } = object.as_ref() else {
        return None;
    };
    if !state.url_bindings.contains(name) {
        return None;
    }
    let key = render_string_expr(args.first()?, state)?;
    Some(format!(
        "tsgodownURLSearchParam({}, {key})",
        go_binding_ref(name, state)
    ))
}

fn render_event_emitter_new_expr(expr: &JsExpr, state: &AotState) -> Option<String> {
    let JsExpr::New { callee, args } = expr else {
        return None;
    };
    if !args.is_empty()
        || !state.builtin_bindings.contains("EventEmitter")
        || !matches!(callee.as_ref(), JsExpr::Ident { name } if name == "EventEmitter")
    {
        return None;
    }
    Some("tsgodownNewEventEmitter()".to_string())
}

fn render_event_emitter_call_expr(
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
    if !state.event_emitter_bindings.contains(name) {
        return None;
    }
    let target = go_binding_ref(name, state);
    match property.as_str() {
        "on" if args.len() == 2 => {
            let event = render_string_expr(args.first()?, state)?;
            let listener = render_inline_function_value_expr(args.get(1)?, state)?;
            Some(format!(
                "tsgodownEventEmitterOn({target}, {event}, {listener})"
            ))
        }
        "emit" if args.len() == 2 => {
            let event = render_string_expr(args.first()?, state)?;
            let payload = render_json_value_expr(args.get(1)?, state)?;
            Some(format!(
                "tsgodownEventEmitterEmit({target}, {event}, {payload})"
            ))
        }
        _ => None,
    }
}

fn render_map_expr(expr: &JsExpr, state: &AotState) -> Option<String> {
    match expr {
        JsExpr::Ident { name } if state.map_bindings.contains(name) => {
            Some(go_binding_ref(name, state))
        }
        JsExpr::Call { callee, args, .. } if is_map_set_call(callee, args, state) => {
            render_map_call_expr(callee, args, state)
        }
        _ => None,
    }
}

fn is_map_size_member(
    object: &JsExpr,
    property: &str,
    property_expr: Option<&JsExpr>,
    state: &AotState,
) -> bool {
    property_expr.is_none() && property == "size" && render_map_expr(object, state).is_some()
}

fn render_map_call_expr(callee: &JsExpr, args: &[JsExpr], state: &AotState) -> Option<String> {
    let JsExpr::Member {
        object,
        property,
        property_expr: None,
        optional: false,
    } = callee
    else {
        return None;
    };
    let target = render_map_expr(object, state)?;
    match property.as_str() {
        "set" if args.len() == 2 => {
            let key = render_string_expr(args.first()?, state)?;
            let value = render_json_value_expr(args.get(1)?, state)?;
            Some(format!("tsgodownMapSet({target}, {key}, {value})"))
        }
        "get" if args.len() == 1 => {
            let key = render_string_expr(args.first()?, state)?;
            Some(format!("tsgodownMapGet({target}, {key})"))
        }
        "has" if args.len() == 1 => {
            let key = render_string_expr(args.first()?, state)?;
            Some(format!("tsgodownMapHas({target}, {key})"))
        }
        "delete" if args.len() == 1 => {
            let key = render_string_expr(args.first()?, state)?;
            Some(format!("tsgodownMapDelete({target}, {key})"))
        }
        _ => None,
    }
}

fn map_method_name(callee: &JsExpr) -> Option<&str> {
    let JsExpr::Member {
        property,
        property_expr: None,
        optional: false,
        ..
    } = callee
    else {
        return None;
    };
    match property.as_str() {
        "set" | "get" | "has" | "delete" => Some(property.as_str()),
        _ => None,
    }
}

fn is_map_method_call_shape(callee: &JsExpr, args: &[JsExpr]) -> bool {
    matches!(
        (map_method_name(callee), args.len()),
        (Some("set"), 2) | (Some("get" | "has" | "delete"), 1)
    )
}

fn is_map_set_call(callee: &JsExpr, args: &[JsExpr], state: &AotState) -> bool {
    let JsExpr::Member {
        object,
        property,
        property_expr: None,
        optional: false,
    } = callee
    else {
        return false;
    };
    args.len() == 2 && property == "set" && render_map_expr(object, state).is_some()
}

fn is_map_has_call(callee: &JsExpr, args: &[JsExpr], state: &AotState) -> bool {
    let JsExpr::Member {
        object,
        property,
        property_expr: None,
        optional: false,
    } = callee
    else {
        return false;
    };
    args.len() == 1 && property == "has" && render_map_expr(object, state).is_some()
}

fn render_map_bool_call(callee: &JsExpr, args: &[JsExpr], state: &AotState) -> Option<String> {
    if !is_map_has_call(callee, args, state) {
        return None;
    }
    let JsExpr::Member { object, .. } = callee else {
        return None;
    };
    let target = render_map_expr(object, state)?;
    let key = render_string_expr(args.first()?, state)?;
    Some(format!("tsgodownMapHas({target}, {key})"))
}

fn render_string_array_expr(expr: &JsExpr, state: &AotState) -> Option<String> {
    match expr {
        expr if is_process_argv_ref(expr) => render_process_argv_expr(state),
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
        JsExpr::Call { callee, args, .. } if is_object_keys_call(callee, args) => {
            render_object_keys_call(callee, args, state)
        }
        JsExpr::Call { callee, args, .. } if is_array_map_to_string_call(callee, args) => {
            render_array_map_to_string_call(callee, args, state)
        }
        JsExpr::Call { callee, args, .. } if is_string_match_call(callee, args) => {
            render_string_match_call(callee, args, state)
        }
        JsExpr::Call { callee, args, .. } if is_string_array_slice_call(callee, args) => {
            render_string_array_slice_call(callee, args, state)
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

fn render_any_array_call_stmt(
    callee: &JsExpr,
    args: &[JsExpr],
    state: &mut AotState,
) -> Option<String> {
    if args.len() != 1 {
        return None;
    }
    let JsExpr::Member {
        object,
        property,
        property_expr: None,
        optional: false,
    } = callee
    else {
        return None;
    };
    if property != "push" {
        return None;
    }
    let JsExpr::Ident { name } = object.as_ref() else {
        return None;
    };
    if !state.any_array_bindings.contains(name) {
        return None;
    }
    let target = go_binding_ref(name, state);
    let value = render_json_value_expr(args.first()?, state)?;
    Some(format!("{target} = append({target}, {value})"))
}

fn render_any_array_push_call_expr(
    callee: &JsExpr,
    args: &[JsExpr],
    state: &AotState,
) -> Option<String> {
    if args.len() != 1 {
        return None;
    }
    let JsExpr::Member {
        object,
        property,
        property_expr: None,
        optional: false,
    } = callee
    else {
        return None;
    };
    if property != "push" {
        return None;
    }
    let JsExpr::Ident { name } = object.as_ref() else {
        return None;
    };
    if !state.any_array_bindings.contains(name) {
        return None;
    }
    let target = go_binding_ref(name, state);
    let value = render_json_value_expr(args.first()?, state)?;
    Some(format!(
        "func() any {{ {target} = append({target}, {value}); return float64(len({target})) }}()"
    ))
}

fn render_number_array_index_expr(expr: &JsExpr, state: &AotState) -> Option<String> {
    let JsExpr::Member {
        object,
        property,
        property_expr,
        optional: false,
    } = expr
    else {
        return None;
    };
    let values = render_number_array_expr(object, state)?;
    if is_length_member_property(property, property_expr.as_deref()) {
        return None;
    }
    let index = if let Some(property_expr) = property_expr {
        render_numeric_expr(property_expr, state)?
    } else {
        number_literal(property)?
    };
    Some(format!("tsgodownNumberArrayAt({values}, {index})"))
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
    if is_length_member_property(property, property_expr.as_deref()) {
        return None;
    }
    let values = render_string_array_expr(object, state)?;
    let index = if let Some(property_expr) = property_expr {
        render_numeric_expr(property_expr, state)?
    } else {
        number_literal(property)?
    };
    Some(format!("tsgodownStringArrayAt({values}, {index})"))
}

fn render_string_array_length_expr(expr: &JsExpr, state: &AotState) -> Option<String> {
    let JsExpr::Member {
        object,
        property,
        property_expr,
        optional: false,
    } = expr
    else {
        return None;
    };
    if !is_length_member_property(property, property_expr.as_deref()) {
        return None;
    }
    let values = render_string_array_expr(object, state)?;
    Some(format!("float64(len({values}))"))
}

fn is_object_keys_call(callee: &JsExpr, args: &[JsExpr]) -> bool {
    args.len() == 1
        && matches!(
            callee,
            JsExpr::Member {
                object,
                property,
                property_expr: None,
                optional: false,
            } if matches!(object.as_ref(), JsExpr::Ident { name } if name == "Object")
                && property == "keys"
        )
}

fn render_object_keys_call(callee: &JsExpr, args: &[JsExpr], state: &AotState) -> Option<String> {
    if !is_object_keys_call(callee, args) {
        return None;
    }
    let JsExpr::Object { props } = args.first()? else {
        return None;
    };
    if props.iter().any(|prop| prop.spread) {
        return None;
    }
    let keys = props
        .iter()
        .map(|prop| match &prop.key_expr {
            Some(key_expr) => render_string_expr(key_expr, state),
            None => Some(go_string_literal(&prop.key)),
        })
        .collect::<Option<Vec<_>>>()?;
    Some(format!(
        "tsgodownObjectKeys([]string{{{}}})",
        keys.join(", ")
    ))
}

fn is_length_member_property(property: &str, property_expr: Option<&JsExpr>) -> bool {
    property == "length"
        || matches!(property_expr, Some(JsExpr::Ident { name }) if name == "length")
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
        "toLowerCase" | "toUpperCase" | "trim" | "includes" | "indexOf" | "charAt"
        | "charCodeAt" | "replace" | "slice" => Some(property.as_str()),
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
        "includes" if args.len() == 1 => {}
        "indexOf" if matches!(args.len(), 1 | 2) => {}
        "charAt" if args.len() == 1 => {}
        "charCodeAt" if args.len() == 1 => {}
        "replace" if args.len() == 2 => {}
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
    if let Some(object) = string_method_receiver(callee, "replace", args, state) {
        let object = render_string_expr(object, state)?;
        let replacement = render_string_expr(args.get(1)?, state)?;
        match args.first()? {
            JsExpr::Value {
                value: JsValue::String { value },
            } => {
                return Some(format!(
                    "strings.Replace({object}, {}, {replacement}, 1)",
                    go_string_literal(value)
                ));
            }
            JsExpr::Value {
                value: JsValue::RegExp { .. },
            } => {
                let (pattern, global) = render_supported_regexp_replace_pattern(args.first()?)?;
                return Some(format!(
                    "tsgodownRegexpReplace({object}, {}, {replacement}, {global})",
                    go_string_literal(&pattern)
                ));
            }
            _ => {}
        }
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

fn number_array_method_target<'a>(
    callee: &'a JsExpr,
    state: &AotState,
) -> Option<(&'a str, &'a str)> {
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
    if !state.number_array_bindings.contains(name) {
        return None;
    }
    if !matches!(property.as_str(), "push" | "pop") {
        return None;
    }
    Some((name.as_str(), property.as_str()))
}

fn is_number_array_pop_call(callee: &JsExpr, args: &[JsExpr], state: &AotState) -> bool {
    args.is_empty() && matches!(number_array_method_target(callee, state), Some((_, "pop")))
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

fn is_string_array_slice_call(callee: &JsExpr, args: &[JsExpr]) -> bool {
    matches!(args.len(), 1 | 2)
        && matches!(
            callee,
            JsExpr::Member {
                property,
                property_expr: None,
                optional: false,
                ..
            } if property == "slice"
        )
}

fn render_string_array_slice_call(
    callee: &JsExpr,
    args: &[JsExpr],
    state: &AotState,
) -> Option<String> {
    if !is_string_array_slice_call(callee, args) {
        return None;
    }
    let JsExpr::Member { object, .. } = callee else {
        return None;
    };
    let values = render_string_array_expr(object, state)?;
    let start = render_numeric_expr(args.first()?, state)?;
    let end = match args.get(1) {
        Some(expr) => Some(render_numeric_expr(expr, state)?),
        None => None,
    };
    match end {
        Some(end) => Some(format!(
            "tsgodownStringArraySlice({values}, {start}, {end})"
        )),
        None => Some(format!("tsgodownStringArraySlice({values}, {start})")),
    }
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

fn is_string_replace_call(callee: &JsExpr, args: &[JsExpr]) -> bool {
    string_method_name(callee) == Some("replace")
        && args.len() == 2
        && matches!(
            args.first(),
            Some(
                JsExpr::Value {
                    value: JsValue::String { .. },
                } | JsExpr::Value {
                    value: JsValue::RegExp { .. },
                }
            )
        )
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

fn render_supported_regexp_replace_pattern(expr: &JsExpr) -> Option<(String, bool)> {
    let JsExpr::Value {
        value: JsValue::RegExp { pattern, flags },
    } = expr
    else {
        return None;
    };
    if pattern.contains("\\1") || pattern.contains("\\2") || pattern.contains("\\3") {
        return None;
    }
    if !flags.chars().all(|flag| matches!(flag, 'g' | 'i' | 'm')) {
        return None;
    }
    let mut prefix = String::new();
    if flags.contains('i') {
        prefix.push('i');
    }
    if flags.contains('m') {
        prefix.push('m');
    }
    let pattern = if prefix.is_empty() {
        pattern.clone()
    } else {
        format!("(?{prefix}){pattern}")
    };
    Some((pattern, flags.contains('g')))
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
            } if is_node_path_call_receiver(object)
            && matches!(
                property.as_str(),
                "basename" | "dirname" | "join" | "normalize" | "relative" | "resolve"
            )
    )
}

fn is_node_path_call_receiver(object: &JsExpr) -> bool {
    matches!(object, JsExpr::Ident { name } if name == "path")
        || matches!(
            object,
            JsExpr::Member {
                object,
                property,
                property_expr: None,
                optional: false,
            } if matches!(object.as_ref(), JsExpr::Ident { name } if name == "path")
                && matches!(property.as_str(), "posix" | "win32")
        )
}

fn is_node_path_posix_call(callee: &JsExpr) -> bool {
    matches!(
        callee,
        JsExpr::Member {
            object,
            property_expr: None,
            optional: false,
            ..
        } if matches!(
            object.as_ref(),
            JsExpr::Member {
                object,
                property,
                property_expr: None,
                optional: false,
            } if matches!(object.as_ref(), JsExpr::Ident { name } if name == "path")
                && property == "posix"
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
    if !state.builtin_bindings.contains("path") || !is_node_path_call_receiver(object) {
        return None;
    }
    let rendered_args = args
        .iter()
        .map(|arg| render_string_expr(arg, state))
        .collect::<Option<Vec<_>>>()?;
    if is_node_path_posix_call(callee) {
        return match property.as_str() {
            "join" => Some(format!("path.Join({})", rendered_args.join(", "))),
            "normalize" if rendered_args.len() == 1 => {
                Some(format!("path.Clean({})", rendered_args[0]))
            }
            "dirname" if rendered_args.len() == 1 => {
                Some(format!("path.Dir({})", rendered_args[0]))
            }
            "basename" if rendered_args.len() == 1 => {
                Some(format!("path.Base({})", rendered_args[0]))
            }
            _ => None,
        };
    }
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

fn is_node_os_tmpdir_call(callee: &JsExpr, args: &[JsExpr]) -> bool {
    args.is_empty()
        && matches!(
            callee,
            JsExpr::Member {
                object,
                property,
                property_expr: None,
                optional: false,
            } if matches!(object.as_ref(), JsExpr::Ident { name } if name == "os")
                && property == "tmpdir"
        )
}

fn render_node_os_tmpdir_call(
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
    if state.builtin_bindings.contains(name) && is_node_os_tmpdir_call(callee, args) {
        return Some("tsgodownOsTmpdir()".to_string());
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

fn is_node_fs_mkdtemp_sync_call(callee: &JsExpr, args: &[JsExpr]) -> bool {
    args.len() == 1
        && matches!(
            callee,
            JsExpr::Member {
                object,
                property,
                property_expr: None,
                optional: false,
            } if matches!(object.as_ref(), JsExpr::Ident { name } if name == "fs")
                && property == "mkdtempSync"
        )
}

fn is_node_fs_write_file_sync_call(callee: &JsExpr, args: &[JsExpr]) -> bool {
    matches!(args.len(), 2 | 3)
        && matches!(
            callee,
            JsExpr::Member {
                object,
                property,
                property_expr: None,
                optional: false,
            } if matches!(object.as_ref(), JsExpr::Ident { name } if name == "fs")
                && property == "writeFileSync"
        )
}

fn is_node_fs_read_file_sync_call(callee: &JsExpr, args: &[JsExpr]) -> bool {
    matches!(args.len(), 1 | 2)
        && matches!(
            callee,
            JsExpr::Member {
                object,
                property,
                property_expr: None,
                optional: false,
            } if matches!(object.as_ref(), JsExpr::Ident { name } if name == "fs")
                && property == "readFileSync"
        )
}

fn is_node_fs_rm_sync_call(callee: &JsExpr, args: &[JsExpr]) -> bool {
    matches!(args.len(), 1 | 2)
        && matches!(
            callee,
            JsExpr::Member {
                object,
                property,
                property_expr: None,
                optional: false,
            } if matches!(object.as_ref(), JsExpr::Ident { name } if name == "fs")
                && property == "rmSync"
        )
}

fn node_fs_builtin_receiver(callee: &JsExpr, state: &AotState) -> Option<()> {
    let JsExpr::Member { object, .. } = callee else {
        return None;
    };
    let JsExpr::Ident { name } = object.as_ref() else {
        return None;
    };
    state.builtin_bindings.contains(name).then_some(())
}

fn render_node_fs_mkdtemp_sync_call(
    callee: &JsExpr,
    args: &[JsExpr],
    state: &AotState,
) -> Option<String> {
    node_fs_builtin_receiver(callee, state)?;
    if !is_node_fs_mkdtemp_sync_call(callee, args) {
        return None;
    }
    let prefix = render_string_expr(args.first()?, state)?;
    Some(format!("tsgodownFsMkdtempSync({prefix})"))
}

fn render_node_fs_read_file_sync_call(
    callee: &JsExpr,
    args: &[JsExpr],
    state: &AotState,
) -> Option<String> {
    node_fs_builtin_receiver(callee, state)?;
    if !is_node_fs_read_file_sync_call(callee, args) {
        return None;
    }
    let path = render_string_expr(args.first()?, state)?;
    let encoding = args
        .get(1)
        .and_then(|expr| render_string_expr(expr, state))
        .unwrap_or_else(|| "\"utf8\"".to_string());
    Some(format!("tsgodownFsReadFileSync({path}, {encoding})"))
}

fn render_node_fs_write_file_sync_stmt(
    callee: &JsExpr,
    args: &[JsExpr],
    state: &AotState,
) -> Option<String> {
    node_fs_builtin_receiver(callee, state)?;
    if !is_node_fs_write_file_sync_call(callee, args) {
        return None;
    }
    let path = render_string_expr(args.first()?, state)?;
    let data = render_string_expr(args.get(1)?, state)?;
    Some(format!("tsgodownFsWriteFileSync({path}, {data})"))
}

fn render_node_fs_rm_sync_stmt(
    callee: &JsExpr,
    args: &[JsExpr],
    state: &AotState,
) -> Option<String> {
    node_fs_builtin_receiver(callee, state)?;
    if !is_node_fs_rm_sync_call(callee, args) {
        return None;
    }
    let path = render_string_expr(args.first()?, state)?;
    Some(format!("tsgodownFsRmSync({path})"))
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

fn is_buffer_to_string_call(callee: &JsExpr, args: &[JsExpr]) -> bool {
    matches!(args.len(), 0 | 1)
        && matches!(
            callee,
            JsExpr::Member {
                object,
                property,
                property_expr: None,
                optional: false,
            } if property == "toString" && matches!(
                object.as_ref(),
                JsExpr::Call { callee, args, .. } if is_node_buffer_from_call(callee, args)
                    || is_node_buffer_alloc_call(callee, args)
            )
        )
}

fn render_buffer_to_string_call(
    callee: &JsExpr,
    args: &[JsExpr],
    state: &AotState,
) -> Option<String> {
    if !is_buffer_to_string_call(callee, args) {
        return None;
    }
    let JsExpr::Member { object, .. } = callee else {
        return None;
    };
    let value = render_bytes_expr(object, state)?;
    let encoding = args
        .first()
        .and_then(|expr| render_string_expr(expr, state))
        .unwrap_or_else(|| "\"utf8\"".to_string());
    Some(format!("tsgodownBytesToString({value}, {encoding})"))
}

fn is_querystring_parse_call(callee: &JsExpr, args: &[JsExpr]) -> bool {
    args.len() == 1
        && matches!(
            callee,
            JsExpr::Member {
                object,
                property,
                property_expr: None,
                optional: false,
            } if matches!(object.as_ref(), JsExpr::Ident { name } if name == "querystring")
                && property == "parse"
        )
}

fn render_querystring_parse_call(
    callee: &JsExpr,
    args: &[JsExpr],
    state: &AotState,
) -> Option<String> {
    if !state.builtin_bindings.contains("querystring") || !is_querystring_parse_call(callee, args) {
        return None;
    }
    let value = render_string_expr(args.first()?, state)?;
    Some(format!("tsgodownQuerystringParse({value})"))
}

fn is_json_parse_call(callee: &JsExpr, args: &[JsExpr]) -> bool {
    args.len() == 1
        && matches!(
            callee,
            JsExpr::Member {
                object,
                property,
                property_expr: None,
                optional: false,
            } if matches!(object.as_ref(), JsExpr::Ident { name } if name == "JSON")
                && property == "parse"
        )
}

fn render_json_parse_call(callee: &JsExpr, args: &[JsExpr], state: &AotState) -> Option<String> {
    if !is_json_parse_call(callee, args) {
        return None;
    }
    let value = render_string_expr(args.first()?, state)?;
    Some(format!("tsgodownJSONParseObject({value})"))
}

fn crypto_sha256_digest_source_expr(expr: &JsExpr) -> Option<&JsExpr> {
    let JsExpr::Call { callee, args, .. } = expr else {
        return None;
    };
    if args.len() != 1
        || !matches!(
            args.first().and_then(string_literal_value).as_deref(),
            Some("hex")
        )
    {
        return None;
    }
    let JsExpr::Member {
        object,
        property,
        property_expr: None,
        optional: false,
    } = callee.as_ref()
    else {
        return None;
    };
    if property != "digest" {
        return None;
    }
    let JsExpr::Call {
        callee: update_callee,
        args: update_args,
        ..
    } = object.as_ref()
    else {
        return None;
    };
    if update_args.len() != 1 {
        return None;
    }
    let JsExpr::Member {
        object: hash_object,
        property,
        property_expr: None,
        optional: false,
    } = update_callee.as_ref()
    else {
        return None;
    };
    if property != "update" {
        return None;
    }
    let JsExpr::Call {
        callee: create_hash_callee,
        args: create_hash_args,
        ..
    } = hash_object.as_ref()
    else {
        return None;
    };
    if create_hash_args.len() != 1
        || !matches!(
            create_hash_args
                .first()
                .and_then(string_literal_value)
                .as_deref(),
            Some("sha256")
        )
    {
        return None;
    }
    let JsExpr::Member {
        object,
        property,
        property_expr: None,
        optional: false,
    } = create_hash_callee.as_ref()
    else {
        return None;
    };
    if property != "createHash"
        || !matches!(object.as_ref(), JsExpr::Ident { name } if name == "crypto")
    {
        return None;
    }
    update_args.first()
}

fn is_crypto_sha256_hex_digest_call(callee: &JsExpr) -> bool {
    let JsExpr::Member { object, .. } = callee else {
        return false;
    };
    crypto_sha256_digest_source_expr(object).is_some()
}

fn is_crypto_sha256_hex_slice_call(callee: &JsExpr) -> bool {
    matches!(
        callee,
        JsExpr::Member {
            object,
            property,
            property_expr: None,
            optional: false,
        } if property == "slice" && crypto_sha256_digest_source_expr(object).is_some()
    )
}

fn render_crypto_sha256_hex_call(
    callee: &JsExpr,
    args: &[JsExpr],
    state: &AotState,
) -> Option<String> {
    if !state.builtin_bindings.contains("crypto") {
        return None;
    }
    let JsExpr::Member {
        object,
        property,
        property_expr: None,
        optional: false,
    } = callee
    else {
        return None;
    };
    if property == "digest" {
        let source = crypto_sha256_digest_source_expr(object)?;
        let source = render_string_expr(source, state)?;
        return Some(format!("tsgodownSHA256Hex({source})"));
    }
    if property == "slice" && matches!(args.len(), 1 | 2) {
        let source = crypto_sha256_digest_source_expr(object)?;
        let source = render_string_expr(source, state)?;
        let start = render_numeric_expr(args.first()?, state)?;
        if let Some(end) = args.get(1) {
            let end = render_numeric_expr(end, state)?;
            return Some(format!(
                "tsgodownStringSlice(tsgodownSHA256Hex({source}), {start}, {end})"
            ));
        }
        return Some(format!(
            "tsgodownStringSlice(tsgodownSHA256Hex({source}), {start})"
        ));
    }
    None
}

fn render_string_numeric_method_call(
    callee: &JsExpr,
    args: &[JsExpr],
    state: &AotState,
) -> Option<String> {
    if let Some(object) = string_method_receiver(callee, "indexOf", args, state) {
        let object = render_string_expr(object, state)?;
        let needle = render_string_expr(args.first()?, state)?;
        if let Some(start) = args.get(1) {
            let start = render_numeric_expr(start, state)?;
            return Some(format!(
                "func() float64 {{ value := {object}; offset := int({start}); if offset < 0 {{ offset = 0 }}; if offset > len(value) {{ offset = len(value) }}; found := strings.Index(value[offset:], {needle}); if found < 0 {{ return -1 }}; return float64(offset + found) }}()"
            ));
        }
        return Some(format!("float64(strings.Index({object}, {needle}))"));
    }
    if let Some(object) = string_method_receiver(callee, "charCodeAt", args, state) {
        let object = render_string_expr(object, state)?;
        let index = render_numeric_expr(args.first()?, state)?;
        return Some(format!("tsgodownStringCharCodeAt({object}, {index})"));
    }
    None
}

fn is_string_cast_call(callee: &JsExpr, args: &[JsExpr]) -> bool {
    args.len() == 1 && matches!(callee, JsExpr::Ident { name } if name == "String")
}

fn is_string_from_char_code_call(callee: &JsExpr, args: &[JsExpr]) -> bool {
    !args.is_empty()
        && matches!(
            callee,
            JsExpr::Member {
                object,
                property,
                property_expr: None,
                optional: false,
            } if matches!(object.as_ref(), JsExpr::Ident { name } if name == "String")
                && property == "fromCharCode"
        )
}

fn is_boolean_cast_call(callee: &JsExpr, args: &[JsExpr]) -> bool {
    args.len() == 1 && matches!(callee, JsExpr::Ident { name } if name == "Boolean")
}

fn is_global_is_nan_call(callee: &JsExpr, args: &[JsExpr]) -> bool {
    args.len() == 1 && matches!(callee, JsExpr::Ident { name } if name == "isNaN")
}

fn is_parse_int_call(callee: &JsExpr, args: &[JsExpr]) -> bool {
    matches!(args.len(), 1 | 2) && matches!(callee, JsExpr::Ident { name } if name == "parseInt")
}

fn is_process_supported_builtin_expr(expr: &JsExpr) -> bool {
    is_process_stdout_is_tty(expr)
        || process_env_lookup_name(expr).is_some()
        || is_process_argv_ref(expr)
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

fn is_process_argv_ref(expr: &JsExpr) -> bool {
    let JsExpr::Member {
        object,
        property,
        property_expr: None,
        optional: _,
    } = expr
    else {
        return false;
    };
    property == "argv" && matches!(object.as_ref(), JsExpr::Ident { name } if name == "process")
}

fn render_process_argv_expr(state: &AotState) -> Option<String> {
    let entry = state.entry_source_path.as_deref()?;
    Some(format!("tsgodownProcessArgv({})", go_string_literal(entry)))
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
    if let Some(value) = render_any_array_push_call_expr(callee, args, state) {
        return Some(value);
    }
    if let Some(value) = render_event_emitter_call_expr(callee, args, state) {
        return Some(value);
    }
    if let Some(value) = render_map_call_expr(callee, args, state) {
        return Some(value);
    }
    if let Some(value) = render_array_at_call(callee, args, state) {
        return Some(value);
    }
    if is_json_stringify(callee) {
        let value = render_json_value_expr(args.first()?, state)?;
        if let Some(space) = args.get(2).and_then(render_json_stringify_space_expr) {
            return Some(format!("tsgodownJSONStringifyIndent({value}, {space})"));
        }
        return Some(format!("tsgodownJSONStringify({value})"));
    }
    if is_object_keys_call(callee, args) {
        return render_object_keys_call(callee, args, state);
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
            let rendered_args = render_call_args(args, &function.param_kinds, state)?;
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
    if let Some(value) = render_dynamic_function_call(callee, args, state) {
        return Some(value);
    }
    let JsExpr::Ident { name } = callee else {
        return None;
    };
    if is_any_binding(name, state) {
        let function = go_binding_ref(name, state);
        let args = args
            .iter()
            .map(|arg| render_expr(arg, state))
            .collect::<Option<Vec<_>>>()?;
        if args.is_empty() {
            return Some(format!("tsgodownCall({function})"));
        }
        return Some(format!("tsgodownCall({function}, {})", args.join(", ")));
    }
    if args.len() == 1 && state.number_closure_bindings.contains(name) {
        let arg = render_numeric_expr(args.first()?, state)?;
        return Some(format!("{}({arg})", go_binding_ref(name, state)));
    }
    if args.is_empty() && state.string_function_bindings.contains(name) {
        return Some(format!("{}()", go_binding_ref(name, state)));
    }
    let function = state.functions.get(name)?;
    let rendered_args = render_call_args(args, &function.param_kinds, state)?;
    Some(format!(
        "{}({})",
        function.go_name,
        rendered_args.join(", ")
    ))
}

fn render_dynamic_function_call(
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
    let function = render_dynamic_object_member_expr(object, property, state)?;
    let args = args
        .iter()
        .map(|arg| render_expr(arg, state))
        .collect::<Option<Vec<_>>>()?;
    if args.is_empty() {
        return Some(format!("tsgodownCall({function})"));
    }
    Some(format!("tsgodownCall({function}, {})", args.join(", ")))
}

fn render_array_at_call(callee: &JsExpr, args: &[JsExpr], state: &AotState) -> Option<String> {
    if args.len() != 1 {
        return None;
    }
    let JsExpr::Member {
        object,
        property,
        property_expr: None,
        optional: false,
    } = callee
    else {
        return None;
    };
    if property != "at" {
        return None;
    }
    let value = render_json_value_expr(object, state)?;
    let index = render_numeric_expr(args.first()?, state)?;
    Some(format!("tsgodownArrayAt({value}, {index})"))
}

fn render_json_stringify_space_expr(expr: &JsExpr) -> Option<String> {
    match expr {
        JsExpr::Value {
            value: JsValue::Number { value },
        } => {
            let spaces = value.parse::<usize>().ok()?.min(10);
            Some(go_string_literal(&" ".repeat(spaces)))
        }
        JsExpr::Value {
            value: JsValue::String { value },
        } => Some(go_string_literal(value)),
        _ => None,
    }
}

fn render_call_args(
    args: &[JsExpr],
    param_kinds: &[AotSlotKind],
    state: &AotState,
) -> Option<Vec<String>> {
    if args.len() > param_kinds.len() {
        return None;
    }
    let mut rendered = Vec::new();
    for (index, kind) in param_kinds.iter().enumerate() {
        if let Some(arg) = args.get(index) {
            rendered.push(render_arg_for_kind(arg, *kind, state)?);
        } else if *kind == AotSlotKind::Any {
            rendered.push("nil".to_string());
        } else {
            return None;
        }
    }
    Some(rendered)
}

fn render_arg_for_kind(expr: &JsExpr, kind: AotSlotKind, state: &AotState) -> Option<String> {
    match kind {
        AotSlotKind::Any => {
            render_json_value_expr(expr, state).or_else(|| render_expr(expr, state))
        }
        AotSlotKind::Bool => render_bool_expr(expr, state),
        AotSlotKind::Bytes => render_bytes_expr(expr, state),
        AotSlotKind::Number => render_numeric_expr(expr, state),
        AotSlotKind::NumberArray => render_number_array_expr(expr, state),
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
        JsExpr::Function { .. } => render_inline_function_value_expr(expr, state),
        JsExpr::Ident { name } if state.bindings.contains(name) => {
            Some(go_binding_ref(name, state))
        }
        expr if render_string_array_index_expr(expr, state).is_some() => {
            render_string_array_index_expr(expr, state)
        }
        expr if render_string_array_length_expr(expr, state).is_some() => {
            render_string_array_length_expr(expr, state)
        }
        expr if render_string_array_expr(expr, state).is_some() => {
            render_string_array_expr(expr, state)
        }
        JsExpr::Array { items } => {
            let items = items
                .iter()
                .map(|item| render_json_value_expr(item, state))
                .collect::<Option<Vec<_>>>()?;
            Some(format!("[]any{{{}}}", items.join(", ")))
        }
        JsExpr::Object { props } => {
            if props.iter().any(|prop| prop.spread) {
                return render_object_map_expr(expr, state);
            }
            let mut fields = Vec::new();
            for prop in props {
                let key = match &prop.key_expr {
                    Some(key_expr) => render_string_expr(key_expr, state)?,
                    None => go_string_literal(&prop.key),
                };
                let value = render_json_value_expr(&prop.value, state)?;
                fields.push(format!("{key}: {value}"));
            }
            Some(format!("map[string]any{{{}}}", fields.join(", ")))
        }
        JsExpr::Binary { op, .. } if op == "+" => render_expr(expr, state),
        JsExpr::Binary { op, .. } if go_comparison_op(op).is_some() => {
            render_bool_expr(expr, state)
        }
        JsExpr::Unary { .. } => render_expr(expr, state),
        JsExpr::Call { callee, args, .. } if is_json_parse_call(callee, args) => {
            render_json_parse_call(callee, args, state)
        }
        JsExpr::Call { callee, args, .. } if is_querystring_parse_call(callee, args) => {
            render_querystring_parse_call(callee, args, state)
        }
        JsExpr::Call { callee, args, .. }
            if render_string_expr(expr, state)
                .or_else(|| render_node_fs_mkdtemp_sync_call(callee, args, state))
                .is_some() =>
        {
            render_string_expr(expr, state)
                .or_else(|| render_node_fs_mkdtemp_sync_call(callee, args, state))
        }
        expr if is_process_version_expr(expr) => render_process_version_expr(expr),
        expr if is_process_platform_expr(expr) => Some("tsgodownProcessPlatform()".to_string()),
        expr if is_process_argv_ref(expr) => render_process_argv_expr(state),
        expr if is_process_env_ref(expr) => Some("tsgodownProcessEnv()".to_string()),
        expr if is_process_versions_ref(expr) => Some(render_process_versions_expr()),
        expr if is_process_cwd_ref(expr) => render_string_function_expr(expr),
        JsExpr::Call { callee, args, .. } => render_call_expr(callee, args, state),
        JsExpr::Member {
            object,
            property,
            property_expr: None,
            optional: false,
        } => render_class_getter_member_expr(object, property, state)
            .or_else(|| render_static_member_expr(object, property, state))
            .or_else(|| render_url_string_member_expr(object, property, state))
            .or_else(|| {
                if property == "length" || property.parse::<usize>().is_ok() {
                    render_numeric_expr(expr, state).or_else(|| render_string_expr(expr, state))
                } else {
                    None
                }
            })
            .or_else(|| render_dynamic_object_member_expr(object, property, state))
            .or_else(|| render_numeric_expr(expr, state)),
        _ => None,
    }
}

fn render_inline_function_value_expr(expr: &JsExpr, state: &AotState) -> Option<String> {
    let JsExpr::Function {
        params,
        rest_param: None,
        r#async: false,
        generator: false,
        body,
        ..
    } = expr
    else {
        return None;
    };
    let mut function_state = clone_aot_state(state);
    for param in params {
        function_state.bind_slot(param, sanitize_go_identifier(param), AotSlotKind::Any);
    }
    mark_dynamic_object_locals(body, &mut function_state);
    let rendered_body = render_function_body(body, &function_state)?;
    let rendered_body = if rendered_body.trim_end().ends_with("return nil") {
        rendered_body
    } else {
        format!("{rendered_body}\nreturn nil")
    };
    let rendered_params = params
        .iter()
        .map(|param| format!("{} any", sanitize_go_identifier(param)))
        .collect::<Vec<_>>()
        .join(", ");
    Some(format!(
        "func({rendered_params}) any {{\n{}\n}}",
        indent_lines(&rendered_body)
    ))
}

fn render_function_reference_expr(name: &str, state: &AotState) -> Option<String> {
    let function = state.functions.get(name)?;
    let go_name = &function.go_name;
    match function.param_kinds.as_slice() {
        [] => Some(format!("func() any {{ return {go_name}() }}")),
        [kind] => {
            let arg = render_any_arg_to_kind("arg0", *kind);
            Some(format!("func(arg0 any) any {{ return {go_name}({arg}) }}"))
        }
        [first, second] => {
            let first = render_any_arg_to_kind("arg0", *first);
            let second = render_any_arg_to_kind("arg1", *second);
            Some(format!(
                "func(arg0 any, arg1 any) any {{ return {go_name}({first}, {second}) }}"
            ))
        }
        _ => None,
    }
}

fn render_any_arg_to_kind(name: &str, kind: AotSlotKind) -> String {
    match kind {
        AotSlotKind::Any => name.to_string(),
        AotSlotKind::Bool => format!("tsgodownToBool({name})"),
        AotSlotKind::Bytes => format!("{name}.([]byte)"),
        AotSlotKind::Number => format!("tsgodownToFloat64({name})"),
        AotSlotKind::NumberArray => format!("{name}.([]float64)"),
        AotSlotKind::String => format!("tsgodownToString({name})"),
        AotSlotKind::StringArray => format!("{name}.([]string)"),
        AotSlotKind::BoolFunction => format!("{name}.(func() bool)"),
        AotSlotKind::StringFunction => format!("{name}.(func() string)"),
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
        && !state.number_array_bindings.contains(name)
        && !state.string_array_bindings.contains(name)
        && !state.map_bindings.contains(name)
        && !state.url_bindings.contains(name)
        && !state.event_emitter_bindings.contains(name)
        && !state.number_closure_bindings.contains(name)
        && !state.string_function_bindings.contains(name)
        && !state.object_bindings.contains_key(name)
        && !state.class_instance_bindings.contains_key(name)
        && !state.functions.contains_key(name)
        && !state.builtin_bindings.contains(name)
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

fn is_bitwise_binary_op(op: &str) -> bool {
    matches!(op, ">>" | "<<" | "&" | "|" | "^")
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
