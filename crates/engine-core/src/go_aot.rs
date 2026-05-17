use std::collections::{BTreeMap, BTreeSet};

use crate::contract::{AnalyzeResponse, IrDocument, JsExpr, JsStmt, JsValue, Module};
use crate::emit_go::{go_string_literal, sanitize_go_identifier};

const CJS_DEFAULT_EXPORT_FUNCTION: &str = "__cjs_default_export";
const NODE_LTS_VERSION: &str = "24.15.0";
const NODE_LTS_VERSION_WITH_PREFIX: &str = "v24.15.0";
const AOT_FUNCTION_RENDER_LIMIT: usize = 4096;

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
            object_exports: &module_object_exports,
            slots: &module_slots,
        },
    )?;
    state.go_imports = collect_aot_imports(&analyzed.ir);
    mark_dynamic_object_locals(&module.executable.as_ref()?.stmts, &mut state);
    mark_logical_assignment_any_locals(&module.executable.as_ref()?.stmts, &mut state);
    mark_number_array_locals(&module.executable.as_ref()?.stmts, &mut state);
    mark_string_array_locals(&module.executable.as_ref()?.stmts, &mut state);
    mark_any_array_locals(&module.executable.as_ref()?.stmts, &mut state);
    mark_array_property_locals(&module.executable.as_ref()?.stmts, &mut state);
    let mut body = module_init_order(&analyzed.ir, module)
        .into_iter()
        .filter(|candidate| candidate.id != module.id)
        .map(|candidate| format!("{}()", module_init_go_name(candidate)))
        .collect::<Vec<_>>();
    for stmt in &module.executable.as_ref()?.stmts {
        if matches!(stmt, JsStmt::FunctionDecl { .. } | JsStmt::ClassDecl { .. })
            || is_function_binding_stmt(stmt)
        {
            continue;
        }
        if matches!(
            stmt,
            JsStmt::VarDecl { name, .. }
                if module_slots.contains_key(&(module.id.clone(), name.clone()))
        ) || is_create_require_alias_decl(stmt)
            || is_resolved_cjs_export_metadata_stmt(stmt, &state)
            || is_resolved_cjs_export_metadata_decl_stmt(stmt, &state)
            || is_resolved_default_export_metadata_decl_stmt(stmt, &state)
        {
            continue;
        }
        body.push(render_stmt(stmt, &mut state)?);
    }
    let declarations = declarations.join("\n\n");
    let helpers = render_aot_helpers(&state.go_imports);
    let body = indent_lines(&body.join("\n"));
    let source_without_imports = format!("{declarations}\n{helpers}\nfunc main() {{\n{body}\n}}\n");
    let imports = prune_unused_go_imports(&state.go_imports, &source_without_imports);
    Some(format!(
        r#"package {package_name}

{imports}

{declarations}
{helpers}
func main() {{
{body}
}}
"#,
        imports = render_go_imports(&imports),
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
            if import.resolved.is_none()
                && !is_node_builtin_spec(&import.spec)
                && !module_has_caught_require_spec(module, &import.spec)
            {
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
                object_exports: &module_object_exports,
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
                if module_classes.contains_key(&(module.id.clone(), parts.name.clone())) {
                    continue;
                }
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
            mark_logical_assignment_any_locals(&executable.stmts, &mut render_state);
            mark_string_array_locals(&executable.stmts, &mut render_state);
            mark_any_array_locals(&executable.stmts, &mut render_state);
            mark_array_property_locals(&executable.stmts, &mut render_state);
            for stmt in &executable.stmts {
                if matches!(stmt, JsStmt::FunctionDecl { .. } | JsStmt::ClassDecl { .. })
                    || is_function_binding_stmt(stmt)
                {
                    continue;
                }
                if matches!(
                    stmt,
                    JsStmt::VarDecl { name, .. }
                        if module_slots.contains_key(&(module.id.clone(), name.clone()))
                ) || is_create_require_alias_decl(stmt)
                    || is_resolved_cjs_export_metadata_stmt(stmt, &render_state)
                    || is_resolved_cjs_export_metadata_decl_stmt(stmt, &render_state)
                    || is_resolved_default_export_metadata_decl_stmt(stmt, &render_state)
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
    imports.insert("fmt");
    imports.insert("math");
    imports.insert("strconv");
    imports.insert("strings");
    imports.insert("time");
    for module in &ir.modules {
        if module
            .imports
            .iter()
            .any(|import| import.resolved.is_none() && is_node_fs_promises_spec(&import.spec))
        {
            imports.insert("os");
            imports.insert("path/filepath");
            imports.insert("sort");
        }
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
        } => {
            if matches!(
                builtin_function_alias(init),
                Some(AotBuiltinFunctionAlias::RegExpTest)
            ) {
                imports.insert("regexp");
            }
            collect_expr_imports(init, imports);
        }
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
        JsStmt::ForOf { right, body, .. } => {
            collect_expr_imports(right, imports);
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
        JsStmt::Try {
            body,
            catch_body,
            finally_body,
            ..
        } => {
            for stmt in body {
                collect_stmt_imports(stmt, imports);
            }
            for stmt in catch_body {
                collect_stmt_imports(stmt, imports);
            }
            for stmt in finally_body {
                collect_stmt_imports(stmt, imports);
            }
        }
        _ => {}
    }
}

fn collect_expr_imports(expr: &JsExpr, imports: &mut BTreeSet<&'static str>) {
    match expr {
        JsExpr::Call { callee, args, .. } => {
            if is_json_parse_call(callee, args) {
                imports.insert("encoding/json");
                imports.insert("sort");
                imports.insert("strconv");
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
                imports.insert("sort");
                imports.insert("strconv");
            }
            if is_object_keys_call(callee, args) {
                imports.insert("sort");
                imports.insert("strconv");
            }
            if is_object_entries_call(callee, args) {
                imports.insert("sort");
                imports.insert("strconv");
            }
            if is_string_cast_call(callee, args) {
                imports.insert("strconv");
            }
            if is_number_cast_call(callee, args) {
                imports.insert("strconv");
                imports.insert("strings");
            }
            if is_number_is_integer_call(callee, args) {
                imports.insert("math");
            }
            if is_number_is_finite_call(callee, args)
                || is_number_is_safe_integer_call(callee, args)
                || is_global_is_finite_call(callee, args)
            {
                imports.insert("math");
            }
            if is_array_sort_call(callee, args) {
                imports.insert("sort");
            }
            if is_uri_string_call(callee, args) {
                imports.insert("strings");
            }
            if is_number_to_string_call(callee, args) && !is_buffer_to_string_call(callee, args) {
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
            if is_regexp_exec_call(callee, args) {
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
            if is_crypto_sha256_hex_slice_call(callee)
                || is_crypto_sha256_hex_digest_call(callee)
                || is_crypto_hash_hex_digest_call(callee, args)
            {
                imports.insert("encoding/hex");
            }
            if let Some(algorithm) = crypto_hash_bytes_digest_algorithm(callee, args)
                .or_else(|| crypto_hash_hex_digest_algorithm(callee, args))
            {
                match algorithm {
                    "md5" => {
                        imports.insert("crypto/md5");
                    }
                    "sha1" => {
                        imports.insert("crypto/sha1");
                    }
                    "sha256" => {
                        imports.insert("crypto/sha256");
                    }
                    _ => {}
                }
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
            if is_crypto_random_fill_sync_call_shape(callee, args) {
                imports.insert("crypto/rand");
            }
            if is_crypto_random_uuid_call_shape(callee, args) {
                imports.insert("crypto/rand");
                imports.insert("encoding/base64");
                imports.insert("encoding/hex");
            }
            if call_uses_strings_import(callee) {
                imports.insert("strings");
            }
            if is_string_split_call(callee, args) {
                imports.insert("strings");
            }
            if is_string_array_join_call(callee, args) {
                imports.insert("strings");
                imports.insert("strconv");
            }
            if string_method_alias_call_uses_regexp(callee, args) {
                imports.insert("regexp");
            }
            if string_method_name(callee).is_some() {
                imports.insert("strconv");
            }
            if matches!(string_method_name(callee), Some("trimStart" | "trimEnd")) {
                imports.insert("unicode");
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
        JsExpr::Template { exprs, .. } => {
            if exprs.iter().any(template_part_needs_to_string_helper) {
                imports.insert("strconv");
            }
            for expr in exprs {
                collect_expr_imports(expr, imports);
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
        JsExpr::Assign { op, left, right } => {
            if matches!(op.as_str(), "%=" | "**=") {
                imports.insert("math");
            }
            collect_expr_imports(left, imports);
            collect_expr_imports(right, imports);
        }
        JsExpr::Binary { op, left, right } => {
            if op == "**" {
                imports.insert("math");
            }
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
        JsExpr::Ident { name } if name == "randomUUID" => {
            imports.insert("crypto/rand");
            imports.insert("encoding/base64");
            imports.insert("encoding/hex");
        }
        expr if is_node_path_sep_expr(expr) => {
            imports.insert("os");
        }
        expr if is_node_path_delimiter_expr(expr) => {
            imports.insert("runtime");
        }
        expr if matches!(
            builtin_function_alias(expr),
            Some(AotBuiltinFunctionAlias::RegExpTest)
        ) =>
        {
            imports.insert("regexp");
        }
        JsExpr::Member { object, .. } => {
            if is_length_member_expr(expr) {
                imports.insert("math");
                imports.insert("strconv");
            }
            collect_expr_imports(object, imports);
        }
        _ => {}
    }
}

fn render_go_imports(imports: &BTreeSet<&'static str>) -> String {
    if imports.is_empty() {
        return String::new();
    }
    if imports.len() == 1 {
        return format!(
            "import {}",
            render_go_import_spec(imports.iter().next().expect("single import"))
        );
    }
    format!(
        "import (\n{}\n)",
        imports
            .iter()
            .map(|import| format!("\t{}", render_go_import_spec(import)))
            .collect::<Vec<_>>()
            .join("\n")
    )
}

fn render_go_import_spec(import: &str) -> String {
    match import {
        "crypto/md5" => "tsgodownmd5 \"crypto/md5\"".to_string(),
        "crypto/sha1" => "tsgodownsha1 \"crypto/sha1\"".to_string(),
        "crypto/sha256" => "tsgodownsha256 \"crypto/sha256\"".to_string(),
        _ => format!("{import:?}"),
    }
}

fn prune_unused_go_imports(
    imports: &BTreeSet<&'static str>,
    source: &str,
) -> BTreeSet<&'static str> {
    imports
        .iter()
        .copied()
        .filter(|import| source.contains(go_import_usage_token(import)))
        .collect()
}

fn go_import_usage_token(import: &str) -> &'static str {
    match import {
        "compress/gzip" => "gzip.",
        "compress/zlib" => "zlib.",
        "crypto/md5" => "tsgodownmd5.",
        "crypto/rand" => "rand.",
        "crypto/sha1" => "tsgodownsha1.",
        "crypto/sha256" => "tsgodownsha256.",
        "encoding/base64" => "base64.",
        "encoding/hex" => "hex.",
        "encoding/json" => "json.",
        "fmt" => "fmt.",
        "io" => "io.",
        "math" => "math.",
        "net/http" => "http.",
        "net/url" => "url.",
        "os" => "os.",
        "path" => "path.",
        "path/filepath" => "filepath.",
        "regexp" => "regexp.",
        "sort" => "sort.",
        "strconv" => "strconv.",
        "strings" => "strings.",
        "time" => "time.",
        "unicode" => "unicode.",
        _ => ".",
    }
}

fn render_aot_helpers(imports: &BTreeSet<&'static str>) -> String {
    let mut helpers = vec![r#"type tsgodownRegExp string

type tsgodownSymbol struct {
	description string
}

func tsgodownNewSymbol(description string) tsgodownSymbol {
	return tsgodownSymbol{description: description}
}

func tsgodownWellKnownSymbol(name string) tsgodownSymbol {
	return tsgodownSymbol{description: name}
}

func tsgodownSymbolToString(value any) string {
	switch value := value.(type) {
	case tsgodownSymbol:
		if value.description == "" {
			return "Symbol()"
		}
		return "Symbol(" + value.description + ")"
	default:
		return tsgodownToString(value)
	}
}

func tsgodownObjectToStringTag(value any) string {
	switch value.(type) {
	case nil:
		return "[object Undefined]"
	case tsgodownRegExp:
		return "[object RegExp]"
	case tsgodownSymbol:
		return "[object Symbol]"
	case bool:
		return "[object Boolean]"
	case float64, int, int64:
		return "[object Number]"
	case string:
		return "[object String]"
	case []any, []string, []float64:
		return "[object Array]"
	case []byte:
		return "[object Uint8Array]"
	case map[string]any:
		return "[object Object]"
	case func() any, func(any) any, func(any, any) any:
		return "[object Function]"
	default:
		return "[object Object]"
	}
}

func tsgodownSetBytesIndexAny(target any, index float64, value float64) {
	switch typed := target.(type) {
	case []byte:
		typed[int(index)] = byte(value)
	case []any:
		typed[int(index)] = value
	case []float64:
		typed[int(index)] = value
	}
}

func tsgodownBitwiseOrAssignIndexAny(target any, index float64, value float64) {
	offset := int(index)
	switch typed := target.(type) {
	case []byte:
		typed[offset] = byte(int(typed[offset]) | int(value))
	case []any:
		typed[offset] = float64(int(tsgodownToFloat64(typed[offset])) | int(value))
	case []float64:
		typed[offset] = float64(int(typed[offset]) | int(value))
	}
}

func tsgodownIndexFloat(value any, index float64) float64 {
	offset := int(index)
	if offset < 0 {
		return 0
	}
	switch typed := value.(type) {
	case []byte:
		if offset >= len(typed) {
			return 0
		}
		return float64(typed[offset])
	case []any:
		if offset >= len(typed) {
			return 0
		}
		switch item := typed[offset].(type) {
		case float64:
			return item
		case int:
			return float64(item)
		case int64:
			return float64(item)
		case byte:
			return float64(item)
		case bool:
			if item {
				return 1
			}
			return 0
		default:
			return 0
		}
	case []float64:
		if offset >= len(typed) {
			return 0
		}
		return typed[offset]
	case []string:
		return 0
	default:
		return 0
	}
}

func tsgodownDateFormatISO(value time.Time) string {
	value = value.UTC()
	return value.Format("2006-01-02T15:04:05.") + fmt.Sprintf("%03dZ", value.Nanosecond()/int(time.Millisecond))
}

func tsgodownDateUTCISOString(year float64, month float64, day float64, values ...float64) string {
	hour := 0
	minute := 0
	second := 0
	millisecond := 0
	if len(values) > 0 {
		hour = int(values[0])
	}
	if len(values) > 1 {
		minute = int(values[1])
	}
	if len(values) > 2 {
		second = int(values[2])
	}
	if len(values) > 3 {
		millisecond = int(values[3])
	}
	return tsgodownDateFormatISO(time.Date(int(year), time.Month(int(month)+1), int(day), hour, minute, second, millisecond*int(time.Millisecond), time.UTC))
}

func tsgodownDateFromUnixMilliISOString(value float64) string {
	return tsgodownDateFormatISO(time.UnixMilli(int64(value)))
}

func tsgodownMathMax(values ...float64) float64 {
	if len(values) == 0 {
		return math.Inf(-1)
	}
	max := values[0]
	for _, value := range values[1:] {
		if value > max {
			max = value
		}
	}
	return max
}

func tsgodownMathMin(values ...float64) float64 {
	if len(values) == 0 {
		return math.Inf(1)
	}
	min := values[0]
	for _, value := range values[1:] {
		if value < min {
			min = value
		}
	}
	return min
}

func tsgodownMathRound(value float64) float64 {
	return math.Floor(value + 0.5)
}

func tsgodownMathRandom() float64 {
	return float64(time.Now().UnixNano()&0x1fffffffffffff) / float64(0x20000000000000)
}

func tsgodownNumberToString(value float64, radix float64) string {
	base := int(radix)
	if base < 2 || base > 36 {
		base = 10
	}
	if math.IsNaN(value) {
		return "NaN"
	}
	if math.IsInf(value, 1) {
		return "Infinity"
	}
	if math.IsInf(value, -1) {
		return "-Infinity"
	}
	if value == 0 {
		return "0"
	}
	sign := ""
	if value < 0 {
		sign = "-"
		value = -value
	}
	if math.Trunc(value) == value {
		return sign + strconv.FormatInt(int64(value), base)
	}
	if base == 10 {
		return sign + strconv.FormatFloat(value, 'f', -1, 64)
	}
	digits := "0123456789abcdefghijklmnopqrstuvwxyz"
	integer := math.Floor(value)
	output := strconv.FormatInt(int64(integer), base) + "."
	fraction := value - integer
	for index := 0; index < 32 && fraction > 0; index++ {
		fraction *= float64(base)
		digit := int(math.Floor(fraction))
		if digit < 0 {
			digit = 0
		}
		if digit >= base {
			digit = base - 1
		}
		output += digits[digit : digit+1]
		fraction -= float64(digit)
	}
	if strings.HasSuffix(output, ".") {
		output += "0"
	}
	return sign + output
}

func tsgodownStringArrayAt(values []string, index float64) string {
	offset := int(index)
	if values == nil || offset < 0 || offset >= len(values) {
		return ""
	}
	return values[offset]
}

func tsgodownStringArrayIncludes(values []string, needle string) bool {
	for _, value := range values {
		if value == needle {
			return true
		}
	}
	return false
}

func tsgodownNumberArrayIndexOf(values []float64, needle float64) float64 {
	for index, value := range values {
		if value == needle {
			return float64(index)
		}
	}
	return -1
}

func tsgodownNumberArrayIncludes(values []float64, needle float64) bool {
	return tsgodownNumberArrayIndexOf(values, needle) >= 0
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

func tsgodownStringArraySetLength(values []string, length float64) []string {
	size := int(length)
	if size < 0 {
		size = 0
	}
	if size <= len(values) {
		return values[:size]
	}
	for len(values) < size {
		values = append(values, "")
	}
	return values
}

func tsgodownStringArrayAdd(values []string, index float64, value string) []string {
	current := tsgodownStringArrayAt(values, index)
	return tsgodownStringArraySet(values, index, current+value)
}

func tsgodownStringArrayShift(values *[]string) string {
	if values == nil || len(*values) == 0 {
		return ""
	}
	value := (*values)[0]
	*values = (*values)[1:]
	return value
}

func tsgodownStringArrayUnshift(values *[]string, items ...string) float64 {
	if values == nil {
		return 0
	}
	*values = append(append([]string{}, items...), (*values)...)
	return float64(len(*values))
}

func tsgodownStringArraySplice(values *[]string, start float64, deleteCount float64, items ...string) []string {
	if values == nil {
		return []string{}
	}
	length := len(*values)
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
	count := int(deleteCount)
	if count < 0 {
		count = 0
	}
	if from+count > length {
		count = length - from
	}
	removed := append([]string(nil), (*values)[from:from+count]...)
	next := append([]string{}, (*values)[:from]...)
	next = append(next, items...)
	next = append(next, (*values)[from+count:]...)
	*values = next
	return removed
}

func tsgodownRegexpPattern(pattern string, flags string) string {
	ignoreCase := false
	for _, flag := range flags {
		if flag == 'i' {
			ignoreCase = true
		}
	}
	if ignoreCase {
		return "(?i)" + pattern
	}
	return pattern
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

func tsgodownAnyArrayAt(values []any, index float64) any {
	offset := int(index)
	if values == nil || offset < 0 || offset >= len(values) {
		return nil
	}
	return values[offset]
}

func tsgodownAnyArraySet(values []any, index float64, value any) []any {
	offset := int(index)
	if offset < 0 {
		return values
	}
	for len(values) <= offset {
		values = append(values, nil)
	}
	values[offset] = value
	return values
}

func tsgodownAnyArraySetLength(values []any, length float64) []any {
	size := int(length)
	if size < 0 {
		size = 0
	}
	if size <= len(values) {
		return values[:size]
	}
	for len(values) < size {
		values = append(values, nil)
	}
	return values
}

func tsgodownObjectArraySet(value any, key string, index float64, propertyValue any) {
	object := tsgodownObjectFromAny(value)
	values := tsgodownAnyArraySet(tsgodownAnyArrayFromAny(object[key]), index, propertyValue)
	object[key] = values
}

func tsgodownAnyArrayWithLength(length float64) []any {
	size := int(length)
	if size < 0 {
		size = 0
	}
	return make([]any, size)
}

func tsgodownAnyArrayFromLengthMap(length float64, mapper func(any, float64) any) []any {
	values := tsgodownAnyArrayWithLength(length)
	for index := range values {
		values[index] = mapper(nil, float64(index))
	}
	return values
}

func tsgodownAnyArrayMap(values []any, mapper func(any, float64) any) []any {
	mapped := make([]any, len(values))
	for index, value := range values {
		mapped[index] = mapper(value, float64(index))
	}
	return mapped
}

func tsgodownStringArrayMap(values []string, mapper func(any, float64) any) []any {
	mapped := make([]any, len(values))
	for index, value := range values {
		mapped[index] = mapper(value, float64(index))
	}
	return mapped
}

func tsgodownAnyArraySome(values []any, predicate func(any, float64) bool) bool {
	for index, value := range values {
		if predicate(value, float64(index)) {
			return true
		}
	}
	return false
}

func tsgodownAnyArrayEvery(values []any, predicate func(any, float64) bool) bool {
	for index, value := range values {
		if !predicate(value, float64(index)) {
			return false
		}
	}
	return true
}

func tsgodownStringArraySome(values []string, predicate func(any, float64) bool) bool {
	for index, value := range values {
		if predicate(value, float64(index)) {
			return true
		}
	}
	return false
}

func tsgodownStringArrayEvery(values []string, predicate func(any, float64) bool) bool {
	for index, value := range values {
		if !predicate(value, float64(index)) {
			return false
		}
	}
	return true
}

func tsgodownBytesSome(values []byte, predicate func(any, float64) bool) bool {
	for index, value := range values {
		if predicate(float64(value), float64(index)) {
			return true
		}
	}
	return false
}

func tsgodownBytesEvery(values []byte, predicate func(any, float64) bool) bool {
	for index, value := range values {
		if !predicate(float64(value), float64(index)) {
			return false
		}
	}
	return true
}

func tsgodownAnyArrayFilter(values []any, predicate func(any, float64) bool) []any {
	filtered := make([]any, 0, len(values))
	for index, value := range values {
		if predicate(value, float64(index)) {
			filtered = append(filtered, value)
		}
	}
	return filtered
}

func tsgodownStringArrayFilter(values []string, predicate func(any, float64) bool) []string {
	filtered := make([]string, 0, len(values))
	for index, value := range values {
		if predicate(value, float64(index)) {
			filtered = append(filtered, value)
		}
	}
	return filtered
}

func tsgodownAnyArrayFind(values []any, predicate func(any, float64) bool, reverse bool) any {
	if reverse {
		for index := len(values) - 1; index >= 0; index-- {
			value := values[index]
			if predicate(value, float64(index)) {
				return value
			}
		}
		return nil
	}
	for index, value := range values {
		if predicate(value, float64(index)) {
			return value
		}
	}
	return nil
}

func tsgodownStringArrayFind(values []string, predicate func(any, float64) bool, reverse bool) any {
	if reverse {
		for index := len(values) - 1; index >= 0; index-- {
			value := values[index]
			if predicate(value, float64(index)) {
				return value
			}
		}
		return nil
	}
	for index, value := range values {
		if predicate(value, float64(index)) {
			return value
		}
	}
	return nil
}

func tsgodownAnyArrayReduce(values []any, reducer func(any, any, float64) any, initial any) any {
	accumulator := initial
	for index, value := range values {
		accumulator = reducer(accumulator, value, float64(index))
	}
	return accumulator
}

func tsgodownAnyArrayReduceRight(values []any, reducer func(any, any, float64) any, initial any) any {
	accumulator := initial
	for index := len(values) - 1; index >= 0; index-- {
		accumulator = reducer(accumulator, values[index], float64(index))
	}
	return accumulator
}

func tsgodownStringArrayReduce(values []string, reducer func(any, any, float64) any, initial any) any {
	accumulator := initial
	for index, value := range values {
		accumulator = reducer(accumulator, value, float64(index))
	}
	return accumulator
}

func tsgodownStringArrayReduceRight(values []string, reducer func(any, any, float64) any, initial any) any {
	accumulator := initial
	for index := len(values) - 1; index >= 0; index-- {
		accumulator = reducer(accumulator, values[index], float64(index))
	}
	return accumulator
}

func tsgodownAnyArrayFindIndex(values []any, predicate func(any, float64) bool, reverse bool) float64 {
	if reverse {
		for index := len(values) - 1; index >= 0; index-- {
			if predicate(values[index], float64(index)) {
				return float64(index)
			}
		}
		return -1
	}
	for index, value := range values {
		if predicate(value, float64(index)) {
			return float64(index)
		}
	}
	return -1
}

func tsgodownStringArrayFindIndex(values []string, predicate func(any, float64) bool, reverse bool) float64 {
	if reverse {
		for index := len(values) - 1; index >= 0; index-- {
			if predicate(values[index], float64(index)) {
				return float64(index)
			}
		}
		return -1
	}
	for index, value := range values {
		if predicate(value, float64(index)) {
			return float64(index)
		}
	}
	return -1
}

func tsgodownAnyArrayFlat(values []any, depth float64) []any {
	limit := int(depth)
	if limit < 0 {
		limit = 0
	}
	var flatten func([]any, int) []any
	flatten = func(items []any, currentDepth int) []any {
		out := []any{}
		for _, item := range items {
			if currentDepth > 0 {
				if nested, ok := item.([]any); ok {
					out = append(out, flatten(nested, currentDepth-1)...)
					continue
				}
			}
			out = append(out, item)
		}
		return out
	}
	return flatten(values, limit)
}

func tsgodownAnyArrayFill(values []any, value any, indexes ...float64) []any {
	length := len(values)
	start := 0
	end := length
	if len(indexes) > 0 {
		start = int(indexes[0])
		if start < 0 {
			start = length + start
		}
	}
	if len(indexes) > 1 {
		end = int(indexes[1])
		if end < 0 {
			end = length + end
		}
	}
	if start < 0 {
		start = 0
	}
	if start > length {
		start = length
	}
	if end < 0 {
		end = 0
	}
	if end > length {
		end = length
	}
	for index := start; index < end; index++ {
		values[index] = value
	}
	return values
}

func tsgodownAnyArraySlice(values []any, start float64, endValues ...float64) []any {
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
	return append([]any(nil), values[from:to]...)
}

func tsgodownAnyArrayFromAny(value any) []any {
	switch values := value.(type) {
	case []any:
		out := make([]any, len(values))
		copy(out, values)
		return out
	case []string:
		out := make([]any, len(values))
		for index, item := range values {
			out[index] = item
		}
		return out
	case []float64:
		out := make([]any, len(values))
		for index, item := range values {
			out[index] = item
		}
		return out
	case []byte:
		out := make([]any, len(values))
		for index, item := range values {
			out[index] = float64(item)
		}
		return out
	default:
		return []any{}
	}
}

func tsgodownBytesFromAny(value any) []byte {
	switch values := value.(type) {
	case []byte:
		out := make([]byte, len(values))
		copy(out, values)
		return out
	case []any:
		out := make([]byte, len(values))
		for index, item := range values {
			out[index] = byte(tsgodownToFloat64(item))
		}
		return out
	case []float64:
		out := make([]byte, len(values))
		for index, item := range values {
			out[index] = byte(item)
		}
		return out
	default:
		return []byte{}
	}
}

func tsgodownAnyArrayConcatBase(value any) []any {
	switch values := value.(type) {
	case []any:
		out := make([]any, len(values))
		copy(out, values)
		return out
	case []string:
		out := make([]any, len(values))
		for index, item := range values {
			out[index] = item
		}
		return out
	case []float64:
		out := make([]any, len(values))
		for index, item := range values {
			out[index] = item
		}
		return out
	default:
		return []any{value}
	}
}

func tsgodownAnyArrayConcat(base []any, values ...any) []any {
	out := make([]any, len(base))
	copy(out, base)
	for _, value := range values {
		switch items := value.(type) {
		case []any:
			out = append(out, items...)
		case []string:
			for _, item := range items {
				out = append(out, item)
			}
		case []float64:
			for _, item := range items {
				out = append(out, item)
			}
		default:
			out = append(out, value)
		}
	}
	return out
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

func tsgodownAnyArrayPop(values *[]any) any {
	if values == nil || len(*values) == 0 {
		return nil
	}
	offset := len(*values) - 1
	value := (*values)[offset]
	*values = (*values)[:offset]
	return value
}

func tsgodownAnyArrayPopValue(values []any) any {
	if len(values) == 0 {
		return nil
	}
	return values[len(values)-1]
}

func tsgodownStringArrayPop(values *[]string) string {
	if values == nil || len(*values) == 0 {
		return ""
	}
	offset := len(*values) - 1
	value := (*values)[offset]
	*values = (*values)[:offset]
	return value
}

func tsgodownStringArrayPopValue(values []string) string {
	if len(values) == 0 {
		return ""
	}
	return values[len(values)-1]
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

func tsgodownURIHexNibble(value byte) (byte, bool) {
	switch {
	case value >= '0' && value <= '9':
		return value - '0', true
	case value >= 'A' && value <= 'F':
		return value - 'A' + 10, true
	case value >= 'a' && value <= 'f':
		return value - 'a' + 10, true
	default:
		return 0, false
	}
}

func tsgodownEncodeURIComponent(value string) string {
	const hex = "0123456789ABCDEF"
	var out strings.Builder
	for _, item := range []byte(value) {
		if item >= 'A' && item <= 'Z' || item >= 'a' && item <= 'z' || item >= '0' && item <= '9' ||
			item == '-' || item == '_' || item == '.' || item == '!' || item == '~' || item == '*' || item == '\'' || item == '(' || item == ')' {
			out.WriteByte(item)
			continue
		}
		out.WriteByte('%')
		out.WriteByte(hex[item>>4])
		out.WriteByte(hex[item&0x0f])
	}
	return out.String()
}

func tsgodownUnescape(value string) string {
	var out strings.Builder
	for index := 0; index < len(value); index++ {
		if value[index] != '%' {
			out.WriteByte(value[index])
			continue
		}
		if index+5 < len(value) && value[index+1] == 'u' {
			a, okA := tsgodownURIHexNibble(value[index+2])
			b, okB := tsgodownURIHexNibble(value[index+3])
			c, okC := tsgodownURIHexNibble(value[index+4])
			d, okD := tsgodownURIHexNibble(value[index+5])
			if okA && okB && okC && okD {
				out.WriteRune(rune(a)<<12 | rune(b)<<8 | rune(c)<<4 | rune(d))
				index += 5
				continue
			}
		}
		if index+2 < len(value) {
			a, okA := tsgodownURIHexNibble(value[index+1])
			b, okB := tsgodownURIHexNibble(value[index+2])
			if okA && okB {
				out.WriteRune(rune(a<<4 | b))
				index += 2
				continue
			}
		}
		out.WriteByte(value[index])
	}
	return out.String()
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

func tsgodownStringSubstring(value string, start float64, end float64) string {
	chars := []rune(value)
	length := len(chars)
	from := int(start)
	to := int(end)
	if from < 0 {
		from = 0
	}
	if to < 0 {
		to = 0
	}
	if from > length {
		from = length
	}
	if to > length {
		to = length
	}
	if from > to {
		from, to = to, from
	}
	return string(chars[from:to])
}

func tsgodownStringSubstr(value string, start float64, lengthValues ...float64) string {
	chars := []rune(value)
	total := len(chars)
	from := int(start)
	if from < 0 {
		from = total + from
	}
	if from < 0 {
		from = 0
	}
	if from > total {
		from = total
	}
	to := total
	if len(lengthValues) > 0 {
		length := int(lengthValues[0])
		if length < 0 {
			length = 0
		}
		to = from + length
		if to > total {
			to = total
		}
	}
	return string(chars[from:to])
}

func tsgodownStringLastIndexOf(value string, needle string, startValues ...float64) float64 {
	search := value
	if len(startValues) > 0 {
		chars := []rune(value)
		offset := int(startValues[0])
		if offset < 0 {
			return -1
		}
		if offset >= len(chars) {
			offset = len(chars) - 1
		}
		search = string(chars[:offset+1])
	}
	found := strings.LastIndex(search, needle)
	if found < 0 {
		return -1
	}
	return float64(found)
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

func tsgodownSameValueStrict(left any, right any) bool {
	switch leftValue := left.(type) {
	case nil:
		return right == nil
	case bool:
		rightValue, ok := right.(bool)
		return ok && leftValue == rightValue
	case float64:
		rightValue, ok := right.(float64)
		return ok && leftValue == rightValue
	case int:
		rightValue, ok := right.(int)
		return ok && leftValue == rightValue
	case int64:
		rightValue, ok := right.(int64)
		return ok && leftValue == rightValue
	case string:
		rightValue, ok := right.(string)
		return ok && leftValue == rightValue
	default:
		return tsgodownComparableEqual(left, right)
	}
}

func tsgodownComparableEqual(left any, right any) (equal bool) {
	defer func() {
		if recover() != nil {
			equal = false
		}
	}()
	return left == right
}

func tsgodownDeepStrictEqual(left any, right any) bool {
	if tsgodownSameValueStrict(left, right) {
		return true
	}
	switch leftValue := left.(type) {
	case map[string]any:
		rightValue, ok := right.(map[string]any)
		if !ok || len(leftValue) != len(rightValue) {
			return false
		}
		for key, leftItem := range leftValue {
			rightItem, ok := rightValue[key]
			if !ok || !tsgodownDeepStrictEqual(leftItem, rightItem) {
				return false
			}
		}
		return true
	case []any:
		rightValue, ok := right.([]any)
		if !ok || len(leftValue) != len(rightValue) {
			return false
		}
		for index, leftItem := range leftValue {
			if !tsgodownDeepStrictEqual(leftItem, rightValue[index]) {
				return false
			}
		}
		return true
	default:
		return false
	}
}

func tsgodownAssert(condition bool) {
	if !condition {
		panic("AssertionError")
	}
}

func tsgodownObjectFromAny(value any) map[string]any {
	switch value := value.(type) {
	case nil:
		return map[string]any{}
	case map[string]any:
		return value
	case tsgodownError:
		return map[string]any{"name": value.Name, "message": value.Message, "code": value.Code}
	default:
		return map[string]any{}
	}
}

func tsgodownObjectAssignKeys(groups ...[]string) []string {
	seen := map[string]bool{}
	out := []string{}
	for _, group := range groups {
		for _, key := range group {
			if seen[key] {
				continue
			}
			seen[key] = true
			out = append(out, key)
		}
	}
	return out
}

func tsgodownObjectProp(value any, key string) any {
	switch typed := value.(type) {
	case map[string]any:
		return typed[key]
	case []any:
		if key == "length" {
			return float64(len(typed))
		}
		index, err := strconv.Atoi(key)
		if err == nil && index >= 0 && index < len(typed) {
			return typed[index]
		}
		return nil
	case []string:
		if key == "length" {
			return float64(len(typed))
		}
		index, err := strconv.Atoi(key)
		if err == nil && index >= 0 && index < len(typed) {
			return typed[index]
		}
		return nil
	case []float64:
		if key == "length" {
			return float64(len(typed))
		}
		index, err := strconv.Atoi(key)
		if err == nil && index >= 0 && index < len(typed) {
			return typed[index]
		}
		return nil
	case []byte:
		if key == "length" {
			return float64(len(typed))
		}
		index, err := strconv.Atoi(key)
		if err == nil && index >= 0 && index < len(typed) {
			return float64(typed[index])
		}
		return nil
	}
	object := tsgodownObjectFromAny(value)
	return object[key]
}

func tsgodownNullish(value any, fallback any) any {
	if value == nil {
		return fallback
	}
	return value
}

func tsgodownOptionalCallMember(value any, key string, args ...any) any {
	member := tsgodownObjectProp(value, key)
	if member == nil {
		return nil
	}
	return tsgodownCall(member, args...)
}

func tsgodownObjectSetProp(value any, key string, propertyValue any) {
	object := tsgodownObjectFromAny(value)
	object[key] = propertyValue
}

func tsgodownObjectDelete(value any, key string) bool {
	object := tsgodownObjectFromAny(value)
	delete(object, key)
	return true
}

func tsgodownPostIncFloat(target *float64) float64 {
	old := *target
	*target = old + 1
	return old
}

func tsgodownObjectPostInc(value any, key string) float64 {
	object := tsgodownObjectFromAny(value)
	old := tsgodownToFloat64(object[key])
	object[key] = old + 1
	return old
}

func tsgodownDateNow() float64 {
	return float64(time.Now().UnixMilli())
}

type tsgodownJSMap struct {
	order []string
	items map[string]any
}

type tsgodownJSSet struct {
	order []string
	items map[string]any
}

func tsgodownNewMap() *tsgodownJSMap {
	return &tsgodownJSMap{order: []string{}, items: map[string]any{}}
}

func tsgodownNewSet(values []any) *tsgodownJSSet {
	target := &tsgodownJSSet{order: []string{}, items: map[string]any{}}
	for _, value := range values {
		tsgodownSetAdd(target, value)
	}
	return target
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

func tsgodownMapKeys(target *tsgodownJSMap) []any {
	if target == nil {
		return []any{}
	}
	values := make([]any, 0, len(target.order))
	for _, key := range target.order {
		if _, ok := target.items[key]; ok {
			values = append(values, key)
		}
	}
	return values
}

func tsgodownMapValues(target *tsgodownJSMap) []any {
	if target == nil {
		return []any{}
	}
	values := make([]any, 0, len(target.order))
	for _, key := range target.order {
		if value, ok := target.items[key]; ok {
			values = append(values, value)
		}
	}
	return values
}

func tsgodownSetKey(value any) string {
	return fmt.Sprintf("%T:%v", value, value)
}

func tsgodownSetAdd(target *tsgodownJSSet, value any) *tsgodownJSSet {
	if target == nil {
		target = tsgodownNewSet(nil)
	}
	key := tsgodownSetKey(value)
	if _, ok := target.items[key]; !ok {
		target.order = append(target.order, key)
	}
	target.items[key] = value
	return target
}

func tsgodownSetValues(target *tsgodownJSSet) []any {
	if target == nil {
		return []any{}
	}
	values := make([]any, 0, len(target.order))
	for _, key := range target.order {
		if value, ok := target.items[key]; ok {
			values = append(values, value)
		}
	}
	return values
}

func tsgodownSetSize(target *tsgodownJSSet) float64 {
	if target == nil {
		return 0
	}
	return float64(len(target.items))
}

func tsgodownIteratorFirstValue(values []any) any {
	if len(values) == 0 {
		return nil
	}
	return values[0]
}

func tsgodownCall(value any, args ...any) any {
	switch fn := value.(type) {
	case func(...any) any:
		return fn(args...)
	case func() any:
		if len(args) == 0 {
			return fn()
		}
	case func(any) any:
		if len(args) == 1 {
			return fn(args[0])
		}
	case func(string) any:
		if len(args) == 1 {
			return fn(tsgodownToString(args[0]))
		}
	case func(any, any) any:
		if len(args) == 2 {
			return fn(args[0], args[1])
		}
	}
	return nil
}
"#
    .to_string()];
    if imports.contains("encoding/json") {
        helpers.push(
            r#"func tsgodownJSONStringify(value any) string {
	bytes, err := json.Marshal(value)
	if err != nil {
		return ""
	}
	return string(bytes)
}

func tsgodownJSONStringifyOrdered(object map[string]any, keys []string) string {
	out := "{"
	first := true
	seen := map[string]bool{}
	write := func(key string) {
		value, ok := object[key]
		if !ok || seen[key] {
			return
		}
		seen[key] = true
		if !first {
			out += ","
		}
		first = false
		keyBytes, keyErr := json.Marshal(key)
		valueBytes, valueErr := json.Marshal(value)
		if keyErr != nil || valueErr != nil {
			return
		}
		out += string(keyBytes) + ":" + string(valueBytes)
	}
	for _, key := range keys {
		write(key)
	}
	for _, key := range tsgodownObjectMapKeys(object) {
		write(key)
	}
	return out + "}"
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

func tsgodownBufferFromAny(value any, encoding string) []byte {
	switch typed := value.(type) {
	case []byte:
		return append([]byte(nil), typed...)
	case []any:
		out := make([]byte, len(typed))
		for index, item := range typed {
			out[index] = byte(tsgodownToFloat64(item))
		}
		return out
	case []float64:
		out := make([]byte, len(typed))
		for index, item := range typed {
			out[index] = byte(item)
		}
		return out
	case []string:
		out := make([]byte, len(typed))
		for index, item := range typed {
			out[index] = byte(tsgodownToFloat64(item))
		}
		return out
	case string:
		return tsgodownBufferFromString(typed, encoding)
	default:
		return tsgodownBufferFromString(tsgodownToString(value), encoding)
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
    helpers.push(
        r#"type tsgodownError struct {
	Name string
	Message string
	Code string
}

func (err tsgodownError) Error() string {
	if err.Name == "" {
		return err.Message
	}
	if err.Message == "" {
		return err.Name
	}
	return err.Name + ": " + err.Message
}

func tsgodownNewError(name string, message string) map[string]any {
	return map[string]any{"name": name, "message": message}
}

func tsgodownErrorStringProp(object map[string]any, key string) string {
	value, ok := object[key].(string)
	if ok {
		return value
	}
	return ""
}

func tsgodownErrorFromAny(value any) tsgodownError {
	switch value := value.(type) {
	case tsgodownError:
		return value
	case map[string]any:
		name := tsgodownErrorStringProp(value, "name")
		if name == "" {
			name = "Error"
		}
		return tsgodownError{Name: name, Message: tsgodownErrorStringProp(value, "message"), Code: tsgodownErrorStringProp(value, "code")}
	case error:
		return tsgodownError{Name: "Error", Message: value.Error()}
	case string:
		return tsgodownError{Name: "Error", Message: value}
	default:
		return tsgodownError{Name: "Error"}
	}
}

func tsgodownCaughtError(value any) map[string]any {
	if object, ok := value.(map[string]any); ok {
		if _, ok := object["name"]; !ok {
			object["name"] = "Error"
		}
		if _, ok := object["message"]; !ok {
			object["message"] = ""
		}
		return object
	}
	err := tsgodownErrorFromAny(value)
	object := map[string]any{"name": err.Name, "message": err.Message}
	if err.Code != "" {
		object["code"] = err.Code
	}
	return object
}

func tsgodownCaughtValue(value any) any {
	switch value.(type) {
	case nil, bool, float64, int, int64, string:
		return value
	default:
		return tsgodownCaughtError(value)
	}
}

func tsgodownThrow(value any) {
	panic(value)
}

func tsgodownErrorInstanceOf(value any, constructor string) bool {
	err := tsgodownErrorFromAny(value)
	if constructor == "Error" {
		return err.Name == "Error" || err.Name == "TypeError" || err.Name == "RangeError"
	}
	return err.Name == constructor
}

func tsgodownRequire(spec string) any {
	panic(tsgodownError{Name: "Error", Message: "Cannot find module '" + spec + "'"})
}
"#
        .to_string(),
    );
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
    if imports.contains("crypto/rand") && imports.contains("encoding/hex") {
        helpers.push(
            r#"func tsgodownCryptoRandomUUID() string {
	bytes := make([]byte, 16)
	if _, err := rand.Read(bytes); err != nil {
		return ""
	}
	bytes[6] = (bytes[6] & 0x0f) | 0x40
	bytes[8] = (bytes[8] & 0x3f) | 0x80
	text := hex.EncodeToString(bytes)
	return text[0:8] + "-" + text[8:12] + "-" + text[12:16] + "-" + text[16:20] + "-" + text[20:32]
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
    if imports.contains("os") && imports.contains("path/filepath") && imports.contains("sort") {
        helpers.push(
            r#"func tsgodownFsReaddirSync(path string) []string {
	entries, err := os.ReadDir(path)
	if err != nil {
		return []string{}
	}
	names := make([]string, 0, len(entries))
	for _, entry := range entries {
		names = append(names, entry.Name())
	}
	sort.Strings(names)
	return names
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
	case tsgodownSymbol:
		return tsgodownSymbolToString(value)
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
		text := strings.TrimSpace(value)
		if text == "" {
			return 0
		}
		sign := 1.0
		if strings.HasPrefix(text, "-") || strings.HasPrefix(text, "+") {
			if text[0] == '-' {
				sign = -1
			}
			text = text[1:]
		}
		lower := strings.ToLower(text)
		switch {
		case strings.HasPrefix(lower, "0b"):
			parsed, err := strconv.ParseInt(lower[2:], 2, 64)
			if err != nil {
				return 0
			}
			return sign * float64(parsed)
		case strings.HasPrefix(lower, "0o"):
			parsed, err := strconv.ParseInt(lower[2:], 8, 64)
			if err != nil {
				return 0
			}
			return sign * float64(parsed)
		case strings.HasPrefix(lower, "0x"):
			parsed, err := strconv.ParseInt(lower[2:], 16, 64)
			if err != nil {
				return 0
			}
			return sign * float64(parsed)
		}
		number, err := strconv.ParseFloat(text, 64)
		if err != nil {
			return 0
		}
		return sign * number
	default:
		return 0
	}
}

func tsgodownAdd(left any, right any) any {
	if _, ok := left.(string); ok {
		return tsgodownToString(left) + tsgodownToString(right)
	}
	if _, ok := right.(string); ok {
		return tsgodownToString(left) + tsgodownToString(right)
	}
	return tsgodownToFloat64(left) + tsgodownToFloat64(right)
}

func tsgodownToUint32(value float64) uint32 {
	if math.IsNaN(value) || math.IsInf(value, 0) || value == 0 {
		return 0
	}
	truncated := math.Trunc(value)
	modulo := math.Mod(truncated, 4294967296)
	return uint32(int64(modulo))
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

func tsgodownObjectHasOwn(value any, key any) bool {
	name := tsgodownToString(key)
	switch object := value.(type) {
	case map[string]any:
		_, ok := object[name]
		return ok
	case []any:
		if name == "length" {
			return true
		}
		index, err := strconv.Atoi(name)
		return err == nil && index >= 0 && index < len(object)
	case []string:
		if name == "length" {
			return true
		}
		index, err := strconv.Atoi(name)
		return err == nil && index >= 0 && index < len(object)
	case []float64:
		if name == "length" {
			return true
		}
		index, err := strconv.Atoi(name)
		return err == nil && index >= 0 && index < len(object)
	case []byte:
		if name == "length" {
			return true
		}
		index, err := strconv.Atoi(name)
		return err == nil && index >= 0 && index < len(object)
	default:
		return false
	}
}

func tsgodownArrayHasOwn(length int, props map[string]any, key any) bool {
	name := tsgodownToString(key)
	if name == "length" {
		return true
	}
	index, err := strconv.Atoi(name)
	if err == nil && index >= 0 && index < length {
		return true
	}
	_, ok := props[name]
	return ok
}

func tsgodownObjectPrototypeHasOwn(key any) bool {
	switch tsgodownToString(key) {
	case "constructor", "__defineGetter__", "__defineSetter__", "hasOwnProperty", "__lookupGetter__", "__lookupSetter__", "isPrototypeOf", "propertyIsEnumerable", "toString", "valueOf", "__proto__", "toLocaleString":
		return true
	default:
		return false
	}
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

func tsgodownAnyArrayJoin(values []any, separator string) string {
	parts := make([]string, len(values))
	for index, item := range values {
		if item == nil {
			parts[index] = ""
			continue
		}
		parts[index] = tsgodownToString(item)
	}
	return strings.Join(parts, separator)
}
"#
            .to_string(),
        );
    }
    if imports.contains("math") {
        helpers.push(
            r#"func tsgodownLengthFloat(value any, optional bool) float64 {
	if value == nil {
		if optional {
			return math.NaN()
		}
		return 0
	}
	switch typed := value.(type) {
	case []byte:
		return float64(len(typed))
	case []any:
		return float64(len(typed))
	case []float64:
		return float64(len(typed))
	case []string:
		return float64(len(typed))
	case string:
		return float64(len([]rune(typed)))
	default:
		return float64(len([]rune(tsgodownToString(value))))
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

func tsgodownArrayObjectKeys(length int, propKeys []string) []string {
	keys := make([]string, 0, length+len(propKeys))
	for index := 0; index < length; index++ {
		keys = append(keys, strconv.Itoa(index))
	}
	keys = append(keys, propKeys...)
	return tsgodownObjectKeys(keys)
}

func tsgodownStringArraySort(values []string) []string {
	out := append([]string(nil), values...)
	sort.Strings(out)
	return out
}

func tsgodownObjectMapKeys(value any) []string {
	switch typed := value.(type) {
	case []any:
		keys := make([]string, 0, len(typed))
		for index := range typed {
			keys = append(keys, strconv.Itoa(index))
		}
		return tsgodownObjectKeys(keys)
	case []string:
		keys := make([]string, 0, len(typed))
		for index := range typed {
			keys = append(keys, strconv.Itoa(index))
		}
		return tsgodownObjectKeys(keys)
	case []float64:
		keys := make([]string, 0, len(typed))
		for index := range typed {
			keys = append(keys, strconv.Itoa(index))
		}
		return tsgodownObjectKeys(keys)
	case []byte:
		keys := make([]string, 0, len(typed))
		for index := range typed {
			keys = append(keys, strconv.Itoa(index))
		}
		return tsgodownObjectKeys(keys)
	}
	object := tsgodownObjectFromAny(value)
	keys := make([]string, 0, len(object))
	for key := range object {
	keys = append(keys, key)
	}
	return tsgodownObjectKeys(keys)
}

func tsgodownObjectEntries(value any) []any {
	object := tsgodownObjectFromAny(value)
	keys := tsgodownObjectMapKeys(object)
	entries := make([]any, 0, len(keys))
	for _, key := range keys {
		entries = append(entries, []any{key, object[key]})
	}
	return entries
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

func tsgodownRegExpExec(pattern string, value string, cursor *int) any {
	re := regexp.MustCompile(pattern)
	if *cursor < 0 {
		*cursor = 0
	}
	if *cursor > len(value) {
		return nil
	}
	offset := *cursor
	indexes := re.FindStringSubmatchIndex(value[offset:])
	if indexes == nil {
		*cursor = 0
		return nil
	}
	matchStart := offset + indexes[0]
	matchEnd := offset + indexes[1]
	if matchEnd == matchStart {
		*cursor = matchEnd + 1
	} else {
		*cursor = matchEnd
	}
	out := make([]any, len(indexes)/2)
	for i := 0; i < len(indexes); i += 2 {
		if indexes[i] < 0 || indexes[i+1] < 0 {
			out[i/2] = nil
			continue
		}
		out[i/2] = value[offset+indexes[i]:offset+indexes[i+1]]
	}
	return out
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

func tsgodownRegexpReplaceBackref(value string, pattern string, replacement string, global bool) string {
	captures, ok := tsgodownAnchoredDelimiterBackrefCaptures(value, pattern)
	if !ok {
		return value
	}
	return tsgodownExpandRegexpReplacement(replacement, captures)
}

func tsgodownAnchoredDelimiterBackrefCaptures(value string, pattern string) ([]string, bool) {
	if strings.HasPrefix(pattern, "(?") {
		end := strings.Index(pattern, ")")
		if end < 0 {
			return nil, false
		}
		pattern = pattern[end+1:]
	}
	const prefix = "^(["
	const suffix = "])([\\s\\S]*)\\1$"
	if !strings.HasPrefix(pattern, prefix) || !strings.HasSuffix(pattern, suffix) {
		return nil, false
	}
	class := pattern[len(prefix) : len(pattern)-len(suffix)]
	runes := []rune(value)
	if len(runes) < 2 {
		return nil, false
	}
	first := runes[0]
	last := runes[len(runes)-1]
	if first != last || !strings.ContainsRune(class, first) {
		return nil, false
	}
	inner := string(runes[1 : len(runes)-1])
	return []string{value, string(first), inner}, true
}

func tsgodownExpandRegexpReplacement(replacement string, captures []string) string {
	var out strings.Builder
	for index := 0; index < len(replacement); index++ {
		ch := replacement[index]
		if ch != '$' || index+1 >= len(replacement) {
			out.WriteByte(ch)
			continue
		}
		next := replacement[index+1]
		if next == '$' {
			out.WriteByte('$')
			index++
			continue
		}
		if next == '&' {
			out.WriteString(captures[0])
			index++
			continue
		}
		if next < '0' || next > '9' {
			out.WriteByte(ch)
			continue
		}
		captureIndex := int(next - '0')
		index++
		if index+1 < len(replacement) {
			following := replacement[index+1]
			if following >= '0' && following <= '9' {
				twoDigit := captureIndex*10 + int(following-'0')
				if twoDigit < len(captures) {
					captureIndex = twoDigit
					index++
				}
			}
		}
		if captureIndex < len(captures) {
			out.WriteString(captures[captureIndex])
		}
	}
	return out.String()
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
                module_has_caught_require_spec(module, &import.spec)
                    || matches!(import.kind.as_str(), "esm" | "cjs" | "dynamic")
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
        let builtin_aliases = collect_builtin_function_aliases(&executable.stmts);
        for stmt in &executable.stmts {
            if let Some(parts) = function_parts(stmt) {
                let go_name = function_go_name(module, entry, parts.name);
                functions.insert(
                    (module.id.clone(), parts.name.clone()),
                    AotFunction {
                        params: parts.params.clone(),
                        param_kinds: infer_function_param_kinds(
                            parts.params,
                            parts.body,
                            &builtin_aliases,
                        ),
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
                        param_kinds: infer_function_param_kinds(
                            parts.params,
                            parts.body,
                            &builtin_aliases,
                        ),
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
    let export_aliases = collect_module_export_aliases(ir);
    let imported_function_aliases =
        collect_imported_function_aliases(ir, &functions, &export_aliases);
    propagate_function_param_kinds(&mut functions, &imported_function_aliases);
    functions
}

fn collect_builtin_function_aliases(stmts: &[JsStmt]) -> BTreeMap<String, AotBuiltinFunctionAlias> {
    let mut aliases = BTreeMap::new();
    for stmt in stmts {
        if let JsStmt::VarDecl {
            name,
            init: Some(expr),
        } = stmt
        {
            if let Some(alias) = builtin_function_alias(expr) {
                aliases.insert(name.clone(), alias);
            }
        }
    }
    aliases
}

fn collect_imported_function_aliases(
    ir: &IrDocument,
    functions: &BTreeMap<(String, String), AotFunction>,
    export_aliases: &BTreeMap<(String, String), String>,
) -> BTreeMap<(String, String), (String, String)> {
    let mut aliases = BTreeMap::new();
    for module in &ir.modules {
        for import in &module.imports {
            let Some(resolved) = &import.resolved else {
                continue;
            };
            for binding in &import.bindings {
                let imported = binding.imported.as_deref().unwrap_or(&binding.local);
                let mut candidates = Vec::new();
                candidates.push(imported.to_string());
                if let Some(local) = export_aliases.get(&(resolved.clone(), imported.to_string())) {
                    candidates.push(local.clone());
                }
                if imported == "default" {
                    candidates.push(CJS_DEFAULT_EXPORT_FUNCTION.to_string());
                }
                for candidate in candidates {
                    let key = (resolved.clone(), candidate);
                    if functions.contains_key(&key) {
                        aliases.insert((module.id.clone(), binding.local.clone()), key);
                        break;
                    }
                }
            }
        }
    }
    aliases
}

fn propagate_function_param_kinds(
    functions: &mut BTreeMap<(String, String), AotFunction>,
    imported_function_aliases: &BTreeMap<(String, String), (String, String)>,
) {
    for _ in 0..functions.len().max(1) {
        let snapshot = functions.clone();
        let mut changed = false;
        for ((module_id, _name), function) in functions.iter_mut() {
            let param_index = function
                .params
                .iter()
                .enumerate()
                .map(|(index, param)| (param.clone(), index))
                .collect::<BTreeMap<_, _>>();
            let mut propagated = function.param_kinds.clone();
            for stmt in &function.body {
                propagate_stmt_param_kinds(
                    stmt,
                    module_id,
                    &snapshot,
                    imported_function_aliases,
                    &param_index,
                    &mut propagated,
                );
            }
            if propagated != function.param_kinds {
                function.param_kinds = propagated;
                changed = true;
            }
        }
        if !changed {
            break;
        }
    }
}

fn propagate_stmt_param_kinds(
    stmt: &JsStmt,
    module_id: &str,
    functions: &BTreeMap<(String, String), AotFunction>,
    imported_function_aliases: &BTreeMap<(String, String), (String, String)>,
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
        } => propagate_expr_param_kinds(
            expr,
            module_id,
            functions,
            imported_function_aliases,
            param_index,
            kinds,
        ),
        JsStmt::If {
            test,
            consequent,
            alternate,
        } => {
            propagate_expr_param_kinds(
                test,
                module_id,
                functions,
                imported_function_aliases,
                param_index,
                kinds,
            );
            for stmt in consequent {
                propagate_stmt_param_kinds(
                    stmt,
                    module_id,
                    functions,
                    imported_function_aliases,
                    param_index,
                    kinds,
                );
            }
            for stmt in alternate {
                propagate_stmt_param_kinds(
                    stmt,
                    module_id,
                    functions,
                    imported_function_aliases,
                    param_index,
                    kinds,
                );
            }
        }
        JsStmt::For {
            init,
            test,
            update,
            body,
        } => {
            for stmt in init {
                propagate_stmt_param_kinds(
                    stmt,
                    module_id,
                    functions,
                    imported_function_aliases,
                    param_index,
                    kinds,
                );
            }
            if let Some(test) = test {
                propagate_expr_param_kinds(
                    test,
                    module_id,
                    functions,
                    imported_function_aliases,
                    param_index,
                    kinds,
                );
            }
            if let Some(update) = update {
                propagate_expr_param_kinds(
                    update,
                    module_id,
                    functions,
                    imported_function_aliases,
                    param_index,
                    kinds,
                );
            }
            for stmt in body {
                propagate_stmt_param_kinds(
                    stmt,
                    module_id,
                    functions,
                    imported_function_aliases,
                    param_index,
                    kinds,
                );
            }
        }
        JsStmt::ForOf { right, body, .. } => {
            propagate_expr_param_kinds(
                right,
                module_id,
                functions,
                imported_function_aliases,
                param_index,
                kinds,
            );
            for stmt in body {
                propagate_stmt_param_kinds(
                    stmt,
                    module_id,
                    functions,
                    imported_function_aliases,
                    param_index,
                    kinds,
                );
            }
        }
        JsStmt::While { test, body } | JsStmt::DoWhile { test, body } => {
            propagate_expr_param_kinds(
                test,
                module_id,
                functions,
                imported_function_aliases,
                param_index,
                kinds,
            );
            for stmt in body {
                propagate_stmt_param_kinds(
                    stmt,
                    module_id,
                    functions,
                    imported_function_aliases,
                    param_index,
                    kinds,
                );
            }
        }
        JsStmt::Try {
            body,
            catch_body,
            finally_body,
            ..
        } => {
            for stmt in body {
                propagate_stmt_param_kinds(
                    stmt,
                    module_id,
                    functions,
                    imported_function_aliases,
                    param_index,
                    kinds,
                );
            }
            for stmt in catch_body {
                propagate_stmt_param_kinds(
                    stmt,
                    module_id,
                    functions,
                    imported_function_aliases,
                    param_index,
                    kinds,
                );
            }
            for stmt in finally_body {
                propagate_stmt_param_kinds(
                    stmt,
                    module_id,
                    functions,
                    imported_function_aliases,
                    param_index,
                    kinds,
                );
            }
        }
        _ => {}
    }
}

fn propagate_expr_param_kinds(
    expr: &JsExpr,
    module_id: &str,
    functions: &BTreeMap<(String, String), AotFunction>,
    imported_function_aliases: &BTreeMap<(String, String), (String, String)>,
    param_index: &BTreeMap<String, usize>,
    kinds: &mut [AotSlotKind],
) {
    match expr {
        JsExpr::Call { callee, args, .. } => {
            if let JsExpr::Ident { name } = callee.as_ref() {
                if let Some(callee_function) = functions.get(&(module_id.to_string(), name.clone()))
                {
                    for (arg, required) in args.iter().zip(callee_function.param_kinds.iter()) {
                        mark_forwarded_param_kind(arg, *required, param_index, kinds);
                    }
                }
                if let Some(target_key) =
                    imported_function_aliases.get(&(module_id.to_string(), name.clone()))
                {
                    if let Some(callee_function) = functions.get(target_key) {
                        for (arg, required) in args.iter().zip(callee_function.param_kinds.iter()) {
                            mark_forwarded_param_kind(arg, *required, param_index, kinds);
                        }
                    }
                }
            }
            propagate_expr_param_kinds(
                callee,
                module_id,
                functions,
                imported_function_aliases,
                param_index,
                kinds,
            );
            for arg in args {
                propagate_expr_param_kinds(
                    arg,
                    module_id,
                    functions,
                    imported_function_aliases,
                    param_index,
                    kinds,
                );
            }
        }
        JsExpr::Assign { left, right, .. } | JsExpr::Binary { left, right, .. } => {
            propagate_expr_param_kinds(
                left,
                module_id,
                functions,
                imported_function_aliases,
                param_index,
                kinds,
            );
            propagate_expr_param_kinds(
                right,
                module_id,
                functions,
                imported_function_aliases,
                param_index,
                kinds,
            );
        }
        JsExpr::Member {
            object,
            property_expr,
            ..
        } => {
            propagate_expr_param_kinds(
                object,
                module_id,
                functions,
                imported_function_aliases,
                param_index,
                kinds,
            );
            if let Some(property_expr) = property_expr {
                propagate_expr_param_kinds(
                    property_expr,
                    module_id,
                    functions,
                    imported_function_aliases,
                    param_index,
                    kinds,
                );
            }
        }
        JsExpr::Array { items } => {
            for item in items {
                propagate_expr_param_kinds(
                    item,
                    module_id,
                    functions,
                    imported_function_aliases,
                    param_index,
                    kinds,
                );
            }
        }
        JsExpr::ArraySpread { items } => {
            for item in items {
                propagate_expr_param_kinds(
                    &item.value,
                    module_id,
                    functions,
                    imported_function_aliases,
                    param_index,
                    kinds,
                );
            }
        }
        JsExpr::Object { props } => {
            for prop in props {
                propagate_expr_param_kinds(
                    &prop.value,
                    module_id,
                    functions,
                    imported_function_aliases,
                    param_index,
                    kinds,
                );
            }
        }
        JsExpr::Unary { arg, .. }
        | JsExpr::Await { arg }
        | JsExpr::Update { arg, .. }
        | JsExpr::Spread { arg }
        | JsExpr::ObjectRest { object: arg, .. } => {
            propagate_expr_param_kinds(
                arg,
                module_id,
                functions,
                imported_function_aliases,
                param_index,
                kinds,
            );
        }
        JsExpr::Conditional {
            test,
            consequent,
            alternate,
        } => {
            propagate_expr_param_kinds(
                test,
                module_id,
                functions,
                imported_function_aliases,
                param_index,
                kinds,
            );
            propagate_expr_param_kinds(
                consequent,
                module_id,
                functions,
                imported_function_aliases,
                param_index,
                kinds,
            );
            propagate_expr_param_kinds(
                alternate,
                module_id,
                functions,
                imported_function_aliases,
                param_index,
                kinds,
            );
        }
        JsExpr::New { callee, args } => {
            propagate_expr_param_kinds(
                callee,
                module_id,
                functions,
                imported_function_aliases,
                param_index,
                kinds,
            );
            for arg in args {
                propagate_expr_param_kinds(
                    arg,
                    module_id,
                    functions,
                    imported_function_aliases,
                    param_index,
                    kinds,
                );
            }
        }
        JsExpr::Template { exprs, .. } | JsExpr::Sequence { exprs } => {
            for expr in exprs {
                propagate_expr_param_kinds(
                    expr,
                    module_id,
                    functions,
                    imported_function_aliases,
                    param_index,
                    kinds,
                );
            }
        }
        JsExpr::Function { body, .. } => {
            for stmt in body {
                propagate_stmt_param_kinds(
                    stmt,
                    module_id,
                    functions,
                    imported_function_aliases,
                    param_index,
                    kinds,
                );
            }
        }
        _ => {}
    }
}

fn mark_forwarded_param_kind(
    expr: &JsExpr,
    required: AotSlotKind,
    param_index: &BTreeMap<String, usize>,
    kinds: &mut [AotSlotKind],
) {
    let JsExpr::Ident { name } = expr else {
        return;
    };
    let Some(index) = param_index.get(name) else {
        return;
    };
    if required == AotSlotKind::Any {
        kinds[*index] = AotSlotKind::Any;
        return;
    }
    if kinds[*index] == AotSlotKind::Number || kinds[*index] == required {
        kinds[*index] = required;
    }
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
        if let Some(class) = collect_function_constructor_class(module, entry, stmt) {
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
    let super_error_name = super_class
        .as_ref()
        .and_then(error_constructor_name)
        .map(str::to_string);
    if super_class.is_some() && super_error_name.is_none() {
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
    let mut constructor_body = Vec::new();
    for method in methods {
        if method.r#async || method.generator || method.rest_param.is_some() || method.is_static {
            return None;
        }
        if method.kind == "constructor" {
            for param in &method.params {
                constructor_params.push(param.clone());
            }
            if super_error_name.is_some() {
                constructor_body = method.body.clone();
                for stmt in &method.body {
                    if let Some((property, right)) = this_assignment(stmt) {
                        let kind = match right {
                            JsExpr::Value {
                                value: JsValue::String { .. },
                            } => AotSlotKind::String,
                            JsExpr::Value {
                                value: JsValue::Number { .. },
                            } => AotSlotKind::Number,
                            JsExpr::Value {
                                value: JsValue::Bool { .. },
                            } => AotSlotKind::Bool,
                            JsExpr::Ident { name } if method.params.contains(name) => {
                                AotSlotKind::Any
                            }
                            _ => AotSlotKind::Any,
                        };
                        fields.insert(property.clone(), kind);
                        constructor_values.push((property, right.clone()));
                    }
                }
                continue;
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
        constructor_body,
        methods: class_methods,
        getters: class_getters,
        super_error_name,
    })
}

fn collect_function_constructor_class(
    module: &Module,
    entry: &Module,
    stmt: &JsStmt,
) -> Option<AotClass> {
    let JsStmt::FunctionDecl {
        name,
        params,
        rest_param: None,
        r#async: false,
        generator: false,
        body,
    } = stmt
    else {
        return None;
    };
    if body.is_empty() {
        return None;
    }
    let go_name = if module.id == entry.id {
        sanitize_go_identifier(name)
    } else {
        module_member_go_name(module, name)
    };
    let mut fields = BTreeMap::new();
    let mut constructor_values = Vec::new();
    for stmt in body {
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
            JsExpr::Ident { name } if params.contains(name) => AotSlotKind::Any,
            _ => return None,
        };
        fields.insert(property.clone(), kind);
        constructor_values.push((property, right.as_ref().clone()));
    }
    Some(AotClass {
        name: name.clone(),
        go_name,
        fields,
        constructor_params: params.clone(),
        constructor_values,
        constructor_body: Vec::new(),
        methods: BTreeMap::new(),
        getters: BTreeMap::new(),
        super_error_name: None,
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
            let Some(expr) = cjs_default_export_value_expr(stmt) else {
                continue;
            };
            let function = match expr {
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
            if let JsStmt::VarDecl {
                name,
                init: Some(JsExpr::Ident { name: local }),
            } = stmt
            {
                if is_exported_name(module, name) && name != local {
                    aliases.insert((module.id.clone(), name.clone()), local.clone());
                }
                continue;
            }
            if let JsStmt::VarDecl {
                name,
                init: Some(init),
            } = stmt
            {
                if let Some(exported) = cjs_export_alias_assignment_name(init) {
                    aliases.insert((module.id.clone(), exported), name.clone());
                }
                continue;
            }
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
                let mut visited = BTreeSet::new();
                if let Some(object_exports) = module_exported_object_functions(
                    ir,
                    module,
                    right,
                    module_functions,
                    module_default_exports,
                    module_object_exports,
                    &mut visited,
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
    visited: &mut BTreeSet<String>,
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
            module_object_exports,
            visited,
        )),
        _ => None,
    }
}

fn collect_module_object_function_exports(
    ir: &IrDocument,
    module_functions: &BTreeMap<(String, String), AotFunction>,
    module_default_exports: &BTreeMap<String, AotFunction>,
) -> BTreeMap<(String, String), BTreeMap<String, AotFunction>> {
    let mut objects: BTreeMap<(String, String), BTreeMap<String, AotFunction>> = BTreeMap::new();
    for _ in 0..=ir.modules.len() {
        let mut changed = false;
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
                let mut visited = BTreeSet::new();
                let functions = collect_object_function_props(
                    ir,
                    module,
                    props,
                    module_functions,
                    module_default_exports,
                    &objects,
                    &mut visited,
                );
                if functions.is_empty() {
                    continue;
                }
                let key = (module.id.clone(), name.clone());
                let same_functions = objects
                    .get(&key)
                    .map(|existing| {
                        existing.len() == functions.len()
                            && existing.iter().all(|(name, existing_function)| {
                                functions
                                    .get(name)
                                    .map(|function| function.go_name == existing_function.go_name)
                                    .unwrap_or(false)
                            })
                    })
                    .unwrap_or(false);
                if !same_functions {
                    objects.insert(key, functions);
                    changed = true;
                }
            }
        }
        if !changed {
            break;
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
    module_object_exports: &BTreeMap<(String, String), BTreeMap<String, AotFunction>>,
    visited: &mut BTreeSet<String>,
) -> BTreeMap<String, AotFunction> {
    let mut functions = BTreeMap::new();
    for prop in props {
        if prop.key_expr.is_some() {
            continue;
        }
        if prop.spread {
            if let Some(spread_functions) = resolve_object_function_namespace(
                ir,
                module,
                &prop.value,
                module_functions,
                module_default_exports,
                module_object_exports,
                visited,
            ) {
                functions.extend(spread_functions);
            }
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

fn resolve_object_function_namespace(
    ir: &IrDocument,
    module: &Module,
    expr: &JsExpr,
    module_functions: &BTreeMap<(String, String), AotFunction>,
    module_default_exports: &BTreeMap<String, AotFunction>,
    module_object_exports: &BTreeMap<(String, String), BTreeMap<String, AotFunction>>,
    visited: &mut BTreeSet<String>,
) -> Option<BTreeMap<String, AotFunction>> {
    match expr {
        JsExpr::Ident { name } => module_object_exports
            .get(&(module.id.clone(), name.clone()))
            .cloned()
            .or_else(|| {
                resolve_required_module_from_local(ir, module, name).and_then(|required_module| {
                    collect_module_exported_function_namespace(
                        ir,
                        required_module,
                        module_functions,
                        module_default_exports,
                        module_object_exports,
                        visited,
                    )
                })
            }),
        JsExpr::Call { .. } => resolve_required_module(ir, module, expr).and_then(|required| {
            collect_module_exported_function_namespace(
                ir,
                required,
                module_functions,
                module_default_exports,
                module_object_exports,
                visited,
            )
        }),
        _ => None,
    }
}

fn collect_module_exported_function_namespace(
    ir: &IrDocument,
    module: &Module,
    module_functions: &BTreeMap<(String, String), AotFunction>,
    module_default_exports: &BTreeMap<String, AotFunction>,
    module_object_exports: &BTreeMap<(String, String), BTreeMap<String, AotFunction>>,
    visited: &mut BTreeSet<String>,
) -> Option<BTreeMap<String, AotFunction>> {
    if !visited.insert(module.id.clone()) {
        return None;
    }
    let executable = module.executable.as_ref()?;
    let mut functions = BTreeMap::new();
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
            if let Some(exported) = module_exported_object_functions(
                ir,
                module,
                right,
                module_functions,
                module_default_exports,
                module_object_exports,
                visited,
            ) {
                functions.extend(exported);
            }
            continue;
        }
        let Some(property) = cjs_named_export_property(left) else {
            continue;
        };
        if let Some(function) = resolve_module_function_binding(
            ir,
            module,
            right,
            module_functions,
            module_default_exports,
        ) {
            functions.insert(property, function);
        }
    }
    (!functions.is_empty()).then_some(functions)
}

fn resolve_module_function_binding(
    ir: &IrDocument,
    module: &Module,
    expr: &JsExpr,
    module_functions: &BTreeMap<(String, String), AotFunction>,
    module_default_exports: &BTreeMap<String, AotFunction>,
) -> Option<AotFunction> {
    if let Some(required_module) = resolve_required_module(ir, module, expr) {
        return module_default_exports.get(&required_module.id).cloned();
    }
    let JsExpr::Ident { name } = expr else {
        return None;
    };
    if let Some(function) = module_functions.get(&(module.id.clone(), name.clone())) {
        return Some(function.clone());
    }
    if let Some(imported_module) = resolve_required_module_from_local(ir, module, name) {
        if let Some(function) = module_default_exports.get(&imported_module.id) {
            return Some(function.clone());
        }
    }
    None
}

fn resolve_required_module_from_local<'a>(
    ir: &'a IrDocument,
    module: &Module,
    name: &str,
) -> Option<&'a Module> {
    for import in &module.imports {
        if import.kind != "cjs" {
            continue;
        }
        if !import.bindings.iter().any(|binding| binding.local == name) {
            continue;
        }
        let resolved = import.resolved.as_ref()?;
        return ir
            .modules
            .iter()
            .find(|candidate| &candidate.id == resolved);
    }
    None
}

fn resolve_required_module<'a>(
    ir: &'a IrDocument,
    module: &Module,
    expr: &JsExpr,
) -> Option<&'a Module> {
    let JsExpr::Call { callee, args, .. } = expr else {
        return None;
    };
    if !matches!(callee.as_ref(), JsExpr::Ident { name } if name == "require") || args.len() != 1 {
        return None;
    }
    let spec = string_literal_value(args.first()?)?;
    let import = module
        .imports
        .iter()
        .find(|import| import.kind == "cjs" && import.spec == spec)?;
    let resolved = import.resolved.as_ref()?;
    ir.modules
        .iter()
        .find(|candidate| &candidate.id == resolved)
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
        let Some(mut state) = module_aot_state(
            module,
            ir,
            &AotModuleContext {
                functions: module_functions,
                classes: &BTreeMap::new(),
                export_aliases: &module_export_aliases,
                default_exports: &BTreeMap::new(),
                default_class_exports: &BTreeMap::new(),
                named_exports: &BTreeMap::new(),
                object_exports: &BTreeMap::new(),
                slots: &BTreeMap::new(),
            },
        ) else {
            continue;
        };
        let mut dynamic_object_writes = BTreeSet::new();
        collect_dynamic_object_candidates(&executable.stmts, &mut dynamic_object_writes);
        let mut mutated_array_writes = BTreeSet::new();
        collect_mutated_array_bindings(&executable.stmts, &mut mutated_array_writes);
        mark_number_array_locals(&executable.stmts, &mut state);
        mark_string_array_locals(&executable.stmts, &mut state);
        mark_logical_assignment_any_locals(&executable.stmts, &mut state);
        mark_any_array_locals(&executable.stmts, &mut state);
        mark_array_property_locals(&executable.stmts, &mut state);
        let mut logical_assignment_targets = BTreeSet::new();
        collect_logical_assignment_targets(&executable.stmts, &mut logical_assignment_targets);
        for stmt in &executable.stmts {
            let JsStmt::VarDecl {
                name,
                init: Some(init),
            } = stmt
            else {
                continue;
            };
            let slot_init = cjs_export_alias_assignment_value(init).unwrap_or(init);
            if expr_references_any_name(slot_init, &mutated_array_writes) {
                continue;
            }
            if dynamic_object_writes.contains(name)
                && !state.number_array_bindings.contains(name)
                && !state.string_array_bindings.contains(name)
                && !state.any_array_bindings.contains(name)
                && render_dynamic_object_init_expr(slot_init, &state).is_none()
            {
                continue;
            }
            let rendered_dynamic_object = dynamic_object_writes
                .contains(name)
                .then(|| render_dynamic_object_init_expr(slot_init, &state))
                .flatten()
                .map(|rendered| {
                    (
                        AotSlotKind::Any,
                        rendered,
                        Some("map[string]any"),
                        None,
                        true,
                        render_dynamic_object_order_init_expr(slot_init, &state),
                    )
                });
            let rendered_any_array = state
                .any_array_bindings
                .contains(name)
                .then(|| render_any_array_expr(slot_init, &state))
                .flatten()
                .map(|rendered| {
                    (
                        AotSlotKind::AnyArray,
                        rendered,
                        Some("[]any"),
                        None,
                        false,
                        None,
                    )
                });
            let rendered_empty_number_array = (state.number_array_bindings.contains(name)
                && matches!(slot_init, JsExpr::Array { items } if items.is_empty()))
            .then(|| {
                (
                    AotSlotKind::NumberArray,
                    "[]float64{}".to_string(),
                    Some("[]float64"),
                    None,
                    false,
                    None,
                )
            });
            let rendered_logical_any = logical_assignment_targets
                .contains(name)
                .then(|| {
                    render_json_value_expr(slot_init, &state)
                        .or_else(|| render_expr(slot_init, &state))
                })
                .flatten()
                .map(|rendered| (AotSlotKind::Any, rendered, Some("any"), None, false, None));
            let rendered_typed =
                render_typed_slot_expr(slot_init, &state).map(|(kind, rendered, go_type)| {
                    (kind, rendered, Some(go_type), None, false, None)
                });
            let rendered_object =
                render_object_literal(slot_init, &state).map(|(rendered, object)| {
                    (AotSlotKind::Any, rendered, None, Some(object), false, None)
                });
            let selected_slot = match rendered_typed {
                Some((AotSlotKind::Bytes, rendered, go_type, object, dynamic_object, order)) => {
                    Some((
                        AotSlotKind::Bytes,
                        rendered,
                        go_type,
                        object,
                        dynamic_object,
                        order,
                    ))
                }
                other => rendered_dynamic_object
                    .or(rendered_any_array)
                    .or(rendered_empty_number_array)
                    .or(rendered_logical_any)
                    .or(other)
                    .or(rendered_object),
            };
            let Some((kind, rendered, go_type, object, dynamic_object, dynamic_object_order)) =
                selected_slot
            else {
                continue;
            };
            let go_name = module_member_go_name(module, name);
            slots.insert(
                (module.id.clone(), name.clone()),
                AotModuleSlot {
                    kind,
                    go_name: go_name.clone(),
                    go_type,
                    rendered,
                    object,
                    dynamic_object,
                    dynamic_object_order,
                },
            );
            state.bind_slot(name, go_name, kind);
            if dynamic_object {
                state.dynamic_object_bindings.insert(name.clone());
                state.ordered_dynamic_object_bindings.insert(name.clone());
            }
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
    let entry_id = entry_module(ir)?.id.clone();
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
        let mut state = module_aot_state(
            module,
            ir,
            &AotModuleContext {
                functions: module_functions,
                classes: module_classes,
                export_aliases: &module_export_aliases,
                default_exports: &module_default_exports,
                default_class_exports: &module_default_class_exports,
                named_exports: &module_named_exports,
                object_exports: &module_object_exports,
                slots: module_slots,
            },
        )?;
        if let Some(executable) = &module.executable {
            mark_dynamic_object_locals(&executable.stmts, &mut state);
            mark_logical_assignment_any_locals(&executable.stmts, &mut state);
            mark_string_array_locals(&executable.stmts, &mut state);
            mark_any_array_locals(&executable.stmts, &mut state);
            mark_array_property_locals(&executable.stmts, &mut state);
        }
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
                let Some(slot) = module_slots.get(&(module.id.clone(), name.clone())) else {
                    continue;
                };
                let declaration = match slot.go_type {
                    Some(go_type) => {
                        format!("var {} {} = {}", slot.go_name, go_type, slot.rendered)
                    }
                    None => format!("var {} = {}", slot.go_name, slot.rendered),
                };
                declarations.push(declaration);
                if state.array_property_bindings.contains(name) {
                    declarations.push(format!(
                        "var {} map[string]any = map[string]any{{}}",
                        array_property_map_go_name(&slot.go_name)
                    ));
                    declarations.push(format!(
                        "var {} []string = []string{{}}",
                        array_property_order_go_name(&slot.go_name)
                    ));
                }
                if slot.dynamic_object {
                    declarations.push(format!(
                        "var {} []string = {}",
                        dynamic_object_order_go_name(&slot.go_name),
                        slot.dynamic_object_order.as_deref().unwrap_or("[]string{}")
                    ));
                }
            }
        }
        if module_uses_runtime_cjs_exports(module, &state) {
            declarations.push(format!(
                "var {} any = map[string]any{{}}",
                module_exports_go_name(module)
            ));
        }
        for stmt in &module.executable.as_ref()?.stmts {
            if let Some(parts) = function_parts(stmt) {
                if module_classes.contains_key(&(module.id.clone(), parts.name.clone())) {
                    continue;
                }
                let function = module_functions.get(&(module.id.clone(), parts.name.clone()))?;
                declarations.push(render_function_decl(function, &state)?);
            }
            if cjs_default_function_expr(stmt).is_some() {
                let function = module_functions
                    .get(&(module.id.clone(), CJS_DEFAULT_EXPORT_FUNCTION.to_string()))?;
                declarations.push(render_function_decl(function, &state)?);
            }
        }
        if module.id != entry_id {
            declarations.push(format!(
                "func {}() {{\n{}\n}}",
                module_init_go_name(module),
                indent_lines(&render_module_init_body(module, &state, module_slots)?)
            ));
        }
    }
    Some(declarations)
}

fn render_module_init_body(
    module: &Module,
    state: &AotState,
    module_slots: &BTreeMap<(String, String), AotModuleSlot>,
) -> Option<String> {
    let executable = module.executable.as_ref()?;
    let mut state = clone_aot_state(state);
    mark_dynamic_object_locals(&executable.stmts, &mut state);
    mark_logical_assignment_any_locals(&executable.stmts, &mut state);
    mark_string_array_locals(&executable.stmts, &mut state);
    mark_any_array_locals(&executable.stmts, &mut state);
    mark_array_property_locals(&executable.stmts, &mut state);
    let mut body = Vec::new();
    for stmt in &executable.stmts {
        if matches!(stmt, JsStmt::FunctionDecl { .. } | JsStmt::ClassDecl { .. })
            || is_function_binding_stmt(stmt)
        {
            continue;
        }
        if matches!(
            stmt,
            JsStmt::VarDecl { name, .. }
                if module_slots.contains_key(&(module.id.clone(), name.clone()))
        ) || is_create_require_alias_decl(stmt)
            || is_resolved_cjs_export_metadata_stmt(stmt, &state)
            || is_resolved_cjs_export_metadata_decl_stmt(stmt, &state)
            || is_resolved_default_export_metadata_decl_stmt(stmt, &state)
        {
            continue;
        }
        body.push(render_stmt(stmt, &mut state)?);
    }
    Some(body.join("\n"))
}

fn module_aot_state(
    module: &Module,
    ir: &IrDocument,
    context: &AotModuleContext<'_>,
) -> Option<AotState> {
    let mut state = AotState {
        entry_source_path: entry_module(ir).map(|module| module.source_path.clone()),
        module_exports_ref: Some(module_exports_go_name(module)),
        ..AotState::default()
    };
    for stmt in &module.executable.as_ref()?.stmts {
        if let Some(parts) = function_parts(stmt) {
            let function = context
                .functions
                .get(&(module.id.clone(), parts.name.clone()))?;
            state.functions.insert(parts.name.clone(), function.clone());
        }
        if cjs_default_function_expr(stmt).is_some() {
            let function = context
                .functions
                .get(&(module.id.clone(), CJS_DEFAULT_EXPORT_FUNCTION.to_string()))?;
            state
                .functions
                .insert(CJS_DEFAULT_EXPORT_FUNCTION.to_string(), function.clone());
        }
        if let JsStmt::VarDecl { name, .. } = stmt {
            if let Some(slot) = context.slots.get(&(module.id.clone(), name.clone())) {
                state.bind_slot(name, slot.go_name.clone(), slot.kind);
                if let Some(object) = &slot.object {
                    state.object_bindings.insert(name.clone(), object.clone());
                }
                if slot.dynamic_object {
                    state.dynamic_object_bindings.insert(name.clone());
                    state.ordered_dynamic_object_bindings.insert(name.clone());
                }
            }
        }
        if let JsStmt::VarDecl {
            name,
            init: Some(expr),
        } = stmt
        {
            if let Some(spec) = awaited_dynamic_import_spec(expr) {
                bind_dynamic_import_namespace_slots(&mut state, name, spec, module, ir, context);
            }
        }
        if let JsStmt::VarDecl {
            name,
            init: Some(expr),
        } = stmt
        {
            if let Some(method) = string_prototype_method_alias(expr) {
                state.bindings.insert(name.clone());
                state
                    .binding_refs
                    .insert(name.clone(), sanitize_go_identifier(name));
                state
                    .string_method_aliases
                    .insert(name.clone(), method.to_string());
            }
            if let Some(alias) = builtin_function_alias(expr) {
                state.bindings.insert(name.clone());
                state
                    .binding_refs
                    .insert(name.clone(), sanitize_go_identifier(name));
                state.builtin_function_aliases.insert(name.clone(), alias);
            }
            if let Some((pattern, global)) = render_supported_regexp_replace_pattern(expr) {
                if !state.bindings.contains(name) {
                    state.bind_slot(name, sanitize_go_identifier(name), AotSlotKind::RegExp);
                }
                state
                    .regexp_replace_bindings
                    .insert(name.clone(), (pattern, global));
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
    for ((module_id, namespace), functions) in context.object_exports {
        if module_id == &module.id {
            for (property, function) in functions {
                state
                    .namespace_functions
                    .insert((namespace.clone(), property.clone()), function.clone());
            }
        }
    }
    for import in &module.imports {
        if import.resolved.is_none() && module_has_caught_require_spec(module, &import.spec) {
            continue;
        }
        if import.resolved.is_none() && is_node_builtin_spec(&import.spec) {
            for binding in &import.bindings {
                state.builtin_bindings.insert(binding.local.clone());
                if is_node_assert_spec(&import.spec) {
                    state.assert_builtin_bindings.insert(binding.local.clone());
                }
                if is_node_fs_promises_spec(&import.spec) {
                    let imported = binding.imported.as_deref().unwrap_or(&binding.local);
                    if matches!(imported, "readFile" | "writeFile" | "readdir") {
                        state
                            .fs_promises_bindings
                            .insert(binding.local.clone(), imported.to_string());
                    }
                }
            }
            continue;
        }
        let resolved = import.resolved.as_ref()?;
        let imported_module = ir
            .modules
            .iter()
            .find(|candidate| &candidate.id == resolved)?;
        if import.kind == "dynamic" {
            bind_dynamic_import_spec_slots(&mut state, &import.spec, imported_module, context);
        }
        for binding in &import.bindings {
            if import.kind == "cjs" {
                if let Some(class) = context.default_class_exports.get(&imported_module.id) {
                    state.classes.insert(binding.local.clone(), class.clone());
                    continue;
                }
                if let Some(function) = context.default_exports.get(&imported_module.id) {
                    state
                        .functions
                        .insert(binding.local.clone(), function.clone());
                    bind_cjs_default_function_static_slots(
                        &mut state,
                        &binding.local,
                        imported_module,
                        ir,
                        context,
                    );
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
                        bind_imported_slot(&mut state, &binding.local, slot);
                        continue;
                    }
                }
                if let Some(named) = context
                    .named_exports
                    .get(&imported_module.id)
                    .filter(|named| !named.is_empty())
                {
                    for (property, function) in named {
                        state
                            .namespace_functions
                            .insert((binding.local.clone(), property.clone()), function.clone());
                    }
                    if module_uses_runtime_cjs_exports(
                        imported_module,
                        &AotState {
                            module_exports_ref: Some(module_exports_go_name(imported_module)),
                            ..AotState::default()
                        },
                    ) {
                        state.bind_slot(
                            &binding.local,
                            module_exports_go_name(imported_module),
                            AotSlotKind::Any,
                        );
                        state.dynamic_object_bindings.insert(binding.local.clone());
                    }
                    continue;
                }
                if module_uses_runtime_cjs_exports(
                    imported_module,
                    &AotState {
                        module_exports_ref: Some(module_exports_go_name(imported_module)),
                        ..AotState::default()
                    },
                ) {
                    state.bind_slot(
                        &binding.local,
                        module_exports_go_name(imported_module),
                        AotSlotKind::Any,
                    );
                    state.dynamic_object_bindings.insert(binding.local.clone());
                    continue;
                }
                continue;
            }
            let imported = binding.imported.as_deref().unwrap_or(&binding.local);
            if imported == "default" {
                if let Some(class) = context.default_class_exports.get(&imported_module.id) {
                    state.classes.insert(binding.local.clone(), class.clone());
                    continue;
                }
                if let Some(function) = context.default_exports.get(&imported_module.id) {
                    state
                        .functions
                        .insert(binding.local.clone(), function.clone());
                    continue;
                }
                if let Some(named) = context
                    .named_exports
                    .get(&imported_module.id)
                    .filter(|named| !named.is_empty())
                {
                    for (property, function) in named {
                        state
                            .namespace_functions
                            .insert((binding.local.clone(), property.clone()), function.clone());
                    }
                    continue;
                }
            }
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
                    bind_forwarded_function_static_slots(
                        &mut state,
                        &binding.local,
                        &imported_module.id,
                        local,
                        ir,
                        context,
                    );
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
                    bind_imported_slot(&mut state, &binding.local, slot);
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
            if bind_forwarded_import(
                &mut state,
                &binding.local,
                imported_module,
                imported,
                ir,
                context,
            ) {
                continue;
            }
            let slot = context
                .slots
                .get(&(imported_module.id.clone(), imported.to_string()))?;
            bind_imported_slot(&mut state, &binding.local, slot);
        }
    }
    for stmt in &module.executable.as_ref()?.stmts {
        if let Some((function, property, kind, rendered)) =
            function_static_member_assignment(stmt, &state)
                .or_else(|| cjs_default_function_static_member_assignment(stmt, &state))
                .or_else(|| object_define_property_static_member(stmt, &state))
        {
            state
                .function_static_members
                .insert((function, property), (kind, rendered));
        }
    }
    Some(state)
}

fn bind_dynamic_import_namespace_slots(
    state: &mut AotState,
    local: &str,
    spec: &str,
    module: &Module,
    ir: &IrDocument,
    context: &AotModuleContext<'_>,
) {
    let Some(resolved) = module
        .imports
        .iter()
        .find(|import| import.kind == "dynamic" && import.spec == spec)
        .and_then(|import| import.resolved.as_deref())
    else {
        return;
    };
    let Some(imported_module) = ir.modules.iter().find(|candidate| candidate.id == resolved) else {
        return;
    };
    bind_dynamic_import_spec_slots(state, spec, imported_module, context);
    bind_dynamic_import_local_from_spec(state, local, spec);
}

fn bind_dynamic_import_spec_slots(
    state: &mut AotState,
    spec: &str,
    imported_module: &Module,
    context: &AotModuleContext<'_>,
) {
    for exported in &imported_module.exports {
        if let Some(slot) = context
            .slots
            .get(&(imported_module.id.clone(), exported.clone()))
        {
            state.dynamic_import_spec_member_slots.insert(
                (spec.to_string(), exported.clone()),
                (slot.kind, slot.go_name.clone()),
            );
            continue;
        }
        if let Some(local_name) = context
            .export_aliases
            .get(&(imported_module.id.clone(), exported.clone()))
        {
            if let Some(slot) = context
                .slots
                .get(&(imported_module.id.clone(), local_name.clone()))
            {
                state.dynamic_import_spec_member_slots.insert(
                    (spec.to_string(), exported.clone()),
                    (slot.kind, slot.go_name.clone()),
                );
            }
        }
    }
}

fn bind_dynamic_import_local_from_spec(state: &mut AotState, local: &str, spec: &str) {
    for ((_, property), (kind, rendered)) in state
        .dynamic_import_spec_member_slots
        .iter()
        .filter(|((slot_spec, _), _)| slot_spec == spec)
    {
        state.dynamic_import_member_slots.insert(
            (local.to_string(), property.clone()),
            (*kind, rendered.clone()),
        );
    }
}

fn bind_cjs_default_function_static_slots(
    state: &mut AotState,
    local: &str,
    target_module: &Module,
    ir: &IrDocument,
    context: &AotModuleContext<'_>,
) {
    let Some(target_state) = module_aot_state(target_module, ir, context) else {
        return;
    };
    for ((function, property), (kind, rendered)) in target_state.function_static_members {
        if function == CJS_DEFAULT_EXPORT_FUNCTION {
            state
                .function_static_members
                .insert((local.to_string(), property), (kind, rendered));
        }
    }
}

fn bind_forwarded_import(
    state: &mut AotState,
    local: &str,
    imported_module: &Module,
    imported: &str,
    ir: &IrDocument,
    context: &AotModuleContext<'_>,
) -> bool {
    let Some((target_module_id, target_imported)) =
        forwarded_import_binding(imported_module, imported)
    else {
        return false;
    };
    if target_imported == "default" {
        if let Some(class) = context.default_class_exports.get(target_module_id) {
            state.classes.insert(local.to_string(), class.clone());
            return true;
        }
        if let Some(function) = context.default_exports.get(target_module_id) {
            state.functions.insert(local.to_string(), function.clone());
            return true;
        }
        if let Some(target_local) = context
            .export_aliases
            .get(&(target_module_id.to_string(), "default".to_string()))
        {
            if let Some(function) = context
                .functions
                .get(&(target_module_id.to_string(), target_local.clone()))
            {
                state.functions.insert(local.to_string(), function.clone());
                bind_forwarded_function_static_slots(
                    state,
                    local,
                    target_module_id,
                    target_local,
                    ir,
                    context,
                );
                return true;
            }
            if let Some(class) = context
                .classes
                .get(&(target_module_id.to_string(), target_local.clone()))
            {
                state.classes.insert(local.to_string(), class.clone());
                return true;
            }
            if let Some(slot) = context
                .slots
                .get(&(target_module_id.to_string(), target_local.clone()))
            {
                bind_imported_slot(state, local, slot);
                return true;
            }
        }
    }
    if let Some(function) = context
        .functions
        .get(&(target_module_id.to_string(), target_imported.to_string()))
    {
        state.functions.insert(local.to_string(), function.clone());
        return true;
    }
    if let Some(slot) = context
        .slots
        .get(&(target_module_id.to_string(), target_imported.to_string()))
    {
        bind_imported_slot(state, local, slot);
        return true;
    }
    false
}

fn bind_forwarded_function_static_slots(
    state: &mut AotState,
    local: &str,
    target_module_id: &str,
    target_local: &str,
    ir: &IrDocument,
    context: &AotModuleContext<'_>,
) {
    let Some(target_module) = ir
        .modules
        .iter()
        .find(|module| module.id == target_module_id)
    else {
        return;
    };
    let Some(executable) = &target_module.executable else {
        return;
    };
    for stmt in &executable.stmts {
        let JsStmt::Expr {
            expr: JsExpr::Assign { op, left, right },
        } = stmt
        else {
            continue;
        };
        if op != "=" {
            continue;
        }
        let JsExpr::Member {
            object,
            property,
            property_expr: None,
            optional: false,
        } = left.as_ref()
        else {
            continue;
        };
        if !matches!(object.as_ref(), JsExpr::Ident { name } if name == target_local) {
            continue;
        }
        if let JsExpr::Ident { name: slot_name } = right.as_ref() {
            let slot = context
                .slots
                .get(&(target_module_id.to_string(), slot_name.clone()))
                .or_else(|| forwarded_slot(target_module, slot_name, context));
            if let Some(slot) = slot {
                state.function_static_members.insert(
                    (local.to_string(), property.clone()),
                    (slot.kind, slot.go_name.clone()),
                );
                continue;
            }
        }
        let target_state = AotState {
            entry_source_path: state.entry_source_path.clone(),
            module_exports_ref: Some(module_exports_go_name(target_module)),
            ..AotState::default()
        };
        if let Some((kind, rendered, _)) = render_typed_slot_expr(right, &target_state) {
            state
                .function_static_members
                .insert((local.to_string(), property.clone()), (kind, rendered));
        }
    }
}

fn forwarded_slot<'a>(
    module: &Module,
    local: &str,
    context: &'a AotModuleContext<'_>,
) -> Option<&'a AotModuleSlot> {
    for import in &module.imports {
        let resolved = import.resolved.as_ref()?;
        for binding in &import.bindings {
            if binding.local != local {
                continue;
            }
            let imported = binding.imported.as_deref().unwrap_or(&binding.local);
            return context.slots.get(&(resolved.clone(), imported.to_string()));
        }
    }
    None
}

fn forwarded_import_binding<'a>(module: &'a Module, exported: &str) -> Option<(&'a str, &'a str)> {
    for import in &module.imports {
        let resolved = import.resolved.as_deref()?;
        for binding in &import.bindings {
            if binding.local == exported {
                return Some((
                    resolved,
                    binding.imported.as_deref().unwrap_or(&binding.local),
                ));
            }
        }
    }
    None
}

fn bind_imported_slot(state: &mut AotState, local: &str, slot: &AotModuleSlot) {
    state.bind_slot(local, slot.go_name.clone(), slot.kind);
    if let Some(object) = &slot.object {
        state
            .object_bindings
            .insert(local.to_string(), object.clone());
    }
    if slot.dynamic_object {
        state.dynamic_object_bindings.insert(local.to_string());
        state
            .ordered_dynamic_object_bindings
            .insert(local.to_string());
    }
}

fn function_static_member_assignment(
    stmt: &JsStmt,
    state: &AotState,
) -> Option<(String, String, AotSlotKind, String)> {
    let JsStmt::Expr {
        expr: JsExpr::Assign { op, left, right },
    } = stmt
    else {
        return None;
    };
    if op != "=" {
        return None;
    }
    let JsExpr::Member {
        object,
        property,
        property_expr: None,
        optional: false,
    } = left.as_ref()
    else {
        return None;
    };
    let JsExpr::Ident { name } = object.as_ref() else {
        return None;
    };
    if !state.functions.contains_key(name) {
        return None;
    }
    let (kind, rendered, _) = render_typed_slot_expr(right, state)?;
    Some((name.clone(), property.clone(), kind, rendered))
}

fn cjs_default_function_static_member_assignment(
    stmt: &JsStmt,
    state: &AotState,
) -> Option<(String, String, AotSlotKind, String)> {
    let JsStmt::Expr { expr } = stmt else {
        return None;
    };
    cjs_default_function_static_member_assignment_expr(expr, state)
}

fn cjs_default_function_static_member_assignment_expr(
    expr: &JsExpr,
    state: &AotState,
) -> Option<(String, String, AotSlotKind, String)> {
    let JsExpr::Assign { op, left, right } = expr else {
        return None;
    };
    if op != "=" || !state.functions.contains_key(CJS_DEFAULT_EXPORT_FUNCTION) {
        return None;
    }
    let property = cjs_named_export_property(left)?;
    let (kind, rendered, _) = render_typed_slot_expr(right, state)?;
    Some((
        CJS_DEFAULT_EXPORT_FUNCTION.to_string(),
        property,
        kind,
        rendered,
    ))
}

fn object_define_property_static_member(
    stmt: &JsStmt,
    state: &AotState,
) -> Option<(String, String, AotSlotKind, String)> {
    let JsStmt::Expr { expr } = stmt else {
        return None;
    };
    object_define_property_static_member_expr(expr, state)
}

fn object_define_property_static_member_expr(
    expr: &JsExpr,
    state: &AotState,
) -> Option<(String, String, AotSlotKind, String)> {
    let JsExpr::Call {
        callee,
        args,
        optional: false,
    } = expr
    else {
        return None;
    };
    if !is_object_define_property_ref(callee) || args.len() != 3 {
        return None;
    }
    let function = static_member_target_function_name(args.first()?, state)?;
    let property = string_literal_value(args.get(1)?)?.to_string();
    let value = descriptor_static_member_value_expr(args.get(2)?)?;
    let (kind, rendered, _) = render_typed_slot_expr(value, state)?;
    Some((function, property, kind, rendered))
}

fn static_member_target_function_name(expr: &JsExpr, state: &AotState) -> Option<String> {
    match expr {
        JsExpr::Ident { name } if state.functions.contains_key(name) => Some(name.clone()),
        expr if (is_exports_ident(expr) || is_module_exports_member(expr))
            && state.functions.contains_key(CJS_DEFAULT_EXPORT_FUNCTION) =>
        {
            Some(CJS_DEFAULT_EXPORT_FUNCTION.to_string())
        }
        _ => None,
    }
}

fn is_object_define_property_ref(expr: &JsExpr) -> bool {
    matches!(
        expr,
        JsExpr::Member {
            object,
            property,
            property_expr: None,
            optional: false,
        } if property == "defineProperty"
            && matches!(object.as_ref(), JsExpr::Ident { name } if name == "Object")
    )
}

fn descriptor_static_member_value_expr(expr: &JsExpr) -> Option<&JsExpr> {
    let JsExpr::Object { props } = expr else {
        return None;
    };
    if let Some(prop) = props
        .iter()
        .find(|prop| !prop.spread && prop.key_expr.is_none() && prop.key == "value")
    {
        return Some(&prop.value);
    }
    let getter = props
        .iter()
        .find(|prop| !prop.spread && prop.key_expr.is_none() && prop.key == "get")?;
    let JsExpr::Function { body, .. } = &getter.value else {
        return None;
    };
    single_return_expr(body)
}

fn single_return_expr(body: &[JsStmt]) -> Option<&JsExpr> {
    let [JsStmt::Return { value: Some(expr) }] = body else {
        return None;
    };
    Some(expr)
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

fn is_exports_ident(expr: &JsExpr) -> bool {
    matches!(expr, JsExpr::Ident { name } if name == "exports")
}

fn is_cjs_export_target(expr: &JsExpr) -> bool {
    is_module_exports_member(expr) || cjs_named_export_property(expr).is_some()
}

fn is_resolved_default_cjs_export_assignment_expr(expr: &JsExpr, state: &AotState) -> bool {
    let JsExpr::Assign { op, left, right } = expr else {
        return false;
    };
    if op != "=" {
        return false;
    }
    if is_module_exports_member(left) {
        return is_resolved_default_cjs_export_value(right, state);
    }
    is_exports_ident(left) && is_resolved_default_cjs_export_assignment_expr(right, state)
}

fn is_resolved_cjs_export_metadata_stmt(stmt: &JsStmt, state: &AotState) -> bool {
    let JsStmt::Expr {
        expr: JsExpr::Assign { op, left, right },
    } = stmt
    else {
        return false;
    };
    if op != "=" || cjs_named_export_property(left).is_none() {
        return false;
    }
    is_resolved_export_metadata_expr(right, state)
}

fn module_uses_runtime_cjs_exports(module: &Module, state: &AotState) -> bool {
    module
        .executable
        .as_ref()
        .is_some_and(|executable| stmt_list_uses_runtime_cjs_exports(&executable.stmts, state))
}

fn stmt_list_uses_runtime_cjs_exports(stmts: &[JsStmt], state: &AotState) -> bool {
    stmts
        .iter()
        .any(|stmt| stmt_uses_runtime_cjs_exports(stmt, state))
}

fn stmt_uses_runtime_cjs_exports(stmt: &JsStmt, state: &AotState) -> bool {
    match stmt {
        JsStmt::Expr { expr } => expr_uses_runtime_cjs_exports(expr, state),
        JsStmt::FunctionDecl { body, .. } => stmt_list_uses_runtime_cjs_exports(body, state),
        JsStmt::ClassDecl { methods, .. } => methods
            .iter()
            .any(|method| stmt_list_uses_runtime_cjs_exports(&method.body, state)),
        JsStmt::If {
            test,
            consequent,
            alternate,
        } => {
            expr_uses_runtime_cjs_exports(test, state)
                || stmt_list_uses_runtime_cjs_exports(consequent, state)
                || stmt_list_uses_runtime_cjs_exports(alternate, state)
        }
        JsStmt::For {
            init,
            test,
            update,
            body,
        } => {
            stmt_list_uses_runtime_cjs_exports(init, state)
                || test
                    .as_ref()
                    .is_some_and(|expr| expr_uses_runtime_cjs_exports(expr, state))
                || update
                    .as_ref()
                    .is_some_and(|expr| expr_uses_runtime_cjs_exports(expr, state))
                || stmt_list_uses_runtime_cjs_exports(body, state)
        }
        JsStmt::While { test, body } | JsStmt::DoWhile { test, body } => {
            expr_uses_runtime_cjs_exports(test, state)
                || stmt_list_uses_runtime_cjs_exports(body, state)
        }
        JsStmt::Try {
            body,
            catch_body,
            finally_body,
            ..
        } => {
            stmt_list_uses_runtime_cjs_exports(body, state)
                || stmt_list_uses_runtime_cjs_exports(catch_body, state)
                || stmt_list_uses_runtime_cjs_exports(finally_body, state)
        }
        JsStmt::Switch {
            discriminant,
            cases,
        } => {
            expr_uses_runtime_cjs_exports(discriminant, state)
                || cases.iter().any(|case| {
                    case.test
                        .as_ref()
                        .is_some_and(|expr| expr_uses_runtime_cjs_exports(expr, state))
                        || stmt_list_uses_runtime_cjs_exports(&case.consequent, state)
                })
        }
        JsStmt::Return { value } => value
            .as_ref()
            .is_some_and(|expr| expr_uses_runtime_cjs_exports(expr, state)),
        JsStmt::Throw { value } => expr_uses_runtime_cjs_exports(value, state),
        JsStmt::ForOf { right, body, .. } => {
            expr_uses_runtime_cjs_exports(right, state)
                || stmt_list_uses_runtime_cjs_exports(body, state)
        }
        JsStmt::Label { body, .. } => stmt_list_uses_runtime_cjs_exports(body, state),
        JsStmt::VarDecl { init, .. } => init
            .as_ref()
            .is_some_and(|expr| expr_uses_runtime_cjs_exports(expr, state)),
        _ => false,
    }
}

fn expr_uses_runtime_cjs_exports(expr: &JsExpr, state: &AotState) -> bool {
    match expr {
        expr if cjs_default_function_static_member_assignment_expr(expr, state).is_some() => false,
        expr if object_define_property_static_member_expr(expr, state).is_some() => false,
        JsExpr::Member { object, .. } if is_module_exports_member(object) => true,
        JsExpr::Assign { op, left, right } if op == "=" && is_module_exports_member(left) => {
            !is_resolved_default_cjs_export_value(right, state)
                || expr_uses_runtime_cjs_exports(right, state)
        }
        JsExpr::Assign { op, left, right }
            if op == "=" && cjs_named_export_property(left).is_some() =>
        {
            !is_resolved_export_metadata_expr(right, state)
                || expr_uses_runtime_cjs_exports(right, state)
        }
        JsExpr::Assign { left, right, .. } | JsExpr::Binary { left, right, .. } => {
            expr_uses_runtime_cjs_exports(left, state)
                || expr_uses_runtime_cjs_exports(right, state)
        }
        JsExpr::Unary { arg, .. } | JsExpr::Await { arg } | JsExpr::Spread { arg } => {
            expr_uses_runtime_cjs_exports(arg, state)
        }
        JsExpr::Conditional {
            test,
            consequent,
            alternate,
        } => {
            expr_uses_runtime_cjs_exports(test, state)
                || expr_uses_runtime_cjs_exports(consequent, state)
                || expr_uses_runtime_cjs_exports(alternate, state)
        }
        JsExpr::Call { callee, args, .. } | JsExpr::New { callee, args } => {
            expr_uses_runtime_cjs_exports(callee, state)
                || args
                    .iter()
                    .any(|arg| expr_uses_runtime_cjs_exports(arg, state))
        }
        JsExpr::Member {
            object,
            property_expr,
            ..
        } => {
            expr_uses_runtime_cjs_exports(object, state)
                || property_expr
                    .as_ref()
                    .is_some_and(|key| expr_uses_runtime_cjs_exports(key, state))
        }
        JsExpr::Array { items } | JsExpr::Sequence { exprs: items } => items
            .iter()
            .any(|item| expr_uses_runtime_cjs_exports(item, state)),
        JsExpr::ArraySpread { items } => items
            .iter()
            .any(|item| expr_uses_runtime_cjs_exports(&item.value, state)),
        JsExpr::Object { props } => props.iter().any(|prop| {
            prop.key_expr
                .as_ref()
                .is_some_and(|key| expr_uses_runtime_cjs_exports(key, state))
                || expr_uses_runtime_cjs_exports(&prop.value, state)
        }),
        JsExpr::Template { exprs, .. } => exprs
            .iter()
            .any(|expr| expr_uses_runtime_cjs_exports(expr, state)),
        _ => false,
    }
}

fn is_resolved_default_cjs_export_value(expr: &JsExpr, state: &AotState) -> bool {
    is_resolved_export_metadata_expr(expr, state)
        || matches!(expr, JsExpr::Function { .. })
        || is_resolved_default_cjs_function_namespace_value(expr, state)
}

fn is_resolved_default_cjs_function_namespace_value(expr: &JsExpr, state: &AotState) -> bool {
    let JsExpr::Object { props } = expr else {
        return false;
    };
    !props.is_empty()
        && props.iter().all(|prop| {
            prop.key_expr.is_none()
                && (prop.spread
                    || is_require_call(&prop.value)
                    || is_resolved_export_metadata_expr(&prop.value, state))
        })
}

fn is_resolved_default_export_metadata_decl_stmt(stmt: &JsStmt, state: &AotState) -> bool {
    let JsStmt::VarDecl {
        name,
        init: Some(expr),
    } = stmt
    else {
        return false;
    };
    name == "default" && is_resolved_export_metadata_expr(expr, state)
}

fn is_resolved_cjs_export_metadata_decl_stmt(stmt: &JsStmt, state: &AotState) -> bool {
    let JsStmt::VarDecl {
        name,
        init: Some(expr),
    } = stmt
    else {
        return false;
    };
    name == "module.exports" && is_resolved_default_cjs_export_value(expr, state)
}

fn is_create_require_alias_decl(stmt: &JsStmt) -> bool {
    let JsStmt::VarDecl {
        name,
        init:
            Some(JsExpr::Call {
                callee,
                args,
                optional: false,
            }),
    } = stmt
    else {
        return false;
    };
    name == "require"
        && args.is_empty()
        && matches!(callee.as_ref(), JsExpr::Ident { name } if name == "createRequire")
}

fn is_resolved_export_metadata_expr(expr: &JsExpr, state: &AotState) -> bool {
    match expr {
        JsExpr::Ident { name } => {
            state.functions.contains_key(name)
                || state.classes.contains_key(name)
                || state.bindings.contains(name)
        }
        JsExpr::Member {
            object,
            property,
            property_expr: None,
            optional: false,
        } => {
            let JsExpr::Ident { name } = object.as_ref() else {
                return false;
            };
            state
                .namespace_functions
                .contains_key(&(name.clone(), property.clone()))
        }
        _ => false,
    }
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

fn is_exports_module_exports_alias_assignment(expr: &JsExpr) -> bool {
    let JsExpr::Assign { op, left, right } = expr else {
        return false;
    };
    if op != "=" || !matches!(left.as_ref(), JsExpr::Ident { name } if name == "exports") {
        return false;
    }
    let JsExpr::Assign {
        op: right_op,
        left: right_left,
        ..
    } = right.as_ref()
    else {
        return false;
    };
    right_op == "=" && is_module_exports_member(right_left)
}

fn is_cjs_destructure_member_init(expr: &JsExpr) -> bool {
    matches!(
        expr,
        JsExpr::Member {
            object,
            property_expr: None,
            ..
        } if matches!(object.as_ref(), JsExpr::Ident { name } if name.starts_with("__tsgodown_destructure_"))
    )
}

fn cjs_export_alias_assignment_value(expr: &JsExpr) -> Option<&JsExpr> {
    let JsExpr::Assign { op, left, right } = expr else {
        return None;
    };
    if op != "=" || cjs_named_export_property(left).is_none() {
        return None;
    }
    Some(right)
}

fn cjs_export_alias_assignment_name(expr: &JsExpr) -> Option<String> {
    let JsExpr::Assign { op, left, .. } = expr else {
        return None;
    };
    if op != "=" {
        return None;
    }
    cjs_named_export_property(left)
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

fn this_assignment(stmt: &JsStmt) -> Option<(String, &JsExpr)> {
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
    Some((property, right.as_ref()))
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
        let sanitized = sanitize_go_identifier(name);
        if sanitized == "main" {
            "main__tsgodown".to_string()
        } else {
            sanitized
        }
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

fn module_init_go_name(module: &Module) -> String {
    module_member_go_name(module, "__init")
}

fn module_exports_go_name(module: &Module) -> String {
    module_member_go_name(module, "__exports")
}

fn module_init_order<'a>(ir: &'a IrDocument, entry: &'a Module) -> Vec<&'a Module> {
    let mut visited = BTreeSet::new();
    let mut ordered = Vec::new();
    visit_module_init_order(ir, &entry.id, &mut visited, &mut ordered);
    ordered
}

fn visit_module_init_order<'a>(
    ir: &'a IrDocument,
    module_id: &str,
    visited: &mut BTreeSet<String>,
    ordered: &mut Vec<&'a Module>,
) {
    if !visited.insert(module_id.to_string()) {
        return;
    }
    let Some(module) = ir
        .modules
        .iter()
        .find(|candidate| candidate.id == module_id)
    else {
        return;
    };
    for import in &module.imports {
        if let Some(resolved) = import.resolved.as_deref() {
            visit_module_init_order(ir, resolved, visited, ordered);
        }
    }
    ordered.push(module);
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
    array_property_bindings: BTreeSet<String>,
    logical_assignment_bindings: BTreeSet<String>,
    narrowed_any_array_bindings: BTreeSet<String>,
    date_bindings: BTreeSet<String>,
    regexp_bindings: BTreeSet<String>,
    regexp_replace_bindings: BTreeMap<String, (String, bool)>,
    map_bindings: BTreeSet<String>,
    set_bindings: BTreeSet<String>,
    url_bindings: BTreeSet<String>,
    event_emitter_bindings: BTreeSet<String>,
    number_closure_bindings: BTreeSet<String>,
    string_function_bindings: BTreeSet<String>,
    string_method_aliases: BTreeMap<String, String>,
    builtin_function_aliases: BTreeMap<String, AotBuiltinFunctionAlias>,
    dynamic_object_bindings: BTreeSet<String>,
    ordered_dynamic_object_bindings: BTreeSet<String>,
    object_bindings: BTreeMap<String, AotObject>,
    class_instance_bindings: BTreeMap<String, String>,
    current_receiver: Option<String>,
    current_fields: BTreeMap<String, AotSlotKind>,
    functions: BTreeMap<String, AotFunction>,
    function_static_members: BTreeMap<(String, String), (AotSlotKind, String)>,
    classes: BTreeMap<String, AotClass>,
    namespace_functions: BTreeMap<(String, String), AotFunction>,
    builtin_bindings: BTreeSet<String>,
    assert_builtin_bindings: BTreeSet<String>,
    fs_promises_bindings: BTreeMap<String, String>,
    dynamic_import_spec_member_slots: BTreeMap<(String, String), (AotSlotKind, String)>,
    dynamic_import_member_slots: BTreeMap<(String, String), (AotSlotKind, String)>,
    dynamic_import_namespaces: BTreeMap<String, String>,
    entry_source_path: Option<String>,
    module_exports_ref: Option<String>,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum AotBuiltinFunctionAlias {
    ArrayIsArray,
    ArrayConcat,
    ArrayJoin,
    ArrayPush,
    ArraySlice,
    DateToISOString,
    ObjectHasOwnProperty,
    ObjectToString,
    RegExpTest,
}

impl AotState {
    fn bind_slot(&mut self, name: &str, go_ref: String, kind: AotSlotKind) {
        self.bindings.insert(name.to_string());
        self.binding_refs.insert(name.to_string(), go_ref);
        self.bool_bindings.remove(name);
        self.numeric_bindings.remove(name);
        self.date_bindings.remove(name);
        self.string_bindings.remove(name);
        self.bytes_bindings.remove(name);
        self.number_array_bindings.remove(name);
        self.any_array_bindings.remove(name);
        self.regexp_bindings.remove(name);
        self.string_array_bindings.remove(name);
        self.array_property_bindings.remove(name);
        self.map_bindings.remove(name);
        self.set_bindings.remove(name);
        self.url_bindings.remove(name);
        self.event_emitter_bindings.remove(name);
        self.number_closure_bindings.remove(name);
        self.string_function_bindings.remove(name);
        self.dynamic_object_bindings.remove(name);
        self.ordered_dynamic_object_bindings.remove(name);
        self.object_bindings.remove(name);
        self.class_instance_bindings.remove(name);
        match kind {
            AotSlotKind::Any => {}
            AotSlotKind::Bool => {
                self.bool_bindings.insert(name.to_string());
            }
            AotSlotKind::Number => {
                self.numeric_bindings.insert(name.to_string());
            }
            AotSlotKind::Date => {
                self.date_bindings.insert(name.to_string());
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
            AotSlotKind::AnyArray => {
                self.any_array_bindings.insert(name.to_string());
            }
            AotSlotKind::RegExp => {
                self.regexp_bindings.insert(name.to_string());
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

fn clear_collection_slot_metadata(state: &mut AotState, name: &str) {
    state.number_array_bindings.remove(name);
    state.string_array_bindings.remove(name);
    state.any_array_bindings.remove(name);
    state.array_property_bindings.remove(name);
    state.bytes_bindings.remove(name);
    state.map_bindings.remove(name);
    state.set_bindings.remove(name);
    state.object_bindings.remove(name);
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
        array_property_bindings: state.array_property_bindings.clone(),
        logical_assignment_bindings: state.logical_assignment_bindings.clone(),
        narrowed_any_array_bindings: state.narrowed_any_array_bindings.clone(),
        date_bindings: state.date_bindings.clone(),
        regexp_bindings: state.regexp_bindings.clone(),
        regexp_replace_bindings: state.regexp_replace_bindings.clone(),
        map_bindings: state.map_bindings.clone(),
        set_bindings: state.set_bindings.clone(),
        url_bindings: state.url_bindings.clone(),
        event_emitter_bindings: state.event_emitter_bindings.clone(),
        number_closure_bindings: state.number_closure_bindings.clone(),
        string_function_bindings: state.string_function_bindings.clone(),
        string_method_aliases: state.string_method_aliases.clone(),
        builtin_function_aliases: state.builtin_function_aliases.clone(),
        dynamic_object_bindings: state.dynamic_object_bindings.clone(),
        ordered_dynamic_object_bindings: state.ordered_dynamic_object_bindings.clone(),
        object_bindings: state.object_bindings.clone(),
        class_instance_bindings: state.class_instance_bindings.clone(),
        current_receiver: state.current_receiver.clone(),
        current_fields: state.current_fields.clone(),
        functions: state.functions.clone(),
        function_static_members: state.function_static_members.clone(),
        classes: state.classes.clone(),
        namespace_functions: state.namespace_functions.clone(),
        builtin_bindings: state.builtin_bindings.clone(),
        assert_builtin_bindings: state.assert_builtin_bindings.clone(),
        fs_promises_bindings: state.fs_promises_bindings.clone(),
        dynamic_import_spec_member_slots: state.dynamic_import_spec_member_slots.clone(),
        dynamic_import_member_slots: state.dynamic_import_member_slots.clone(),
        dynamic_import_namespaces: state.dynamic_import_namespaces.clone(),
        entry_source_path: state.entry_source_path.clone(),
        module_exports_ref: state.module_exports_ref.clone(),
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
    object_exports: &'a BTreeMap<(String, String), BTreeMap<String, AotFunction>>,
    slots: &'a BTreeMap<(String, String), AotModuleSlot>,
}

#[derive(Clone)]
struct AotClass {
    name: String,
    go_name: String,
    fields: BTreeMap<String, AotSlotKind>,
    constructor_params: Vec<String>,
    constructor_values: Vec<(String, JsExpr)>,
    constructor_body: Vec<JsStmt>,
    methods: BTreeMap<String, AotMethod>,
    getters: BTreeMap<String, AotMethod>,
    super_error_name: Option<String>,
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
    go_type: Option<&'static str>,
    rendered: String,
    object: Option<AotObject>,
    dynamic_object: bool,
    dynamic_object_order: Option<String>,
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
    Date,
    Number,
    AnyArray,
    NumberArray,
    RegExp,
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
        AotSlotKind::Date => "string",
        AotSlotKind::Number => "float64",
        AotSlotKind::AnyArray => "[]any",
        AotSlotKind::NumberArray => "[]float64",
        AotSlotKind::RegExp => "string",
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
                    bind_dynamic_import_local_from_spec(state, name, spec);
                    if is_node_builtin_spec(spec) && !name.starts_with("__tsgodown_destructure_") {
                        state.builtin_bindings.insert(name.clone());
                        if is_node_assert_spec(spec) {
                            state.assert_builtin_bindings.insert(name.clone());
                        }
                    }
                    return Some(String::new());
                }
                if let Some(spec) = awaited_dynamic_import_default_spec(expr) {
                    if is_node_builtin_spec(spec) {
                        state.bindings.insert(name.clone());
                        state.binding_refs.insert(name.clone(), ident.clone());
                        state.builtin_bindings.insert(name.clone());
                        if is_node_assert_spec(spec) {
                            state.assert_builtin_bindings.insert(name.clone());
                        }
                        return Some(String::new());
                    }
                }
                if let Some((spec, _property)) = dynamic_import_namespace_member(expr, state) {
                    let spec = spec.to_string();
                    if is_node_builtin_spec(&spec) {
                        state.bindings.insert(name.clone());
                        state.binding_refs.insert(name.clone(), ident.clone());
                        state.builtin_bindings.insert(name.clone());
                        if is_node_assert_spec(&spec) {
                            state.assert_builtin_bindings.insert(name.clone());
                        }
                        return Some(String::new());
                    }
                }
                if matches!(expr, JsExpr::Function { .. }) && state.functions.contains_key(name) {
                    return Some(String::new());
                }
                if let Some(method) = string_prototype_method_alias(expr) {
                    state.bindings.insert(name.clone());
                    state.binding_refs.insert(name.clone(), ident.clone());
                    state
                        .string_method_aliases
                        .insert(name.clone(), method.to_string());
                    return Some(String::new());
                }
                if let Some(alias) = builtin_function_alias(expr) {
                    state.bindings.insert(name.clone());
                    state.binding_refs.insert(name.clone(), ident.clone());
                    state.builtin_function_aliases.insert(name.clone(), alias);
                    return Some(String::new());
                }
                if is_local_function_namespace_object(name, expr, state) {
                    state.bindings.insert(name.clone());
                    state.binding_refs.insert(name.clone(), ident.clone());
                    return Some(String::new());
                }
                if state.number_array_bindings.contains(name) && is_nullish_expr(expr) {
                    return Some(format!("var {ident} []float64 = nil"));
                }
                if state.dynamic_object_bindings.contains(name) && is_nullish_expr(expr) {
                    clear_collection_slot_metadata(state, name);
                    state.bindings.insert(name.clone());
                    state.binding_refs.insert(name.clone(), ident.clone());
                    state.ordered_dynamic_object_bindings.insert(name.clone());
                    let order = dynamic_object_order_ref(name, state);
                    return Some(format!(
                        "var {ident} map[string]any = map[string]any{{}}\nvar {order} []string = []string{{}}\n_ = {order}"
                    ));
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
                if state.bindings.contains(name) && is_cjs_destructure_member_init(expr) {
                    return Some(String::new());
                }
                if state.logical_assignment_bindings.contains(name) && is_any_binding(name, state) {
                    let value =
                        render_json_value_expr(expr, state).or_else(|| render_expr(expr, state))?;
                    return Some(format!("var {ident} any = {value}"));
                }
                if state.number_array_bindings.contains(name)
                    && matches!(expr, JsExpr::Array { items } if items.is_empty())
                {
                    state.bind_slot(name, ident.clone(), AotSlotKind::NumberArray);
                    return Some(append_array_property_decls(
                        name,
                        state,
                        format!("var {ident} []float64 = []float64{{}}"),
                    ));
                }
                if state.any_array_bindings.contains(name)
                    && matches!(expr, JsExpr::Array { items } if items.is_empty())
                {
                    state.bindings.insert(name.clone());
                    state.binding_refs.insert(name.clone(), ident.clone());
                    return Some(append_array_property_decls(
                        name,
                        state,
                        format!("var {ident} []any = []any{{}}"),
                    ));
                }
                if let Some(value) = render_any_array_index_expr(expr, state) {
                    state.bind_slot(name, ident.clone(), AotSlotKind::Any);
                    return Some(format!("var {ident} any = {value}"));
                }
                if matches!(expr, JsExpr::Ident { name } if state.any_array_bindings.contains(name))
                {
                    let value = render_any_array_expr(expr, state)?;
                    state.bind_slot(name, ident.clone(), AotSlotKind::AnyArray);
                    return Some(append_array_property_decls(
                        name,
                        state,
                        format!("var {ident} []any = {value}"),
                    ));
                }
                if name.starts_with("__tsgodown_destructure_")
                    && is_array_destructure_source_expr(expr, state)
                {
                    let value = render_any_array_from_any_expr(expr, state)?;
                    state.bind_slot(name, ident.clone(), AotSlotKind::AnyArray);
                    return Some(format!("var {ident} []any = {value}"));
                }
                if let Some(value) = render_numeric_expr(expr, state) {
                    state.bind_slot(name, ident.clone(), AotSlotKind::Number);
                    return Some(format!("var {ident} float64 = {value}"));
                }
                if let Some(value) = render_date_expr(expr, state) {
                    state.bind_slot(name, ident.clone(), AotSlotKind::Date);
                    return Some(format!("var {ident} string = {value}"));
                }
                if let Some(value) = render_string_expr(expr, state) {
                    state.bind_slot(name, ident.clone(), AotSlotKind::String);
                    return Some(format!("var {ident} string = {value}"));
                }
                if let Some(value) = render_bool_expr(expr, state) {
                    state.bind_slot(name, ident.clone(), AotSlotKind::Bool);
                    return Some(format!("var {ident} bool = {value}"));
                }
                if state.any_array_bindings.contains(name) {
                    if let Some(value) = render_any_array_expr(expr, state) {
                        state.bind_slot(name, ident.clone(), AotSlotKind::AnyArray);
                        return Some(append_array_property_decls(
                            name,
                            state,
                            format!("var {ident} []any = {value}"),
                        ));
                    }
                }
                if let Some(value) = render_number_array_expr(expr, state) {
                    state.bind_slot(name, ident.clone(), AotSlotKind::NumberArray);
                    return Some(append_array_property_decls(
                        name,
                        state,
                        format!("var {ident} []float64 = {value}"),
                    ));
                }
                if let Some((pattern, global)) = render_supported_regexp_replace_pattern(expr) {
                    state.bind_slot(name, ident.clone(), AotSlotKind::RegExp);
                    state
                        .regexp_replace_bindings
                        .insert(name.clone(), (pattern.clone(), global));
                    return Some(format!(
                        "var {ident} string = {}",
                        go_string_literal(&pattern)
                    ));
                }
                if let Some(value) = render_regexp_expr(expr, state) {
                    state.bind_slot(name, ident.clone(), AotSlotKind::RegExp);
                    return Some(format!("var {ident} string = {value}"));
                }
                if let Some(value) = render_string_array_expr(expr, state) {
                    state.bind_slot(name, ident.clone(), AotSlotKind::StringArray);
                    return Some(append_array_property_decls(
                        name,
                        state,
                        format!("var {ident} []string = {value}"),
                    ));
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
                    return Some(append_array_property_decls(
                        name,
                        state,
                        format!("var {ident} []any = {value}"),
                    ));
                }
                if let Some(value) = render_js_map_expr(expr) {
                    state.bindings.insert(name.clone());
                    state.binding_refs.insert(name.clone(), ident.clone());
                    state.map_bindings.insert(name.clone());
                    return Some(format!("var {ident} *tsgodownJSMap = {value}"));
                }
                if let Some(value) = render_js_set_expr(expr, state) {
                    state.bindings.insert(name.clone());
                    state.binding_refs.insert(name.clone(), ident.clone());
                    state.set_bindings.insert(name.clone());
                    return Some(format!("var {ident} *tsgodownJSSet = {value}"));
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
                        clear_collection_slot_metadata(state, name);
                        state.bindings.insert(name.clone());
                        state.binding_refs.insert(name.clone(), ident.clone());
                        state.ordered_dynamic_object_bindings.insert(name.clone());
                        let keys = render_dynamic_object_order_init_expr(expr, state)?;
                        let order = dynamic_object_order_ref(name, state);
                        return Some(format!(
                            "var {ident} map[string]any = {value}\nvar {order} []string = {keys}\n_ = {order}"
                        ));
                    }
                }
                if state.bytes_bindings.contains(name) {
                    if let Some(value) = render_bytes_expr_with_any_cast(expr, state) {
                        state.bind_slot(name, ident.clone(), AotSlotKind::Bytes);
                        return Some(format!("var {ident} []byte = {value}"));
                    }
                }
                if let Some(value) = render_bytes_expr(expr, state) {
                    state.bind_slot(name, ident.clone(), AotSlotKind::Bytes);
                    return Some(format!("var {ident} []byte = {value}"));
                }
                if let Some(value) = render_string_function_expr(expr, state) {
                    state.bind_slot(name, ident.clone(), AotSlotKind::StringFunction);
                    return Some(format!("var {ident} func() string = {value}"));
                }
                if let Some(value) = render_any_function_expr(expr, state) {
                    state.bindings.insert(name.clone());
                    state.binding_refs.insert(name.clone(), ident.clone());
                    return Some(format!("var {ident} any = {value}"));
                }
                if let Some(value) = render_cjs_export_alias_var_decl(name, expr, state) {
                    return Some(value);
                }
                if let Some((value, object)) = render_object_literal(expr, state) {
                    clear_collection_slot_metadata(state, name);
                    state.bindings.insert(name.clone());
                    state.binding_refs.insert(name.clone(), ident.clone());
                    state.object_bindings.insert(name.clone(), object);
                    return Some(format!("var {ident} = {value}"));
                }
                if let Some(value) = render_object_map_expr(expr, state) {
                    clear_collection_slot_metadata(state, name);
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
            if state.dynamic_object_bindings.contains(name) {
                state.ordered_dynamic_object_bindings.insert(name.clone());
                let order = dynamic_object_order_ref(name, state);
                return Some(format!(
                    "var {ident} map[string]any = map[string]any{{}}\nvar {order} []string = []string{{}}\n_ = {order}"
                ));
            }
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
            let test = render_bool_test_expr(test_expr, state)?;
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
        JsStmt::ForOf { left, right, body } => render_for_of_stmt(left, right, body, state),
        JsStmt::While { test, body } => render_while_stmt(test, body, state),
        JsStmt::Switch {
            discriminant,
            cases,
        } => render_switch_stmt(discriminant, cases, state),
        JsStmt::Try {
            body,
            catch_param,
            catch_body,
            finally_body,
            ..
        } => render_try_finally_stmt(
            body,
            catch_param.as_deref(),
            catch_body,
            finally_body,
            state,
        ),
        JsStmt::Throw { value } => render_throw_stmt(value, state),
        JsStmt::Break { label: None } => Some("break".to_string()),
        JsStmt::Continue { label: None } => Some("continue".to_string()),
        _ => None,
    }
}

fn render_switch_stmt(
    discriminant: &JsExpr,
    cases: &[crate::contract::JsSwitchCase],
    state: &AotState,
) -> Option<String> {
    let discriminant = render_expr(discriminant, state)?;
    let mut rendered_cases = Vec::new();
    for case in cases {
        let body = indent_lines(&render_stmt_block_with_state(&case.consequent, state)?);
        if let Some(test) = &case.test {
            let test = render_expr(test, state)?;
            rendered_cases.push(format!("case {test}:\n{body}"));
        } else {
            rendered_cases.push(format!("default:\n{body}"));
        }
    }
    Some(format!(
        "switch {discriminant} {{\n{}\n}}",
        rendered_cases.join("\n")
    ))
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
        array_property_bindings: state.array_property_bindings.clone(),
        logical_assignment_bindings: state.logical_assignment_bindings.clone(),
        narrowed_any_array_bindings: state.narrowed_any_array_bindings.clone(),
        date_bindings: state.date_bindings.clone(),
        regexp_bindings: state.regexp_bindings.clone(),
        regexp_replace_bindings: state.regexp_replace_bindings.clone(),
        map_bindings: state.map_bindings.clone(),
        set_bindings: state.set_bindings.clone(),
        url_bindings: state.url_bindings.clone(),
        event_emitter_bindings: state.event_emitter_bindings.clone(),
        number_closure_bindings: state.number_closure_bindings.clone(),
        string_function_bindings: state.string_function_bindings.clone(),
        string_method_aliases: state.string_method_aliases.clone(),
        builtin_function_aliases: state.builtin_function_aliases.clone(),
        dynamic_object_bindings: state.dynamic_object_bindings.clone(),
        ordered_dynamic_object_bindings: state.ordered_dynamic_object_bindings.clone(),
        object_bindings: state.object_bindings.clone(),
        class_instance_bindings: state.class_instance_bindings.clone(),
        current_receiver: state.current_receiver.clone(),
        current_fields: state.current_fields.clone(),
        functions: state.functions.clone(),
        function_static_members: state.function_static_members.clone(),
        classes: state.classes.clone(),
        namespace_functions: state.namespace_functions.clone(),
        builtin_bindings: state.builtin_bindings.clone(),
        assert_builtin_bindings: state.assert_builtin_bindings.clone(),
        fs_promises_bindings: state.fs_promises_bindings.clone(),
        dynamic_import_spec_member_slots: state.dynamic_import_spec_member_slots.clone(),
        dynamic_import_member_slots: state.dynamic_import_member_slots.clone(),
        dynamic_import_namespaces: state.dynamic_import_namespaces.clone(),
        entry_source_path: state.entry_source_path.clone(),
        module_exports_ref: state.module_exports_ref.clone(),
    };
    let init = init
        .first()
        .map(|stmt| render_for_init(stmt, &mut loop_state))
        .unwrap_or_else(|| Some(String::new()))?;
    let test = test
        .map(|expr| render_bool_test_expr(expr, &loop_state))
        .unwrap_or_else(|| Some(String::new()))?;
    let update = update
        .map(|expr| render_for_update(expr, &loop_state))
        .unwrap_or_else(|| Some(String::new()))?;
    let body = indent_lines(&render_stmt_block_with_state(body, &loop_state)?);
    Some(format!("for {init}; {test}; {update} {{\n{body}\n}}"))
}

fn render_for_of_stmt(
    left: &str,
    right: &JsExpr,
    body: &[JsStmt],
    state: &AotState,
) -> Option<String> {
    let ident = sanitize_go_identifier(left);
    if let Some(values) = render_string_array_expr(right, state) {
        let mut loop_state = clone_aot_state(state);
        loop_state.bind_slot(left, ident.clone(), AotSlotKind::String);
        let body = indent_lines(&render_stmt_block_with_state(body, &loop_state)?);
        return Some(format!("for _, {ident} := range {values} {{\n{body}\n}}"));
    }
    if let Some(values) = render_number_array_expr(right, state) {
        let mut loop_state = clone_aot_state(state);
        loop_state.bind_slot(left, ident.clone(), AotSlotKind::Number);
        let body = indent_lines(&render_stmt_block_with_state(body, &loop_state)?);
        return Some(format!("for _, {ident} := range {values} {{\n{body}\n}}"));
    }
    if let Some(values) = render_any_array_expr(right, state) {
        let mut loop_state = clone_aot_state(state);
        loop_state.bind_slot(left, ident.clone(), AotSlotKind::Any);
        let body = indent_lines(&render_stmt_block_with_state(body, &loop_state)?);
        return Some(format!("for _, {ident} := range {values} {{\n{body}\n}}"));
    }
    if let Some(values) = render_expr(right, state) {
        let mut loop_state = clone_aot_state(state);
        loop_state.bind_slot(left, ident.clone(), AotSlotKind::Any);
        let body = indent_lines(&render_stmt_block_with_state(body, &loop_state)?);
        return Some(format!(
            "for _, {ident} := range tsgodownAnyArrayFromAny({values}) {{\n{body}\n}}"
        ));
    }
    None
}

fn render_while_stmt(test: &JsExpr, body: &[JsStmt], state: &AotState) -> Option<String> {
    if let Some(rendered) = render_regexp_exec_while_stmt(test, body, state) {
        return Some(rendered);
    }
    let loop_state = clone_aot_state(state);
    let test = render_bool_test_expr(test, &loop_state)?;
    let body = indent_lines(&render_stmt_block_with_state(body, &loop_state)?);
    Some(format!("for {test} {{\n{body}\n}}"))
}

fn render_regexp_exec_while_stmt(
    test: &JsExpr,
    body: &[JsStmt],
    state: &AotState,
) -> Option<String> {
    let JsExpr::Binary { op, left, right } = test else {
        return None;
    };
    if !matches!(op.as_str(), "!=" | "!==") || !is_nullish_expr(right) {
        return None;
    }
    let JsExpr::Assign {
        op: assign_op,
        left: assign_left,
        right: assign_right,
    } = left.as_ref()
    else {
        return None;
    };
    if assign_op != "=" {
        return None;
    }
    let JsExpr::Ident { name: match_name } = assign_left.as_ref() else {
        return None;
    };
    let (pattern, input) = render_regexp_exec_call_expr(assign_right, state)?;
    let match_target = go_binding_ref(match_name, state);
    let mut loop_state = clone_aot_state(state);
    loop_state.bind_slot(
        match_name,
        sanitize_go_identifier(match_name),
        AotSlotKind::Any,
    );
    loop_state
        .narrowed_any_array_bindings
        .insert(match_name.clone());
    let body = indent_lines(&render_stmt_block_with_state(body, &loop_state)?);
    let cursor = format!(
        "__tsgodownRegexpCursor_{}",
        sanitize_go_identifier(match_name)
    );
    Some(format!(
        "{cursor} := 0\nfor {{\n\t{match_target} = tsgodownRegExpExec({pattern}, {input}, &{cursor})\n\tif {match_target} == nil {{\n\t\tbreak\n\t}}\n{body}\n}}"
    ))
}

fn render_regexp_exec_call_expr(expr: &JsExpr, state: &AotState) -> Option<(String, String)> {
    let JsExpr::Call { callee, args, .. } = expr else {
        return None;
    };
    if !is_regexp_exec_call(callee, args) {
        return None;
    }
    let JsExpr::Member { object, .. } = callee.as_ref() else {
        return None;
    };
    let pattern = render_regexp_pattern_expr(object, state)?;
    let input = render_string_expr(args.first()?, state)?;
    Some((pattern, input))
}

fn render_try_finally_stmt(
    body: &[JsStmt],
    catch_param: Option<&str>,
    catch_body: &[JsStmt],
    finally_body: &[JsStmt],
    state: &mut AotState,
) -> Option<String> {
    if !catch_body.is_empty() {
        return render_try_catch_stmt(body, catch_param, catch_body, finally_body, state);
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

fn render_try_catch_stmt(
    body: &[JsStmt],
    catch_param: Option<&str>,
    catch_body: &[JsStmt],
    finally_body: &[JsStmt],
    state: &AotState,
) -> Option<String> {
    let mut body_state = clone_aot_state(state);
    let rendered_body = render_stmt_sequence(body, &mut body_state)?;
    let body = indent_lines(&rendered_body);
    let mut catch_state = clone_aot_state(state);
    let catch_binding = catch_param.unwrap_or("__tsgodownCaughtError");
    let catch_ident = sanitize_go_identifier(catch_binding);
    catch_state.bindings.insert(catch_binding.to_string());
    catch_state
        .binding_refs
        .insert(catch_binding.to_string(), catch_ident.clone());
    catch_state
        .dynamic_object_bindings
        .insert(catch_binding.to_string());
    let rendered_catch_body = render_stmt_sequence(catch_body, &mut catch_state)?;
    let catch_body = indent_lines(&rendered_catch_body);
    let finally = if finally_body.is_empty() {
        String::new()
    } else {
        let mut finally_state = clone_aot_state(state);
        let finally_body = indent_lines(&render_stmt_sequence(finally_body, &mut finally_state)?);
        format!("defer func() {{\n{}\n}}()\n", indent_lines(&finally_body))
    };
    Some(format!(
        "func() {{\n{finally}var __tsgodownCaught any\nfunc() {{\n\tdefer func() {{\n\t\tif __tsgodownRecovered := recover(); __tsgodownRecovered != nil {{\n\t\t\t__tsgodownCaught = __tsgodownRecovered\n\t\t}}\n\t}}()\n{body}\n}}()\nif __tsgodownCaught != nil {{\n\t{catch_ident} := tsgodownCaughtValue(__tsgodownCaught)\n{catch_body}\n}}\n}}()"
    ))
}

fn render_try_catch_return_expr(
    body: &[JsStmt],
    catch_param: Option<&str>,
    catch_body: &[JsStmt],
    finally_body: &[JsStmt],
    state: &AotState,
) -> Option<String> {
    if catch_body.is_empty()
        || !stmt_list_has_return(body)
            && !stmt_list_has_return(catch_body)
            && !stmt_list_has_throw(body)
    {
        return None;
    }
    let mut body_state = clone_aot_state(state);
    let rendered_body =
        render_try_result_stmt_sequence(body, &mut body_state, "__tsgodownResult", true)?;
    let body = indent_lines(&rendered_body);
    let catch_binding = catch_param.unwrap_or("__tsgodownCaughtError");
    let catch_ident = sanitize_go_identifier(catch_binding);
    let mut catch_state = clone_aot_state(state);
    catch_state.bindings.insert(catch_binding.to_string());
    catch_state
        .binding_refs
        .insert(catch_binding.to_string(), catch_ident.clone());
    catch_state
        .dynamic_object_bindings
        .insert(catch_binding.to_string());
    let rendered_catch_body =
        render_try_result_stmt_sequence(catch_body, &mut catch_state, "__tsgodownResult", true)?;
    let catch_body = indent_lines(&rendered_catch_body);
    let finally = if finally_body.is_empty() {
        String::new()
    } else {
        let mut finally_state = clone_aot_state(state);
        let rendered_finally_body = render_stmt_sequence(finally_body, &mut finally_state)?;
        let finally_body = indent_lines(&rendered_finally_body);
        format!("\n{}", finally_body)
    };
    Some(format!(
        "func() any {{\n\tvar __tsgodownResult any\n\tvar __tsgodownCaught any\n\tfunc() {{\n\t\tdefer func() {{\n\t\t\tif __tsgodownRecovered := recover(); __tsgodownRecovered != nil {{\n\t\t\t\t__tsgodownCaught = __tsgodownRecovered\n\t\t\t}}\n\t\t}}()\n{body}\n\t}}()\n\tif __tsgodownCaught != nil {{\n\t\t{catch_ident} := tsgodownCaughtValue(__tsgodownCaught)\n\t\tfunc() {{\n{catch_body}\n\t\t}}()\n\t}}\n{finally}\n\treturn __tsgodownResult\n}}()"
    ))
}

fn render_try_result_stmt_sequence(
    stmts: &[JsStmt],
    state: &mut AotState,
    result_name: &str,
    terminate_on_return: bool,
) -> Option<String> {
    stmts
        .iter()
        .map(|stmt| render_try_result_stmt(stmt, state, result_name, terminate_on_return))
        .collect::<Option<Vec<_>>>()
        .map(|stmts| stmts.join("\n"))
}

fn render_try_result_stmt(
    stmt: &JsStmt,
    state: &mut AotState,
    result_name: &str,
    terminate_on_return: bool,
) -> Option<String> {
    match stmt {
        JsStmt::Return { value: Some(value) } => {
            let value = render_expr(value, state)?;
            if terminate_on_return {
                Some(format!("{result_name} = {value}\nreturn"))
            } else {
                Some(format!("{result_name} = {value}"))
            }
        }
        JsStmt::Return { value: None } => {
            if terminate_on_return {
                Some(format!("{result_name} = nil\nreturn"))
            } else {
                Some(format!("{result_name} = nil"))
            }
        }
        JsStmt::If {
            test,
            consequent,
            alternate,
        } => {
            let test_expr = test;
            let test = render_bool_test_expr(test_expr, state)?;
            let mut consequent_state = narrowed_typeof_state(test_expr, state);
            let consequent = indent_lines(&render_try_result_stmt_sequence(
                consequent,
                &mut consequent_state,
                result_name,
                terminate_on_return,
            )?);
            if alternate.is_empty() {
                return Some(format!("if {test} {{\n{consequent}\n}}"));
            }
            let mut alternate_state = clone_aot_state(state);
            let alternate = indent_lines(&render_try_result_stmt_sequence(
                alternate,
                &mut alternate_state,
                result_name,
                terminate_on_return,
            )?);
            Some(format!(
                "if {test} {{\n{consequent}\n}} else {{\n{alternate}\n}}"
            ))
        }
        JsStmt::Throw { value } => render_throw_stmt(value, state),
        other => render_function_stmt(other, state),
    }
}

fn stmt_list_has_return(stmts: &[JsStmt]) -> bool {
    stmts.iter().any(stmt_has_return)
}

fn stmt_has_return(stmt: &JsStmt) -> bool {
    match stmt {
        JsStmt::Return { .. } => true,
        JsStmt::If {
            consequent,
            alternate,
            ..
        } => stmt_list_has_return(consequent) || stmt_list_has_return(alternate),
        JsStmt::Try {
            body,
            catch_body,
            finally_body,
            ..
        } => {
            stmt_list_has_return(body)
                || stmt_list_has_return(catch_body)
                || stmt_list_has_return(finally_body)
        }
        _ => false,
    }
}

fn stmt_list_has_throw(stmts: &[JsStmt]) -> bool {
    stmts.iter().any(stmt_has_throw)
}

fn stmt_has_throw(stmt: &JsStmt) -> bool {
    match stmt {
        JsStmt::Throw { .. } => true,
        JsStmt::If {
            consequent,
            alternate,
            ..
        } => stmt_list_has_throw(consequent) || stmt_list_has_throw(alternate),
        JsStmt::Try {
            body,
            catch_body,
            finally_body,
            ..
        } => {
            stmt_list_has_throw(body)
                || stmt_list_has_throw(catch_body)
                || stmt_list_has_throw(finally_body)
        }
        _ => false,
    }
}

fn render_throw_stmt(value: &JsExpr, state: &AotState) -> Option<String> {
    if let Some((name, message)) = render_error_constructor_expr(value, state) {
        return Some(format!(
            "panic(tsgodownError{{Name: {name}, Message: {message}}})"
        ));
    }
    let value = render_expr(value, state)?;
    Some(format!("tsgodownThrow({value})"))
}

fn render_error_constructor_expr(expr: &JsExpr, state: &AotState) -> Option<(String, String)> {
    let (callee, args) = match expr {
        JsExpr::Call { callee, args, .. } | JsExpr::New { callee, args } => {
            (callee.as_ref(), args.as_slice())
        }
        _ => return None,
    };
    let JsExpr::Ident { name } = callee else {
        return None;
    };
    if !matches!(name.as_str(), "Error" | "TypeError" | "RangeError") {
        return None;
    }
    let message = args
        .first()
        .map(|arg| render_string_expr(arg, state))
        .unwrap_or_else(|| Some("\"\"".to_string()))?;
    Some((go_string_literal(name), message))
}

fn render_error_object_expr(expr: &JsExpr, state: &AotState) -> Option<String> {
    let (name, message) = render_error_constructor_expr(expr, state)?;
    Some(format!("tsgodownNewError({name}, {message})"))
}

fn error_constructor_name(expr: &JsExpr) -> Option<&str> {
    let JsExpr::Ident { name } = expr else {
        return None;
    };
    match name.as_str() {
        "Error" | "TypeError" | "RangeError" => Some(name),
        _ => None,
    }
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
            Some(format!("{}{}", go_binding_ref(name, state), op))
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
        array_property_bindings: state.array_property_bindings.clone(),
        logical_assignment_bindings: state.logical_assignment_bindings.clone(),
        narrowed_any_array_bindings: state.narrowed_any_array_bindings.clone(),
        date_bindings: state.date_bindings.clone(),
        regexp_bindings: state.regexp_bindings.clone(),
        regexp_replace_bindings: state.regexp_replace_bindings.clone(),
        map_bindings: state.map_bindings.clone(),
        set_bindings: state.set_bindings.clone(),
        url_bindings: state.url_bindings.clone(),
        event_emitter_bindings: state.event_emitter_bindings.clone(),
        number_closure_bindings: state.number_closure_bindings.clone(),
        string_function_bindings: state.string_function_bindings.clone(),
        string_method_aliases: state.string_method_aliases.clone(),
        builtin_function_aliases: state.builtin_function_aliases.clone(),
        dynamic_object_bindings: state.dynamic_object_bindings.clone(),
        ordered_dynamic_object_bindings: state.ordered_dynamic_object_bindings.clone(),
        object_bindings: state.object_bindings.clone(),
        class_instance_bindings: state.class_instance_bindings.clone(),
        current_receiver: state.current_receiver.clone(),
        current_fields: state.current_fields.clone(),
        functions: state.functions.clone(),
        function_static_members: state.function_static_members.clone(),
        classes: state.classes.clone(),
        namespace_functions: state.namespace_functions.clone(),
        builtin_bindings: state.builtin_bindings.clone(),
        assert_builtin_bindings: state.assert_builtin_bindings.clone(),
        fs_promises_bindings: state.fs_promises_bindings.clone(),
        dynamic_import_spec_member_slots: state.dynamic_import_spec_member_slots.clone(),
        dynamic_import_member_slots: state.dynamic_import_member_slots.clone(),
        dynamic_import_namespaces: state.dynamic_import_namespaces.clone(),
        entry_source_path: state.entry_source_path.clone(),
        module_exports_ref: state.module_exports_ref.clone(),
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
        array_property_bindings: state.array_property_bindings.clone(),
        logical_assignment_bindings: state.logical_assignment_bindings.clone(),
        narrowed_any_array_bindings: state.narrowed_any_array_bindings.clone(),
        date_bindings: state.date_bindings.clone(),
        regexp_bindings: state.regexp_bindings.clone(),
        regexp_replace_bindings: state.regexp_replace_bindings.clone(),
        map_bindings: state.map_bindings.clone(),
        set_bindings: state.set_bindings.clone(),
        url_bindings: state.url_bindings.clone(),
        event_emitter_bindings: state.event_emitter_bindings.clone(),
        number_closure_bindings: state.number_closure_bindings.clone(),
        string_function_bindings: state.string_function_bindings.clone(),
        string_method_aliases: state.string_method_aliases.clone(),
        builtin_function_aliases: state.builtin_function_aliases.clone(),
        dynamic_object_bindings: state.dynamic_object_bindings.clone(),
        ordered_dynamic_object_bindings: state.ordered_dynamic_object_bindings.clone(),
        object_bindings: state.object_bindings.clone(),
        class_instance_bindings: state.class_instance_bindings.clone(),
        current_receiver: state.current_receiver.clone(),
        current_fields: state.current_fields.clone(),
        functions: state.functions.clone(),
        function_static_members: state.function_static_members.clone(),
        classes: state.classes.clone(),
        namespace_functions: state.namespace_functions.clone(),
        builtin_bindings: state.builtin_bindings.clone(),
        assert_builtin_bindings: state.assert_builtin_bindings.clone(),
        fs_promises_bindings: state.fs_promises_bindings.clone(),
        dynamic_import_spec_member_slots: state.dynamic_import_spec_member_slots.clone(),
        dynamic_import_member_slots: state.dynamic_import_member_slots.clone(),
        dynamic_import_namespaces: state.dynamic_import_namespaces.clone(),
        entry_source_path: state.entry_source_path.clone(),
        module_exports_ref: state.module_exports_ref.clone(),
    };
    mark_number_array_locals(stmts, &mut block_state);
    mark_string_array_locals(stmts, &mut block_state);
    mark_any_array_locals(stmts, &mut block_state);
    mark_array_property_locals(stmts, &mut block_state);
    stmts
        .iter()
        .map(|stmt| render_stmt(stmt, &mut block_state))
        .collect::<Option<Vec<_>>>()
        .map(|stmts| stmts.join("\n"))
}

fn render_class_decl(class: &AotClass) -> Option<String> {
    if let Some(super_error_name) = &class.super_error_name {
        return render_error_subclass_decl(class, super_error_name);
    }
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

fn render_error_subclass_decl(class: &AotClass, super_error_name: &str) -> Option<String> {
    let params = class
        .constructor_params
        .iter()
        .map(|param| format!("{} any", sanitize_go_identifier(param)))
        .collect::<Vec<_>>()
        .join(", ");
    let mut state = AotState::default();
    for param in &class.constructor_params {
        state.bind_slot(param, sanitize_go_identifier(param), AotSlotKind::Any);
    }
    let mut prelude = Vec::new();
    let mut super_message = None;
    let mut field_assignments = Vec::new();
    for stmt in &class.constructor_body {
        if let Some(args) = super_call_args(stmt) {
            let message = args
                .first()
                .map(|arg| render_string_expr(arg, &state))
                .unwrap_or_else(|| Some("\"\"".to_string()))?;
            super_message = Some(message);
            continue;
        }
        if let Some((field, right)) = this_assignment(stmt) {
            let value =
                render_json_value_expr(right, &state).or_else(|| render_expr(right, &state))?;
            field_assignments.push(format!(
                "__tsgodownErr[{}] = {value}",
                go_string_literal(&field)
            ));
            continue;
        }
        prelude.push(render_error_constructor_prelude_stmt(stmt, &mut state)?);
    }
    let message = super_message.unwrap_or_else(|| "\"\"".to_string());
    prelude.push(format!(
        "__tsgodownErr := tsgodownNewError({}, {message})",
        go_string_literal(super_error_name)
    ));
    prelude.extend(field_assignments);
    prelude.push("return __tsgodownErr".to_string());
    Some(format!(
        "func new_{}({params}) map[string]any {{\n{}\n}}",
        class.go_name,
        indent_lines(&prelude.join("\n"))
    ))
}

fn super_call_args(stmt: &JsStmt) -> Option<&[JsExpr]> {
    let JsStmt::Expr { expr } = stmt else {
        return None;
    };
    let JsExpr::Call { callee, args, .. } = expr else {
        return None;
    };
    if matches!(callee.as_ref(), JsExpr::Super) {
        Some(args.as_slice())
    } else {
        None
    }
}

fn render_error_constructor_prelude_stmt(stmt: &JsStmt, state: &mut AotState) -> Option<String> {
    if let JsStmt::VarDecl {
        name,
        init: Some(init),
    } = stmt
    {
        let ident = sanitize_go_identifier(name);
        let value = render_js_to_string_expr(init, state)?;
        state.bind_slot(name, ident.clone(), AotSlotKind::String);
        return Some(format!("var {ident} string = {value}"));
    }
    render_stmt(stmt, state)
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

fn infer_function_param_kinds(
    params: &[String],
    body: &[JsStmt],
    builtin_aliases: &BTreeMap<String, AotBuiltinFunctionAlias>,
) -> Vec<AotSlotKind> {
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
        infer_stmt_param_kinds(stmt, &param_index, &mut kinds, builtin_aliases);
    }
    let local_functions = collect_direct_local_function_map(body, builtin_aliases);
    let imported_function_aliases = BTreeMap::new();
    for stmt in body {
        propagate_stmt_param_kinds(
            stmt,
            "",
            &local_functions,
            &imported_function_aliases,
            &param_index,
            &mut kinds,
        );
    }
    mark_string_accumulator_params(body, &param_index, &mut kinds, &mut BTreeSet::new());
    kinds
}

fn collect_direct_local_function_map(
    body: &[JsStmt],
    builtin_aliases: &BTreeMap<String, AotBuiltinFunctionAlias>,
) -> BTreeMap<(String, String), AotFunction> {
    let mut functions = BTreeMap::new();
    for stmt in body {
        if let JsStmt::FunctionDecl {
            name,
            params,
            rest_param,
            r#async,
            generator,
            body,
        } = stmt
        {
            functions.insert(
                ("".to_string(), name.clone()),
                AotFunction {
                    params: params.clone(),
                    param_kinds: infer_function_param_kinds(params, body, builtin_aliases),
                    rest_param: rest_param.clone(),
                    r#async: *r#async,
                    generator: *generator,
                    body: body.clone(),
                    go_name: sanitize_go_identifier(name),
                },
            );
        }
    }
    functions
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
            | JsStmt::ForOf { body, .. }
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
    builtin_aliases: &BTreeMap<String, AotBuiltinFunctionAlias>,
) {
    match stmt {
        JsStmt::Return {
            value: Some(JsExpr::Ident { .. }),
        } => {
            if let JsStmt::Return { value: Some(expr) } = stmt {
                mark_ident_param_kind(expr, param_index, kinds, AotSlotKind::Any);
                infer_expr_param_kinds(expr, param_index, kinds, builtin_aliases);
            }
        }
        JsStmt::VarDecl {
            init: Some(expr), ..
        }
        | JsStmt::Expr { expr }
        | JsStmt::Return { value: Some(expr) }
        | JsStmt::Throw { value: expr }
        | JsStmt::Yield {
            value: Some(expr), ..
        } => infer_expr_param_kinds(expr, param_index, kinds, builtin_aliases),
        JsStmt::If {
            test,
            consequent,
            alternate,
        } => {
            infer_bool_context_param_kinds(test, param_index, kinds, builtin_aliases);
            for stmt in consequent {
                infer_stmt_param_kinds(stmt, param_index, kinds, builtin_aliases);
            }
            for stmt in alternate {
                infer_stmt_param_kinds(stmt, param_index, kinds, builtin_aliases);
            }
        }
        JsStmt::For {
            init,
            test,
            update,
            body,
        } => {
            for stmt in init {
                infer_stmt_param_kinds(stmt, param_index, kinds, builtin_aliases);
            }
            if let Some(test) = test {
                infer_bool_context_param_kinds(test, param_index, kinds, builtin_aliases);
            }
            if let Some(update) = update {
                infer_expr_param_kinds(update, param_index, kinds, builtin_aliases);
            }
            for stmt in body {
                infer_stmt_param_kinds(stmt, param_index, kinds, builtin_aliases);
            }
        }
        JsStmt::ForOf { right, body, .. } => {
            infer_expr_param_kinds(right, param_index, kinds, builtin_aliases);
            for stmt in body {
                infer_stmt_param_kinds(stmt, param_index, kinds, builtin_aliases);
            }
        }
        JsStmt::While { test, body } | JsStmt::DoWhile { test, body } => {
            infer_bool_context_param_kinds(test, param_index, kinds, builtin_aliases);
            for stmt in body {
                infer_stmt_param_kinds(stmt, param_index, kinds, builtin_aliases);
            }
        }
        JsStmt::Try {
            body,
            catch_body,
            finally_body,
            ..
        } => {
            infer_try_param_kinds_shallow(body, param_index, kinds, builtin_aliases, 16);
            infer_try_param_kinds_shallow(catch_body, param_index, kinds, builtin_aliases, 16);
            infer_try_param_kinds_shallow(finally_body, param_index, kinds, builtin_aliases, 16);
        }
        _ => {}
    }
}

fn infer_try_param_kinds_shallow(
    stmts: &[JsStmt],
    param_index: &BTreeMap<String, usize>,
    kinds: &mut [AotSlotKind],
    builtin_aliases: &BTreeMap<String, AotBuiltinFunctionAlias>,
    depth: usize,
) {
    if depth == 0 {
        return;
    }
    for stmt in stmts {
        match stmt {
            JsStmt::If {
                test,
                consequent,
                alternate,
            } => {
                infer_bool_context_param_kinds(test, param_index, kinds, builtin_aliases);
                infer_try_param_kinds_shallow(
                    consequent,
                    param_index,
                    kinds,
                    builtin_aliases,
                    depth - 1,
                );
                infer_try_param_kinds_shallow(
                    alternate,
                    param_index,
                    kinds,
                    builtin_aliases,
                    depth - 1,
                );
            }
            JsStmt::For {
                init,
                test,
                update,
                body,
            } => {
                infer_try_param_kinds_shallow(init, param_index, kinds, builtin_aliases, depth - 1);
                if let Some(test) = test {
                    infer_bool_context_param_kinds(test, param_index, kinds, builtin_aliases);
                }
                if let Some(update) = update {
                    infer_expr_param_kinds(update, param_index, kinds, builtin_aliases);
                }
                infer_try_param_kinds_shallow(body, param_index, kinds, builtin_aliases, depth - 1);
            }
            JsStmt::While { test, body } | JsStmt::DoWhile { test, body } => {
                infer_bool_context_param_kinds(test, param_index, kinds, builtin_aliases);
                infer_try_param_kinds_shallow(body, param_index, kinds, builtin_aliases, depth - 1);
            }
            JsStmt::Try {
                body,
                catch_body,
                finally_body,
                ..
            } => {
                infer_try_param_kinds_shallow(body, param_index, kinds, builtin_aliases, depth - 1);
                infer_try_param_kinds_shallow(
                    catch_body,
                    param_index,
                    kinds,
                    builtin_aliases,
                    depth - 1,
                );
                infer_try_param_kinds_shallow(
                    finally_body,
                    param_index,
                    kinds,
                    builtin_aliases,
                    depth - 1,
                );
            }
            JsStmt::FunctionDecl { .. } | JsStmt::ClassDecl { .. } => {}
            _ => {}
        }
    }
}

fn infer_expr_param_kinds(
    expr: &JsExpr,
    param_index: &BTreeMap<String, usize>,
    kinds: &mut [AotSlotKind],
    builtin_aliases: &BTreeMap<String, AotBuiltinFunctionAlias>,
) {
    match expr {
        JsExpr::Binary { op, left, right } if op == "+" => {
            if is_string_literal_like(right) {
                mark_ident_param_kind(left, param_index, kinds, AotSlotKind::String);
            }
            if is_string_literal_like(left) {
                mark_ident_param_kind(right, param_index, kinds, AotSlotKind::String);
            }
            infer_expr_param_kinds(left, param_index, kinds, builtin_aliases);
            infer_expr_param_kinds(right, param_index, kinds, builtin_aliases);
        }
        JsExpr::Binary { op, left, right } if op == "||" => {
            if is_string_literal_like(right) {
                mark_ident_param_kind(left, param_index, kinds, AotSlotKind::String);
            }
            if is_string_literal_like(left) {
                mark_ident_param_kind(right, param_index, kinds, AotSlotKind::String);
            }
            if is_number_literal_like(right) {
                mark_ident_param_kind(left, param_index, kinds, AotSlotKind::Any);
            }
            if is_number_literal_like(left) {
                mark_ident_param_kind(right, param_index, kinds, AotSlotKind::Any);
            }
            if matches!(right.as_ref(), JsExpr::Object { .. }) {
                mark_ident_param_kind(left, param_index, kinds, AotSlotKind::Any);
            }
            if matches!(left.as_ref(), JsExpr::Object { .. }) {
                mark_ident_param_kind(right, param_index, kinds, AotSlotKind::Any);
            }
            infer_expr_param_kinds(left, param_index, kinds, builtin_aliases);
            infer_expr_param_kinds(right, param_index, kinds, builtin_aliases);
        }
        JsExpr::Binary { op, left, right } if op == "??" => {
            mark_ident_param_kind(left, param_index, kinds, AotSlotKind::Any);
            mark_ident_param_kind(right, param_index, kinds, AotSlotKind::Any);
            if is_string_result_shape(right)
                || matches!(
                    right.as_ref(),
                    JsExpr::Object { .. } | JsExpr::Array { .. } | JsExpr::New { .. }
                )
            {
                mark_ident_param_kind(left, param_index, kinds, AotSlotKind::Any);
            }
            if is_string_result_shape(left)
                || matches!(
                    left.as_ref(),
                    JsExpr::Object { .. } | JsExpr::Array { .. } | JsExpr::New { .. }
                )
            {
                mark_ident_param_kind(right, param_index, kinds, AotSlotKind::Any);
            }
            infer_expr_param_kinds(left, param_index, kinds, builtin_aliases);
            infer_expr_param_kinds(right, param_index, kinds, builtin_aliases);
        }
        JsExpr::Binary { op, left, right } if go_comparison_op(op).is_some() => {
            infer_comparison_param_kind(left, right, param_index, kinds);
            infer_comparison_param_kind(right, left, param_index, kinds);
            infer_expr_param_kinds(left, param_index, kinds, builtin_aliases);
            infer_expr_param_kinds(right, param_index, kinds, builtin_aliases);
        }
        JsExpr::Assign { op, left, right } if op == "??=" => {
            mark_ident_param_kind(left, param_index, kinds, AotSlotKind::Any);
            mark_assigned_member_object_param_kind(left, param_index, kinds);
            infer_expr_param_kinds(left, param_index, kinds, builtin_aliases);
            infer_expr_param_kinds(right, param_index, kinds, builtin_aliases);
        }
        JsExpr::Assign { op, left, right } if matches!(op.as_str(), "=" | "|=") => {
            mark_assigned_member_object_param_kind(left, param_index, kinds);
            if op == "=" && is_string_result_shape(right) {
                mark_ident_param_kind(left, param_index, kinds, AotSlotKind::String);
            }
            infer_expr_param_kinds(left, param_index, kinds, builtin_aliases);
            infer_expr_param_kinds(right, param_index, kinds, builtin_aliases);
        }
        JsExpr::Assign { op, left, right } if op == "=" && is_string_result_shape(right) => {
            mark_ident_param_kind(left, param_index, kinds, AotSlotKind::String);
            infer_expr_param_kinds(left, param_index, kinds, builtin_aliases);
            infer_expr_param_kinds(right, param_index, kinds, builtin_aliases);
        }
        JsExpr::Assign { left, right, .. } | JsExpr::Binary { left, right, .. } => {
            infer_expr_param_kinds(left, param_index, kinds, builtin_aliases);
            infer_expr_param_kinds(right, param_index, kinds, builtin_aliases);
        }
        JsExpr::Call { callee, args, .. } if is_string_split_call(callee, args) => {
            if let JsExpr::Member { object, .. } = callee.as_ref() {
                mark_ident_param_kind(object, param_index, kinds, AotSlotKind::String);
                infer_expr_param_kinds(object, param_index, kinds, builtin_aliases);
            }
            if let Some(separator) = args.first() {
                mark_ident_param_kind(separator, param_index, kinds, AotSlotKind::String);
                infer_expr_param_kinds(separator, param_index, kinds, builtin_aliases);
            }
        }
        JsExpr::Call { callee, args, .. } if is_member_slice_call_shape(callee, args) => {
            if let JsExpr::Member { object, .. } = callee.as_ref() {
                mark_ident_param_kind_if_default(object, param_index, kinds, AotSlotKind::Any);
                infer_expr_param_kinds(object, param_index, kinds, builtin_aliases);
            }
            for arg in args {
                infer_expr_param_kinds(arg, param_index, kinds, builtin_aliases);
            }
        }
        JsExpr::Call { callee, args, .. } if string_method_name(callee).is_some() => {
            if let JsExpr::Member { object, .. } = callee.as_ref() {
                mark_ident_param_kind(object, param_index, kinds, AotSlotKind::String);
                infer_expr_param_kinds(object, param_index, kinds, builtin_aliases);
            }
            if matches!(
                string_method_name(callee),
                Some(
                    "includes"
                        | "indexOf"
                        | "lastIndexOf"
                        | "replace"
                        | "replaceAll"
                        | "startsWith"
                        | "endsWith"
                )
            ) {
                if let Some(arg) = args.first() {
                    mark_ident_param_kind(arg, param_index, kinds, AotSlotKind::String);
                }
            }
            if matches!(string_method_name(callee), Some("replace" | "replaceAll")) {
                if let Some(arg) = args.get(1) {
                    mark_ident_param_kind(arg, param_index, kinds, AotSlotKind::String);
                }
            }
            for arg in args {
                infer_expr_param_kinds(arg, param_index, kinds, builtin_aliases);
            }
        }
        JsExpr::Call { callee, args, .. }
            if is_string_cast_call(callee, args) || is_boolean_cast_call(callee, args) =>
        {
            mark_ident_param_kind(&args[0], param_index, kinds, AotSlotKind::Any);
            infer_expr_param_kinds(&args[0], param_index, kinds, builtin_aliases);
        }
        JsExpr::Call { callee, args, .. } if is_uri_string_call(callee, args) => {
            mark_ident_param_kind(&args[0], param_index, kinds, AotSlotKind::String);
            infer_expr_param_kinds(&args[0], param_index, kinds, builtin_aliases);
        }
        JsExpr::Call { callee, args, .. } if is_array_is_array_call(callee, args) => {
            mark_ident_param_kind(&args[0], param_index, kinds, AotSlotKind::Any);
            infer_expr_param_kinds(&args[0], param_index, kinds, builtin_aliases);
        }
        JsExpr::Call { callee, args, .. }
            if is_array_is_array_alias_call_in_context(callee, args, builtin_aliases) =>
        {
            mark_ident_param_kind(&args[0], param_index, kinds, AotSlotKind::Any);
            infer_expr_param_kinds(&args[0], param_index, kinds, builtin_aliases);
        }
        JsExpr::Call { callee, args, .. }
            if is_object_has_own_property_call_in_context(callee, args, builtin_aliases) =>
        {
            mark_ident_param_kind(&args[0], param_index, kinds, AotSlotKind::Any);
            mark_ident_param_kind(&args[1], param_index, kinds, AotSlotKind::Any);
            infer_expr_param_kinds(&args[0], param_index, kinds, builtin_aliases);
            infer_expr_param_kinds(&args[1], param_index, kinds, builtin_aliases);
        }
        JsExpr::Call { callee, args, .. } if is_object_prototype_to_string_call(callee, args) => {
            mark_ident_param_kind(&args[0], param_index, kinds, AotSlotKind::Any);
            infer_expr_param_kinds(&args[0], param_index, kinds, builtin_aliases);
        }
        JsExpr::Call { callee, args, .. }
            if is_object_to_string_alias_call_in_context(callee, args, builtin_aliases) =>
        {
            mark_ident_param_kind(&args[0], param_index, kinds, AotSlotKind::Any);
            infer_expr_param_kinds(&args[0], param_index, kinds, builtin_aliases);
        }
        JsExpr::Call { callee, args, .. }
            if is_regexp_test_alias_call_in_context(callee, args, builtin_aliases) =>
        {
            mark_ident_param_kind(&args[0], param_index, kinds, AotSlotKind::RegExp);
            mark_ident_param_kind(&args[1], param_index, kinds, AotSlotKind::String);
            infer_expr_param_kinds(&args[0], param_index, kinds, builtin_aliases);
            infer_expr_param_kinds(&args[1], param_index, kinds, builtin_aliases);
        }
        JsExpr::Call { callee, args, .. }
            if is_date_to_iso_alias_call_in_context(callee, args, builtin_aliases) =>
        {
            mark_ident_param_kind(&args[0], param_index, kinds, AotSlotKind::Date);
            infer_expr_param_kinds(&args[0], param_index, kinds, builtin_aliases);
        }
        JsExpr::Call { callee, args, .. }
            if is_array_push_apply_call_in_context(callee, args, builtin_aliases) =>
        {
            mark_ident_param_kind(&args[0], param_index, kinds, AotSlotKind::AnyArray);
            mark_ident_param_kind(&args[1], param_index, kinds, AotSlotKind::Any);
            infer_expr_param_kinds(&args[0], param_index, kinds, builtin_aliases);
            infer_expr_param_kinds(&args[1], param_index, kinds, builtin_aliases);
        }
        JsExpr::Call { callee, args, .. }
            if is_array_prototype_alias_call_in_context(
                callee,
                args,
                builtin_aliases,
                AotBuiltinFunctionAlias::ArrayConcat,
            ) =>
        {
            for arg in args {
                mark_ident_param_kind(arg, param_index, kinds, AotSlotKind::Any);
                infer_expr_param_kinds(arg, param_index, kinds, builtin_aliases);
            }
        }
        JsExpr::Call { callee, args, .. }
            if is_array_prototype_alias_call_in_context(
                callee,
                args,
                builtin_aliases,
                AotBuiltinFunctionAlias::ArrayJoin,
            ) =>
        {
            mark_ident_param_kind(&args[0], param_index, kinds, AotSlotKind::Any);
            if let Some(separator) = args.get(1) {
                mark_ident_param_kind(separator, param_index, kinds, AotSlotKind::String);
            }
            for arg in args {
                infer_expr_param_kinds(arg, param_index, kinds, builtin_aliases);
            }
        }
        JsExpr::Call { callee, args, .. }
            if is_array_prototype_alias_call_in_context(
                callee,
                args,
                builtin_aliases,
                AotBuiltinFunctionAlias::ArraySlice,
            ) =>
        {
            mark_ident_param_kind(&args[0], param_index, kinds, AotSlotKind::Any);
            for arg in args {
                infer_expr_param_kinds(arg, param_index, kinds, builtin_aliases);
            }
        }
        JsExpr::Call { callee, args, .. } if is_array_concat_call_shape(callee) => {
            if let JsExpr::Member { object, .. } = callee.as_ref() {
                mark_ident_param_kind(object, param_index, kinds, AotSlotKind::AnyArray);
                infer_expr_param_kinds(object, param_index, kinds, builtin_aliases);
            }
            for arg in args {
                mark_ident_param_kind(arg, param_index, kinds, AotSlotKind::Any);
                infer_expr_param_kinds(arg, param_index, kinds, builtin_aliases);
            }
        }
        JsExpr::Call { callee, args, .. } if is_array_fill_call_shape(callee) => {
            if let JsExpr::Member { object, .. } = callee.as_ref() {
                mark_ident_param_kind(object, param_index, kinds, AotSlotKind::AnyArray);
                infer_expr_param_kinds(object, param_index, kinds, builtin_aliases);
            }
            if let Some(value) = args.first() {
                mark_ident_param_kind(value, param_index, kinds, AotSlotKind::Any);
            }
            for arg in args {
                infer_expr_param_kinds(arg, param_index, kinds, builtin_aliases);
            }
        }
        JsExpr::Call { callee, args, .. } if is_object_keys_call(callee, args) => {
            mark_ident_param_kind(&args[0], param_index, kinds, AotSlotKind::Any);
            infer_expr_param_kinds(&args[0], param_index, kinds, builtin_aliases);
        }
        JsExpr::Call { callee, args, .. } if is_string_replace_alias_call_shape(callee, args) => {
            mark_ident_param_kind(&args[0], param_index, kinds, AotSlotKind::String);
            infer_expr_param_kinds(&args[0], param_index, kinds, builtin_aliases);
            for arg in args.iter().skip(1) {
                infer_expr_param_kinds(arg, param_index, kinds, builtin_aliases);
            }
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
            infer_expr_param_kinds(callee, param_index, kinds, builtin_aliases);
            for arg in args {
                infer_expr_param_kinds(arg, param_index, kinds, builtin_aliases);
            }
        }
        JsExpr::Call { callee, args, .. } if is_regexp_test_call(callee, args) => {
            mark_ident_param_kind(&args[0], param_index, kinds, AotSlotKind::Any);
            infer_expr_param_kinds(&args[0], param_index, kinds, builtin_aliases);
        }
        JsExpr::Call { callee, args, .. } => {
            mark_ident_param_kind(callee, param_index, kinds, AotSlotKind::Any);
            infer_expr_param_kinds(callee, param_index, kinds, builtin_aliases);
            for arg in args {
                infer_expr_param_kinds(arg, param_index, kinds, builtin_aliases);
            }
        }
        JsExpr::Member {
            object,
            property,
            property_expr,
            ..
        } => {
            if let JsExpr::Ident { name } = object.as_ref() {
                let indexed_member = property_expr.is_some() || property.parse::<usize>().is_ok();
                if indexed_member && !is_length_member_property(property, property_expr.as_deref())
                {
                    if let Some(index) = param_index.get(name) {
                        if property.parse::<usize>().is_ok()
                            || property_expr
                                .as_deref()
                                .is_some_and(is_numeric_property_key_expr)
                        {
                            if kinds[*index] != AotSlotKind::Any
                                && kinds[*index] != AotSlotKind::String
                            {
                                kinds[*index] = AotSlotKind::AnyArray;
                            }
                        } else if kinds[*index] != AotSlotKind::Any {
                            kinds[*index] = AotSlotKind::Any;
                        }
                    }
                }
            }
            if let Some(property_expr) = property_expr {
                if !matches!(object.as_ref(), JsExpr::Ident { name } if param_index.contains_key(name))
                {
                    mark_ident_param_kind(property_expr, param_index, kinds, AotSlotKind::String);
                }
            }
            infer_expr_param_kinds(object, param_index, kinds, builtin_aliases);
            if let Some(property_expr) = property_expr {
                infer_expr_param_kinds(property_expr, param_index, kinds, builtin_aliases);
            }
        }
        JsExpr::Array { items } => {
            for item in items {
                infer_expr_param_kinds(item, param_index, kinds, builtin_aliases);
            }
        }
        JsExpr::ArraySpread { items } => {
            for item in items {
                infer_expr_param_kinds(&item.value, param_index, kinds, builtin_aliases);
            }
        }
        JsExpr::Object { props } => {
            for prop in props {
                mark_ident_param_kind(&prop.value, param_index, kinds, AotSlotKind::Any);
                infer_expr_param_kinds(&prop.value, param_index, kinds, builtin_aliases);
            }
        }
        JsExpr::Unary { arg, .. }
        | JsExpr::Await { arg }
        | JsExpr::Update { arg, .. }
        | JsExpr::Spread { arg }
        | JsExpr::ObjectRest { object: arg, .. } => {
            infer_expr_param_kinds(arg, param_index, kinds, builtin_aliases)
        }
        JsExpr::Conditional {
            test,
            consequent,
            alternate,
        } => {
            infer_bool_context_param_kinds(test, param_index, kinds, builtin_aliases);
            infer_expr_param_kinds(consequent, param_index, kinds, builtin_aliases);
            infer_expr_param_kinds(alternate, param_index, kinds, builtin_aliases);
        }
        JsExpr::New { callee, args } if matches!(callee.as_ref(), JsExpr::Ident { name } if name == "RegExp") =>
        {
            if let Some(pattern) = args.first() {
                mark_ident_param_kind(pattern, param_index, kinds, AotSlotKind::String);
                infer_expr_param_kinds(pattern, param_index, kinds, builtin_aliases);
            }
            if let Some(flags) = args.get(1) {
                infer_expr_param_kinds(flags, param_index, kinds, builtin_aliases);
            }
        }
        JsExpr::New { callee, args } => {
            infer_expr_param_kinds(callee, param_index, kinds, builtin_aliases);
            for arg in args {
                infer_expr_param_kinds(arg, param_index, kinds, builtin_aliases);
            }
        }
        JsExpr::Template { exprs, .. } | JsExpr::Sequence { exprs } => {
            if let JsExpr::Template { exprs, .. } = expr {
                for item in exprs {
                    mark_ident_param_kind_if_default(item, param_index, kinds, AotSlotKind::Any);
                }
            }
            for expr in exprs {
                infer_expr_param_kinds(expr, param_index, kinds, builtin_aliases);
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
            if let JsExpr::Member {
                object,
                property,
                property_expr,
                optional: false,
            } = candidate
            {
                if property.parse::<usize>().is_ok()
                    || property_expr
                        .as_deref()
                        .is_some_and(is_numeric_property_key_expr)
                {
                    mark_ident_param_kind(object, param_index, kinds, AotSlotKind::String);
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

fn mark_assigned_member_object_param_kind(
    expr: &JsExpr,
    param_index: &BTreeMap<String, usize>,
    kinds: &mut [AotSlotKind],
) {
    let JsExpr::Member {
        object,
        property,
        property_expr,
        optional: false,
    } = expr
    else {
        return;
    };
    let JsExpr::Ident { name } = object.as_ref() else {
        return;
    };
    let Some(index) = param_index.get(name) else {
        return;
    };
    if property.parse::<usize>().is_ok()
        || property_expr
            .as_deref()
            .is_some_and(is_numeric_property_key_expr)
    {
        kinds[*index] = AotSlotKind::Any;
        return;
    }
    kinds[*index] = AotSlotKind::Any;
}

fn infer_bool_context_param_kinds(
    expr: &JsExpr,
    param_index: &BTreeMap<String, usize>,
    kinds: &mut [AotSlotKind],
    builtin_aliases: &BTreeMap<String, AotBuiltinFunctionAlias>,
) {
    match expr {
        JsExpr::Ident { .. } => mark_ident_param_kind(expr, param_index, kinds, AotSlotKind::Any),
        JsExpr::Unary { op, arg } if op == "!" => {
            infer_bool_context_param_kinds(arg, param_index, kinds, builtin_aliases);
        }
        JsExpr::Binary { op, left, right } if matches!(op.as_str(), "&&" | "||") => {
            infer_bool_context_param_kinds(left, param_index, kinds, builtin_aliases);
            infer_bool_context_param_kinds(right, param_index, kinds, builtin_aliases);
        }
        _ => infer_expr_param_kinds(expr, param_index, kinds, builtin_aliases),
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

fn is_number_literal_like(expr: &JsExpr) -> bool {
    matches!(
        expr,
        JsExpr::Value {
            value: JsValue::Number { .. },
        }
    )
}

fn is_string_result_shape(expr: &JsExpr) -> bool {
    match expr {
        expr if is_string_literal_like(expr) => true,
        JsExpr::Template { .. } => true,
        JsExpr::Binary { op, left, right } if op == "+" => {
            is_string_result_shape(left) || is_string_result_shape(right)
        }
        JsExpr::Call { callee, args, .. } if is_string_cast_call(callee, args) => true,
        JsExpr::Call { callee, .. } => matches!(
            callee.as_ref(),
            JsExpr::Member {
                property,
                property_expr: None,
                optional: false,
                ..
            } if matches!(
                property.as_str(),
                "join"
                    | "replace"
                    | "slice"
                    | "substring"
                    | "substr"
                    | "trim"
                    | "trimStart"
                    | "trimEnd"
                    | "repeat"
                    | "replaceAll"
                    | "toLowerCase"
                    | "toUpperCase"
                    | "charAt"
                    | "at"
            )
        ),
        JsExpr::Conditional {
            consequent,
            alternate,
            ..
        } => is_string_result_shape(consequent) && is_string_result_shape(alternate),
        _ => false,
    }
}

fn is_array_destructure_source_expr(expr: &JsExpr, state: &AotState) -> bool {
    match expr {
        JsExpr::Array { .. } => true,
        JsExpr::Ident { name } if name == "__tsgodown_forof_value" => true,
        JsExpr::Ident { name } if state.any_array_bindings.contains(name) => true,
        JsExpr::Ident { name } if is_any_binding(name, state) => true,
        _ => false,
    }
}

fn template_part_needs_to_string_helper(expr: &JsExpr) -> bool {
    match expr {
        JsExpr::Value {
            value: JsValue::String { .. } | JsValue::Undefined | JsValue::Null,
        } => false,
        JsExpr::Ident { name } if name == "undefined" => false,
        _ => true,
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

fn mark_ident_param_kind_if_default(
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
    if kinds[*index] == AotSlotKind::Number {
        kinds[*index] = kind;
    }
}

fn render_function_decl(function: &AotFunction, state: &AotState) -> Option<String> {
    if function.generator {
        return None;
    }
    let _async_lowered_synchronously = function.r#async;
    if let Some(rendered) = render_number_closure_function_decl(function) {
        return Some(rendered);
    }
    let mutated_any_array_param = function_mutated_any_array_param(function, state);
    let returns_any_array = function_returns_any_array(function, state);
    let mut function_state = clone_aot_state(state);
    for (param, kind) in function.params.iter().zip(function.param_kinds.iter()) {
        function_state.bind_slot(param, sanitize_go_identifier(param), *kind);
    }
    if let Some(rest_param) = &function.rest_param {
        function_state.bind_slot(
            rest_param,
            sanitize_go_identifier(rest_param),
            AotSlotKind::AnyArray,
        );
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
    let rendered_params = if let Some(rest_param) = &function.rest_param {
        let rest = format!("{} ...any", sanitize_go_identifier(rest_param));
        if rendered_params.is_empty() {
            rest
        } else {
            format!("{rendered_params}, {rest}")
        }
    } else {
        rendered_params
    };
    let function_body = render_function_body(&function.body, &function_state)?;
    let return_type = if mutated_any_array_param.is_some() || returns_any_array {
        "[]any"
    } else {
        "any"
    };
    let function_body = if let Some(index) = mutated_any_array_param {
        let param = function.params.get(index)?;
        format!("{function_body}\nreturn {}", sanitize_go_identifier(param))
    } else if returns_any_array || function_body.trim_end().ends_with("return nil") {
        function_body
    } else {
        format!("{function_body}\nreturn nil")
    };
    Some(format!(
        "func {}({rendered_params}) {return_type} {{\n{}\n}}",
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
    if let Some(rest_param) = &function.rest_param {
        function_state.bind_slot(
            rest_param,
            sanitize_go_identifier(rest_param),
            AotSlotKind::AnyArray,
        );
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
        param_kinds: infer_function_param_kinds(parts.params, parts.body, &BTreeMap::new()),
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
    if function.generator {
        return None;
    }
    let _async_lowered_synchronously = function.r#async;
    let mutated_any_array_param = function_mutated_any_array_param(function, state);
    let returns_any_array = function_returns_any_array(function, state);
    let mut function_state = clone_aot_state(state);
    for (param, kind) in function.params.iter().zip(function.param_kinds.iter()) {
        function_state.bind_slot(param, sanitize_go_identifier(param), *kind);
    }
    if let Some(rest_param) = &function.rest_param {
        function_state.bind_slot(
            rest_param,
            sanitize_go_identifier(rest_param),
            AotSlotKind::AnyArray,
        );
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
    let rendered_params = if let Some(rest_param) = &function.rest_param {
        let rest = format!("{} ...any", sanitize_go_identifier(rest_param));
        if rendered_params.is_empty() {
            rest
        } else {
            format!("{rendered_params}, {rest}")
        }
    } else {
        rendered_params
    };
    let function_body = render_function_body(&function.body, &function_state)?;
    let return_type = if mutated_any_array_param.is_some() || returns_any_array {
        "[]any"
    } else {
        "any"
    };
    let function_body = if let Some(index) = mutated_any_array_param {
        let param = function.params.get(index)?;
        format!("{function_body}\nreturn {}", sanitize_go_identifier(param))
    } else if returns_any_array || function_body.trim_end().ends_with("return nil") {
        function_body
    } else {
        format!("{function_body}\nreturn nil")
    };
    let function_type = format!(
        "func({}) {return_type}",
        if let Some(rest_param) = &function.rest_param {
            let fixed = function
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
            let rest = format!("{} ...any", sanitize_go_identifier(rest_param));
            if fixed.is_empty() {
                rest
            } else {
                format!("{fixed}, {rest}")
            }
        } else {
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
        }
    );
    Some(format!(
        "var {} {function_type}\n{} = func({rendered_params}) {return_type} {{\n{}\n}}",
        function.go_name,
        function.go_name,
        indent_lines(&function_body)
    ))
}

fn function_mutated_any_array_param(function: &AotFunction, state: &AotState) -> Option<usize> {
    if function
        .body
        .iter()
        .any(|stmt| matches!(stmt, JsStmt::Return { .. }))
    {
        return None;
    }
    let mut mutated = BTreeSet::new();
    for stmt in &function.body {
        collect_mutated_any_array_params(stmt, function, state, &mut mutated);
    }
    if mutated.len() == 1 {
        mutated.iter().next().copied()
    } else {
        None
    }
}

fn collect_mutated_any_array_params(
    stmt: &JsStmt,
    function: &AotFunction,
    state: &AotState,
    mutated: &mut BTreeSet<usize>,
) {
    match stmt {
        JsStmt::Expr { expr } => {
            collect_mutated_any_array_params_expr(expr, function, state, mutated)
        }
        JsStmt::If {
            consequent,
            alternate,
            ..
        } => {
            for stmt in consequent {
                collect_mutated_any_array_params(stmt, function, state, mutated);
            }
            for stmt in alternate {
                collect_mutated_any_array_params(stmt, function, state, mutated);
            }
        }
        JsStmt::For { body, .. } | JsStmt::While { body, .. } | JsStmt::DoWhile { body, .. } => {
            for stmt in body {
                collect_mutated_any_array_params(stmt, function, state, mutated);
            }
        }
        JsStmt::Try {
            body,
            catch_body,
            finally_body,
            ..
        } => {
            for stmt in body {
                collect_mutated_any_array_params(stmt, function, state, mutated);
            }
            for stmt in catch_body {
                collect_mutated_any_array_params(stmt, function, state, mutated);
            }
            for stmt in finally_body {
                collect_mutated_any_array_params(stmt, function, state, mutated);
            }
        }
        _ => {}
    }
}

fn collect_mutated_any_array_params_expr(
    expr: &JsExpr,
    function: &AotFunction,
    state: &AotState,
    mutated: &mut BTreeSet<usize>,
) {
    match expr {
        JsExpr::Call { callee, args, .. } => {
            let target = any_array_candidate_push_target(callee, args).or_else(|| {
                is_array_push_apply_call(callee, args, state).then(|| {
                    let JsExpr::Ident { name } = args.first().expect("checked len") else {
                        return "";
                    };
                    name.as_str()
                })
            });
            if let Some(target) = target {
                for (index, (param, kind)) in function
                    .params
                    .iter()
                    .zip(function.param_kinds.iter())
                    .enumerate()
                {
                    if param == target && *kind == AotSlotKind::AnyArray {
                        mutated.insert(index);
                    }
                }
            }
        }
        JsExpr::Assign { left, right, .. } | JsExpr::Binary { left, right, .. } => {
            collect_mutated_any_array_params_expr(left, function, state, mutated);
            collect_mutated_any_array_params_expr(right, function, state, mutated);
        }
        JsExpr::Conditional {
            test,
            consequent,
            alternate,
        } => {
            collect_mutated_any_array_params_expr(test, function, state, mutated);
            collect_mutated_any_array_params_expr(consequent, function, state, mutated);
            collect_mutated_any_array_params_expr(alternate, function, state, mutated);
        }
        _ => {}
    }
}

fn cjs_default_function_expr(stmt: &JsStmt) -> Option<AotInlineFunctionParts<'_>> {
    let expr = cjs_default_export_value_expr(stmt)?;
    let JsExpr::Function {
        params,
        rest_param,
        r#async,
        generator,
        body,
        ..
    } = expr
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

fn cjs_default_export_value_expr(stmt: &JsStmt) -> Option<&JsExpr> {
    let JsStmt::Expr { expr } = stmt else {
        return None;
    };
    cjs_default_export_value_from_assignment(expr)
}

fn cjs_default_export_value_from_assignment(expr: &JsExpr) -> Option<&JsExpr> {
    let JsExpr::Assign { op, left, right } = expr else {
        return None;
    };
    if op != "=" {
        return None;
    }
    if is_module_exports_member(left) {
        return Some(right);
    }
    if is_exports_ident(left) {
        return cjs_default_export_value_from_assignment(right);
    }
    None
}

fn render_function_body(body: &[JsStmt], state: &AotState) -> Option<String> {
    let mut function_state = clone_aot_state(state);
    mark_number_array_locals(body, &mut function_state);
    mark_string_array_locals(body, &mut function_state);
    mark_logical_assignment_any_locals(body, &mut function_state);
    mark_any_array_locals(body, &mut function_state);
    mark_array_property_locals(body, &mut function_state);
    mark_dynamic_object_locals(body, &mut function_state);
    let mut local_functions = Vec::new();
    for stmt in body {
        if let JsStmt::FunctionDecl { name, .. } = stmt {
            let function = aot_function_from_stmt(stmt, name)?;
            function_state.functions.insert(name.clone(), function);
            local_functions.push(name.clone());
        }
    }
    let mut rendered = Vec::new();
    for name in local_functions {
        let function = function_state.functions.get(&name)?;
        rendered.push(render_local_function_decl(function, &function_state)?);
    }
    for stmt in body {
        if matches!(stmt, JsStmt::FunctionDecl { .. }) {
            continue;
        }
        rendered.push(render_function_stmt(stmt, &mut function_state)?);
    }
    Some(rendered.join("\n"))
}

fn mark_dynamic_object_locals(stmts: &[JsStmt], state: &mut AotState) {
    mark_number_array_locals(stmts, state);
    mark_string_array_locals(stmts, state);
    mark_any_array_locals(stmts, state);
    mark_array_property_locals(stmts, state);
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
            || state.numeric_bindings.contains(&name)
            || state.string_bindings.contains(&name)
            || state.bool_bindings.contains(&name)
            || state.bytes_bindings.contains(&name)
            || state.number_array_bindings.contains(&name)
            || state.string_array_bindings.contains(&name)
            || state.any_array_bindings.contains(&name)
            || state.map_bindings.contains(&name)
            || state.set_bindings.contains(&name)
            || state.url_bindings.contains(&name)
            || state.event_emitter_bindings.contains(&name)
            || state.number_closure_bindings.contains(&name)
            || state.string_function_bindings.contains(&name)
            || state.class_instance_bindings.contains_key(&name)
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
            .entry(name.clone())
            .or_insert_with(|| sanitize_go_identifier(&name));
        state.dynamic_object_bindings.insert(name);
    }
}

fn mark_array_property_locals(stmts: &[JsStmt], state: &mut AotState) {
    let mut candidates = BTreeSet::new();
    collect_array_property_candidates(stmts, &mut candidates);
    for name in candidates {
        if state.number_array_bindings.contains(&name)
            || state.string_array_bindings.contains(&name)
            || state.any_array_bindings.contains(&name)
            || state.bytes_bindings.contains(&name)
        {
            state.array_property_bindings.insert(name);
        }
    }
}

fn collect_array_property_candidates(stmts: &[JsStmt], candidates: &mut BTreeSet<String>) {
    for stmt in stmts {
        match stmt {
            JsStmt::Expr { expr } | JsStmt::Return { value: Some(expr) } => {
                collect_array_property_candidates_expr(expr, candidates);
            }
            JsStmt::VarDecl {
                init: Some(expr), ..
            } => collect_array_property_candidates_expr(expr, candidates),
            JsStmt::FunctionDecl { body, .. } => {
                collect_array_property_candidates(body, candidates)
            }
            JsStmt::If {
                test,
                consequent,
                alternate,
            } => {
                collect_array_property_candidates_expr(test, candidates);
                collect_array_property_candidates(consequent, candidates);
                collect_array_property_candidates(alternate, candidates);
            }
            JsStmt::For {
                init,
                test,
                update,
                body,
            } => {
                collect_array_property_candidates(init, candidates);
                if let Some(test) = test {
                    collect_array_property_candidates_expr(test, candidates);
                }
                if let Some(update) = update {
                    collect_array_property_candidates_expr(update, candidates);
                }
                collect_array_property_candidates(body, candidates);
            }
            JsStmt::ForOf { right, body, .. } => {
                collect_array_property_candidates_expr(right, candidates);
                collect_array_property_candidates(body, candidates);
            }
            JsStmt::While { test, body } | JsStmt::DoWhile { test, body } => {
                collect_array_property_candidates_expr(test, candidates);
                collect_array_property_candidates(body, candidates);
            }
            JsStmt::Switch {
                discriminant,
                cases,
            } => {
                collect_array_property_candidates_expr(discriminant, candidates);
                for case in cases {
                    if let Some(test) = &case.test {
                        collect_array_property_candidates_expr(test, candidates);
                    }
                    collect_array_property_candidates(&case.consequent, candidates);
                }
            }
            JsStmt::Try {
                body,
                catch_body,
                finally_body,
                ..
            } => {
                collect_array_property_candidates(body, candidates);
                collect_array_property_candidates(catch_body, candidates);
                collect_array_property_candidates(finally_body, candidates);
            }
            _ => {}
        }
    }
}

fn collect_array_property_candidates_expr(expr: &JsExpr, candidates: &mut BTreeSet<String>) {
    match expr {
        JsExpr::Assign { left, right, .. } => {
            if let Some(name) = array_property_member_target(left) {
                candidates.insert(name.to_string());
            }
            collect_array_property_candidates_expr(left, candidates);
            collect_array_property_candidates_expr(right, candidates);
        }
        JsExpr::Call { callee, args, .. } => {
            if is_object_keys_call(callee, args) {
                if let Some(JsExpr::Ident { name }) = args.first() {
                    candidates.insert(name.clone());
                }
            }
            if is_object_has_own_property_call_shape(callee, args) {
                if let Some(JsExpr::Ident { name }) = args.first() {
                    candidates.insert(name.clone());
                }
            }
            collect_array_property_candidates_expr(callee, candidates);
            for arg in args {
                collect_array_property_candidates_expr(arg, candidates);
            }
        }
        JsExpr::Member {
            object,
            property,
            property_expr,
            ..
        } => {
            if let Some(name) =
                array_property_member_name(object, property, property_expr.as_deref())
            {
                candidates.insert(name.to_string());
            }
            collect_array_property_candidates_expr(object, candidates);
            if let Some(property_expr) = property_expr {
                collect_array_property_candidates_expr(property_expr, candidates);
            }
        }
        JsExpr::Binary { left, right, .. } => {
            collect_array_property_candidates_expr(left, candidates);
            collect_array_property_candidates_expr(right, candidates);
        }
        JsExpr::Array { items } => {
            for item in items {
                collect_array_property_candidates_expr(item, candidates);
            }
        }
        JsExpr::ArraySpread { items } => {
            for item in items {
                collect_array_property_candidates_expr(&item.value, candidates);
            }
        }
        JsExpr::Object { props } => {
            for prop in props {
                if let Some(key_expr) = &prop.key_expr {
                    collect_array_property_candidates_expr(key_expr, candidates);
                }
                collect_array_property_candidates_expr(&prop.value, candidates);
            }
        }
        JsExpr::Function { body, .. } => collect_array_property_candidates(body, candidates),
        JsExpr::Unary { arg, .. }
        | JsExpr::Await { arg }
        | JsExpr::Update { arg, .. }
        | JsExpr::Spread { arg }
        | JsExpr::ObjectRest { object: arg, .. } => {
            collect_array_property_candidates_expr(arg, candidates);
        }
        JsExpr::Conditional {
            test,
            consequent,
            alternate,
        } => {
            collect_array_property_candidates_expr(test, candidates);
            collect_array_property_candidates_expr(consequent, candidates);
            collect_array_property_candidates_expr(alternate, candidates);
        }
        JsExpr::Template { exprs, .. } | JsExpr::Sequence { exprs } => {
            for expr in exprs {
                collect_array_property_candidates_expr(expr, candidates);
            }
        }
        _ => {}
    }
}

fn array_property_member_target(expr: &JsExpr) -> Option<&str> {
    let JsExpr::Member {
        object,
        property,
        property_expr,
        optional: false,
    } = expr
    else {
        return None;
    };
    array_property_member_name(object, property, property_expr.as_deref())
}

fn array_property_member_name<'a>(
    object: &'a JsExpr,
    property: &str,
    property_expr: Option<&JsExpr>,
) -> Option<&'a str> {
    if is_length_member_property(property, property_expr)
        || is_numeric_member_index_shape(property, property_expr)
        || is_collection_member_property(property)
    {
        return None;
    }
    let JsExpr::Ident { name } = object else {
        return None;
    };
    Some(name.as_str())
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
            JsStmt::ForOf { right, body, .. } => {
                collect_dynamic_object_candidates_expr(right, candidates);
                collect_dynamic_object_candidates(body, candidates);
            }
            JsStmt::While { test, body } | JsStmt::DoWhile { test, body } => {
                collect_dynamic_object_candidates_expr(test, candidates);
                collect_dynamic_object_candidates(body, candidates);
            }
            JsStmt::Switch {
                discriminant,
                cases,
            } => {
                collect_dynamic_object_candidates_expr(discriminant, candidates);
                for case in cases {
                    if let Some(test) = &case.test {
                        collect_dynamic_object_candidates_expr(test, candidates);
                    }
                    collect_dynamic_object_candidates(&case.consequent, candidates);
                }
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
            if matches!(op.as_str(), "=" | "??=" | "|=") {
                if let Some(name) = dynamic_object_assignment_target(left) {
                    candidates.insert(name.to_string());
                }
                if let Some(name) = dynamic_object_array_assignment_target(left) {
                    candidates.insert(name.to_string());
                }
            }
            collect_dynamic_object_candidates_expr(left, candidates);
            collect_dynamic_object_candidates_expr(right, candidates);
        }
        JsExpr::Call { callee, args, .. } => {
            if let Some(name) = ts_enum_iife_target(callee, args) {
                candidates.insert(name.to_string());
            }
            if is_object_keys_call(callee, args) {
                if let Some(JsExpr::Ident { name }) = args.first() {
                    candidates.insert(name.clone());
                }
            }
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
        JsExpr::Binary { op, left, right } => {
            if op == "in" {
                if let JsExpr::Ident { name } = right.as_ref() {
                    candidates.insert(name.clone());
                }
            }
            collect_dynamic_object_candidates_expr(left, candidates);
            collect_dynamic_object_candidates_expr(right, candidates);
        }
        JsExpr::Unary { op, arg } => {
            if op == "delete" {
                if let JsExpr::Member { object, .. } = arg.as_ref() {
                    if let JsExpr::Ident { name } = object.as_ref() {
                        candidates.insert(name.clone());
                    }
                }
            }
            collect_dynamic_object_candidates_expr(arg, candidates);
        }
        JsExpr::Await { arg }
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
            JsStmt::ForOf { right, body, .. } => {
                collect_dynamic_object_member_read_candidates_expr(right, candidates);
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
            JsStmt::ForOf { body, .. } => {
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
        property,
        property_expr,
        optional: false,
    } = expr
    else {
        return None;
    };
    if property.parse::<usize>().is_ok()
        || property_expr
            .as_deref()
            .is_some_and(is_numeric_property_key_expr)
    {
        return None;
    }
    let JsExpr::Ident { name } = object.as_ref() else {
        return None;
    };
    Some(name)
}

fn dynamic_object_array_assignment_target(expr: &JsExpr) -> Option<&str> {
    let JsExpr::Member {
        object,
        property,
        property_expr,
        optional: false,
    } = expr
    else {
        return None;
    };
    if is_length_member_property(property, property_expr.as_deref())
        || !is_numeric_member_index_shape(property, property_expr.as_deref())
    {
        return None;
    }
    let JsExpr::Member { object, .. } = object.as_ref() else {
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
        "at" | "concat"
            | "copyWithin"
            | "entries"
            | "every"
            | "fill"
            | "filter"
            | "find"
            | "findIndex"
            | "flat"
            | "forEach"
            | "get"
            | "has"
            | "includes"
            | "indexOf"
            | "join"
            | "keys"
            | "lastIndexOf"
            | "length"
            | "map"
            | "pop"
            | "push"
            | "reduce"
            | "reduceRight"
            | "reverse"
            | "set"
            | "shift"
            | "size"
            | "slice"
            | "some"
            | "sort"
            | "splice"
            | "unshift"
            | "values"
            | "delete"
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
    let mut any_candidates = BTreeSet::new();
    collect_any_array_candidates(stmts, state, &mut any_candidates);
    for name in candidates {
        if any_candidates.contains(&name) {
            continue;
        }
        let go_ref = state
            .binding_refs
            .get(&name)
            .cloned()
            .unwrap_or_else(|| sanitize_go_identifier(&name));
        state.bind_slot(&name, go_ref, AotSlotKind::NumberArray);
    }
}

fn mark_string_array_locals(stmts: &[JsStmt], state: &mut AotState) {
    let mut candidates = BTreeSet::new();
    collect_string_array_candidates(stmts, state, &mut candidates);
    for name in candidates {
        if state.number_array_bindings.contains(&name)
            || state.any_array_bindings.contains(&name)
            || state.bytes_bindings.contains(&name)
            || state.map_bindings.contains(&name)
            || state.set_bindings.contains(&name)
            || state.object_bindings.contains_key(&name)
            || state.class_instance_bindings.contains_key(&name)
        {
            continue;
        }
        if state.bindings.contains(&name) {
            if is_any_binding(&name, state) {
                continue;
            }
            state.string_array_bindings.insert(name);
            continue;
        }
        state.bind_slot(
            &name,
            sanitize_go_identifier(&name),
            AotSlotKind::StringArray,
        );
    }
}

fn mark_logical_assignment_any_locals(stmts: &[JsStmt], state: &mut AotState) {
    let mut candidates = BTreeSet::new();
    collect_logical_assignment_targets(stmts, &mut candidates);
    for name in candidates {
        let go_ref = state
            .binding_refs
            .get(&name)
            .cloned()
            .unwrap_or_else(|| sanitize_go_identifier(&name));
        state.bind_slot(&name, go_ref, AotSlotKind::Any);
        state.logical_assignment_bindings.insert(name);
    }
}

fn collect_logical_assignment_targets(stmts: &[JsStmt], candidates: &mut BTreeSet<String>) {
    for stmt in stmts {
        match stmt {
            JsStmt::Expr { expr } | JsStmt::Return { value: Some(expr) } => {
                collect_logical_assignment_targets_expr(expr, candidates);
            }
            JsStmt::VarDecl {
                init: Some(expr), ..
            } => collect_logical_assignment_targets_expr(expr, candidates),
            JsStmt::FunctionDecl { .. } => {}
            JsStmt::If {
                test,
                consequent,
                alternate,
            } => {
                collect_logical_assignment_targets_expr(test, candidates);
                collect_logical_assignment_targets(consequent, candidates);
                collect_logical_assignment_targets(alternate, candidates);
            }
            JsStmt::For {
                init,
                test,
                update,
                body,
            } => {
                collect_logical_assignment_targets(init, candidates);
                if let Some(test) = test {
                    collect_logical_assignment_targets_expr(test, candidates);
                }
                if let Some(update) = update {
                    collect_logical_assignment_targets_expr(update, candidates);
                }
                collect_logical_assignment_targets(body, candidates);
            }
            JsStmt::ForOf { right, body, .. } => {
                collect_logical_assignment_targets_expr(right, candidates);
                collect_logical_assignment_targets(body, candidates);
            }
            JsStmt::While { test, body } | JsStmt::DoWhile { test, body } => {
                collect_logical_assignment_targets_expr(test, candidates);
                collect_logical_assignment_targets(body, candidates);
            }
            JsStmt::Try {
                body,
                catch_body,
                finally_body,
                ..
            } => {
                collect_logical_assignment_targets(body, candidates);
                collect_logical_assignment_targets(catch_body, candidates);
                collect_logical_assignment_targets(finally_body, candidates);
            }
            _ => {}
        }
    }
}

fn collect_logical_assignment_targets_expr(expr: &JsExpr, candidates: &mut BTreeSet<String>) {
    match expr {
        JsExpr::Assign { op, left, right } => {
            if matches!(op.as_str(), "&&=" | "||=" | "??=") {
                if let JsExpr::Ident { name } = left.as_ref() {
                    candidates.insert(name.clone());
                }
            }
            collect_logical_assignment_targets_expr(left, candidates);
            collect_logical_assignment_targets_expr(right, candidates);
        }
        JsExpr::Binary { left, right, .. } => {
            collect_logical_assignment_targets_expr(left, candidates);
            collect_logical_assignment_targets_expr(right, candidates);
        }
        JsExpr::Call { callee, args, .. } => {
            collect_logical_assignment_targets_expr(callee, candidates);
            for arg in args {
                collect_logical_assignment_targets_expr(arg, candidates);
            }
        }
        JsExpr::Array { items } => {
            for item in items {
                collect_logical_assignment_targets_expr(item, candidates);
            }
        }
        JsExpr::ArraySpread { items } => {
            for item in items {
                collect_logical_assignment_targets_expr(&item.value, candidates);
            }
        }
        JsExpr::Object { props } => {
            for prop in props {
                if let Some(key_expr) = &prop.key_expr {
                    collect_logical_assignment_targets_expr(key_expr, candidates);
                }
                collect_logical_assignment_targets_expr(&prop.value, candidates);
            }
        }
        JsExpr::Function { .. } => {}
        JsExpr::Member {
            object,
            property_expr,
            ..
        } => {
            collect_logical_assignment_targets_expr(object, candidates);
            if let Some(property_expr) = property_expr {
                collect_logical_assignment_targets_expr(property_expr, candidates);
            }
        }
        JsExpr::Unary { arg, .. }
        | JsExpr::Update { arg, .. }
        | JsExpr::Await { arg }
        | JsExpr::Spread { arg }
        | JsExpr::ObjectRest { object: arg, .. } => {
            collect_logical_assignment_targets_expr(arg, candidates);
        }
        JsExpr::Conditional {
            test,
            consequent,
            alternate,
        } => {
            collect_logical_assignment_targets_expr(test, candidates);
            collect_logical_assignment_targets_expr(consequent, candidates);
            collect_logical_assignment_targets_expr(alternate, candidates);
        }
        JsExpr::Template { exprs, .. } | JsExpr::Sequence { exprs } => {
            for expr in exprs {
                collect_logical_assignment_targets_expr(expr, candidates);
            }
        }
        _ => {}
    }
}

fn collect_mutated_array_bindings(stmts: &[JsStmt], candidates: &mut BTreeSet<String>) {
    for stmt in stmts {
        match stmt {
            JsStmt::Expr { expr } | JsStmt::Return { value: Some(expr) } => {
                collect_mutated_array_bindings_expr(expr, candidates);
            }
            JsStmt::VarDecl {
                init: Some(expr), ..
            } => collect_mutated_array_bindings_expr(expr, candidates),
            JsStmt::FunctionDecl { body, .. } => collect_mutated_array_bindings(body, candidates),
            JsStmt::If {
                test,
                consequent,
                alternate,
            } => {
                collect_mutated_array_bindings_expr(test, candidates);
                collect_mutated_array_bindings(consequent, candidates);
                collect_mutated_array_bindings(alternate, candidates);
            }
            JsStmt::For {
                init,
                test,
                update,
                body,
            } => {
                collect_mutated_array_bindings(init, candidates);
                if let Some(test) = test {
                    collect_mutated_array_bindings_expr(test, candidates);
                }
                if let Some(update) = update {
                    collect_mutated_array_bindings_expr(update, candidates);
                }
                collect_mutated_array_bindings(body, candidates);
            }
            JsStmt::ForOf { right, body, .. } => {
                collect_mutated_array_bindings_expr(right, candidates);
                collect_mutated_array_bindings(body, candidates);
            }
            JsStmt::While { test, body } | JsStmt::DoWhile { test, body } => {
                collect_mutated_array_bindings_expr(test, candidates);
                collect_mutated_array_bindings(body, candidates);
            }
            JsStmt::Try {
                body,
                catch_body,
                finally_body,
                ..
            } => {
                collect_mutated_array_bindings(body, candidates);
                collect_mutated_array_bindings(catch_body, candidates);
                collect_mutated_array_bindings(finally_body, candidates);
            }
            _ => {}
        }
    }
}

fn collect_mutated_array_bindings_expr(expr: &JsExpr, candidates: &mut BTreeSet<String>) {
    match expr {
        JsExpr::Call { callee, args, .. } => {
            if let Some(name) = mutating_array_call_target(callee, args) {
                candidates.insert(name);
            }
            collect_mutated_array_bindings_expr(callee, candidates);
            for arg in args {
                collect_mutated_array_bindings_expr(arg, candidates);
            }
        }
        JsExpr::Assign { left, right, .. } => {
            if let Some(name) = array_member_assignment_target(left) {
                candidates.insert(name.to_string());
            }
            if let Some(name) = array_property_member_target(left) {
                candidates.insert(name.to_string());
            }
            collect_mutated_array_bindings_expr(left, candidates);
            collect_mutated_array_bindings_expr(right, candidates);
        }
        JsExpr::Binary { left, right, .. } => {
            collect_mutated_array_bindings_expr(left, candidates);
            collect_mutated_array_bindings_expr(right, candidates);
        }
        JsExpr::Member {
            object,
            property_expr,
            ..
        } => {
            collect_mutated_array_bindings_expr(object, candidates);
            if let Some(property_expr) = property_expr {
                collect_mutated_array_bindings_expr(property_expr, candidates);
            }
        }
        JsExpr::Array { items } => {
            for item in items {
                collect_mutated_array_bindings_expr(item, candidates);
            }
        }
        JsExpr::ArraySpread { items } => {
            for item in items {
                collect_mutated_array_bindings_expr(&item.value, candidates);
            }
        }
        JsExpr::Object { props } => {
            for prop in props {
                if let Some(key_expr) = &prop.key_expr {
                    collect_mutated_array_bindings_expr(key_expr, candidates);
                }
                collect_mutated_array_bindings_expr(&prop.value, candidates);
            }
        }
        JsExpr::Function { body, .. } => collect_mutated_array_bindings(body, candidates),
        JsExpr::Conditional {
            test,
            consequent,
            alternate,
        } => {
            collect_mutated_array_bindings_expr(test, candidates);
            collect_mutated_array_bindings_expr(consequent, candidates);
            collect_mutated_array_bindings_expr(alternate, candidates);
        }
        JsExpr::Unary { arg, .. }
        | JsExpr::Update { arg, .. }
        | JsExpr::Await { arg }
        | JsExpr::Spread { arg }
        | JsExpr::ObjectRest { object: arg, .. } => {
            collect_mutated_array_bindings_expr(arg, candidates);
        }
        JsExpr::Template { exprs, .. } | JsExpr::Sequence { exprs } => {
            for expr in exprs {
                collect_mutated_array_bindings_expr(expr, candidates);
            }
        }
        _ => {}
    }
}

fn mutating_array_call_target(callee: &JsExpr, args: &[JsExpr]) -> Option<String> {
    let JsExpr::Member {
        object,
        property,
        property_expr: None,
        optional: false,
    } = callee
    else {
        return None;
    };
    if property == "apply" {
        let JsExpr::Member {
            property,
            property_expr: None,
            optional: false,
            ..
        } = object.as_ref()
        else {
            return None;
        };
        if property != "push" {
            return None;
        }
        let JsExpr::Ident { name } = args.first()? else {
            return None;
        };
        return Some(name.clone());
    }
    if !matches!(
        property.as_str(),
        "copyWithin"
            | "fill"
            | "pop"
            | "push"
            | "reverse"
            | "shift"
            | "sort"
            | "splice"
            | "unshift"
    ) {
        return None;
    }
    let JsExpr::Ident { name } = object.as_ref() else {
        return None;
    };
    Some(name.clone())
}

fn array_member_assignment_target(expr: &JsExpr) -> Option<&str> {
    let JsExpr::Member {
        object,
        property,
        property_expr,
        optional: false,
    } = expr
    else {
        return None;
    };
    if !is_length_member_property(property, property_expr.as_deref())
        && !is_numeric_member_index_shape(property, property_expr.as_deref())
    {
        return None;
    }
    let JsExpr::Ident { name } = object.as_ref() else {
        return None;
    };
    Some(name.as_str())
}

fn expr_references_any_name(expr: &JsExpr, names: &BTreeSet<String>) -> bool {
    match expr {
        JsExpr::Ident { name } => names.contains(name),
        JsExpr::Assign { left, right, .. } | JsExpr::Binary { left, right, .. } => {
            expr_references_any_name(left, names) || expr_references_any_name(right, names)
        }
        JsExpr::Call { callee, args, .. } | JsExpr::New { callee, args } => {
            expr_references_any_name(callee, names)
                || args.iter().any(|arg| expr_references_any_name(arg, names))
        }
        JsExpr::Member {
            object,
            property_expr,
            ..
        } => {
            expr_references_any_name(object, names)
                || property_expr
                    .as_deref()
                    .is_some_and(|property| expr_references_any_name(property, names))
        }
        JsExpr::Array { items } => items
            .iter()
            .any(|item| expr_references_any_name(item, names)),
        JsExpr::ArraySpread { items } => items
            .iter()
            .any(|item| expr_references_any_name(&item.value, names)),
        JsExpr::Object { props } => props.iter().any(|prop| {
            prop.key_expr
                .as_ref()
                .is_some_and(|key| expr_references_any_name(key, names))
                || expr_references_any_name(&prop.value, names)
        }),
        JsExpr::Function { .. } => false,
        JsExpr::Conditional {
            test,
            consequent,
            alternate,
        } => {
            expr_references_any_name(test, names)
                || expr_references_any_name(consequent, names)
                || expr_references_any_name(alternate, names)
        }
        JsExpr::Unary { arg, .. }
        | JsExpr::Update { arg, .. }
        | JsExpr::Await { arg }
        | JsExpr::Spread { arg }
        | JsExpr::ObjectRest { object: arg, .. } => expr_references_any_name(arg, names),
        JsExpr::Template { exprs, .. } | JsExpr::Sequence { exprs } => exprs
            .iter()
            .any(|expr| expr_references_any_name(expr, names)),
        _ => false,
    }
}

fn mark_any_array_locals(stmts: &[JsStmt], state: &mut AotState) {
    let mut candidates = BTreeSet::new();
    collect_any_array_candidates(stmts, state, &mut candidates);
    for name in candidates {
        if state.number_array_bindings.contains(&name)
            || state.string_array_bindings.contains(&name)
            || state.bytes_bindings.contains(&name)
            || state.map_bindings.contains(&name)
            || state.set_bindings.contains(&name)
            || state.object_bindings.contains_key(&name)
            || state.class_instance_bindings.contains_key(&name)
        {
            continue;
        }
        if state.bindings.contains(&name) {
            if is_any_binding(&name, state) {
                continue;
            }
            state.any_array_bindings.insert(name);
            continue;
        }
        state.bindings.insert(name.clone());
        state
            .binding_refs
            .insert(name.clone(), sanitize_go_identifier(&name));
        state.any_array_bindings.insert(name);
    }
}

fn collect_string_array_candidates(
    stmts: &[JsStmt],
    state: &AotState,
    candidates: &mut BTreeSet<String>,
) {
    for stmt in stmts {
        match stmt {
            JsStmt::Expr { expr } | JsStmt::Return { value: Some(expr) } => {
                collect_string_array_candidates_expr(expr, state, candidates);
            }
            JsStmt::VarDecl {
                name,
                init: Some(expr),
            } => {
                if is_string_array_candidate_expr(expr, state) {
                    candidates.insert(name.clone());
                }
                collect_string_array_candidates_expr(expr, state, candidates);
            }
            JsStmt::FunctionDecl { body, .. } => {
                collect_string_array_candidates(body, state, candidates);
            }
            JsStmt::If {
                test,
                consequent,
                alternate,
            } => {
                collect_string_array_candidates_expr(test, state, candidates);
                collect_string_array_candidates(consequent, state, candidates);
                collect_string_array_candidates(alternate, state, candidates);
            }
            JsStmt::For {
                init,
                test,
                update,
                body,
            } => {
                collect_string_array_candidates(init, state, candidates);
                if let Some(test) = test {
                    collect_string_array_candidates_expr(test, state, candidates);
                }
                if let Some(update) = update {
                    collect_string_array_candidates_expr(update, state, candidates);
                }
                collect_string_array_candidates(body, state, candidates);
            }
            JsStmt::ForOf { right, body, .. } => {
                collect_string_array_candidates_expr(right, state, candidates);
                collect_string_array_candidates(body, state, candidates);
            }
            JsStmt::While { test, body } | JsStmt::DoWhile { test, body } => {
                collect_string_array_candidates_expr(test, state, candidates);
                collect_string_array_candidates(body, state, candidates);
            }
            JsStmt::Try {
                body,
                catch_body,
                finally_body,
                ..
            } => {
                collect_string_array_candidates(body, state, candidates);
                collect_string_array_candidates(catch_body, state, candidates);
                collect_string_array_candidates(finally_body, state, candidates);
            }
            _ => {}
        }
    }
}

fn collect_string_array_candidates_expr(
    expr: &JsExpr,
    state: &AotState,
    candidates: &mut BTreeSet<String>,
) {
    match expr {
        JsExpr::Assign { op, left, right } => {
            if op == "=" {
                if let Some(name) = string_array_candidate_assignment_target(left, right, state) {
                    candidates.insert(name);
                }
            }
            collect_string_array_candidates_expr(left, state, candidates);
            collect_string_array_candidates_expr(right, state, candidates);
        }
        JsExpr::Binary { left, right, .. } => {
            collect_string_array_candidates_expr(left, state, candidates);
            collect_string_array_candidates_expr(right, state, candidates);
        }
        JsExpr::Call { callee, args, .. } => {
            if let Some(name) = string_array_candidate_push_target(callee, args, state) {
                candidates.insert(name.to_string());
            }
            collect_string_array_candidates_expr(callee, state, candidates);
            for arg in args {
                collect_string_array_candidates_expr(arg, state, candidates);
            }
        }
        JsExpr::Member {
            object,
            property_expr,
            ..
        } => {
            collect_string_array_candidates_expr(object, state, candidates);
            if let Some(property_expr) = property_expr {
                collect_string_array_candidates_expr(property_expr, state, candidates);
            }
        }
        JsExpr::Array { items } => {
            for item in items {
                collect_string_array_candidates_expr(item, state, candidates);
            }
        }
        JsExpr::ArraySpread { items } => {
            for item in items {
                collect_string_array_candidates_expr(&item.value, state, candidates);
            }
        }
        JsExpr::Object { props } => {
            for prop in props {
                collect_string_array_candidates_expr(&prop.value, state, candidates);
            }
        }
        JsExpr::Function { body, .. } => collect_string_array_candidates(body, state, candidates),
        JsExpr::Conditional {
            test,
            consequent,
            alternate,
        } => {
            collect_string_array_candidates_expr(test, state, candidates);
            collect_string_array_candidates_expr(consequent, state, candidates);
            collect_string_array_candidates_expr(alternate, state, candidates);
        }
        JsExpr::Unary { arg, .. }
        | JsExpr::Update { arg, .. }
        | JsExpr::Await { arg }
        | JsExpr::Spread { arg }
        | JsExpr::ObjectRest { object: arg, .. } => {
            collect_string_array_candidates_expr(arg, state, candidates);
        }
        JsExpr::Template { exprs, .. } | JsExpr::Sequence { exprs } => {
            for expr in exprs {
                collect_string_array_candidates_expr(expr, state, candidates);
            }
        }
        _ => {}
    }
}

fn string_array_candidate_assignment_target(
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
    let indexed_member = property_expr.is_some() || property.parse::<usize>().is_ok();
    if !indexed_member || !is_string_array_candidate_value(right, state) {
        return None;
    }
    Some(name.clone())
}

fn is_string_array_candidate_value(expr: &JsExpr, state: &AotState) -> bool {
    if render_string_expr(expr, state).is_some() {
        return true;
    }
    if render_regexp_expr(expr, state).is_some() {
        return true;
    }
    if matches!(expr, JsExpr::New { callee, .. } if matches!(callee.as_ref(), JsExpr::Ident { name } if name == "RegExp"))
    {
        return true;
    }
    match expr {
        JsExpr::Binary { op, left, right } if op == "+" => {
            is_string_array_candidate_value(left, state)
                || is_string_array_candidate_value(right, state)
                || is_string_literal_like(left)
                || is_string_literal_like(right)
        }
        JsExpr::Conditional {
            consequent,
            alternate,
            ..
        } => {
            is_string_array_candidate_value(consequent, state)
                && is_string_array_candidate_value(alternate, state)
        }
        JsExpr::Call { callee, args, .. } => {
            string_method_name(callee).is_some()
                || is_string_cast_call(callee, args)
                || is_string_from_char_code_call(callee, args)
        }
        _ => is_string_literal_like(expr),
    }
}

fn is_string_array_candidate_expr(expr: &JsExpr, state: &AotState) -> bool {
    match expr {
        JsExpr::Array { items } => {
            !items.is_empty()
                && items
                    .iter()
                    .all(|item| is_string_array_candidate_value(item, state))
        }
        JsExpr::Call { callee, args, .. } if is_array_filter_call(callee, args) => {
            render_string_array_expr(expr, state).is_some()
        }
        _ => render_string_array_expr(expr, state).is_some(),
    }
}

fn is_any_array_candidate_expr(expr: &JsExpr, state: &AotState) -> bool {
    match expr {
        JsExpr::Array { items } => {
            !items.is_empty()
                && !items.iter().all(is_numeric_array_candidate_item)
                && !items
                    .iter()
                    .all(|item| is_string_array_candidate_value(item, state))
                && items
                    .iter()
                    .all(|item| render_json_value_expr(item, state).is_some())
        }
        _ => false,
    }
}

fn collect_any_array_candidates(
    stmts: &[JsStmt],
    state: &AotState,
    candidates: &mut BTreeSet<String>,
) {
    for stmt in stmts {
        match stmt {
            JsStmt::Expr { expr } | JsStmt::Return { value: Some(expr) } => {
                collect_any_array_candidates_expr(expr, state, candidates);
            }
            JsStmt::VarDecl {
                name,
                init: Some(expr),
            } => {
                if is_any_array_candidate_expr(expr, state) {
                    candidates.insert(name.clone());
                }
                collect_any_array_candidates_expr(expr, state, candidates);
            }
            JsStmt::If {
                test,
                consequent,
                alternate,
            } => {
                collect_any_array_candidates_expr(test, state, candidates);
                collect_any_array_candidates(consequent, state, candidates);
                collect_any_array_candidates(alternate, state, candidates);
            }
            JsStmt::For {
                init,
                test,
                update,
                body,
            } => {
                collect_any_array_candidates(init, state, candidates);
                if let Some(test) = test {
                    collect_any_array_candidates_expr(test, state, candidates);
                }
                if let Some(update) = update {
                    collect_any_array_candidates_expr(update, state, candidates);
                }
                collect_any_array_candidates(body, state, candidates);
            }
            JsStmt::ForOf { right, body, .. } => {
                collect_any_array_candidates_expr(right, state, candidates);
                collect_any_array_candidates(body, state, candidates);
            }
            JsStmt::While { test, body } | JsStmt::DoWhile { test, body } => {
                collect_any_array_candidates_expr(test, state, candidates);
                collect_any_array_candidates(body, state, candidates);
            }
            JsStmt::Try {
                body,
                catch_body,
                finally_body,
                ..
            } => {
                collect_any_array_candidates(body, state, candidates);
                collect_any_array_candidates(catch_body, state, candidates);
                collect_any_array_candidates(finally_body, state, candidates);
            }
            _ => {}
        }
    }
}

fn collect_any_array_candidates_expr(
    expr: &JsExpr,
    state: &AotState,
    candidates: &mut BTreeSet<String>,
) {
    match expr {
        JsExpr::Call { callee, args, .. } => {
            if let Some(name) = any_array_candidate_push_target(callee, args) {
                candidates.insert(name.to_string());
            }
            if let JsExpr::Ident { name } = callee.as_ref() {
                if let Some(function) = state.functions.get(name) {
                    for (arg, kind) in args.iter().zip(function.param_kinds.iter()) {
                        if *kind == AotSlotKind::AnyArray {
                            if let JsExpr::Ident { name } = arg {
                                candidates.insert(name.clone());
                            }
                        }
                    }
                }
            }
            collect_any_array_candidates_expr(callee, state, candidates);
            for arg in args {
                collect_any_array_candidates_expr(arg, state, candidates);
            }
        }
        JsExpr::Assign { left, right, .. } | JsExpr::Binary { left, right, .. } => {
            collect_any_array_candidates_expr(left, state, candidates);
            collect_any_array_candidates_expr(right, state, candidates);
        }
        JsExpr::Member { object, .. }
        | JsExpr::Unary { arg: object, .. }
        | JsExpr::Update { arg: object, .. }
        | JsExpr::Await { arg: object }
        | JsExpr::Spread { arg: object }
        | JsExpr::ObjectRest { object, .. } => {
            collect_any_array_candidates_expr(object, state, candidates);
        }
        JsExpr::Array { items } => {
            for item in items {
                collect_any_array_candidates_expr(item, state, candidates);
            }
        }
        JsExpr::ArraySpread { items } => {
            for item in items {
                collect_any_array_candidates_expr(&item.value, state, candidates);
            }
        }
        JsExpr::Object { props } => {
            for prop in props {
                collect_any_array_candidates_expr(&prop.value, state, candidates);
            }
        }
        JsExpr::Function { body, .. } => {
            collect_any_array_candidates(body, state, candidates);
        }
        JsExpr::Conditional {
            test,
            consequent,
            alternate,
        } => {
            collect_any_array_candidates_expr(test, state, candidates);
            collect_any_array_candidates_expr(consequent, state, candidates);
            collect_any_array_candidates_expr(alternate, state, candidates);
        }
        JsExpr::Template { exprs, .. } | JsExpr::Sequence { exprs } => {
            for expr in exprs {
                collect_any_array_candidates_expr(expr, state, candidates);
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
            JsStmt::VarDecl {
                name,
                init: Some(JsExpr::ArraySpread { items }),
            } if is_numeric_array_spread_candidate_items(items) => {
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
                expr: JsExpr::Assign { op, left, right },
            } if op == "="
                && matches!(right.as_ref(), JsExpr::ArraySpread { items } if is_numeric_array_spread_candidate_items(items)) =>
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
        JsExpr::Function { body, .. } => {
            collect_number_array_candidates(body, candidates);
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

fn string_array_candidate_push_target<'a>(
    callee: &'a JsExpr,
    args: &[JsExpr],
    state: &AotState,
) -> Option<&'a str> {
    if args.len() != 1 || !is_string_array_candidate_value(args.first()?, state) {
        return None;
    }
    number_array_candidate_method_target(callee, "push")
}

fn any_array_candidate_push_target<'a>(callee: &'a JsExpr, args: &[JsExpr]) -> Option<&'a str> {
    if args.len() != 1 {
        return None;
    }
    if is_numeric_array_candidate_item(args.first()?) {
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
            | JsExpr::Member { .. }
    ) || matches!(expr, JsExpr::Call { callee, .. } if string_method_name(callee).is_none())
}

fn is_numeric_array_spread_candidate_items(items: &[crate::contract::JsArrayElement]) -> bool {
    !items.is_empty()
        && items.iter().all(|item| {
            if item.spread {
                matches!(item.value, JsExpr::Ident { .. } | JsExpr::Array { .. })
            } else {
                is_numeric_array_candidate_item(&item.value)
            }
        })
}

fn function_returns_string_array(function: &AotFunction, state: &AotState) -> bool {
    let mut function_state = aot_function_state(function, state);
    mark_string_array_locals(&function.body, &mut function_state);
    mark_any_array_locals(&function.body, &mut function_state);
    mark_array_property_locals(&function.body, &mut function_state);
    mark_dynamic_object_locals(&function.body, &mut function_state);
    let mut saw_return = false;
    collect_string_array_returns(&function.body, &mut function_state, &mut saw_return)
        .unwrap_or(false)
        && saw_return
}

fn function_returns_string(function: &AotFunction, state: &AotState) -> bool {
    let mut function_state = aot_function_state(function, state);
    let mut saw_return = false;
    collect_string_returns(&function.body, &mut function_state, &mut saw_return).unwrap_or(false)
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

fn function_returns_bytes(function: &AotFunction, state: &AotState) -> bool {
    let mut function_state = aot_function_state(function, state);
    let mut saw_return = false;
    collect_bytes_returns(&function.body, &mut function_state, &mut saw_return).unwrap_or(false)
        && saw_return
}

fn function_returns_any_array(function: &AotFunction, state: &AotState) -> bool {
    if function.body.iter().any(|stmt| {
        matches!(
            stmt,
            JsStmt::For { .. } | JsStmt::While { .. } | JsStmt::DoWhile { .. } | JsStmt::Try { .. }
        )
    }) {
        return false;
    }
    let mut function_state = aot_function_state(function, state);
    mark_any_array_locals(&function.body, &mut function_state);
    mark_array_property_locals(&function.body, &mut function_state);
    let mut saw_return = false;
    collect_any_array_returns(&function.body, &mut function_state, &mut saw_return).unwrap_or(false)
        && saw_return
}

fn collect_string_returns(
    body: &[JsStmt],
    state: &mut AotState,
    saw_return: &mut bool,
) -> Option<bool> {
    for stmt in body {
        match stmt {
            JsStmt::Return { value: Some(value) } => {
                *saw_return = true;
                if render_string_return_expr(value, state).is_none() {
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
                if !collect_string_returns(consequent, &mut consequent_state, saw_return)? {
                    return Some(false);
                }
                let mut alternate_state = clone_aot_state(state);
                if !collect_string_returns(alternate, &mut alternate_state, saw_return)? {
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

fn render_string_return_expr(expr: &JsExpr, state: &AotState) -> Option<String> {
    match expr {
        JsExpr::Ident { name }
            if state.string_bindings.contains(name) || state.date_bindings.contains(name) =>
        {
            render_string_expr(expr, state)
        }
        expr if is_string_result_shape(expr) => render_string_expr(expr, state),
        expr if render_string_index_expr(expr, state).is_some() => render_string_expr(expr, state),
        expr if render_string_array_index_expr(expr, state).is_some() => {
            render_string_expr(expr, state)
        }
        JsExpr::Conditional { .. } => render_string_expr(expr, state),
        JsExpr::Member {
            property,
            property_expr: None,
            ..
        } if property == "exports" => None,
        JsExpr::Member { .. } => render_string_expr(expr, state),
        _ => None,
    }
}

fn collect_any_array_returns(
    body: &[JsStmt],
    state: &mut AotState,
    saw_return: &mut bool,
) -> Option<bool> {
    for stmt in body {
        match stmt {
            JsStmt::Return { value: Some(value) } => {
                *saw_return = true;
                if render_any_array_expr(value, state).is_none() {
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
                mark_any_array_locals(consequent, &mut consequent_state);
                mark_array_property_locals(consequent, &mut consequent_state);
                if !collect_any_array_returns(consequent, &mut consequent_state, saw_return)? {
                    return Some(false);
                }
                let mut alternate_state = clone_aot_state(state);
                mark_any_array_locals(alternate, &mut alternate_state);
                mark_array_property_locals(alternate, &mut alternate_state);
                if !collect_any_array_returns(alternate, &mut alternate_state, saw_return)? {
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

fn collect_bytes_returns(
    body: &[JsStmt],
    state: &mut AotState,
    saw_return: &mut bool,
) -> Option<bool> {
    for stmt in body {
        match stmt {
            JsStmt::Return { value: Some(value) } => {
                *saw_return = true;
                if render_bytes_return_expr(value, state).is_none() {
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
                if !collect_bytes_returns(consequent, &mut consequent_state, saw_return)? {
                    return Some(false);
                }
                let mut alternate_state = clone_aot_state(state);
                if !collect_bytes_returns(alternate, &mut alternate_state, saw_return)? {
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
                if render_bytes_expr(value, state).is_some() {
                    return Some(false);
                }
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
        JsStmt::Return { value: None } => Some("return nil".to_string()),
        JsStmt::If {
            test,
            consequent,
            alternate,
        } => {
            let test_expr = test;
            let test = render_bool_test_expr(test_expr, state)?;
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
        JsStmt::ForOf { left, right, body } => render_for_of_stmt(left, right, body, state),
        JsStmt::While { test, body } => render_while_stmt(test, body, state),
        JsStmt::Try {
            body,
            catch_param,
            catch_body,
            finally_body,
            ..
        } => {
            if let Some(value) = render_try_catch_return_expr(
                body,
                catch_param.as_deref(),
                catch_body,
                finally_body,
                state,
            ) {
                return Some(format!("return {value}"));
            }
            render_try_finally_stmt(
                body,
                catch_param.as_deref(),
                catch_body,
                finally_body,
                state,
            )
        }
        JsStmt::Throw { value } => render_throw_stmt(value, state),
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
            render_array_is_array_call(args, state)
        }
        JsExpr::Call { callee, args, .. } if is_array_is_array_alias_call(callee, args, state) => {
            render_array_is_array_call(args, state)
        }
        JsExpr::Call { callee, args, .. } if is_object_has_own_call(callee, args) => {
            render_object_has_own_call(callee, args, state)
        }
        JsExpr::Call { callee, args, .. }
            if is_object_has_own_property_call(callee, args, state) =>
        {
            render_object_has_own_property_call(callee, args, state)
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
        JsExpr::Binary { op, left, right } if op == "in" => {
            let key = render_expr(left, state)?;
            let object = render_expr(right, state)?;
            Some(format!("tsgodownObjectHasOwn({object}, {key})"))
        }
        JsExpr::Binary { op, left, right } if matches!(op.as_str(), "&&" | "||") => {
            let left = render_bool_expr(left, state)?;
            let right = render_bool_expr(right, state)?;
            Some(format!("({left} {op} {right})"))
        }
        JsExpr::Binary { op, left, right }
            if op == "instanceof" && is_date_constructor_ref(right) =>
        {
            render_date_instanceof_expr(left, state)
        }
        JsExpr::Binary { op, left, right }
            if op == "instanceof" && error_constructor_name(right).is_some() =>
        {
            let value = render_expr(left, state)?;
            let constructor = error_constructor_name(right)?;
            Some(format!(
                "tsgodownErrorInstanceOf({value}, {})",
                go_string_literal(constructor)
            ))
        }
        JsExpr::Unary { op, arg } if op == "!" => {
            let arg = render_bool_test_expr(arg, state)?;
            Some(format!("(!{arg})"))
        }
        JsExpr::Unary { op, arg } if op == "delete" => {
            render_delete_object_property_expr(arg, state)
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
        JsExpr::Call { callee, args, .. } if is_number_is_integer_call(callee, args) => {
            let value = render_numeric_expr(args.first()?, state)?;
            Some(format!(
                "(!math.IsNaN({value}) && !math.IsInf({value}, 0) && math.Trunc({value}) == {value})"
            ))
        }
        JsExpr::Call { callee, args, .. } if is_number_is_finite_call(callee, args) => {
            let value = render_numeric_expr(args.first()?, state)?;
            Some(format!("(!math.IsNaN({value}) && !math.IsInf({value}, 0))"))
        }
        JsExpr::Call { callee, args, .. } if is_number_is_safe_integer_call(callee, args) => {
            let value = render_numeric_expr(args.first()?, state)?;
            Some(format!(
                "(!math.IsNaN({value}) && !math.IsInf({value}, 0) && math.Trunc({value}) == {value} && math.Abs({value}) <= 9007199254740991)"
            ))
        }
        JsExpr::Call { callee, args, .. } if is_global_is_finite_call(callee, args) => {
            let value = render_expr(args.first()?, state)?;
            Some(format!(
                "func() bool {{ raw := any({value}); value := tsgodownToFloat64(raw); return !tsgodownIsNaN(raw) && !math.IsNaN(value) && !math.IsInf(value, 0) }}()"
            ))
        }
        JsExpr::Call { callee, args, .. } if is_map_has_call(callee, args, state) => {
            render_map_bool_call(callee, args, state)
        }
        JsExpr::Call { callee, args, .. }
            if render_regexp_test_alias_call(callee, args, state).is_some() =>
        {
            render_regexp_test_alias_call(callee, args, state)
        }
        JsExpr::Call { callee, args, .. }
            if render_regexp_test_call(callee, args, state).is_some() =>
        {
            render_regexp_test_call(callee, args, state)
        }
        JsExpr::Call { callee, args, .. } => render_bool_call_expr(callee, args, state)
            .or_else(|| render_bool_function_call(callee, args, state))
            .or_else(|| render_array_predicate_call(callee, args, state))
            .or_else(|| render_string_bool_method_alias_call(callee, args, state))
            .or_else(|| render_string_bool_method_call(callee, args, state))
            .or_else(|| render_string_array_includes_call(callee, args, state))
            .or_else(|| render_array_includes_call(callee, args, state))
            .or_else(|| render_array_bool_method_call(callee, args, state)),
        _ => None,
    }
}

fn render_bool_test_expr(expr: &JsExpr, state: &AotState) -> Option<String> {
    render_bool_expr(expr, state).or_else(|| {
        let value = render_expr(expr, state)?;
        Some(format!("tsgodownToBool({value})"))
    })
}

fn render_expr_stmt(expr: &JsExpr, state: &mut AotState) -> Option<String> {
    match expr {
        JsExpr::Value {
            value: JsValue::String { .. },
        } => Some(String::new()),
        JsExpr::Await { arg } => render_await_promise_then_stmt(arg, state)
            .or_else(|| render_await_node_fs_promises_stmt(arg, state)),
        expr if is_resolved_default_cjs_export_assignment_expr(expr, state) => Some(String::new()),
        expr if cjs_default_function_static_member_assignment_expr(expr, state).is_some() => {
            Some(String::new())
        }
        expr if object_define_property_static_member_expr(expr, state).is_some() => {
            Some(String::new())
        }
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
        JsExpr::Call { callee, args, .. } if is_require_call(expr) => {
            let spec = render_require_spec_arg(callee, args)?;
            if spec.starts_with("\"./") || spec.starts_with("\"../") {
                return Some(String::new());
            }
            Some(format!("_ = tsgodownRequire({spec})"))
        }
        JsExpr::Call { callee, args, .. } if is_array_for_each_call(callee, args) => {
            render_array_for_each_stmt(callee, args, state)
        }
        JsExpr::Call { callee, args, .. } if ts_enum_iife_target(callee, args).is_some() => {
            render_ts_enum_iife_stmt(callee, args, state)
        }
        JsExpr::Assign { op, left, right }
            if op == "="
                && is_module_exports_member(left)
                && is_resolved_default_cjs_export_value(right, state) =>
        {
            Some(String::new())
        }
        JsExpr::Assign { op, left, .. }
            if op == "="
                && is_cjs_export_target(left)
                && !is_shadowed_cjs_export_target(left, state)
                && matches!(expr, JsExpr::Assign { right, .. } if is_resolved_export_metadata_expr(right, state)) =>
        {
            Some(String::new())
        }
        JsExpr::Assign { op, left, right }
            if op == "="
                && is_module_exports_member(left)
                && render_module_exports_assignment_stmt(right, state).is_some() =>
        {
            render_module_exports_assignment_stmt(right, state)
        }
        JsExpr::Assign { .. } if is_exports_module_exports_alias_assignment(expr) => {
            Some(String::new())
        }
        JsExpr::Assign { op, left, right }
            if render_function_static_member_assignment_stmt(op, left, right, state).is_some() =>
        {
            render_function_static_member_assignment_stmt(op, left, right, state)
        }
        JsExpr::Assign { op, left, right }
            if render_any_array_assignment_stmt(op, left, right, state).is_some() =>
        {
            render_any_array_assignment_stmt(op, left, right, state)
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
            if render_bytes_assignment_stmt(op, left, right, state).is_some() =>
        {
            render_bytes_assignment_stmt(op, left, right, state)
        }
        JsExpr::Assign { op, left, right }
            if render_array_property_assignment_stmt(op, left, right, state).is_some() =>
        {
            render_array_property_assignment_stmt(op, left, right, state)
        }
        JsExpr::Assign { op, left, right }
            if render_dynamic_object_array_assignment_stmt(op, left, right, state).is_some() =>
        {
            render_dynamic_object_array_assignment_stmt(op, left, right, state)
        }
        JsExpr::Assign { op, left, right }
            if render_any_bytes_index_assignment_stmt(op, left, right, state).is_some() =>
        {
            render_any_bytes_index_assignment_stmt(op, left, right, state)
        }
        JsExpr::Assign { op, left, right }
            if render_any_index_compound_assignment_stmt(op, left, right, state).is_some() =>
        {
            render_any_index_compound_assignment_stmt(op, left, right, state)
        }
        JsExpr::Assign { op, left, right }
            if render_process_env_assignment_stmt(op, left, right, state).is_some() =>
        {
            render_process_env_assignment_stmt(op, left, right, state)
        }
        JsExpr::Call { callee, args, .. }
            if render_bytes_set_stmt(callee, args, state).is_some() =>
        {
            render_bytes_set_stmt(callee, args, state)
        }
        JsExpr::Assign { op, left, right }
            if render_dynamic_object_assignment_stmt(op, left, right, state).is_some() =>
        {
            render_dynamic_object_assignment_stmt(op, left, right, state)
        }
        JsExpr::Call { callee, args, .. }
            if render_reflect_delete_property_stmt(callee, args, state).is_some() =>
        {
            render_reflect_delete_property_stmt(callee, args, state)
        }
        JsExpr::Assign { op, left, right }
            if render_url_assignment_stmt(op, left, right, state).is_some() =>
        {
            render_url_assignment_stmt(op, left, right, state)
        }
        JsExpr::Unary { op, arg } if op == "delete" => {
            let deleted = render_delete_object_property_expr(arg, state)?;
            Some(format!("_ = {deleted}"))
        }
        JsExpr::Call { callee, args, .. }
            if render_object_assign_stmt(callee, args, state).is_some() =>
        {
            render_object_assign_stmt(callee, args, state)
        }
        JsExpr::Call { callee, args, .. }
            if render_array_push_apply_stmt(callee, args, state).is_some() =>
        {
            render_array_push_apply_stmt(callee, args, state)
        }
        JsExpr::Call { callee, args, .. }
            if render_crypto_random_fill_sync_stmt(callee, args, state).is_some() =>
        {
            render_crypto_random_fill_sync_stmt(callee, args, state)
        }
        JsExpr::Call { callee, args, .. }
            if render_any_array_call_stmt(callee, args, state).is_some() =>
        {
            render_any_array_call_stmt(callee, args, state)
        }
        JsExpr::Call { callee, args, .. }
            if render_string_array_push_call_stmt(callee, args, state).is_some() =>
        {
            render_string_array_push_call_stmt(callee, args, state)
        }
        JsExpr::Call { callee, args, .. }
            if render_any_array_pop_call(callee, args, state).is_some() =>
        {
            let value = render_any_array_pop_call(callee, args, state)?;
            Some(format!("_ = {value}"))
        }
        JsExpr::Call { callee, args, .. }
            if render_string_array_pop_call(callee, args, state).is_some() =>
        {
            let value = render_string_array_pop_call(callee, args, state)?;
            Some(format!("_ = {value}"))
        }
        JsExpr::Call { callee, args, .. }
            if render_mutating_any_array_function_call_stmt(callee, args, state).is_some() =>
        {
            render_mutating_any_array_function_call_stmt(callee, args, state)
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
            if render_node_fs_promises_write_file_stmt(callee, args, state).is_some() =>
        {
            render_node_fs_promises_write_file_stmt(callee, args, state)
        }
        JsExpr::Call { callee, args, .. }
            if render_node_fs_rm_sync_stmt(callee, args, state).is_some() =>
        {
            render_node_fs_rm_sync_stmt(callee, args, state)
        }
        JsExpr::Call { callee, args, .. }
            if render_node_assert_stmt(callee, args, state).is_some() =>
        {
            render_node_assert_stmt(callee, args, state)
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

fn render_node_assert_stmt(callee: &JsExpr, args: &[JsExpr], state: &AotState) -> Option<String> {
    if args.len() < 2 {
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
    let JsExpr::Ident { name } = object.as_ref() else {
        return None;
    };
    if !state.assert_builtin_bindings.contains(name) {
        return None;
    }
    let left = render_json_value_expr(args.first()?, state)
        .or_else(|| render_expr(args.first()?, state))?;
    let right =
        render_json_value_expr(args.get(1)?, state).or_else(|| render_expr(args.get(1)?, state))?;
    let condition = match property.as_str() {
        "equal" => format!("tsgodownStrictEqual({left}, {right})"),
        "strictEqual" => format!("tsgodownSameValueStrict({left}, {right})"),
        "deepStrictEqual" => format!("tsgodownDeepStrictEqual({left}, {right})"),
        _ => return None,
    };
    Some(format!("tsgodownAssert({condition})"))
}

fn render_mutating_any_array_function_call_stmt(
    callee: &JsExpr,
    args: &[JsExpr],
    state: &AotState,
) -> Option<String> {
    let JsExpr::Ident { name } = callee else {
        return None;
    };
    let function = state.functions.get(name)?;
    let mutated_index = function_mutated_any_array_param(function, state)?;
    let JsExpr::Ident { name: target_name } = args.get(mutated_index)? else {
        return None;
    };
    if !state.any_array_bindings.contains(target_name) {
        return None;
    }
    let target = go_binding_ref(target_name, state);
    let rendered_args = render_call_args(args, &function.param_kinds, state)?;
    Some(format!(
        "{target} = {}({})",
        function.go_name,
        rendered_args.join(", ")
    ))
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

fn render_await_node_fs_promises_stmt(expr: &JsExpr, state: &AotState) -> Option<String> {
    let JsExpr::Call { callee, args, .. } = expr else {
        return None;
    };
    render_node_fs_promises_write_file_stmt(callee, args, state)
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
    if matches!(op, "&&=" | "||=" | "??=") && state.bindings.contains(name) {
        let target = go_binding_ref(name, state);
        if is_any_binding(name, state) {
            let right = render_logical_assignment_any_rhs(right, state)?;
            return match op {
                "&&=" => Some(format!(
                    "if tsgodownToBool({target}) {{ {target} = {right} }}"
                )),
                "||=" => Some(format!(
                    "if !tsgodownToBool({target}) {{ {target} = {right} }}"
                )),
                "??=" => Some(format!("if {target} == nil {{ {target} = {right} }}")),
                _ => None,
            };
        }
        if op == "??=" {
            let _right =
                render_json_value_expr(right, state).or_else(|| render_expr(right, state))?;
            return Some(String::new());
        }
    }
    if !state.numeric_bindings.contains(name) {
        if state.number_array_bindings.contains(name) && op == "=" {
            if matches!(right, JsExpr::Array { items } if items.is_empty()) {
                return Some(format!("{} = []float64{{}}", go_binding_ref(name, state)));
            }
            let right = render_number_array_expr(right, state)?;
            return Some(format!("{} = {right}", go_binding_ref(name, state)));
        }
        if state.string_bindings.contains(name) && matches!(op, "=" | "+=") {
            let target = go_binding_ref(name, state);
            if op == "=" {
                if target.contains(".(") {
                    if let Some(right) = render_bytes_expr_with_any_cast(right, state) {
                        return Some(format!("{} = any({right})", sanitize_go_identifier(name)));
                    }
                }
                if let Some(right) = render_bytes_expr(right, state) {
                    return Some(format!("{target} = {right}"));
                }
            }
            let right = render_string_expr(right, state)?;
            return Some(format!("{target} {op} {right}"));
        }
        if state.string_array_bindings.contains(name) && op == "=" {
            let right = render_string_array_expr(right, state)?;
            return Some(format!("{} = {right}", go_binding_ref(name, state)));
        }
        if state.bool_bindings.contains(name) && op == "=" {
            let right = render_bool_expr(right, state)?;
            return Some(format!("{} = {right}", sanitize_go_identifier(name)));
        }
        if state.bytes_bindings.contains(name) && op == "=" {
            let right = render_bytes_expr_with_any_cast(right, state)?;
            return Some(format!("{} = {right}", go_binding_ref(name, state)));
        }
        if is_any_binding(name, state) && matches!(op, "+=" | "-=" | "*=" | "/=" | "%=") {
            let target = go_binding_ref(name, state);
            let right = render_numeric_expr(right, state)?;
            let numeric_op = op.trim_end_matches('=');
            if op == "%=" {
                return Some(format!(
                    "{target} = math.Mod(tsgodownToFloat64({target}), {right})"
                ));
            }
            return Some(format!(
                "{target} = tsgodownToFloat64({target}) {numeric_op} {right}"
            ));
        }
        if state.bindings.contains(name) && op == "=" {
            if let Some(right) = render_bytes_expr(right, state) {
                return Some(format!("{} = any({right})", sanitize_go_identifier(name)));
            }
            let right = render_expr(right, state)?;
            return Some(format!("{} = {right}", sanitize_go_identifier(name)));
        }
        return None;
    }
    let right = render_numeric_expr(right, state)?;
    match op {
        "=" | "+=" | "-=" | "*=" | "/=" => {
            Some(format!("{} {op} {right}", go_binding_ref(name, state)))
        }
        "%=" => {
            let target = go_binding_ref(name, state);
            Some(format!("{target} = math.Mod({target}, {right})"))
        }
        "**=" => Some(format!(
            "{target} = math.Pow({target}, {right})",
            target = go_binding_ref(name, state)
        )),
        "&=" | "|=" | "^=" | "<<=" | ">>=" | ">>>=" => {
            let target = go_binding_ref(name, state);
            let value = render_bitwise_compound_expr(op, &target, &right)?;
            Some(format!("{target} = {value}"))
        }
        _ => None,
    }
}

fn render_logical_assignment_any_rhs(expr: &JsExpr, state: &AotState) -> Option<String> {
    render_logical_assignment_bytes_slice_rhs(expr, state)
        .or_else(|| render_direct_bytes_expr(expr, state))
        .or_else(|| render_json_value_expr(expr, state))
        .or_else(|| render_expr(expr, state))
}

fn render_logical_assignment_bytes_slice_rhs(expr: &JsExpr, state: &AotState) -> Option<String> {
    let JsExpr::Call { callee, args, .. } = expr else {
        return None;
    };
    if !is_member_slice_call_shape(callee, args) {
        return None;
    }
    let JsExpr::Member { object, .. } = callee.as_ref() else {
        return None;
    };
    let object = render_bytes_expr_with_any_cast(object, state)?;
    let start = render_numeric_expr(args.first()?, state)?;
    let end = args
        .get(1)
        .map(|arg| render_numeric_expr(arg, state))
        .unwrap_or_else(|| Some(format!("float64(len({object}))")))?;
    Some(format!(
        "append([]byte(nil), {object}[int({start}):int({end})]...)"
    ))
}

fn render_direct_bytes_expr(expr: &JsExpr, state: &AotState) -> Option<String> {
    match expr {
        JsExpr::Array { .. } | JsExpr::New { .. } | JsExpr::Call { .. }
            if is_direct_bytes_expr(expr, state) =>
        {
            render_bytes_expr(expr, state)
        }
        _ => None,
    }
}

fn is_direct_bytes_expr(expr: &JsExpr, state: &AotState) -> bool {
    match expr {
        JsExpr::Array { items } => items
            .iter()
            .all(|item| render_numeric_expr(item, state).is_some()),
        JsExpr::New { callee, args } => is_new_uint8_array_expr(callee, args),
        JsExpr::Call { callee, args, .. } => {
            is_uint8_array_of_call(callee, args)
                || is_bytes_slice_call(callee, args, state)
                || is_node_buffer_from_call(callee, args)
                || is_node_buffer_alloc_call(callee, args)
                || is_crypto_hash_bytes_digest_call(callee, args)
        }
        _ => false,
    }
}

fn render_cjs_export_alias_var_decl(
    name: &str,
    expr: &JsExpr,
    state: &mut AotState,
) -> Option<String> {
    let init = cjs_export_alias_assignment_value(expr)?;
    let ident = sanitize_go_identifier(name);
    if state.string_array_bindings.contains(name) {
        let value = render_string_array_expr(init, state)?;
        state.bind_slot(name, ident.clone(), AotSlotKind::StringArray);
        return Some(format!("var {ident} []string = {value}"));
    }
    if state.dynamic_object_bindings.contains(name) {
        let value = render_dynamic_object_init_expr(init, state)?;
        state.bindings.insert(name.to_string());
        state.binding_refs.insert(name.to_string(), ident.clone());
        state
            .ordered_dynamic_object_bindings
            .insert(name.to_string());
        let keys = render_dynamic_object_order_init_expr(init, state)?;
        let order = dynamic_object_order_ref(name, state);
        return Some(format!(
            "var {ident} map[string]any = {value}\nvar {order} []string = {keys}\n_ = {order}"
        ));
    }
    if matches!(init, JsExpr::Array { items } if items.is_empty()) {
        state.bind_slot(name, ident.clone(), AotSlotKind::AnyArray);
        return Some(format!("var {ident} []any = []any{{}}"));
    }
    let value = render_json_value_expr(init, state)?;
    state.bindings.insert(name.to_string());
    state.binding_refs.insert(name.to_string(), ident.clone());
    Some(format!("var {ident} any = {value}"))
}

fn render_numeric_assignment_expr(
    op: &str,
    left: &JsExpr,
    right: &JsExpr,
    state: &AotState,
) -> Option<String> {
    let JsExpr::Ident { name } = left else {
        return None;
    };
    let target = go_binding_ref(name, state);
    let right = render_numeric_expr(right, state)?;
    if !state.numeric_bindings.contains(name) {
        if is_any_binding(name, state) && op == "=" {
            return Some(format!(
                "func() float64 {{ {target} = {right}; return tsgodownToFloat64({target}) }}()"
            ));
        }
        return None;
    }
    match op {
        "=" => Some(format!(
            "func() float64 {{ {target} = {right}; return {target} }}()"
        )),
        "+=" | "-=" | "*=" | "/=" => Some(format!(
            "func() float64 {{ {target} {op} {right}; return {target} }}()"
        )),
        "%=" => Some(format!(
            "func() float64 {{ {target} = math.Mod({target}, {right}); return {target} }}()"
        )),
        "**=" => Some(format!(
            "func() float64 {{ {target} = math.Pow({target}, {right}); return {target} }}()"
        )),
        "&=" | "|=" | "^=" | "<<=" | ">>=" | ">>>=" => {
            let value = render_bitwise_compound_expr(op, &target, &right)?;
            Some(format!(
                "func() float64 {{ {target} = {value}; return {target} }}()"
            ))
        }
        _ => None,
    }
}

fn render_update_numeric_expr(
    op: &str,
    arg: &JsExpr,
    prefix: bool,
    state: &AotState,
) -> Option<String> {
    if !matches!(op, "++" | "--") {
        return None;
    }
    match arg {
        JsExpr::Ident { name } if state.numeric_bindings.contains(name) => {
            let target = go_binding_ref(name, state);
            let delta = if op == "++" { "1" } else { "-1" };
            if prefix {
                return Some(format!(
                    "func() float64 {{ {target} += {delta}; return {target} }}()"
                ));
            }
            if op == "++" {
                return Some(format!("tsgodownPostIncFloat(&{target})"));
            }
            Some(format!(
                "func() float64 {{ old := {target}; {target} -= 1; return old }}()"
            ))
        }
        JsExpr::Member {
            object,
            property,
            property_expr: None,
            optional: false,
        } => {
            let object = render_dynamic_object_source_expr(object, state)?;
            Some(format!(
                "tsgodownObjectPostInc({object}, {})",
                go_string_literal(property)
            ))
        }
        _ => None,
    }
}

fn render_bitwise_numeric_expr(
    op: &str,
    left: &JsExpr,
    right: &JsExpr,
    state: &AotState,
) -> Option<String> {
    let left = render_numeric_expr(left, state)?;
    let right = render_numeric_expr(right, state)?;
    let expr = match op {
        ">>>" => format!("(tsgodownToUint32({left}) >> uint(int({right}) & 31))"),
        ">>" => format!("(int({left}) >> uint(int({right}) & 31))"),
        "<<" => format!("(int({left}) << uint(int({right}) & 31))"),
        "&" => format!("(int({left}) & int({right}))"),
        "|" => format!("(int({left}) | int({right}))"),
        "^" => format!("(int({left}) ^ int({right}))"),
        _ => return None,
    };
    Some(format!("float64({expr})"))
}

fn render_bitwise_compound_expr(op: &str, target: &str, right: &str) -> Option<String> {
    let expr = match op {
        ">>>=" => format!("(tsgodownToUint32({target}) >> uint(int({right}) & 31))"),
        ">>=" => format!("(int({target}) >> uint(int({right}) & 31))"),
        "<<=" => format!("(int({target}) << uint(int({right}) & 31))"),
        "&=" => format!("(int({target}) & int({right}))"),
        "|=" => format!("(int({target}) | int({right}))"),
        "^=" => format!("(int({target}) ^ int({right}))"),
        _ => return None,
    };
    Some(format!("float64({expr})"))
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
    if is_length_member_property(property, property_expr.as_deref()) {
        if op != "=" {
            return None;
        }
        let length = render_numeric_expr(right, state)?;
        return Some(format!(
            "{target} = tsgodownStringArraySetLength({target}, {length})"
        ));
    }
    let index = if let Some(property_expr) = property_expr {
        render_numeric_expr(property_expr, state)?
    } else {
        number_literal(property)?
    };
    let value = render_string_expr(right, state).or_else(|| render_regexp_expr(right, state))?;
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

fn render_any_array_assignment_stmt(
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
        property_expr,
        optional: false,
    } = left
    else {
        return None;
    };
    let JsExpr::Ident { name } = object.as_ref() else {
        return None;
    };
    if !state.any_array_bindings.contains(name) {
        return None;
    }
    let target = go_binding_ref(name, state);
    if is_length_member_property(property, property_expr.as_deref()) {
        let length = render_numeric_expr(right, state)?;
        return Some(format!(
            "{target} = tsgodownAnyArraySetLength({target}, {length})"
        ));
    }
    let index = render_member_index_expr(property, property_expr.as_deref(), state)?;
    let value = render_any_array_value_expr(right, state)?;
    Some(format!(
        "{target} = tsgodownAnyArraySet({target}, {index}, {value})"
    ))
}

fn render_array_property_assignment_stmt(
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
        property_expr,
        optional: false,
    } = left
    else {
        return None;
    };
    let name = array_property_member_name(object, property, property_expr.as_deref())?;
    if !state.array_property_bindings.contains(name) {
        return None;
    }
    let key = render_dynamic_object_property_key_expr(property, property_expr.as_deref(), state)?;
    let props = array_property_map_ref(name, state);
    let order = array_property_order_ref(name, state);
    let value = render_json_value_expr(right, state)
        .or_else(|| render_string_expr(right, state).map(|value| format!("any({value})")))
        .or_else(|| render_numeric_expr(right, state).map(|value| format!("any({value})")))
        .or_else(|| render_bool_expr(right, state).map(|value| format!("any({value})")))
        .or_else(|| render_expr(right, state))?;
    Some(format!(
        "if _, ok := {props}[{key}]; !ok {{ {order} = append({order}, {key}) }}\n{props}[{key}] = {value}"
    ))
}

fn render_dynamic_object_array_assignment_stmt(
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
        property_expr,
        optional: false,
    } = left
    else {
        return None;
    };
    if is_length_member_property(property, property_expr.as_deref()) {
        return None;
    }
    let JsExpr::Member {
        object: parent,
        property: parent_property,
        property_expr: parent_property_expr,
        optional: false,
    } = object.as_ref()
    else {
        return None;
    };
    let parent = render_dynamic_object_source_expr(parent, state)?;
    let key = render_dynamic_object_property_key_expr(
        parent_property,
        parent_property_expr.as_deref(),
        state,
    )?;
    let index = render_member_index_expr(property, property_expr.as_deref(), state)?;
    let value = render_any_array_value_expr(right, state)?;
    Some(format!(
        "tsgodownObjectArraySet({parent}, {key}, {index}, {value})"
    ))
}

fn render_any_array_value_expr(expr: &JsExpr, state: &AotState) -> Option<String> {
    render_json_value_expr(expr, state)
        .or_else(|| render_regexp_expr(expr, state).map(|value| format!("any({value})")))
        .or_else(|| render_expr(expr, state))
}

fn render_function_static_member_assignment_stmt(
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
        property: _,
        property_expr: None,
        optional: false,
    } = left
    else {
        return None;
    };
    let JsExpr::Ident { name } = object.as_ref() else {
        return None;
    };
    if !state.functions.contains_key(name) {
        return None;
    }
    render_typed_slot_expr(right, state)?;
    Some(String::new())
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

fn render_bytes_assignment_stmt(
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
        property_expr,
        optional: false,
    } = left
    else {
        return None;
    };
    if matches!(object.as_ref(), JsExpr::Ident { name } if is_any_binding(name, state)) {
        return None;
    }
    let object = render_bytes_expr(object, state)?;
    let index = render_member_index_expr(property, property_expr.as_deref(), state)?;
    let value = render_numeric_expr(right, state)?;
    Some(format!("{object}[int({index})] = byte({value})"))
}

fn render_any_bytes_index_assignment_stmt(
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
        property_expr,
        optional: false,
    } = left
    else {
        return None;
    };
    let JsExpr::Ident { name } = object.as_ref() else {
        return None;
    };
    if !is_any_binding(name, state) {
        return None;
    }
    if !is_numeric_member_index_shape(property, property_expr.as_deref()) {
        return None;
    }
    let index = render_member_index_expr(property, property_expr.as_deref(), state)?;
    let value = render_numeric_expr(right, state)?;
    Some(format!(
        "tsgodownSetBytesIndexAny({}, {index}, {value})",
        go_binding_ref(name, state)
    ))
}

fn render_any_index_compound_assignment_stmt(
    op: &str,
    left: &JsExpr,
    right: &JsExpr,
    state: &AotState,
) -> Option<String> {
    if op != "|=" {
        return None;
    }
    let JsExpr::Member {
        object,
        property,
        property_expr,
        optional: false,
    } = left
    else {
        return None;
    };
    if !is_numeric_member_index_shape(property, property_expr.as_deref()) {
        return None;
    }
    let object = render_expr(object, state)?;
    let index = render_member_index_expr(property, property_expr.as_deref(), state)?;
    let value = render_numeric_expr(right, state)?;
    Some(format!(
        "tsgodownBitwiseOrAssignIndexAny({object}, {index}, {value})"
    ))
}

fn render_bytes_set_stmt(callee: &JsExpr, args: &[JsExpr], state: &AotState) -> Option<String> {
    let JsExpr::Member {
        object,
        property,
        property_expr: None,
        optional: false,
    } = callee
    else {
        return None;
    };
    if property != "set" || args.is_empty() || args.len() > 2 {
        return None;
    }
    let target = render_bytes_expr(object, state)?;
    let source = render_bytes_expr(args.first()?, state)?;
    let offset = args
        .get(1)
        .map(|expr| render_numeric_expr(expr, state))
        .unwrap_or_else(|| Some("0".to_string()))?;
    Some(format!("copy({target}[int({offset}):], {source})"))
}

fn render_member_index_expr(
    property: &str,
    property_expr: Option<&JsExpr>,
    state: &AotState,
) -> Option<String> {
    if let Some(property_expr) = property_expr {
        if !property.is_empty() {
            return None;
        }
        return render_numeric_expr(property_expr, state);
    }
    number_literal(property)
}

fn is_numeric_member_index_shape(property: &str, property_expr: Option<&JsExpr>) -> bool {
    match property_expr {
        Some(expr) => property.is_empty() && is_numeric_property_key_expr(expr),
        None => number_literal(property).is_some(),
    }
}

fn is_known_numeric_member_index(
    property: &str,
    property_expr: Option<&JsExpr>,
    state: &AotState,
) -> bool {
    if is_numeric_member_index_shape(property, property_expr) {
        return true;
    }
    matches!(
        property_expr,
        Some(JsExpr::Ident { name }) if property.is_empty() && state.numeric_bindings.contains(name)
    )
}

fn render_dynamic_object_assignment_stmt(
    op: &str,
    left: &JsExpr,
    right: &JsExpr,
    state: &AotState,
) -> Option<String> {
    if !matches!(op, "=" | "??=") {
        return None;
    }
    let JsExpr::Member {
        object,
        property,
        property_expr,
        optional: false,
    } = left
    else {
        return None;
    };
    let order_name = if let JsExpr::Ident { name } = object.as_ref() {
        state
            .ordered_dynamic_object_bindings
            .contains(name)
            .then(|| dynamic_object_order_ref(name, state))
    } else {
        None
    };
    let object = render_dynamic_object_source_expr(object, state)?;
    let key = render_dynamic_object_property_key_expr(property, property_expr.as_deref(), state)?;
    let value = render_json_value_expr(right, state)
        .or_else(|| render_numeric_expr(right, state))
        .or_else(|| render_expr(right, state))?;
    if op == "??=" {
        return Some(format!(
            "if tsgodownObjectProp({object}, {key}) == nil {{ tsgodownObjectSetProp({object}, {key}, {value}) }}"
        ));
    }
    if let Some(order) = order_name {
        return Some(format!(
            "if _, ok := {object}[{key}]; !ok {{ {order} = append({order}, {key}) }}\ntsgodownObjectSetProp({object}, {key}, {value})"
        ));
    }
    Some(format!("tsgodownObjectSetProp({object}, {key}, {value})"))
}

fn render_module_exports_assignment_stmt(right: &JsExpr, state: &AotState) -> Option<String> {
    let target = state.module_exports_ref.as_ref()?;
    let value =
        render_dynamic_object_init_expr(right, state).or_else(|| render_expr(right, state))?;
    Some(format!("{target} = {value}"))
}

fn render_process_env_assignment_stmt(
    op: &str,
    left: &JsExpr,
    right: &JsExpr,
    state: &AotState,
) -> Option<String> {
    if op != "=" {
        return None;
    }
    let name = process_env_lookup_name(left)?;
    let value = render_string_expr(right, state).or_else(|| {
        let value = render_expr(right, state)?;
        Some(format!("tsgodownToString({value})"))
    })?;
    Some(format!(
        "_ = os.Setenv({}, {value})",
        go_string_literal(name)
    ))
}

fn render_reflect_delete_property_stmt(
    callee: &JsExpr,
    args: &[JsExpr],
    state: &AotState,
) -> Option<String> {
    if !is_reflect_delete_property_call(callee, args) {
        return None;
    }
    if !matches!(args.first(), Some(expr) if is_process_env_ref(expr)) {
        return None;
    }
    let key = render_string_expr(args.get(1)?, state)?;
    Some(format!("_ = os.Unsetenv({key})"))
}

fn render_delete_object_property_expr(arg: &JsExpr, state: &AotState) -> Option<String> {
    let JsExpr::Member {
        object,
        property,
        property_expr,
        optional: false,
    } = arg
    else {
        return None;
    };
    let object = render_dynamic_object_source_expr(object, state)?;
    let key = render_dynamic_object_property_key_expr(property, property_expr.as_deref(), state)?;
    Some(format!("tsgodownObjectDelete({object}, {key})"))
}

fn is_reflect_delete_property_call(callee: &JsExpr, args: &[JsExpr]) -> bool {
    args.len() == 2
        && matches!(
            callee,
            JsExpr::Member {
                object,
                property,
                property_expr: None,
                optional: false,
            } if property == "deleteProperty"
                && matches!(object.as_ref(), JsExpr::Ident { name } if name == "Reflect")
        )
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
    if !matches!(op, "++" | "--") {
        return None;
    }
    let _ = render_update_numeric_expr(op, arg, false, state)?;
    match arg {
        JsExpr::Ident { name } if state.numeric_bindings.contains(name) => {
            Some(format!("{}{}", go_binding_ref(name, state), op))
        }
        JsExpr::Member {
            object,
            property,
            property_expr: None,
            optional: false,
        } => {
            let object = render_dynamic_object_source_expr(object, state)?;
            Some(format!(
                "_ = tsgodownObjectPostInc({object}, {})",
                go_string_literal(property)
            ))
        }
        _ => None,
    }
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
        JsExpr::Unary { op, .. } if op == "void" => Some("\"undefined\"".to_string()),
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

fn module_has_caught_require_spec(module: &Module, spec: &str) -> bool {
    module
        .executable
        .as_ref()
        .map(|executable| stmt_list_has_caught_require_spec(&executable.stmts, spec, false))
        .unwrap_or(false)
}

fn stmt_list_has_caught_require_spec(stmts: &[JsStmt], spec: &str, caught: bool) -> bool {
    stmts
        .iter()
        .any(|stmt| stmt_has_caught_require_spec(stmt, spec, caught))
}

fn stmt_has_caught_require_spec(stmt: &JsStmt, spec: &str, caught: bool) -> bool {
    match stmt {
        JsStmt::Expr { expr }
        | JsStmt::Throw { value: expr }
        | JsStmt::Return { value: Some(expr) }
        | JsStmt::Yield {
            value: Some(expr), ..
        }
        | JsStmt::VarDecl {
            init: Some(expr), ..
        } => expr_has_caught_require_spec(expr, spec, caught),
        JsStmt::If {
            test,
            consequent,
            alternate,
        } => {
            expr_has_caught_require_spec(test, spec, caught)
                || stmt_list_has_caught_require_spec(consequent, spec, caught)
                || stmt_list_has_caught_require_spec(alternate, spec, caught)
        }
        JsStmt::For {
            init,
            test,
            update,
            body,
        } => {
            stmt_list_has_caught_require_spec(init, spec, caught)
                || test
                    .as_ref()
                    .is_some_and(|expr| expr_has_caught_require_spec(expr, spec, caught))
                || update
                    .as_ref()
                    .is_some_and(|expr| expr_has_caught_require_spec(expr, spec, caught))
                || stmt_list_has_caught_require_spec(body, spec, caught)
        }
        JsStmt::ForOf { right, body, .. } => {
            expr_has_caught_require_spec(right, spec, caught)
                || stmt_list_has_caught_require_spec(body, spec, caught)
        }
        JsStmt::While { test, body } | JsStmt::DoWhile { test, body } => {
            expr_has_caught_require_spec(test, spec, caught)
                || stmt_list_has_caught_require_spec(body, spec, caught)
        }
        JsStmt::Switch {
            discriminant,
            cases,
        } => {
            expr_has_caught_require_spec(discriminant, spec, caught)
                || cases.iter().any(|case| {
                    case.test
                        .as_ref()
                        .is_some_and(|expr| expr_has_caught_require_spec(expr, spec, caught))
                        || stmt_list_has_caught_require_spec(&case.consequent, spec, caught)
                })
        }
        JsStmt::Try {
            body,
            catch_body,
            finally_body,
            ..
        } => {
            let catches = !catch_body.is_empty();
            stmt_list_has_caught_require_spec(body, spec, caught || catches)
                || stmt_list_has_caught_require_spec(catch_body, spec, caught)
                || stmt_list_has_caught_require_spec(finally_body, spec, caught)
        }
        JsStmt::FunctionDecl { body, .. } => stmt_list_has_caught_require_spec(body, spec, caught),
        JsStmt::ClassDecl { methods, .. } => methods
            .iter()
            .any(|method| stmt_list_has_caught_require_spec(&method.body, spec, caught)),
        JsStmt::Label { body, .. } => stmt_list_has_caught_require_spec(body, spec, caught),
        JsStmt::Return { value: None }
        | JsStmt::Yield { value: None, .. }
        | JsStmt::VarDecl { init: None, .. }
        | JsStmt::Break { .. }
        | JsStmt::Continue { .. } => false,
    }
}

fn expr_has_caught_require_spec(expr: &JsExpr, spec: &str, caught: bool) -> bool {
    match expr {
        JsExpr::Call { callee, args, .. } if caught => {
            matches!(
                render_require_spec_arg(callee, args).as_deref(),
                Some(candidate) if candidate == go_string_literal(spec)
            ) || expr_children_have_caught_require_spec(expr, spec, caught)
        }
        _ => expr_children_have_caught_require_spec(expr, spec, caught),
    }
}

fn expr_children_have_caught_require_spec(expr: &JsExpr, spec: &str, caught: bool) -> bool {
    match expr {
        JsExpr::Array { items } => items
            .iter()
            .any(|item| expr_has_caught_require_spec(item, spec, caught)),
        JsExpr::ArraySpread { items } => items
            .iter()
            .any(|item| expr_has_caught_require_spec(&item.value, spec, caught)),
        JsExpr::Object { props } => props.iter().any(|prop| {
            prop.key_expr
                .as_ref()
                .is_some_and(|expr| expr_has_caught_require_spec(expr, spec, caught))
                || expr_has_caught_require_spec(&prop.value, spec, caught)
        }),
        JsExpr::ObjectRest { object, .. }
        | JsExpr::Unary { arg: object, .. }
        | JsExpr::Await { arg: object }
        | JsExpr::Update { arg: object, .. }
        | JsExpr::Spread { arg: object } => expr_has_caught_require_spec(object, spec, caught),
        JsExpr::Function { body, .. } => stmt_list_has_caught_require_spec(body, spec, caught),
        JsExpr::Class {
            super_class,
            methods,
        } => {
            super_class
                .as_ref()
                .is_some_and(|expr| expr_has_caught_require_spec(expr, spec, caught))
                || methods
                    .iter()
                    .any(|method| stmt_list_has_caught_require_spec(&method.body, spec, caught))
        }
        JsExpr::Binary { left, right, .. } | JsExpr::Assign { left, right, .. } => {
            expr_has_caught_require_spec(left, spec, caught)
                || expr_has_caught_require_spec(right, spec, caught)
        }
        JsExpr::Conditional {
            test,
            consequent,
            alternate,
        } => {
            expr_has_caught_require_spec(test, spec, caught)
                || expr_has_caught_require_spec(consequent, spec, caught)
                || expr_has_caught_require_spec(alternate, spec, caught)
        }
        JsExpr::Call { callee, args, .. } | JsExpr::New { callee, args } => {
            expr_has_caught_require_spec(callee, spec, caught)
                || args
                    .iter()
                    .any(|arg| expr_has_caught_require_spec(arg, spec, caught))
        }
        JsExpr::Member {
            object,
            property_expr,
            ..
        } => {
            expr_has_caught_require_spec(object, spec, caught)
                || property_expr
                    .as_ref()
                    .is_some_and(|expr| expr_has_caught_require_spec(expr, spec, caught))
        }
        JsExpr::Template { exprs, .. } | JsExpr::Sequence { exprs } => exprs
            .iter()
            .any(|expr| expr_has_caught_require_spec(expr, spec, caught)),
        JsExpr::Value { .. } | JsExpr::Ident { .. } | JsExpr::This | JsExpr::Super => false,
    }
}

fn render_require_spec_arg(callee: &JsExpr, args: &[JsExpr]) -> Option<String> {
    if !matches!(callee, JsExpr::Ident { name } if name == "require") {
        return None;
    }
    let JsExpr::Value {
        value: JsValue::String { value },
    } = args.first()?
    else {
        return None;
    };
    Some(go_string_literal(value))
}

fn is_local_function_namespace_object(name: &str, expr: &JsExpr, state: &AotState) -> bool {
    matches!(expr, JsExpr::Object { .. })
        && state
            .namespace_functions
            .keys()
            .any(|(namespace, _)| namespace == name)
}

fn string_prototype_method_alias(expr: &JsExpr) -> Option<&'static str> {
    let JsExpr::Member {
        object,
        property,
        property_expr: None,
        optional: false,
    } = expr
    else {
        return None;
    };
    let method = match property.as_str() {
        "toLowerCase" | "toUpperCase" | "trim" | "trimStart" | "trimEnd" | "includes"
        | "indexOf" | "lastIndexOf" | "startsWith" | "endsWith" | "charAt" | "charCodeAt"
        | "replace" | "replaceAll" | "slice" | "substring" | "substr" | "repeat" => {
            property.as_str()
        }
        _ => return None,
    };
    let JsExpr::Member {
        object,
        property: prototype,
        property_expr: None,
        optional: false,
    } = object.as_ref()
    else {
        return None;
    };
    if prototype == "prototype"
        && matches!(object.as_ref(), JsExpr::Ident { name } if name == "String")
    {
        return Some(match method {
            "toLowerCase" => "toLowerCase",
            "toUpperCase" => "toUpperCase",
            "trim" => "trim",
            "includes" => "includes",
            "indexOf" => "indexOf",
            "charAt" => "charAt",
            "charCodeAt" => "charCodeAt",
            "replace" => "replace",
            "slice" => "slice",
            _ => return None,
        });
    }
    None
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

fn is_node_assert_spec(spec: &str) -> bool {
    matches!(
        spec.strip_prefix("node:").unwrap_or(spec),
        "assert" | "assert/strict"
    )
}

fn is_node_fs_promises_spec(spec: &str) -> bool {
    matches!(spec.strip_prefix("node:").unwrap_or(spec), "fs/promises")
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
        expr if render_any_array_index_expr(expr, state).is_some() => {
            render_any_array_index_expr(expr, state)
        }
        JsExpr::Array { .. } => {
            render_string_array_expr(expr, state).or_else(|| render_json_value_expr(expr, state))
        }
        JsExpr::ArraySpread { .. } => render_any_array_expr(expr, state),
        JsExpr::Object { .. } => render_object_map_expr(expr, state),
        JsExpr::Binary { op, left, right } if op == "??" => {
            let left = render_expr(left, state)?;
            let right = render_expr(right, state)?;
            Some(format!("tsgodownNullish({left}, {right})"))
        }
        JsExpr::Binary { op, .. } if op == "+" => render_string_expr(expr, state).or_else(|| {
            let JsExpr::Binary { left, right, .. } = expr else {
                return None;
            };
            if is_any_expr(left, state) || is_any_expr(right, state) {
                let left = render_expr(left, state)?;
                let right = render_expr(right, state)?;
                return Some(format!("tsgodownAdd({left}, {right})"));
            }
            if let (Some(left), Some(right)) = (
                render_numeric_expr(left, state),
                render_numeric_expr(right, state),
            ) {
                return Some(format!("({left} + {right})"));
            }
            let left = render_expr(left, state)?;
            let right = render_expr(right, state)?;
            Some(format!("tsgodownAdd({left}, {right})"))
        }),
        JsExpr::Binary { op, left, right } if is_bitwise_binary_op(op) => {
            render_bitwise_numeric_expr(op, left, right, state)
        }
        JsExpr::Binary { op, left, right } if is_numeric_binary_op(op) => {
            let left = render_numeric_expr(left, state)?;
            let right = render_numeric_expr(right, state)?;
            if op == "%" {
                return Some(format!("math.Mod({left}, {right})"));
            }
            Some(format!("({left} {op} {right})"))
        }
        JsExpr::Binary { op, .. } if op == "instanceof" => render_bool_expr(expr, state),
        JsExpr::Binary { op, .. } if op == "in" => render_bool_expr(expr, state),
        JsExpr::Binary { op, .. } if go_comparison_op(op).is_some() => {
            render_bool_expr(expr, state)
        }
        JsExpr::Binary { op, .. } if matches!(op.as_str(), "&&" | "||") => {
            render_bool_expr(expr, state).or_else(|| render_logical_value_expr(expr, state))
        }
        JsExpr::Unary { op, .. } if op == "void" => Some("nil".to_string()),
        JsExpr::Unary { .. } => render_string_expr(expr, state)
            .or_else(|| render_numeric_expr(expr, state))
            .or_else(|| render_bool_expr(expr, state)),
        JsExpr::Conditional {
            test,
            consequent,
            alternate,
        } => render_conditional_expr(test, consequent, alternate, state, render_expr, "any"),
        JsExpr::Call {
            callee,
            args,
            optional: true,
        } => render_optional_call_expr(callee, args, state),
        JsExpr::Call { callee, args, .. } if is_local_function_call(callee, state) => {
            render_call_expr(callee, args, state)
        }
        JsExpr::Call { callee, args, .. } => render_sync_iife_expr(callee, args, state)
            .or_else(|| render_error_object_expr(expr, state))
            .or_else(|| render_symbol_expr(expr, state))
            .or_else(|| render_string_expr(expr, state))
            .or_else(|| render_numeric_expr(expr, state))
            .or_else(|| render_bytes_expr(expr, state))
            .or_else(|| render_any_array_expr(expr, state))
            .or_else(|| render_string_array_expr(expr, state))
            .or_else(|| render_any_array_pop_call(callee, args, state))
            .or_else(|| render_map_call_expr(callee, args, state))
            .or_else(|| render_set_call_expr(callee, args, state))
            .or_else(|| render_object_map_expr(expr, state))
            .or_else(|| render_array_find_call(callee, args, state))
            .or_else(|| render_call_expr(callee, args, state))
            .or_else(|| render_bool_expr(expr, state)),
        JsExpr::Await { arg } => {
            render_async_iife_expr(arg, state).or_else(|| render_expr(arg, state))
        }
        JsExpr::New { .. } => render_error_object_expr(expr, state)
            .or_else(|| render_date_expr(expr, state))
            .or_else(|| render_bytes_expr(expr, state))
            .or_else(|| render_any_array_expr(expr, state))
            .or_else(|| render_url_new_expr(expr, state))
            .or_else(|| render_event_emitter_new_expr(expr, state))
            .or_else(|| render_js_set_expr(expr, state))
            .or_else(|| render_new_class_expr(expr, state).map(|(_, value)| value)),
        expr if is_process_version_expr(expr) => render_process_version_expr(expr),
        expr if is_process_platform_expr(expr) => Some("tsgodownProcessPlatform()".to_string()),
        expr if is_process_arch_expr(expr) => Some("tsgodownProcessArch()".to_string()),
        expr if is_process_exec_path_expr(expr) => Some("tsgodownProcessExecPath()".to_string()),
        expr if is_process_env_ref(expr) => Some("tsgodownProcessEnv()".to_string()),
        expr if is_process_versions_ref(expr) => Some(render_process_versions_expr()),
        expr if is_process_cwd_ref(expr) => render_string_function_expr(expr, state),
        expr if is_process_stdio_ref(expr).is_some() => render_process_stdio_expr(expr),
        expr if is_process_function_ref(expr).is_some() => render_process_function_ref(expr),
        JsExpr::Member {
            object,
            property,
            property_expr,
            optional: true,
        } => {
            let object = render_expr(object, state)?;
            let key =
                render_dynamic_object_property_key_expr(property, property_expr.as_deref(), state)?;
            Some(format!("tsgodownObjectProp({object}, {key})"))
        }
        JsExpr::Member {
            object,
            property,
            property_expr: None,
            optional: false,
        } => render_symbol_expr(expr, state)
            .or_else(|| render_iterator_value_member_expr(object, property, state))
            .or_else(|| render_class_getter_member_expr(object, property, state))
            .or_else(|| render_class_field_member_expr(object, property, state))
            .or_else(|| render_static_member_expr(object, property, state))
            .or_else(|| {
                if property == "length" || property.parse::<usize>().is_ok() {
                    render_numeric_expr(expr, state).or_else(|| render_string_expr(expr, state))
                } else {
                    None
                }
            })
            .or_else(|| render_array_property_member_expr(object, property, None, state))
            .or_else(|| render_dynamic_object_member_expr(object, property, state))
            .or_else(|| render_string_expr(expr, state))
            .or_else(|| render_numeric_expr(expr, state))
            .or_else(|| render_bool_expr(expr, state)),
        JsExpr::Member {
            object,
            property,
            property_expr: Some(property_expr),
            optional: false,
        } => render_array_property_member_expr(object, property, Some(property_expr), state)
            .or_else(|| {
                render_dynamic_object_member_access_expr(
                    object,
                    property,
                    Some(property_expr),
                    state,
                )
            })
            .or_else(|| render_string_expr(expr, state))
            .or_else(|| render_numeric_expr(expr, state))
            .or_else(|| render_bool_expr(expr, state)),
        JsExpr::Template { quasis, exprs } => render_template_string_expr(quasis, exprs, state),
        _ => None,
    }
}

fn is_any_expr(expr: &JsExpr, state: &AotState) -> bool {
    matches!(expr, JsExpr::Ident { name } if is_any_binding(name, state))
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

fn render_sync_iife_expr(callee: &JsExpr, args: &[JsExpr], state: &AotState) -> Option<String> {
    let JsExpr::Function {
        params,
        rest_param: None,
        r#async: false,
        generator: false,
        body,
        ..
    } = callee
    else {
        return None;
    };
    if args.len() > params.len() {
        return None;
    }
    let param_kinds = infer_function_param_kinds(params, body, &state.builtin_function_aliases);
    let mut function_state = clone_aot_state(state);
    for (param, kind) in params.iter().zip(param_kinds.iter()) {
        function_state.bind_slot(param, sanitize_go_identifier(param), *kind);
    }
    let rendered_params = params
        .iter()
        .zip(param_kinds.iter())
        .map(|(param, kind)| {
            format!(
                "{} {}",
                sanitize_go_identifier(param),
                go_type_for_slot(*kind)
            )
        })
        .collect::<Vec<_>>()
        .join(", ");
    let mut rendered_args = render_call_args(args, &param_kinds[..args.len()], state)?;
    for kind in param_kinds.iter().skip(args.len()) {
        rendered_args.push(render_missing_arg_to_kind(*kind));
    }
    let rendered_body = render_function_body(body, &function_state)?;
    let rendered_body = if rendered_body.trim_end().ends_with("return nil") {
        rendered_body
    } else {
        format!("{rendered_body}\nreturn nil")
    };
    Some(format!(
        "func({rendered_params}) any {{\n{}\n}}({})",
        indent_lines(&rendered_body),
        rendered_args.join(", ")
    ))
}

fn render_missing_arg_to_kind(kind: AotSlotKind) -> String {
    match kind {
        AotSlotKind::Any | AotSlotKind::AnyArray => "nil".to_string(),
        AotSlotKind::Bool => "false".to_string(),
        AotSlotKind::Bytes => "nil".to_string(),
        AotSlotKind::Date => "\"\"".to_string(),
        AotSlotKind::Number => "math.NaN()".to_string(),
        AotSlotKind::NumberArray => "nil".to_string(),
        AotSlotKind::RegExp => "\"\"".to_string(),
        AotSlotKind::String => "\"\"".to_string(),
        AotSlotKind::StringArray => "nil".to_string(),
        AotSlotKind::BoolFunction => "nil".to_string(),
        AotSlotKind::StringFunction => "nil".to_string(),
    }
}

fn render_iife_block_expr(body: &[JsStmt], state: &AotState) -> Option<String> {
    let mut block_state = clone_aot_state(state);
    mark_number_array_locals(body, &mut block_state);
    mark_string_array_locals(body, &mut block_state);
    mark_any_array_locals(body, &mut block_state);
    mark_array_property_locals(body, &mut block_state);
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
        JsExpr::Ident { name } if name == "Infinity" => Some("math.Inf(1)".to_string()),
        JsExpr::Ident { name } if state.numeric_bindings.contains(name) => {
            Some(go_binding_ref(name, state))
        }
        JsExpr::Ident { name } if is_any_binding(name, state) => {
            let value = go_binding_ref(name, state);
            Some(format!("tsgodownToFloat64({value})"))
        }
        JsExpr::Assign { op, left, right } if matches!(op.as_str(), "=" | "+=" | "-=") => {
            render_numeric_assignment_expr(op, left, right, state)
        }
        JsExpr::Update { op, arg, prefix } => render_update_numeric_expr(op, arg, *prefix, state),
        JsExpr::Binary { op, left, right } if is_bitwise_binary_op(op) => {
            render_bitwise_numeric_expr(op, left, right, state)
        }
        JsExpr::Binary { op, left, right } if op == "**" => {
            let left = render_numeric_expr(left, state)?;
            let right = render_numeric_expr(right, state)?;
            Some(format!("math.Pow({left}, {right})"))
        }
        JsExpr::Unary { op, arg } if op == "~" => {
            let value = render_numeric_expr(arg, state)?;
            Some(format!("float64(int32(^int32({value})))"))
        }
        expr if render_number_array_index_expr(expr, state).is_some() => {
            render_number_array_index_expr(expr, state)
        }
        expr if render_string_array_length_expr(expr, state).is_some() => {
            render_string_array_length_expr(expr, state)
        }
        expr if render_any_array_length_expr(expr, state).is_some() => {
            render_any_array_length_expr(expr, state)
        }
        JsExpr::Member {
            object,
            property,
            property_expr,
            optional: true,
        } if is_length_member_property(property, property_expr.as_deref()) => {
            let object = render_expr(object, state)?;
            Some(format!("tsgodownLengthFloat({object}, true)"))
        }
        JsExpr::Member {
            object,
            property,
            property_expr,
            optional: false,
        } if is_length_member_property(property, property_expr.as_deref())
            && matches!(object.as_ref(), JsExpr::Ident { name } if is_any_binding(name, state)) =>
        {
            let object = render_expr(object, state)?;
            Some(format!("tsgodownLengthFloat({object}, false)"))
        }
        expr if render_bytes_length_expr(expr, state).is_some() => {
            render_bytes_length_expr(expr, state)
        }
        expr if render_any_array_index_expr(expr, state).is_some() => {
            let value = render_any_array_index_expr(expr, state)?;
            Some(format!("tsgodownToFloat64({value})"))
        }
        JsExpr::Call { callee, args, .. }
            if render_array_find_call(callee, args, state).is_some() =>
        {
            render_array_find_call(callee, args, state)
        }
        JsExpr::Call { callee, args, .. }
            if render_array_index_of_call(callee, args, state).is_some() =>
        {
            render_array_index_of_call(callee, args, state)
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
        } if is_set_size_member(object, property, property_expr.as_deref(), state) => {
            let object = render_set_expr(object, state)?;
            Some(format!("tsgodownSetSize({object})"))
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
            property_expr,
            optional: false,
        } if is_length_member_property(property, property_expr.as_deref())
            && render_any_array_expr(object, state).is_some() =>
        {
            let object = render_any_array_expr(object, state)?;
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
        } if render_number_static_member_expr(object, property).is_some() => {
            render_number_static_member_expr(object, property)
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
            let object =
                render_expr(object, state).or_else(|| render_string_expr(object, state))?;
            Some(format!("tsgodownLengthFloat({object}, false)"))
        }
        JsExpr::Member {
            object,
            property,
            property_expr,
            optional: false,
        } if render_member_index_expr(property, property_expr.as_deref(), state).is_some()
            && matches!(object.as_ref(), JsExpr::Ident { name } if is_any_binding(name, state))
            && (render_any_array_expr(object, state).is_some()
                || is_known_numeric_member_index(property, property_expr.as_deref(), state)) =>
        {
            let object = render_expr(object, state)?;
            let index = render_member_index_expr(property, property_expr.as_deref(), state)?;
            Some(format!("tsgodownIndexFloat({object}, {index})"))
        }
        JsExpr::Member {
            object,
            property,
            property_expr,
            optional: false,
        } if render_member_index_expr(property, property_expr.as_deref(), state).is_some()
            && render_bytes_expr(object, state).is_some() =>
        {
            let object = render_bytes_expr(object, state)?;
            let index = render_member_index_expr(property, property_expr.as_deref(), state)?;
            Some(format!("float64({object}[int({index})])"))
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
            if op == "%" {
                return Some(format!("math.Mod({left}, {right})"));
            }
            Some(format!("({left} {op} {right})"))
        }
        JsExpr::Binary { op, left, right } if op == "||" => {
            let left = render_numeric_expr(left, state)?;
            let right = render_numeric_expr(right, state)?;
            Some(format!(
                "func() float64 {{ if tsgodownToBool({left}) {{ return {left} }}; return {right} }}()"
            ))
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
        JsExpr::Call { callee, args, .. } if is_date_now_call(callee, args) => {
            Some("tsgodownDateNow()".to_string())
        }
        JsExpr::Call { callee, args, .. } if is_parse_int_call(callee, args) => {
            let value = render_expr(args.first()?, state)?;
            let radix = args
                .get(1)
                .map(|arg| render_numeric_expr(arg, state))
                .unwrap_or_else(|| Some("10".to_string()))?;
            Some(format!("tsgodownParseInt({value}, {radix})"))
        }
        JsExpr::Call { callee, args, .. } if is_parse_float_call(callee, args) => {
            let value = render_expr(args.first()?, state)?;
            Some(format!("tsgodownToFloat64({value})"))
        }
        JsExpr::Call { callee, args, .. } if is_number_cast_call(callee, args) => {
            let value = render_expr(args.first()?, state)?;
            Some(format!("tsgodownToFloat64({value})"))
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
        JsExpr::Call { callee, args, .. } => render_math_numeric_call(callee, args, state)
            .or_else(|| render_string_numeric_method_alias_call(callee, args, state))
            .or_else(|| render_string_numeric_method_call(callee, args, state)),
        _ => None,
    }
}

fn render_math_numeric_call(callee: &JsExpr, args: &[JsExpr], state: &AotState) -> Option<String> {
    let JsExpr::Member {
        object,
        property,
        property_expr: None,
        optional: false,
    } = callee
    else {
        return None;
    };
    if !matches!(object.as_ref(), JsExpr::Ident { name } if name == "Math") {
        return None;
    }
    match property.as_str() {
        "random" if args.is_empty() => Some("tsgodownMathRandom()".to_string()),
        "max" | "min" => {
            let args = args
                .iter()
                .map(|arg| render_numeric_expr(arg, state))
                .collect::<Option<Vec<_>>>()?;
            let helper = if property == "max" {
                "tsgodownMathMax"
            } else {
                "tsgodownMathMin"
            };
            Some(format!("{helper}({})", args.join(", ")))
        }
        "floor" | "ceil" | "trunc" | "round" if args.len() == 1 => {
            let value = render_numeric_expr(args.first()?, state)?;
            let function = match property.as_str() {
                "floor" => "math.Floor",
                "ceil" => "math.Ceil",
                "trunc" => "math.Trunc",
                "round" => "tsgodownMathRound",
                _ => return None,
            };
            Some(format!("{function}({value})"))
        }
        "abs" if args.len() == 1 => {
            let value = render_numeric_expr(args.first()?, state)?;
            Some(format!("math.Abs({value})"))
        }
        "pow" if args.len() == 2 => {
            let base = render_numeric_expr(args.first()?, state)?;
            let exponent = render_numeric_expr(args.get(1)?, state)?;
            Some(format!("math.Pow({base}, {exponent})"))
        }
        _ => None,
    }
}

fn render_local_string_function_call(
    callee: &JsExpr,
    args: &[JsExpr],
    state: &AotState,
) -> Option<String> {
    let JsExpr::Ident { name } = callee else {
        return None;
    };
    let function = state.functions.get(name)?;
    if !function_returns_string(function, state) {
        return None;
    }
    let call = render_call_expr(callee, args, state)?;
    Some(format!("tsgodownToString({call})"))
}

fn is_date_now_call(callee: &JsExpr, args: &[JsExpr]) -> bool {
    args.is_empty()
        && matches!(
            callee,
            JsExpr::Member {
                object,
                property,
                property_expr: None,
                optional: false,
            } if property == "now"
                && matches!(object.as_ref(), JsExpr::Ident { name } if name == "Date")
        )
}

fn render_string_expr(expr: &JsExpr, state: &AotState) -> Option<String> {
    match expr {
        JsExpr::Value {
            value: JsValue::String { value },
        } => Some(go_string_literal(value)),
        JsExpr::Await { arg } => render_string_expr(arg, state),
        JsExpr::Ident { name } if state.string_bindings.contains(name) => {
            Some(go_binding_ref(name, state))
        }
        JsExpr::Ident { name } if state.date_bindings.contains(name) => {
            Some(go_binding_ref(name, state))
        }
        expr if render_any_array_index_expr(expr, state).is_some() => {
            let value = render_any_array_index_expr(expr, state)?;
            Some(format!("tsgodownToString({value})"))
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
        } if render_array_property_member_expr(object, property, None, state).is_some() => {
            let value = render_array_property_member_expr(object, property, None, state)?;
            Some(format!("tsgodownToString({value})"))
        }
        JsExpr::Member {
            object,
            property,
            property_expr: Some(property_expr),
            optional: false,
        } if render_array_property_member_expr(object, property, Some(property_expr), state)
            .is_some() =>
        {
            let value =
                render_array_property_member_expr(object, property, Some(property_expr), state)?;
            Some(format!("tsgodownToString({value})"))
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
            if !is_string_concat_operand(left, state) && !is_string_concat_operand(right, state) {
                return None;
            }
            let left = render_concat_operand_string_expr(left, state)?;
            let right = render_concat_operand_string_expr(right, state)?;
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
        JsExpr::Call { callee, args, .. } if is_uri_string_call(callee, args) => {
            render_uri_string_call(callee, args, state)
        }
        JsExpr::Call { callee, args, .. } if is_string_from_char_code_call(callee, args) => {
            let args = args
                .iter()
                .map(|arg| render_numeric_expr(arg, state))
                .collect::<Option<Vec<_>>>()?;
            Some(format!("tsgodownStringFromCharCode({})", args.join(", ")))
        }
        JsExpr::Call { callee, args, .. } if is_date_to_iso_call(callee, args, state) => {
            render_date_to_iso_call(callee, args, state)
        }
        JsExpr::Call { callee, args, .. }
            if render_number_to_string_call(callee, args, state).is_some() =>
        {
            render_number_to_string_call(callee, args, state)
        }
        JsExpr::Call { callee, args, .. }
            if render_symbol_to_string_call(callee, args, state).is_some() =>
        {
            render_symbol_to_string_call(callee, args, state)
        }
        JsExpr::Call { callee, args, .. } if is_json_stringify(callee) => {
            render_json_stringify_call(args, state)
        }
        JsExpr::Call { callee, args, .. } if is_object_prototype_to_string_call(callee, args) => {
            render_object_prototype_to_string_call(args.first()?, state)
        }
        JsExpr::Call { callee, args, .. } if is_value_to_string_call(callee, args) => {
            let JsExpr::Member { object, .. } = callee.as_ref() else {
                return None;
            };
            let value =
                render_json_value_expr(object, state).or_else(|| render_expr(object, state))?;
            Some(format!("tsgodownToString({value})"))
        }
        JsExpr::Call { callee, args, .. }
            if is_object_to_string_alias_call(callee, args, state) =>
        {
            render_object_prototype_to_string_call(args.first()?, state)
        }
        JsExpr::Call { callee, args, .. }
            if render_string_method_alias_call(callee, args, state).is_some() =>
        {
            render_string_method_alias_call(callee, args, state)
        }
        JsExpr::Call { callee, args, .. }
            if render_array_prototype_join_alias_call(callee, args, state).is_some() =>
        {
            render_array_prototype_join_alias_call(callee, args, state)
        }
        JsExpr::Call { callee, args, .. }
            if render_string_array_join_call(callee, args, state).is_some() =>
        {
            render_string_array_join_call(callee, args, state)
        }
        JsExpr::Call { callee, args, .. }
            if render_string_array_pop_call(callee, args, state).is_some() =>
        {
            render_string_array_pop_call(callee, args, state)
        }
        JsExpr::Call { callee, args, .. }
            if render_string_array_shift_call(callee, args, state).is_some() =>
        {
            render_string_array_shift_call(callee, args, state)
        }
        JsExpr::Call { callee, args, .. }
            if render_any_array_join_call(callee, args, state).is_some() =>
        {
            render_any_array_join_call(callee, args, state)
        }
        JsExpr::Call { callee, args, .. }
            if render_string_method_alias_call(callee, args, state).is_some() =>
        {
            render_string_method_alias_call(callee, args, state)
        }
        JsExpr::Call { callee, args, .. } => render_node_path_string_call(callee, args, state)
            .or_else(|| render_node_os_homedir_call(callee, args, state))
            .or_else(|| render_node_os_tmpdir_call(callee, args, state))
            .or_else(|| render_node_fs_mkdtemp_sync_call(callee, args, state))
            .or_else(|| render_node_fs_read_file_sync_call(callee, args, state))
            .or_else(|| render_node_fs_promises_read_file_call(callee, args, state))
            .or_else(|| render_buffer_to_string_call(callee, args, state))
            .or_else(|| render_url_search_params_get_call(callee, args, state))
            .or_else(|| render_crypto_hash_hex_digest_call(callee, args, state))
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
            .or_else(|| render_local_string_function_call(callee, args, state))
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

fn is_string_concat_operand(expr: &JsExpr, state: &AotState) -> bool {
    is_string_literal_like(expr)
        || matches!(expr, JsExpr::Ident { name } if state.string_bindings.contains(name))
        || render_string_array_index_expr(expr, state).is_some()
        || matches!(
            expr,
            JsExpr::Conditional {
                consequent,
                alternate,
                ..
            } if is_string_concat_operand(consequent, state)
                && is_string_concat_operand(alternate, state)
        )
        || matches!(
            expr,
            JsExpr::Binary { op, left, right }
                if op == "+"
                    && (is_string_concat_operand(left, state)
                        || is_string_concat_operand(right, state))
        )
}

fn render_concat_operand_string_expr(expr: &JsExpr, state: &AotState) -> Option<String> {
    render_string_expr(expr, state).or_else(|| {
        let value = render_expr(expr, state)?;
        Some(format!("tsgodownToString({value})"))
    })
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
        _ => {
            let value = render_expr(expr, state)?;
            Some(format!("tsgodownToString({value})"))
        }
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
        JsExpr::Binary { op, .. } if matches!(op.as_str(), "&&" | "||") => {
            let value = render_logical_value_expr(expr, state)?;
            Some(format!("tsgodownToBool({value})"))
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
    if matches!(
        value,
        JsExpr::Value {
            value: JsValue::Bool { .. },
        }
    ) || is_process_stdout_is_tty(value)
        || matches!(value, JsExpr::Ident { name } if state.bool_bindings.contains(name))
    {
        return true;
    }
    if matches!(value, JsExpr::Call { .. }) {
        return false;
    }
    let mut function_state = clone_aot_state(state);
    for (param, kind) in function.params.iter().zip(function.param_kinds.iter()) {
        function_state.bind_slot(param, sanitize_go_identifier(param), *kind);
    }
    render_bool_expr(value, &function_state).is_some()
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
        expr if is_js_string_numeric_cast_shape(expr, state) => {
            let value = render_numeric_expr(expr, state)?;
            Some(format!("tsgodownToString({value})"))
        }
        expr if is_js_string_bool_cast_shape(expr, state) => {
            let value = render_bool_expr(expr, state)?;
            Some(format!("tsgodownToString({value})"))
        }
        _ => {
            let value = render_expr(expr, state)?;
            Some(format!("tsgodownToString({value})"))
        }
    }
}

fn is_js_string_numeric_cast_shape(expr: &JsExpr, state: &AotState) -> bool {
    match expr {
        JsExpr::Value {
            value: JsValue::Number { .. },
        } => true,
        JsExpr::Ident { name } => state.numeric_bindings.contains(name),
        JsExpr::Unary { op, .. } => matches!(op.as_str(), "+" | "-"),
        JsExpr::Binary { op, .. } => is_numeric_binary_op(op) || is_bitwise_binary_op(op),
        JsExpr::Call { .. } => render_numeric_expr(expr, state).is_some(),
        JsExpr::Member { .. } => {
            render_numeric_expr(expr, state).is_some() && !expr_uses_any_binding(expr, state)
        }
        _ => false,
    }
}

fn is_js_string_bool_cast_shape(expr: &JsExpr, state: &AotState) -> bool {
    match expr {
        JsExpr::Value {
            value: JsValue::Bool { .. },
        } => true,
        JsExpr::Ident { name } => state.bool_bindings.contains(name),
        JsExpr::Unary { op, .. } => op == "!",
        JsExpr::Binary { op, .. } => {
            matches!(op.as_str(), "&&" | "||")
                || go_comparison_op(op).is_some()
                || op == "instanceof"
        }
        JsExpr::Conditional { .. } => render_bool_expr(expr, state).is_some(),
        JsExpr::Call { .. } => render_bool_expr(expr, state).is_some(),
        JsExpr::Member {
            object,
            property,
            property_expr: None,
            optional: false,
        } => static_member_kind(object, property, state) == Some(AotSlotKind::Bool),
        _ => false,
    }
}

fn is_number_to_string_call(callee: &JsExpr, args: &[JsExpr]) -> bool {
    matches!(args.len(), 0 | 1)
        && matches!(
            callee,
            JsExpr::Member {
                property,
                property_expr: None,
                optional: false,
                ..
            } if property == "toString"
        )
}

fn render_number_to_string_call(
    callee: &JsExpr,
    args: &[JsExpr],
    state: &AotState,
) -> Option<String> {
    if !is_number_to_string_call(callee, args) {
        return None;
    }
    let JsExpr::Member { object, .. } = callee else {
        return None;
    };
    if matches!(object.as_ref(), JsExpr::Ident { name } if is_any_binding(name, state)) {
        return None;
    }
    let value = render_numeric_expr(object, state)?;
    let radix = args
        .first()
        .map(|arg| render_numeric_expr(arg, state))
        .unwrap_or_else(|| Some("10".to_string()))?;
    Some(format!("tsgodownNumberToString({value}, {radix})"))
}

fn render_symbol_expr(expr: &JsExpr, state: &AotState) -> Option<String> {
    match expr {
        JsExpr::Call { callee, args, .. } if is_symbol_constructor_call(callee, args) => {
            let description = args
                .first()
                .map(|arg| render_symbol_description_expr(arg, state))
                .unwrap_or_else(|| Some("\"\"".to_string()))?;
            Some(format!("tsgodownNewSymbol({description})"))
        }
        JsExpr::Member {
            object,
            property,
            property_expr: None,
            optional: false,
        } if matches!(object.as_ref(), JsExpr::Ident { name } if name == "Symbol") => {
            match property.as_str() {
                "iterator" | "asyncIterator" | "hasInstance" | "isConcatSpreadable" | "match"
                | "matchAll" | "replace" | "search" | "species" | "split" | "toPrimitive"
                | "toStringTag" | "unscopables" => Some(format!(
                    "tsgodownWellKnownSymbol({})",
                    go_string_literal(&format!("Symbol.{property}"))
                )),
                _ => None,
            }
        }
        _ => None,
    }
}

fn render_symbol_description_expr(expr: &JsExpr, state: &AotState) -> Option<String> {
    if is_nullish_expr(expr) {
        return Some("\"\"".to_string());
    }
    render_js_to_string_expr(expr, state)
}

fn is_symbol_constructor_call(callee: &JsExpr, args: &[JsExpr]) -> bool {
    args.len() <= 1 && matches!(callee, JsExpr::Ident { name } if name == "Symbol")
}

fn render_symbol_to_string_call(
    callee: &JsExpr,
    args: &[JsExpr],
    state: &AotState,
) -> Option<String> {
    if args.is_empty() {
        let JsExpr::Member {
            object,
            property,
            property_expr: None,
            optional: false,
        } = callee
        else {
            return None;
        };
        if property == "toString" {
            let value = render_expr(object, state)?;
            return Some(format!("tsgodownSymbolToString({value})"));
        }
    }
    if is_symbol_prototype_to_string_call(callee, args) {
        let value = render_expr(args.first()?, state)?;
        return Some(format!("tsgodownSymbolToString({value})"));
    }
    None
}

fn is_symbol_prototype_to_string_call(callee: &JsExpr, args: &[JsExpr]) -> bool {
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
    property == "call" && is_symbol_prototype_to_string_ref(object)
}

fn is_symbol_prototype_to_string_ref(expr: &JsExpr) -> bool {
    matches!(
        expr,
        JsExpr::Member {
            object,
            property,
            property_expr: None,
            optional: false,
        } if property == "toString" && matches!(
            object.as_ref(),
            JsExpr::Member {
                object,
                property,
                property_expr: None,
                optional: false,
            } if property == "prototype"
                && matches!(object.as_ref(), JsExpr::Ident { name } if name == "Symbol")
        )
    )
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
        JsExpr::Ident { name } if name == "process" => Some("\"object\"".to_string()),
        JsExpr::Ident { name } if name == "Symbol" => Some("\"function\"".to_string()),
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
        JsExpr::Call { callee, args, .. }
            if render_node_path_string_call(callee, args, state).is_some()
                || render_node_os_homedir_call(callee, args, state).is_some()
                || render_node_os_tmpdir_call(callee, args, state).is_some() =>
        {
            Some("\"string\"".to_string())
        }
        JsExpr::Ident { name } if state.string_bindings.contains(name) => {
            Some("\"string\"".to_string())
        }
        JsExpr::Ident { name } if state.numeric_bindings.contains(name) => {
            Some("\"number\"".to_string())
        }
        JsExpr::Ident { name } if state.bool_bindings.contains(name) => {
            Some("\"boolean\"".to_string())
        }
        JsExpr::Call { callee, args, .. } if is_date_now_call(callee, args) => {
            Some("\"number\"".to_string())
        }
        JsExpr::Member {
            object,
            property,
            property_expr: None,
            optional: false,
        } if matches!(
            static_member_kind(object, property, state),
            Some(AotSlotKind::BoolFunction | AotSlotKind::StringFunction)
        ) =>
        {
            Some("\"function\"".to_string())
        }
        JsExpr::Ident { name } if state.bindings.contains(name) => {
            let value = go_binding_ref(name, state);
            Some(format!(
                "func() string {{ switch any({value}).(type) {{ case nil: return \"undefined\"; case bool: return \"boolean\"; case float64, int, int64: return \"number\"; case string: return \"string\"; case tsgodownSymbol: return \"symbol\"; default: return \"object\" }} }}()"
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
    if let Some(value) = render_string_index_equality_expr(op, go_op, left, right, state) {
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

fn render_string_index_equality_expr(
    op: &str,
    go_op: &str,
    left: &JsExpr,
    right: &JsExpr,
    state: &AotState,
) -> Option<String> {
    if !matches!(op, "==" | "!=" | "===" | "!==") {
        return None;
    }
    let (left, right) = if render_string_index_expr(left, state).is_some() {
        (
            render_string_index_expr(left, state)?,
            render_string_expr(right, state)?,
        )
    } else if render_string_index_expr(right, state).is_some() {
        (
            render_string_expr(left, state)?,
            render_string_index_expr(right, state)?,
        )
    } else {
        return None;
    };
    Some(format!("({left} {go_op} {right})"))
}

fn render_number_static_member_expr(object: &JsExpr, property: &str) -> Option<String> {
    if !matches!(object, JsExpr::Ident { name } if name == "Number") {
        return None;
    }
    match property {
        "MAX_SAFE_INTEGER" => Some("9007199254740991".to_string()),
        "MIN_SAFE_INTEGER" => Some("-9007199254740991".to_string()),
        "MAX_VALUE" => Some("1.7976931348623157e+308".to_string()),
        "MIN_VALUE" => Some("5e-324".to_string()),
        "EPSILON" => Some("2.220446049250313e-16".to_string()),
        "NaN" => None,
        "POSITIVE_INFINITY" | "NEGATIVE_INFINITY" => None,
        _ => None,
    }
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
    if let Some(arg) = object_freeze_arg(expr) {
        return render_object_literal(arg, state);
    }
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
    let type_fields = indent_lines(&type_fields.join("\n"));
    let value_fields = if value_fields.is_empty() {
        String::new()
    } else {
        format!("{},\n", indent_lines(&value_fields.join(",\n")))
    };
    Some((
        format!("struct {{\n{type_fields}\n}}{{\n{value_fields}}}"),
        AotObject { fields },
    ))
}

fn render_object_map_expr(expr: &JsExpr, state: &AotState) -> Option<String> {
    if let Some(arg) = object_freeze_arg(expr) {
        return render_object_map_expr(arg, state);
    }
    if let Some(value) = render_error_object_expr(expr, state) {
        return Some(value);
    }
    if is_process_env_ref(expr) {
        return Some("tsgodownProcessEnv()".to_string());
    }
    match expr {
        JsExpr::Call { callee, args, .. } if is_object_create_null_call(callee, args) => {
            Some("map[string]any{}".to_string())
        }
        JsExpr::Call { callee, args, .. } if is_object_assign_call(callee, args) => {
            render_object_assign_expr(args, state)
        }
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
        expr if is_module_exports_member(expr) => state.module_exports_ref.clone(),
        JsExpr::Ident { name } if is_any_binding(name, state) => {
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
    render_dynamic_object_member_access_expr(object, property, None, state)
}

fn render_array_property_member_expr(
    object: &JsExpr,
    property: &str,
    property_expr: Option<&JsExpr>,
    state: &AotState,
) -> Option<String> {
    let name = array_property_member_name(object, property, property_expr)?;
    if !state.array_property_bindings.contains(name) {
        return None;
    }
    let props = array_property_map_ref(name, state);
    let key = render_dynamic_object_property_key_expr(property, property_expr, state)?;
    Some(format!("tsgodownObjectProp({props}, {key})"))
}

fn render_dynamic_object_member_access_expr(
    object: &JsExpr,
    property: &str,
    property_expr: Option<&JsExpr>,
    state: &AotState,
) -> Option<String> {
    if render_map_expr(object, state).is_some() {
        return None;
    }
    let object = render_dynamic_object_source_expr(object, state)?;
    let key = render_dynamic_object_property_key_expr(property, property_expr, state)?;
    Some(format!("tsgodownObjectProp({object}, {key})"))
}

fn render_dynamic_object_property_key_expr(
    property: &str,
    property_expr: Option<&JsExpr>,
    state: &AotState,
) -> Option<String> {
    if property.is_empty() {
        if let Some(JsExpr::Ident { name }) = property_expr {
            if !state.bindings.contains(name) {
                return Some(go_string_literal(name));
            }
        }
    }
    property_expr
        .map(|expr| {
            render_string_expr(expr, state).or_else(|| {
                let value = render_expr(expr, state)?;
                Some(format!("tsgodownToString({value})"))
            })
        })
        .unwrap_or_else(|| Some(go_string_literal(property)))
}

fn is_numeric_property_key_expr(expr: &JsExpr) -> bool {
    match expr {
        JsExpr::Value {
            value: JsValue::Number { .. },
        } => true,
        JsExpr::Unary { op, arg } if matches!(op.as_str(), "+" | "-") => {
            is_numeric_property_key_expr(arg)
        }
        JsExpr::Binary { op, left, right }
            if is_numeric_binary_op(op) || is_bitwise_binary_op(op) =>
        {
            is_numeric_property_key_operand(left) && is_numeric_property_key_operand(right)
        }
        JsExpr::Update { op, arg, .. } if op == "++" => is_numeric_property_key_operand(arg),
        _ => false,
    }
}

fn is_numeric_property_key_operand(expr: &JsExpr) -> bool {
    matches!(expr, JsExpr::Ident { .. })
        || is_length_member_expr(expr)
        || is_numeric_property_key_expr(expr)
}

fn render_dynamic_object_init_expr(expr: &JsExpr, state: &AotState) -> Option<String> {
    render_error_object_expr(expr, state)
        .or_else(|| render_object_map_expr(expr, state))
        .or_else(|| {
            if let JsExpr::Call { callee, args, .. } = expr {
                let value = render_call_expr(callee, args, state)?;
                Some(format!("tsgodownObjectFromAny({value})"))
            } else {
                None
            }
        })
}

fn render_dynamic_object_order_init_expr(expr: &JsExpr, state: &AotState) -> Option<String> {
    match expr {
        JsExpr::Ident { name } if state.ordered_dynamic_object_bindings.contains(name) => {
            Some(dynamic_object_order_ref(name, state))
        }
        JsExpr::Call { callee, args, .. } if is_object_assign_call(callee, args) => {
            let groups = args
                .iter()
                .map(|arg| render_dynamic_object_order_init_expr(arg, state))
                .collect::<Option<Vec<_>>>()?;
            Some(format!("tsgodownObjectAssignKeys({})", groups.join(", ")))
        }
        JsExpr::Call { callee, args, .. } if is_object_create_null_call(callee, args) => {
            Some("[]string{}".to_string())
        }
        JsExpr::Object { props } => {
            if props.iter().any(|prop| prop.spread) {
                return Some("[]string{}".to_string());
            }
            let keys = props
                .iter()
                .map(|prop| match &prop.key_expr {
                    Some(key_expr) => render_string_expr(key_expr, state),
                    None => Some(go_string_literal(&prop.key)),
                })
                .collect::<Option<Vec<_>>>()?;
            Some(format!("[]string{{{}}}", keys.join(", ")))
        }
        _ => Some("[]string{}".to_string()),
    }
}

fn dynamic_object_order_go_name(name: &str) -> String {
    format!("{}__tsgodownKeys", sanitize_go_identifier(name))
}

fn dynamic_object_order_ref(name: &str, state: &AotState) -> String {
    dynamic_object_order_go_name(&go_binding_ref(name, state))
}

fn array_property_map_go_name(go_name: &str) -> String {
    format!("{go_name}__tsgodownProps")
}

fn array_property_order_go_name(go_name: &str) -> String {
    format!("{go_name}__tsgodownPropKeys")
}

fn array_property_map_ref(name: &str, state: &AotState) -> String {
    array_property_map_go_name(&go_binding_ref(name, state))
}

fn array_property_order_ref(name: &str, state: &AotState) -> String {
    array_property_order_go_name(&go_binding_ref(name, state))
}

fn append_array_property_decls(name: &str, state: &AotState, rendered: String) -> String {
    if !state.array_property_bindings.contains(name) {
        return rendered;
    }
    let props = array_property_map_ref(name, state);
    let order = array_property_order_ref(name, state);
    format!(
        "{rendered}\nvar {props} map[string]any = map[string]any{{}}\nvar {order} []string = []string{{}}\n_ = {props}\n_ = {order}"
    )
}

fn render_object_assign_expr(args: &[JsExpr], state: &AotState) -> Option<String> {
    if args.is_empty() {
        return None;
    }
    let target = render_json_value_expr(args.first()?, state)?;
    let mut statements = vec![format!("out := tsgodownObjectFromAny({target})")];
    for arg in args.iter().skip(1) {
        let source = render_json_value_expr(arg, state)?;
        statements.push(format!(
            "for key, value := range tsgodownObjectFromAny({source}) {{ out[key] = value }}"
        ));
    }
    statements.push("return out".to_string());
    Some(format!(
        "func() map[string]any {{ {} }}()",
        statements.join("; ")
    ))
}

fn render_object_assign_stmt(callee: &JsExpr, args: &[JsExpr], state: &AotState) -> Option<String> {
    if !is_object_assign_call(callee, args) {
        return None;
    }
    let target = render_dynamic_object_source_expr(args.first()?, state)?;
    let mut statements = Vec::new();
    for arg in args.iter().skip(1) {
        let source = render_json_value_expr(arg, state)?;
        statements.push(format!(
            "for key, value := range tsgodownObjectFromAny({source}) {{ {target}[key] = value }}"
        ));
    }
    Some(statements.join("\n"))
}

fn is_crypto_random_fill_sync_call_shape(callee: &JsExpr, args: &[JsExpr]) -> bool {
    args.len() == 1
        && (matches!(callee, JsExpr::Ident { name } if name == "randomFillSync")
            || matches!(
                callee,
                JsExpr::Member {
                    object,
                    property,
                    property_expr: None,
                    optional: false,
                } if property == "randomFillSync"
                    && matches!(object.as_ref(), JsExpr::Ident { name } if name == "crypto")
            ))
}

fn is_crypto_random_uuid_call_shape(callee: &JsExpr, args: &[JsExpr]) -> bool {
    args.is_empty()
        && (matches!(callee, JsExpr::Ident { name } if name == "randomUUID")
            || matches!(
                callee,
                JsExpr::Member {
                    property,
                    property_expr: None,
                    optional: false,
                    ..
                } if property == "randomUUID"
            ))
}

fn is_crypto_random_fill_sync_call(callee: &JsExpr, args: &[JsExpr], state: &AotState) -> bool {
    if !is_crypto_random_fill_sync_call_shape(callee, args) {
        return false;
    }
    match callee {
        JsExpr::Ident { name } => state.builtin_bindings.contains(name),
        JsExpr::Member { object, .. } => {
            matches!(object.as_ref(), JsExpr::Ident { name } if state.builtin_bindings.contains(name))
        }
        _ => false,
    }
}

fn is_crypto_random_uuid_call(callee: &JsExpr, args: &[JsExpr], state: &AotState) -> bool {
    if !is_crypto_random_uuid_call_shape(callee, args) {
        return false;
    }
    match callee {
        JsExpr::Ident { name } => state.builtin_bindings.contains(name),
        JsExpr::Member {
            object,
            property,
            property_expr: None,
            optional: false,
        } if property == "randomUUID" => {
            matches!(object.as_ref(), JsExpr::Ident { name } if name == "crypto" && state.builtin_bindings.contains(name))
        }
        _ => false,
    }
}

fn render_crypto_random_fill_sync_stmt(
    callee: &JsExpr,
    args: &[JsExpr],
    state: &AotState,
) -> Option<String> {
    if !is_crypto_random_fill_sync_call(callee, args, state) {
        return None;
    }
    let target = render_bytes_expr(args.first()?, state)?;
    Some(format!("_, _ = rand.Read({target})"))
}

fn render_dynamic_object_source_expr(object: &JsExpr, state: &AotState) -> Option<String> {
    match object {
        JsExpr::Ident { name } if state.dynamic_object_bindings.contains(name) => {
            Some(go_binding_ref(name, state))
        }
        expr if is_module_exports_member(expr) => state.module_exports_ref.clone(),
        expr if render_error_object_expr(expr, state).is_some() => {
            render_error_object_expr(expr, state)
        }
        JsExpr::Ident { name } if state.bindings.contains(name) => {
            if state.number_array_bindings.contains(name)
                || state.string_array_bindings.contains(name)
                || state.any_array_bindings.contains(name)
                || state.bytes_bindings.contains(name)
                || state.map_bindings.contains(name)
                || state.set_bindings.contains(name)
            {
                return None;
            }
            let value = go_binding_ref(name, state);
            Some(format!("tsgodownObjectFromAny({value})"))
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
    if let Some(value) = render_string_array_index_expr(expr, state) {
        return Some((AotSlotKind::String, value, "string"));
    }
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
    if let Some((pattern, _global)) = render_supported_regexp_replace_pattern(expr) {
        return Some((AotSlotKind::RegExp, go_string_literal(&pattern), "string"));
    }
    if let Some(value) = render_regexp_expr(expr, state) {
        return Some((AotSlotKind::RegExp, value, "string"));
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
    if let Some(value) = render_string_function_expr(expr, state) {
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
    if let Some((_, rendered)) = state
        .function_static_members
        .get(&(name.clone(), property.to_string()))
    {
        return Some(rendered.clone());
    }
    if let Some((_, rendered)) = state
        .dynamic_import_member_slots
        .get(&(name.clone(), property.to_string()))
    {
        return Some(rendered.clone());
    }
    Some(format!(
        "{}.{}",
        go_binding_ref(name, state),
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

fn render_class_field_member_expr(
    object: &JsExpr,
    property: &str,
    state: &AotState,
) -> Option<String> {
    match object {
        JsExpr::Ident { name } => {
            let class_name = state.class_instance_bindings.get(name)?;
            let class = state.classes.get(class_name)?;
            class.fields.get(property)?;
            Some(format!(
                "{}.{}",
                go_binding_ref(name, state),
                sanitize_go_identifier(property)
            ))
        }
        JsExpr::New { .. } => {
            let (class_name, value) = render_new_class_expr(object, state)?;
            let class = state.classes.get(&class_name)?;
            class.fields.get(property)?;
            Some(format!("{}.{}", value, sanitize_go_identifier(property)))
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
    if let Some((kind, _)) = state
        .function_static_members
        .get(&(name.clone(), property.to_string()))
    {
        return Some(*kind);
    }
    if let Some((kind, _)) = state
        .dynamic_import_member_slots
        .get(&(name.clone(), property.to_string()))
    {
        return Some(*kind);
    }
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
        JsExpr::New { callee, args } if is_new_uint8_array_expr(callee, args) => {
            let size = render_numeric_expr(args.first()?, state)?;
            Some(format!("make([]byte, int({size}))"))
        }
        JsExpr::Call { callee, args, .. } if is_uint8_array_of_call(callee, args) => {
            let bytes = args
                .iter()
                .map(|arg| render_numeric_expr(arg, state).map(|value| format!("byte({value})")))
                .collect::<Option<Vec<_>>>()?;
            Some(format!("[]byte{{{}}}", bytes.join(", ")))
        }
        JsExpr::Call { callee, args, .. } if is_bytes_slice_call(callee, args, state) => {
            render_bytes_slice_call(callee, args, state)
        }
        JsExpr::Call { callee, args, .. } if is_node_buffer_from_call(callee, args) => {
            render_node_buffer_from_call(callee, args, state)
        }
        JsExpr::Call { callee, args, .. } if is_node_buffer_alloc_call(callee, args) => {
            render_node_buffer_alloc_call(callee, args, state)
        }
        JsExpr::Call { callee, args, .. } if is_crypto_hash_bytes_digest_call(callee, args) => {
            render_crypto_hash_bytes_digest_call(callee, args, state)
        }
        JsExpr::Binary { op, left, right } if op == "??" => {
            let left = render_expr(left, state)?;
            let right = render_bytes_expr_with_any_cast(right, state)?;
            Some(format!(
                "func() []byte {{ value := {left}; if value != nil {{ return value.([]byte) }}; return {right} }}()"
            ))
        }
        JsExpr::Call { callee, args, .. } if matches!(callee.as_ref(), JsExpr::Ident { name } if is_any_binding(name, state)) =>
        {
            let call = render_call_expr(callee, args, state)?;
            Some(format!("({call}).([]byte)"))
        }
        JsExpr::Call { callee, args, .. } if is_local_function_call(callee, state) => {
            let JsExpr::Ident { name } = callee.as_ref() else {
                return None;
            };
            let function = state.functions.get(name)?;
            if !function_returns_bytes(function, state) {
                return None;
            }
            let call = render_call_expr(callee, args, state)?;
            Some(format!("({call}).([]byte)"))
        }
        JsExpr::Conditional {
            test,
            consequent,
            alternate,
        } => {
            let test_expr = render_bool_expr(test, state)?;
            let consequent_state = narrowed_typeof_state(test, state);
            let consequent = render_bytes_expr_with_any_cast(consequent, &consequent_state)?;
            let alternate = render_bytes_expr_with_any_cast(alternate, state)?;
            Some(format!(
                "func() []byte {{ if {test_expr} {{ return {consequent} }}; return {alternate} }}()"
            ))
        }
        _ => None,
    }
}

fn render_bytes_expr_with_any_cast(expr: &JsExpr, state: &AotState) -> Option<String> {
    if let Some(value) = render_bytes_expr(expr, state) {
        return Some(value);
    }
    let JsExpr::Ident { name } = expr else {
        if matches!(expr, JsExpr::Call { .. }) {
            let value = render_expr(expr, state)?;
            return Some(format!("tsgodownBytesFromAny({value})"));
        }
        return None;
    };
    if !is_any_binding(name, state) {
        return None;
    }
    Some(format!(
        "tsgodownBytesFromAny({})",
        go_binding_ref(name, state)
    ))
}

fn render_bytes_return_expr(expr: &JsExpr, state: &AotState) -> Option<String> {
    if let Some(value) = render_bytes_expr(expr, state) {
        return Some(value);
    }
    let JsExpr::Ident { name } = expr else {
        return None;
    };
    if !is_any_binding(name, state) {
        return None;
    }
    Some(format!(
        "tsgodownBytesFromAny({})",
        go_binding_ref(name, state)
    ))
}

fn is_new_uint8_array_expr(callee: &JsExpr, args: &[JsExpr]) -> bool {
    args.len() == 1 && matches!(callee, JsExpr::Ident { name } if name == "Uint8Array")
}

fn is_uint8_array_of_call(callee: &JsExpr, args: &[JsExpr]) -> bool {
    !args.is_empty()
        && matches!(
            callee,
            JsExpr::Member {
                object,
                property,
                property_expr: None,
                optional: false,
            } if matches!(object.as_ref(), JsExpr::Ident { name } if name == "Uint8Array")
                && property == "of"
        )
}

fn is_bytes_slice_call(callee: &JsExpr, args: &[JsExpr], state: &AotState) -> bool {
    is_member_slice_call_shape(callee, args)
        && matches!(
            callee,
            JsExpr::Member { object, .. } if render_bytes_expr_with_any_cast(object, state).is_some()
        )
}

fn is_member_slice_call_shape(callee: &JsExpr, args: &[JsExpr]) -> bool {
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

fn render_bytes_slice_call(callee: &JsExpr, args: &[JsExpr], state: &AotState) -> Option<String> {
    if !is_bytes_slice_call(callee, args, state) {
        return None;
    }
    let JsExpr::Member { object, .. } = callee else {
        return None;
    };
    let object = render_bytes_expr_with_any_cast(object, state)?;
    let start = render_numeric_expr(args.first()?, state)?;
    let end = args
        .get(1)
        .map(|arg| render_numeric_expr(arg, state))
        .unwrap_or_else(|| Some(format!("float64(len({object}))")))?;
    Some(format!(
        "append([]byte(nil), {object}[int({start}):int({end})]...)"
    ))
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
        JsExpr::ArraySpread { items } => render_number_array_spread_expr(items, state),
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

fn render_number_array_spread_expr(
    items: &[crate::contract::JsArrayElement],
    state: &AotState,
) -> Option<String> {
    if items.is_empty() {
        return None;
    }
    let mut statements = vec!["out := []float64{}".to_string()];
    for item in items {
        if item.spread {
            let value = render_number_array_expr(&item.value, state)?;
            statements.push(format!("out = append(out, {value}...)"));
        } else {
            let value = render_numeric_expr(&item.value, state)?;
            statements.push(format!("out = append(out, {value})"));
        }
    }
    statements.push("return out".to_string());
    Some(format!(
        "func() []float64 {{ {} }}()",
        statements.join("; ")
    ))
}

fn render_any_array_expr(expr: &JsExpr, state: &AotState) -> Option<String> {
    match expr {
        JsExpr::Ident { name } if state.any_array_bindings.contains(name) => {
            let value = go_binding_ref(name, state);
            if state.narrowed_any_array_bindings.contains(name) {
                Some(format!("tsgodownAnyArrayFromAny({value})"))
            } else {
                Some(value)
            }
        }
        JsExpr::Ident { name } if state.narrowed_any_array_bindings.contains(name) => {
            let value = go_binding_ref(name, state);
            Some(format!("tsgodownAnyArrayFromAny({value})"))
        }
        JsExpr::Array { items } => {
            let items = items
                .iter()
                .map(|item| render_json_value_expr(item, state))
                .collect::<Option<Vec<_>>>()?;
            Some(format!("[]any{{{}}}", items.join(", ")))
        }
        JsExpr::ArraySpread { items } if items.len() == 1 => {
            render_iterable_array_expr(&items.first()?.value, state)
        }
        JsExpr::Call { callee, args, .. } if is_array_concat_call_shape(callee) => {
            render_any_array_concat_call(callee, args, state)
        }
        JsExpr::Call { callee, args, .. } if is_array_slice_call_shape(callee) => {
            render_any_array_slice_call(callee, args, state)
        }
        JsExpr::Call { callee, args, .. }
            if render_any_array_flat_call(callee, args, state).is_some() =>
        {
            render_any_array_flat_call(callee, args, state)
        }
        JsExpr::Call { callee, args, .. }
            if render_any_array_prototype_concat_alias_call(callee, args, state).is_some() =>
        {
            render_any_array_prototype_concat_alias_call(callee, args, state)
        }
        JsExpr::Call { callee, args, .. }
            if render_any_array_prototype_slice_alias_call(callee, args, state).is_some() =>
        {
            render_any_array_prototype_slice_alias_call(callee, args, state)
        }
        JsExpr::Call { callee, args, .. } if is_array_map_call(callee, args) => {
            render_any_array_map_call(callee, args, state)
        }
        JsExpr::Call { callee, args, .. } if is_array_filter_call(callee, args) => {
            render_any_array_filter_call(callee, args, state)
        }
        JsExpr::Call { callee, args, .. } if is_object_entries_call(callee, args) => {
            render_object_entries_call(args, state)
        }
        JsExpr::Call { callee, args, .. } if is_array_from_length_map_call(callee, args) => {
            render_array_from_length_map_call(args, state)
        }
        JsExpr::Call { callee, args, .. } if is_array_from_length_call(callee, args) => {
            render_array_from_length_call(args, state)
        }
        JsExpr::Call { callee, args, .. }
            if render_any_array_fill_call(callee, args, state).is_some() =>
        {
            render_any_array_fill_call(callee, args, state)
        }
        JsExpr::New { callee, args } if is_array_constructor_length_new(callee, args) => {
            let length = render_numeric_expr(args.first()?, state)?;
            Some(format!("tsgodownAnyArrayWithLength({length})"))
        }
        JsExpr::Call { callee, args, .. } => {
            let JsExpr::Ident { name } = callee.as_ref() else {
                return None;
            };
            let function = state.functions.get(name)?;
            if !function_returns_any_array(function, state) {
                return None;
            }
            render_call_expr(callee, args, state)
        }
        JsExpr::Conditional {
            test,
            consequent,
            alternate,
        } if is_array_is_array_guard_for_expr(test, consequent, state) => {
            let consequent = render_any_array_from_any_expr(consequent, state)?;
            let alternate = render_any_array_expr(alternate, state)?;
            let test = render_bool_expr(test, state)?;
            Some(format!(
                "func() []any {{ if {test} {{ return {consequent} }}; return {alternate} }}()"
            ))
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
            render_any_array_expr,
            "[]any",
        ),
        _ => None,
    }
}

fn is_array_is_array_guard_for_expr(test: &JsExpr, guarded: &JsExpr, state: &AotState) -> bool {
    let JsExpr::Call { callee, args, .. } = test else {
        return false;
    };
    if args.len() != 1 || args.first() != Some(guarded) {
        return false;
    }
    is_array_is_array_call(callee, args) || is_array_is_array_alias_call(callee, args, state)
}

fn render_any_array_from_any_expr(expr: &JsExpr, state: &AotState) -> Option<String> {
    if let Some(value) = render_any_array_expr(expr, state) {
        return Some(value);
    }
    if let Some(value) = render_number_array_expr(expr, state) {
        return Some(format!("tsgodownAnyArrayFromAny({value})"));
    }
    if let Some(value) = render_string_array_expr(expr, state) {
        return Some(format!("tsgodownAnyArrayFromAny({value})"));
    }
    let value = render_json_value_expr(expr, state).or_else(|| render_expr(expr, state))?;
    Some(format!("tsgodownAnyArrayFromAny({value})"))
}

fn render_any_array_coerced_expr(expr: &JsExpr, state: &AotState) -> Option<String> {
    let value = render_json_value_expr(expr, state)
        .or_else(|| render_expr(expr, state))
        .or_else(|| render_bytes_expr(expr, state))
        .or_else(|| render_any_array_expr(expr, state))?;
    Some(format!("tsgodownAnyArrayFromAny({value})"))
}

fn is_array_from_length_call(callee: &JsExpr, args: &[JsExpr]) -> bool {
    args.len() == 1
        && matches!(
            callee,
            JsExpr::Member {
                object,
                property,
                property_expr: None,
                optional: false,
            } if property == "from"
                && matches!(object.as_ref(), JsExpr::Ident { name } if name == "Array")
        )
        && args.first().and_then(object_literal_length_expr).is_some()
}

fn is_array_from_length_map_call(callee: &JsExpr, args: &[JsExpr]) -> bool {
    args.len() == 2
        && matches!(
            callee,
            JsExpr::Member {
                object,
                property,
                property_expr: None,
                optional: false,
            } if property == "from"
                && matches!(object.as_ref(), JsExpr::Ident { name } if name == "Array")
        )
        && args.first().and_then(object_literal_length_expr).is_some()
}

fn render_array_from_length_call(args: &[JsExpr], state: &AotState) -> Option<String> {
    let length = render_numeric_expr(object_literal_length_expr(args.first()?)?, state)?;
    Some(format!("tsgodownAnyArrayWithLength({length})"))
}

fn render_array_from_length_map_call(args: &[JsExpr], state: &AotState) -> Option<String> {
    let length = render_numeric_expr(object_literal_length_expr(args.first()?)?, state)?;
    let mapper = render_array_from_mapper_expr(args.get(1)?, state)?;
    Some(format!("tsgodownAnyArrayFromLengthMap({length}, {mapper})"))
}

fn render_array_from_mapper_expr(expr: &JsExpr, state: &AotState) -> Option<String> {
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
    if params.len() > 2 {
        return None;
    }
    let (last, prelude) = body.split_last()?;
    let JsStmt::Return { value: Some(value) } = last else {
        return None;
    };
    let mut mapper_state = clone_aot_state(state);
    if let Some(param) = params.first() {
        mapper_state.bind_slot(param, sanitize_go_identifier(param), AotSlotKind::Any);
    }
    if let Some(param) = params.get(1) {
        mapper_state.bind_slot(param, sanitize_go_identifier(param), AotSlotKind::Number);
    }
    mark_number_array_locals(body, &mut mapper_state);
    mark_string_array_locals(body, &mut mapper_state);
    mark_any_array_locals(body, &mut mapper_state);
    mark_array_property_locals(body, &mut mapper_state);
    mark_dynamic_object_locals(body, &mut mapper_state);
    let prelude = prelude
        .iter()
        .map(|stmt| render_stmt(stmt, &mut mapper_state))
        .collect::<Option<Vec<_>>>()?
        .join("\n");
    let value = render_json_value_expr(value, &mapper_state)
        .or_else(|| render_expr(value, &mapper_state))?;
    let value_param = params
        .first()
        .map(|param| sanitize_go_identifier(param))
        .unwrap_or_else(|| "_".to_string());
    let index_param = params
        .get(1)
        .map(|param| sanitize_go_identifier(param))
        .unwrap_or_else(|| "_".to_string());
    let body = if prelude.is_empty() {
        format!("return {value}")
    } else {
        format!("{prelude}\nreturn {value}")
    };
    Some(format!(
        "func({value_param} any, {index_param} float64) any {{\n{}\n}}",
        indent_lines(&body)
    ))
}

fn render_array_predicate_expr(expr: &JsExpr, state: &AotState) -> Option<String> {
    if matches!(expr, JsExpr::Ident { name } if name == "Boolean") {
        return Some(
            "func(value any, _ float64) bool { return tsgodownToBool(value) }".to_string(),
        );
    }
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
    if params.len() > 2 {
        return None;
    }
    let [JsStmt::Return { value: Some(value) }] = body.as_slice() else {
        return None;
    };
    let mut predicate_state = clone_aot_state(state);
    if let Some(param) = params.first() {
        predicate_state.bind_slot(param, sanitize_go_identifier(param), AotSlotKind::Any);
    }
    if let Some(param) = params.get(1) {
        predicate_state.bind_slot(param, sanitize_go_identifier(param), AotSlotKind::Number);
    }
    let value = render_bool_test_expr(value, &predicate_state)?;
    let value_param = params
        .first()
        .map(|param| sanitize_go_identifier(param))
        .unwrap_or_else(|| "_".to_string());
    let index_param = params
        .get(1)
        .map(|param| sanitize_go_identifier(param))
        .unwrap_or_else(|| "_".to_string());
    Some(format!(
        "func({value_param} any, {index_param} float64) bool {{ return {value} }}"
    ))
}

fn is_array_map_call(callee: &JsExpr, args: &[JsExpr]) -> bool {
    args.len() == 1
        && matches!(
            callee,
            JsExpr::Member {
                property,
                property_expr: None,
                optional: false,
                ..
            } if property == "map"
        )
}

fn render_any_array_map_call(callee: &JsExpr, args: &[JsExpr], state: &AotState) -> Option<String> {
    if !is_array_map_call(callee, args) {
        return None;
    }
    let JsExpr::Member { object, .. } = callee else {
        return None;
    };
    let mapper = render_array_from_mapper_expr(args.first()?, state)?;
    if let Some(values) = render_any_array_expr(object, state) {
        return Some(format!("tsgodownAnyArrayMap({values}, {mapper})"));
    }
    let values = render_string_array_expr(object, state)?;
    Some(format!("tsgodownStringArrayMap({values}, {mapper})"))
}

fn is_array_filter_call(callee: &JsExpr, args: &[JsExpr]) -> bool {
    args.len() == 1
        && matches!(
            callee,
            JsExpr::Member {
                property,
                property_expr: None,
                optional: false,
                ..
            } if property == "filter"
        )
}

fn render_any_array_filter_call(
    callee: &JsExpr,
    args: &[JsExpr],
    state: &AotState,
) -> Option<String> {
    if !is_array_filter_call(callee, args) {
        return None;
    }
    let JsExpr::Member { object, .. } = callee else {
        return None;
    };
    let values = render_any_array_expr(object, state)?;
    let predicate = render_array_predicate_expr(args.first()?, state)?;
    Some(format!("tsgodownAnyArrayFilter({values}, {predicate})"))
}

fn render_string_array_filter_call(
    callee: &JsExpr,
    args: &[JsExpr],
    state: &AotState,
) -> Option<String> {
    if !is_array_filter_call(callee, args) {
        return None;
    }
    let JsExpr::Member { object, .. } = callee else {
        return None;
    };
    let values = render_string_array_expr(object, state)?;
    let predicate = render_array_predicate_expr(args.first()?, state)?;
    Some(format!("tsgodownStringArrayFilter({values}, {predicate})"))
}

fn is_array_find_call(callee: &JsExpr, args: &[JsExpr]) -> bool {
    args.len() == 1
        && matches!(
            callee,
            JsExpr::Member {
                property,
                property_expr: None,
                optional: false,
                ..
            } if matches!(property.as_str(), "find" | "findLast" | "findIndex" | "findLastIndex")
        )
}

fn render_array_find_call(callee: &JsExpr, args: &[JsExpr], state: &AotState) -> Option<String> {
    if !is_array_find_call(callee, args) {
        return None;
    }
    let JsExpr::Member {
        object, property, ..
    } = callee
    else {
        return None;
    };
    let predicate = render_array_predicate_expr(args.first()?, state)?;
    let reverse = matches!(property.as_str(), "findLast" | "findLastIndex").to_string();
    let find_index = matches!(property.as_str(), "findIndex" | "findLastIndex");
    if let Some(values) = render_any_array_expr(object, state) {
        if find_index {
            return Some(format!(
                "tsgodownAnyArrayFindIndex({values}, {predicate}, {reverse})"
            ));
        }
        return Some(format!(
            "tsgodownAnyArrayFind({values}, {predicate}, {reverse})"
        ));
    }
    if let Some(values) = render_number_array_expr(object, state) {
        let values = format!("tsgodownAnyArrayFromAny({values})");
        if find_index {
            return Some(format!(
                "tsgodownAnyArrayFindIndex({values}, {predicate}, {reverse})"
            ));
        }
        return Some(format!(
            "tsgodownAnyArrayFind({values}, {predicate}, {reverse})"
        ));
    }
    let values = render_string_array_expr(object, state)?;
    if find_index {
        return Some(format!(
            "tsgodownStringArrayFindIndex({values}, {predicate}, {reverse})"
        ));
    }
    Some(format!(
        "tsgodownStringArrayFind({values}, {predicate}, {reverse})"
    ))
}

fn is_array_reduce_call(callee: &JsExpr, args: &[JsExpr]) -> bool {
    args.len() == 2
        && matches!(
            callee,
            JsExpr::Member {
                property,
                property_expr: None,
                optional: false,
                ..
            } if matches!(property.as_str(), "reduce" | "reduceRight")
        )
}

fn render_array_reduce_call(callee: &JsExpr, args: &[JsExpr], state: &AotState) -> Option<String> {
    if !is_array_reduce_call(callee, args) {
        return None;
    }
    let JsExpr::Member {
        object, property, ..
    } = callee
    else {
        return None;
    };
    let reducer = render_array_reducer_expr(args.first()?, state)?;
    let initial =
        render_json_value_expr(args.get(1)?, state).or_else(|| render_expr(args.get(1)?, state))?;
    let helper = if property == "reduceRight" {
        "ReduceRight"
    } else {
        "Reduce"
    };
    if let Some(values) = render_any_array_expr(object, state) {
        return Some(format!(
            "tsgodownAnyArray{helper}({values}, {reducer}, {initial})"
        ));
    }
    if let Some(values) = render_number_array_expr(object, state) {
        return Some(format!(
            "tsgodownAnyArray{helper}(tsgodownAnyArrayFromAny({values}), {reducer}, {initial})"
        ));
    }
    let values = render_string_array_expr(object, state)?;
    Some(format!(
        "tsgodownStringArray{helper}({values}, {reducer}, {initial})"
    ))
}

fn render_array_reducer_expr(expr: &JsExpr, state: &AotState) -> Option<String> {
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
    if !(2..=3).contains(&params.len()) {
        return None;
    }
    let mut reducer_state = clone_aot_state(state);
    reducer_state.bind_slot(
        params.first()?,
        sanitize_go_identifier(params.first()?),
        AotSlotKind::Any,
    );
    reducer_state.bind_slot(
        params.get(1)?,
        sanitize_go_identifier(params.get(1)?),
        AotSlotKind::Any,
    );
    if let Some(param) = params.get(2) {
        reducer_state.bind_slot(param, sanitize_go_identifier(param), AotSlotKind::Number);
    }
    let body = render_function_body(body, &reducer_state)?;
    let body = if body.trim_end().ends_with("return nil") || body.contains("return ") {
        body
    } else {
        format!("{body}\nreturn nil")
    };
    let accumulator_param = sanitize_go_identifier(params.first()?);
    let value_param = sanitize_go_identifier(params.get(1)?);
    let index_param = params
        .get(2)
        .map(|param| sanitize_go_identifier(param))
        .unwrap_or_else(|| "_".to_string());
    Some(format!(
        "func({accumulator_param} any, {value_param} any, {index_param} float64) any {{\n{}\n}}",
        indent_lines(&body)
    ))
}

fn is_array_for_each_call(callee: &JsExpr, args: &[JsExpr]) -> bool {
    args.len() == 1
        && matches!(
            callee,
            JsExpr::Member {
                property,
                property_expr: None,
                optional: false,
                ..
            } if property == "forEach"
        )
}

fn render_array_for_each_stmt(
    callee: &JsExpr,
    args: &[JsExpr],
    state: &mut AotState,
) -> Option<String> {
    if !is_array_for_each_call(callee, args) {
        return None;
    }
    let JsExpr::Member { object, .. } = callee else {
        return None;
    };
    let callback = args.first()?;
    let JsExpr::Function {
        params,
        rest_param: None,
        r#async: false,
        generator: false,
        body,
        ..
    } = callback
    else {
        return None;
    };
    if params.len() > 2 {
        return None;
    }
    let values = render_any_array_expr(object, state)
        .or_else(|| {
            render_number_array_expr(object, state)
                .map(|value| format!("tsgodownAnyArrayFromAny({value})"))
        })
        .or_else(|| render_string_array_expr(object, state))?;
    let entry_value = matches!(
        object.as_ref(),
        JsExpr::Call { callee, args, .. } if is_object_entries_call(callee, args)
    );
    let value_kind = if entry_value {
        AotSlotKind::AnyArray
    } else {
        AotSlotKind::Any
    };
    let mut callback_state = clone_aot_state(state);
    let value_param = params
        .first()
        .map(|param| sanitize_go_identifier(param))
        .unwrap_or_else(|| "_".to_string());
    let index_param = params
        .get(1)
        .map(|param| sanitize_go_identifier(param))
        .unwrap_or_else(|| "_".to_string());
    if let Some(param) = params.first() {
        callback_state.bind_slot(param, sanitize_go_identifier(param), value_kind);
    }
    if let Some(param) = params.get(1) {
        callback_state.bind_slot(param, sanitize_go_identifier(param), AotSlotKind::Number);
    }
    let mut body = render_for_each_callback_body(body, &mut callback_state)?;
    let header = if params.is_empty() {
        format!("for range {values}")
    } else if params.len() == 1 {
        if entry_value {
            body = format!("{value_param} := tsgodownAnyArrayFromAny(__tsgodownEntry)\n{body}");
            format!("for _, __tsgodownEntry := range {values}")
        } else {
            format!("for _, {value_param} := range {values}")
        }
    } else {
        body = format!("{index_param} := float64(__tsgodownIndex)\n{body}");
        if entry_value {
            body = format!("{value_param} := tsgodownAnyArrayFromAny(__tsgodownEntry)\n{body}");
            format!("for __tsgodownIndex, __tsgodownEntry := range {values}")
        } else {
            format!("for __tsgodownIndex, {value_param} := range {values}")
        }
    };
    Some(format!("{header} {{\n{}\n}}", indent_lines(&body)))
}

fn render_ts_enum_iife_stmt(
    callee: &JsExpr,
    args: &[JsExpr],
    state: &mut AotState,
) -> Option<String> {
    let target = ts_enum_iife_target(callee, args)?.to_string();
    let JsExpr::Function {
        params,
        body,
        r#async: false,
        generator: false,
        rest_param: None,
        ..
    } = callee
    else {
        return None;
    };
    let param = params.first()?;
    let was_dynamic = state.dynamic_object_bindings.contains(&target);
    let had_order = state.ordered_dynamic_object_bindings.contains(&target);
    let target_ident = go_binding_ref(&target, state);
    state.bindings.insert(target.clone());
    state
        .binding_refs
        .insert(target.clone(), target_ident.clone());
    state.dynamic_object_bindings.insert(target.clone());
    state.ordered_dynamic_object_bindings.insert(target.clone());
    let order = dynamic_object_order_go_name(&target_ident);
    let target_object = if was_dynamic {
        target_ident.clone()
    } else {
        format!("tsgodownObjectFromAny({target_ident})")
    };
    let mut rendered = Vec::new();
    if !was_dynamic {
        rendered.push(format!(
            "if _, ok := {target_ident}.(map[string]any); !ok {{ {target_ident} = map[string]any{{}} }}"
        ));
    }
    if !had_order {
        rendered.push(format!("var {order} []string = []string{{}}"));
    }
    for stmt in body {
        rendered.push(render_ts_enum_member_assignment_stmt(
            stmt,
            param,
            &target_object,
            &order,
            state,
        )?);
    }
    Some(rendered.join("\n"))
}

fn render_ts_enum_member_assignment_stmt(
    stmt: &JsStmt,
    param: &str,
    target: &str,
    order: &str,
    state: &AotState,
) -> Option<String> {
    let JsStmt::Expr {
        expr: JsExpr::Assign { op, left, right },
    } = stmt
    else {
        return None;
    };
    if op != "=" {
        return None;
    }
    let JsExpr::Member {
        object,
        property,
        property_expr,
        optional: false,
    } = left.as_ref()
    else {
        return None;
    };
    if !matches!(object.as_ref(), JsExpr::Ident { name } if name == param) {
        return None;
    }
    let key = render_dynamic_object_property_key_expr(property, property_expr.as_deref(), state)?;
    let value = render_json_value_expr(right, state)?;
    Some(format!(
        "if _, ok := {target}[{key}]; !ok {{ {order} = append({order}, {key}) }}\ntsgodownObjectSetProp({target}, {key}, {value})"
    ))
}

fn ts_enum_iife_target<'a>(callee: &'a JsExpr, args: &'a [JsExpr]) -> Option<&'a str> {
    if args.len() != 1 {
        return None;
    }
    let JsExpr::Function {
        params,
        body,
        r#async: false,
        generator: false,
        rest_param: None,
        ..
    } = callee
    else {
        return None;
    };
    if params.len() != 1 || body.is_empty() {
        return None;
    }
    let target = ts_enum_iife_argument_target(args.first()?)?;
    let param = params.first()?;
    body.iter()
        .all(|stmt| is_ts_enum_member_assignment_stmt(stmt, param))
        .then_some(target)
}

fn ts_enum_iife_argument_target(expr: &JsExpr) -> Option<&str> {
    match expr {
        JsExpr::Ident { name } => Some(name.as_str()),
        JsExpr::Binary { op, left, right } if op == "||" => {
            let JsExpr::Ident { name } = left.as_ref() else {
                return None;
            };
            if !is_empty_object_assignment_to(right, name) {
                return None;
            }
            Some(name.as_str())
        }
        _ => None,
    }
}

fn is_empty_object_assignment_to(expr: &JsExpr, target: &str) -> bool {
    let JsExpr::Assign { op, left, right } = expr else {
        return false;
    };
    op == "="
        && matches!(left.as_ref(), JsExpr::Ident { name } if name == target)
        && matches!(right.as_ref(), JsExpr::Object { props } if props.is_empty())
}

fn is_ts_enum_member_assignment_stmt(stmt: &JsStmt, param: &str) -> bool {
    let JsStmt::Expr {
        expr: JsExpr::Assign { op, left, right },
    } = stmt
    else {
        return false;
    };
    if op != "=" || render_json_value_expr(right, &AotState::default()).is_none() {
        return false;
    }
    matches!(
        left.as_ref(),
        JsExpr::Member {
            object,
            optional: false,
            ..
        } if matches!(object.as_ref(), JsExpr::Ident { name } if name == param)
    )
}

fn render_for_each_callback_body(body: &[JsStmt], state: &mut AotState) -> Option<String> {
    body.iter()
        .map(|stmt| match stmt {
            JsStmt::Return { value: Some(expr) } => {
                let expr = render_expr_stmt(expr, state)?;
                if expr.trim().is_empty() {
                    Some("continue".to_string())
                } else {
                    Some(format!("{expr}\ncontinue"))
                }
            }
            JsStmt::Return { value: None } => Some("continue".to_string()),
            _ => render_stmt(stmt, state),
        })
        .collect::<Option<Vec<_>>>()
        .map(|stmts| stmts.join("\n"))
}

fn is_array_predicate_call(callee: &JsExpr, args: &[JsExpr]) -> bool {
    args.len() == 1
        && matches!(
            callee,
            JsExpr::Member {
                property,
                property_expr: None,
                optional: false,
                ..
            } if matches!(property.as_str(), "some" | "every")
        )
}

fn render_array_predicate_call(
    callee: &JsExpr,
    args: &[JsExpr],
    state: &AotState,
) -> Option<String> {
    if !is_array_predicate_call(callee, args) {
        return None;
    }
    let JsExpr::Member {
        object, property, ..
    } = callee
    else {
        return None;
    };
    let predicate = render_array_predicate_expr(args.first()?, state)?;
    let helper_suffix = if property == "some" { "Some" } else { "Every" };
    if let Some(values) = render_any_array_expr(object, state) {
        return Some(format!(
            "tsgodownAnyArray{helper_suffix}({values}, {predicate})"
        ));
    }
    if let Some(values) = render_number_array_expr(object, state) {
        return Some(format!(
            "tsgodownAnyArray{helper_suffix}(tsgodownAnyArrayFromAny({values}), {predicate})"
        ));
    }
    if let Some(values) = render_bytes_expr(object, state) {
        return Some(format!(
            "tsgodownBytes{helper_suffix}({values}, {predicate})"
        ));
    }
    let values = render_string_array_expr(object, state)?;
    Some(format!(
        "tsgodownStringArray{helper_suffix}({values}, {predicate})"
    ))
}

fn object_literal_length_expr(expr: &JsExpr) -> Option<&JsExpr> {
    let JsExpr::Object { props } = expr else {
        return None;
    };
    props
        .iter()
        .find(|prop| !prop.spread && prop.key == "length" && prop.key_expr.is_none())
        .map(|prop| &prop.value)
}

fn is_array_constructor_length_new(callee: &JsExpr, args: &[JsExpr]) -> bool {
    args.len() == 1 && matches!(callee, JsExpr::Ident { name } if name == "Array")
}

fn render_any_array_fill_call(
    callee: &JsExpr,
    args: &[JsExpr],
    state: &AotState,
) -> Option<String> {
    if !(1..=3).contains(&args.len()) {
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
    if property != "fill" {
        return None;
    }
    let values = render_any_array_expr(object, state)?;
    let fill = render_any_array_value_expr(args.first()?, state)?;
    let indexes = args
        .iter()
        .skip(1)
        .map(|arg| render_numeric_expr(arg, state))
        .collect::<Option<Vec<_>>>()?;
    if indexes.is_empty() {
        return Some(format!("tsgodownAnyArrayFill({values}, {fill})"));
    }
    Some(format!(
        "tsgodownAnyArrayFill({values}, {fill}, {})",
        indexes.join(", ")
    ))
}

fn is_array_concat_call_shape(callee: &JsExpr) -> bool {
    matches!(
        callee,
        JsExpr::Member {
            property,
            property_expr: None,
            optional: false,
            ..
        } if property == "concat"
    )
}

fn is_array_fill_call_shape(callee: &JsExpr) -> bool {
    matches!(
        callee,
        JsExpr::Member {
            property,
            property_expr: None,
            optional: false,
            ..
        } if property == "fill"
    )
}

fn render_any_array_concat_call(
    callee: &JsExpr,
    args: &[JsExpr],
    state: &AotState,
) -> Option<String> {
    if !is_array_concat_call_shape(callee) {
        return None;
    }
    let JsExpr::Member { object, .. } = callee else {
        return None;
    };
    let base = render_any_array_from_any_expr(object, state)?;
    let values = args
        .iter()
        .map(|arg| {
            render_json_value_expr(arg, state)
                .or_else(|| {
                    render_number_array_expr(arg, state).map(|value| format!("any({value})"))
                })
                .or_else(|| {
                    render_string_array_expr(arg, state).map(|value| format!("any({value})"))
                })
                .or_else(|| render_expr(arg, state))
        })
        .collect::<Option<Vec<_>>>()?;
    if values.is_empty() {
        return Some(format!("tsgodownAnyArrayConcat({base})"));
    }
    Some(format!(
        "tsgodownAnyArrayConcat({base}, {})",
        values.join(", ")
    ))
}

fn is_array_slice_call_shape(callee: &JsExpr) -> bool {
    matches!(
        callee,
        JsExpr::Member {
            property,
            property_expr: None,
            optional: false,
            ..
        } if property == "slice"
    )
}

fn render_any_array_slice_call(
    callee: &JsExpr,
    args: &[JsExpr],
    state: &AotState,
) -> Option<String> {
    if !is_array_slice_call_shape(callee) || !(1..=2).contains(&args.len()) {
        return None;
    }
    let JsExpr::Member { object, .. } = callee else {
        return None;
    };
    if render_bytes_expr(object, state).is_some() {
        return None;
    }
    let values = render_any_array_from_any_expr(object, state)?;
    let start = render_numeric_expr(args.first()?, state)?;
    if let Some(end) = args.get(1) {
        let end = render_numeric_expr(end, state)?;
        return Some(format!("tsgodownAnyArraySlice({values}, {start}, {end})"));
    }
    Some(format!("tsgodownAnyArraySlice({values}, {start})"))
}

fn render_any_array_flat_call(
    callee: &JsExpr,
    args: &[JsExpr],
    state: &AotState,
) -> Option<String> {
    if args.len() > 1 {
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
    if property != "flat" {
        return None;
    }
    let values = render_any_array_from_any_expr(object, state)?;
    let depth = args
        .first()
        .map(|arg| render_numeric_expr(arg, state))
        .unwrap_or_else(|| Some("1".to_string()))?;
    Some(format!("tsgodownAnyArrayFlat({values}, {depth})"))
}

fn render_any_array_index_expr(expr: &JsExpr, state: &AotState) -> Option<String> {
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
    let receiver_is_known_array = render_any_array_expr(object, state).is_some()
        || render_string_array_expr(object, state).is_some()
        || render_number_array_expr(object, state).is_some()
        || render_bytes_expr(object, state).is_some();
    if !receiver_is_known_array
        && !is_numeric_member_index_shape(property, property_expr.as_deref())
    {
        return None;
    }
    let values = render_any_array_from_any_expr(object, state)?;
    let index = render_member_index_expr(property, property_expr.as_deref(), state)?;
    Some(format!("tsgodownAnyArrayAt({values}, {index})"))
}

fn render_regexp_expr(expr: &JsExpr, state: &AotState) -> Option<String> {
    if let Some(pattern) = render_supported_regexp_pattern(expr) {
        return Some(go_string_literal(&pattern));
    }
    match expr {
        JsExpr::New { callee, args } => render_regexp_new_expr(callee, args, state),
        JsExpr::Ident { name } if state.regexp_bindings.contains(name) => {
            Some(go_binding_ref(name, state))
        }
        _ => None,
    }
}

fn render_date_expr(expr: &JsExpr, state: &AotState) -> Option<String> {
    match expr {
        JsExpr::Ident { name } if state.date_bindings.contains(name) => {
            Some(go_binding_ref(name, state))
        }
        JsExpr::New { callee, args } => render_date_new_expr(callee, args, state),
        _ => None,
    }
}

fn render_date_new_expr(callee: &JsExpr, args: &[JsExpr], state: &AotState) -> Option<String> {
    if !matches!(callee, JsExpr::Ident { name } if name == "Date") {
        return None;
    }
    match args {
        [JsExpr::Call {
            callee: utc_callee,
            args: utc_args,
            ..
        }] if is_date_utc_call(utc_callee) => render_date_utc_iso_expr(utc_args, state),
        [value] => {
            let value = render_numeric_expr(value, state)?;
            Some(format!("tsgodownDateFromUnixMilliISOString({value})"))
        }
        _ => None,
    }
}

fn is_date_utc_call(callee: &JsExpr) -> bool {
    matches!(
        callee,
        JsExpr::Member {
            object,
            property,
            property_expr: None,
            optional: false,
        } if property == "UTC" && matches!(object.as_ref(), JsExpr::Ident { name } if name == "Date")
    )
}

fn render_date_utc_iso_expr(args: &[JsExpr], state: &AotState) -> Option<String> {
    if !(3..=7).contains(&args.len()) {
        return None;
    }
    let values = args
        .iter()
        .map(|arg| render_numeric_expr(arg, state))
        .collect::<Option<Vec<_>>>()?;
    Some(format!("tsgodownDateUTCISOString({})", values.join(", ")))
}

fn is_date_to_iso_call(callee: &JsExpr, args: &[JsExpr], state: &AotState) -> bool {
    if args.is_empty() {
        let JsExpr::Member {
            object,
            property,
            property_expr: None,
            optional: false,
        } = callee
        else {
            return false;
        };
        return matches!(property.as_str(), "toISOString" | "toJSON")
            && render_date_expr(object, state).is_some();
    }
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
    property == "call"
        && matches!(object.as_ref(), JsExpr::Ident { name }
        if state.builtin_function_aliases.get(name).copied() == Some(AotBuiltinFunctionAlias::DateToISOString))
}

fn is_date_to_iso_alias_call_in_context(
    callee: &JsExpr,
    args: &[JsExpr],
    builtin_aliases: &BTreeMap<String, AotBuiltinFunctionAlias>,
) -> bool {
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
    property == "call"
        && matches!(object.as_ref(), JsExpr::Ident { name }
        if builtin_aliases.get(name).copied() == Some(AotBuiltinFunctionAlias::DateToISOString))
}

fn render_date_to_iso_call(callee: &JsExpr, args: &[JsExpr], state: &AotState) -> Option<String> {
    if !is_date_to_iso_call(callee, args, state) {
        return None;
    }
    if args.is_empty() {
        let JsExpr::Member { object, .. } = callee else {
            return None;
        };
        return render_date_expr(object, state);
    }
    render_date_expr(args.first()?, state)
}

fn is_date_constructor_ref(expr: &JsExpr) -> bool {
    matches!(expr, JsExpr::Ident { name } if name == "Date")
}

fn render_date_instanceof_expr(expr: &JsExpr, state: &AotState) -> Option<String> {
    match expr {
        JsExpr::Ident { name } if state.date_bindings.contains(name) => Some("true".to_string()),
        JsExpr::New { callee, args } if render_date_new_expr(callee, args, state).is_some() => {
            Some("true".to_string())
        }
        JsExpr::Value { .. } | JsExpr::Array { .. } | JsExpr::Object { .. } => {
            Some("false".to_string())
        }
        _ => None,
    }
}

fn render_regexp_new_expr(callee: &JsExpr, args: &[JsExpr], state: &AotState) -> Option<String> {
    if !matches!(args.len(), 1 | 2) {
        return None;
    }
    if !matches!(callee, JsExpr::Ident { name } if name == "RegExp") {
        return None;
    }
    let pattern = render_string_expr(args.first()?, state)?;
    let flags = args
        .get(1)
        .map(|expr| render_regexp_flags_expr(expr, state))
        .unwrap_or_else(|| Some("\"\"".to_string()))?;
    Some(format!("tsgodownRegexpPattern({pattern}, {flags})"))
}

fn render_regexp_flags_expr(expr: &JsExpr, state: &AotState) -> Option<String> {
    match expr {
        JsExpr::Value {
            value: JsValue::Undefined,
        } => Some("\"\"".to_string()),
        JsExpr::Ident { name } if name == "undefined" => Some("\"\"".to_string()),
        JsExpr::Conditional {
            test,
            consequent,
            alternate,
        } => render_conditional_expr(
            test,
            consequent,
            alternate,
            state,
            render_regexp_flags_expr,
            "string",
        ),
        _ => render_string_expr(expr, state),
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

fn render_js_set_expr(expr: &JsExpr, state: &AotState) -> Option<String> {
    let JsExpr::New { callee, args } = expr else {
        return None;
    };
    if !matches!(callee.as_ref(), JsExpr::Ident { name } if name == "Set") {
        return None;
    }
    let values = match args.as_slice() {
        [] => "nil".to_string(),
        [value] => render_iterable_array_expr(value, state)?,
        _ => return None,
    };
    Some(format!("tsgodownNewSet({values})"))
}

fn is_new_url_expr(callee: &JsExpr, args: &[JsExpr]) -> bool {
    matches!(args.len(), 1 | 2) && matches!(callee, JsExpr::Ident { name } if name == "URL")
}

fn render_url_new_expr(expr: &JsExpr, state: &AotState) -> Option<String> {
    let JsExpr::New { callee, args } = expr else {
        return None;
    };
    if !is_new_url_expr(callee, args) {
        return None;
    }
    if !state.builtin_bindings.contains("URL")
        && (state.functions.contains_key("URL") || state.bindings.contains("URL"))
    {
        return None;
    }
    let input = render_string_expr(args.first()?, state)?;
    let base = args
        .get(1)
        .map(|arg| render_string_expr(arg, state))
        .unwrap_or_else(|| Some("\"\"".to_string()))?;
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

fn render_set_expr(expr: &JsExpr, state: &AotState) -> Option<String> {
    match expr {
        JsExpr::Ident { name } if state.set_bindings.contains(name) => {
            Some(go_binding_ref(name, state))
        }
        JsExpr::Call { callee, args, .. } if is_set_add_call(callee, args, state) => {
            render_set_call_expr(callee, args, state)
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

fn is_set_size_member(
    object: &JsExpr,
    property: &str,
    property_expr: Option<&JsExpr>,
    state: &AotState,
) -> bool {
    property_expr.is_none() && property == "size" && render_set_expr(object, state).is_some()
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

fn render_set_call_expr(callee: &JsExpr, args: &[JsExpr], state: &AotState) -> Option<String> {
    let JsExpr::Member {
        object,
        property,
        property_expr: None,
        optional: false,
    } = callee
    else {
        return None;
    };
    let target = render_set_expr(object, state)?;
    match property.as_str() {
        "add" if args.len() == 1 => {
            let value = render_json_value_expr(args.first()?, state)?;
            Some(format!("tsgodownSetAdd({target}, {value})"))
        }
        "values" if args.is_empty() => Some(format!("tsgodownSetValues({target})")),
        _ => None,
    }
}

fn is_set_add_call(callee: &JsExpr, args: &[JsExpr], state: &AotState) -> bool {
    let JsExpr::Member {
        object,
        property,
        property_expr: None,
        optional: false,
    } = callee
    else {
        return false;
    };
    args.len() == 1 && property == "add" && render_set_expr(object, state).is_some()
}

fn render_iterable_array_expr(expr: &JsExpr, state: &AotState) -> Option<String> {
    render_map_iterator_array_expr(expr, state)
        .or_else(|| render_set_iterator_array_expr(expr, state))
        .or_else(|| render_any_array_expr(expr, state))
        .or_else(|| {
            render_string_array_expr(expr, state)
                .map(|value| format!("tsgodownAnyArrayFromAny({value})"))
        })
}

fn render_map_iterator_array_expr(expr: &JsExpr, state: &AotState) -> Option<String> {
    let JsExpr::Call { callee, args, .. } = expr else {
        return None;
    };
    if !args.is_empty() {
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
    let target = render_map_expr(object, state)?;
    match property.as_str() {
        "keys" => Some(format!("tsgodownMapKeys({target})")),
        "values" => Some(format!("tsgodownMapValues({target})")),
        _ => None,
    }
}

fn render_set_iterator_array_expr(expr: &JsExpr, state: &AotState) -> Option<String> {
    let JsExpr::Call { callee, args, .. } = expr else {
        return None;
    };
    if !args.is_empty() {
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
    if property != "values" {
        return None;
    }
    let target = render_set_expr(object, state)?;
    Some(format!("tsgodownSetValues({target})"))
}

fn render_iterator_value_member_expr(
    object: &JsExpr,
    property: &str,
    state: &AotState,
) -> Option<String> {
    if property != "value" {
        return None;
    }
    let JsExpr::Call { callee, args, .. } = object else {
        return None;
    };
    if !args.is_empty() {
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
    if property != "next" {
        return None;
    }
    let values = render_iterable_array_expr(object, state)?;
    Some(format!("tsgodownIteratorFirstValue({values})"))
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
        JsExpr::Await { arg } => render_string_array_expr(arg, state),
        expr if is_process_argv_ref(expr) => render_process_argv_expr(state),
        JsExpr::Ident { name }
            if state.string_array_bindings.contains(name)
                && !state.any_array_bindings.contains(name) =>
        {
            Some(go_binding_ref(name, state))
        }
        JsExpr::Array { items } => {
            let items = items
                .iter()
                .map(|item| render_string_expr(item, state))
                .collect::<Option<Vec<_>>>()?;
            Some(format!("[]string{{{}}}", items.join(", ")))
        }
        JsExpr::Call { callee, args, .. }
            if args.is_empty() && matches!(callee.as_ref(), JsExpr::Function { .. }) =>
        {
            render_string_array_iife_expr(callee, state)
        }
        JsExpr::Call { callee, args, .. } if is_object_keys_call(callee, args) => {
            render_object_keys_call(callee, args, state)
        }
        JsExpr::Call { callee, args, .. } if is_array_sort_call(callee, args) => {
            render_string_array_sort_call(callee, args, state)
        }
        JsExpr::Call { callee, args, .. }
            if render_array_map_to_string_values_call(callee, args, state).is_some() =>
        {
            render_array_map_to_string_values_call(callee, args, state)
        }
        JsExpr::Call { callee, args, .. } if is_array_map_to_string_call(callee, args) => {
            render_array_map_to_string_call(callee, args, state)
        }
        JsExpr::Call { callee, args, .. } if is_array_filter_call(callee, args) => {
            render_string_array_filter_call(callee, args, state)
        }
        JsExpr::Call { callee, args, .. }
            if render_string_array_splice_call(callee, args, state).is_some() =>
        {
            render_string_array_splice_call(callee, args, state)
        }
        JsExpr::Call { callee, args, .. } if is_string_match_call(callee, args) => {
            render_string_match_call(callee, args, state)
        }
        JsExpr::Call { callee, args, .. } if is_string_split_call(callee, args) => {
            render_string_split_call(callee, args, state)
        }
        JsExpr::Call { callee, args, .. } if is_string_array_slice_call(callee, args) => {
            render_string_array_slice_call(callee, args, state)
        }
        JsExpr::Call { callee, args, .. }
            if render_string_array_prototype_slice_alias_call(callee, args, state).is_some() =>
        {
            render_string_array_prototype_slice_alias_call(callee, args, state)
        }
        JsExpr::Call { callee, args, .. }
            if render_node_fs_promises_readdir_call(callee, args, state).is_some() =>
        {
            render_node_fs_promises_readdir_call(callee, args, state)
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

fn render_string_array_pop_call(
    callee: &JsExpr,
    args: &[JsExpr],
    state: &AotState,
) -> Option<String> {
    if !args.is_empty() {
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
    if property != "pop" {
        return None;
    }
    if let JsExpr::Ident { name } = object.as_ref() {
        if state.string_array_bindings.contains(name) && !state.any_array_bindings.contains(name) {
            let target = go_binding_ref(name, state);
            return Some(format!("tsgodownStringArrayPop(&{target})"));
        }
    }
    let values = render_string_array_expr(object, state)?;
    Some(format!("tsgodownStringArrayPopValue({values})"))
}

fn render_string_array_shift_call(
    callee: &JsExpr,
    args: &[JsExpr],
    state: &AotState,
) -> Option<String> {
    if !args.is_empty() {
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
    if property != "shift" {
        return None;
    }
    let JsExpr::Ident { name } = object.as_ref() else {
        return None;
    };
    if !state.string_array_bindings.contains(name) {
        return None;
    }
    let target = go_binding_ref(name, state);
    Some(format!("tsgodownStringArrayShift(&{target})"))
}

fn render_string_array_splice_call(
    callee: &JsExpr,
    args: &[JsExpr],
    state: &AotState,
) -> Option<String> {
    if args.len() < 2 {
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
    if property != "splice" {
        return None;
    }
    let JsExpr::Ident { name } = object.as_ref() else {
        return None;
    };
    if !state.string_array_bindings.contains(name) {
        return None;
    }
    let target = go_binding_ref(name, state);
    let start = render_numeric_expr(args.first()?, state)?;
    let delete_count = render_numeric_expr(args.get(1)?, state)?;
    let inserts = args
        .iter()
        .skip(2)
        .map(|arg| render_string_expr(arg, state))
        .collect::<Option<Vec<_>>>()?;
    if inserts.is_empty() {
        return Some(format!(
            "tsgodownStringArraySplice(&{target}, {start}, {delete_count})"
        ));
    }
    Some(format!(
        "tsgodownStringArraySplice(&{target}, {start}, {delete_count}, {})",
        inserts.join(", ")
    ))
}

fn render_string_array_iife_expr(callee: &JsExpr, state: &AotState) -> Option<String> {
    let JsExpr::Function {
        params,
        rest_param: None,
        r#async: false,
        generator: false,
        body,
        ..
    } = callee
    else {
        return None;
    };
    if !params.is_empty() {
        return None;
    }
    let mut block_state = clone_aot_state(state);
    mark_number_array_locals(body, &mut block_state);
    mark_string_array_locals(body, &mut block_state);
    mark_any_array_locals(body, &mut block_state);
    mark_array_property_locals(body, &mut block_state);
    mark_dynamic_object_locals(body, &mut block_state);
    let mut rendered = Vec::new();
    for stmt in body {
        match stmt {
            JsStmt::Break { .. } | JsStmt::Continue { .. } => return None,
            JsStmt::Return { value: Some(value) } => {
                let value = render_string_array_expr(value, &block_state)?;
                rendered.push(format!("return {value}"));
            }
            JsStmt::Return { value: None } => rendered.push("return []string{}".to_string()),
            other => rendered.push(render_stmt(other, &mut block_state)?),
        }
    }
    if !matches!(body.last(), Some(JsStmt::Return { .. })) {
        rendered.push("return []string{}".to_string());
    }
    Some(format!(
        "func() []string {{\n{}\n}}()",
        indent_lines(&rendered.join("\n"))
    ))
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
    let value = render_json_value_expr(args.first()?, state)
        .or_else(|| render_expr(args.first()?, state))?;
    Some(format!("{target} = append({target}, {value})"))
}

fn render_string_array_push_call_stmt(
    callee: &JsExpr,
    args: &[JsExpr],
    state: &mut AotState,
) -> Option<String> {
    if args.is_empty() {
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
    if !matches!(property.as_str(), "push" | "unshift") {
        return None;
    }
    let JsExpr::Ident { name } = object.as_ref() else {
        return None;
    };
    if !state.string_array_bindings.contains(name) {
        return None;
    }
    let target = go_binding_ref(name, state);
    let values = args
        .iter()
        .map(|arg| render_string_expr(arg, state))
        .collect::<Option<Vec<_>>>()?;
    if property == "unshift" {
        return Some(format!(
            "_ = tsgodownStringArrayUnshift(&{target}, {})",
            values.join(", ")
        ));
    }
    Some(format!(
        "{target} = append({target}, {})",
        values.join(", ")
    ))
}

fn render_url_assignment_stmt(
    op: &str,
    left: &JsExpr,
    right: &JsExpr,
    state: &mut AotState,
) -> Option<String> {
    if op != "=" {
        return None;
    }
    let JsExpr::Ident { name } = left else {
        return None;
    };
    let value = render_url_new_expr(right, state)?;
    let ident = sanitize_go_identifier(name);
    state.bindings.insert(name.clone());
    state
        .binding_refs
        .insert(name.clone(), format!("{ident}.(*tsgodownURL)"));
    state.url_bindings.insert(name.clone());
    Some(format!("{ident} = {value}"))
}

fn render_any_array_pop_call(callee: &JsExpr, args: &[JsExpr], state: &AotState) -> Option<String> {
    if !args.is_empty() {
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
    if property != "pop" {
        return None;
    }
    if let JsExpr::Ident { name } = object.as_ref() {
        if state.any_array_bindings.contains(name) {
            let target = go_binding_ref(name, state);
            return Some(format!("tsgodownAnyArrayPop(&{target})"));
        }
    }
    let values = render_any_array_expr(object, state)?;
    Some(format!("tsgodownAnyArrayPopValue({values})"))
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
    let value = render_json_value_expr(args.first()?, state)
        .or_else(|| render_expr(args.first()?, state))?;
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

fn render_any_array_length_expr(expr: &JsExpr, state: &AotState) -> Option<String> {
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
    let values = render_any_array_expr(object, state)?;
    Some(format!("float64(len(tsgodownAnyArrayFromAny({values})))"))
}

fn render_bytes_length_expr(expr: &JsExpr, state: &AotState) -> Option<String> {
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
    let values = render_bytes_expr(object, state)?;
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

fn is_object_entries_call(callee: &JsExpr, args: &[JsExpr]) -> bool {
    args.len() == 1
        && matches!(
            callee,
            JsExpr::Member {
                object,
                property,
                property_expr: None,
                optional: false,
            } if matches!(object.as_ref(), JsExpr::Ident { name } if name == "Object")
                && property == "entries"
        )
}

fn object_freeze_arg(expr: &JsExpr) -> Option<&JsExpr> {
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
    if !matches!(object.as_ref(), JsExpr::Ident { name } if name == "Object")
        || property != "freeze"
    {
        return None;
    }
    args.first()
}

fn is_object_create_null_call(callee: &JsExpr, args: &[JsExpr]) -> bool {
    args.len() == 1
        && matches!(
            callee,
            JsExpr::Member {
                object,
                property,
                property_expr: None,
                optional: false,
            } if matches!(object.as_ref(), JsExpr::Ident { name } if name == "Object")
                && property == "create"
        )
        && matches!(
            args.first(),
            Some(JsExpr::Value {
                value: JsValue::Null
            })
        )
}

fn is_object_assign_call(callee: &JsExpr, args: &[JsExpr]) -> bool {
    !args.is_empty()
        && matches!(
            callee,
            JsExpr::Member {
                object,
                property,
                property_expr: None,
                optional: false,
            } if matches!(object.as_ref(), JsExpr::Ident { name } if name == "Object")
                && property == "assign"
        )
}

fn render_object_keys_call(callee: &JsExpr, args: &[JsExpr], state: &AotState) -> Option<String> {
    if !is_object_keys_call(callee, args) {
        return None;
    }
    match args.first()? {
        JsExpr::Object { props } => {
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
        expr => {
            if let JsExpr::Ident { name } = expr {
                if state.array_property_bindings.contains(name) {
                    let values = render_any_array_from_any_expr(expr, state)?;
                    let order = array_property_order_ref(name, state);
                    return Some(format!("tsgodownArrayObjectKeys(len({values}), {order})"));
                }
                if state.ordered_dynamic_object_bindings.contains(name) {
                    let order = dynamic_object_order_ref(name, state);
                    return Some(format!("tsgodownObjectKeys({order})"));
                }
            }
            let object = render_dynamic_object_source_expr(expr, state)
                .or_else(|| render_object_map_expr(expr, state))?;
            Some(format!("tsgodownObjectMapKeys({object})"))
        }
    }
}

fn render_object_entries_call(args: &[JsExpr], state: &AotState) -> Option<String> {
    let object = args.first()?;
    if let JsExpr::Object { props } = object {
        if props.iter().any(|prop| prop.spread) {
            return None;
        }
        let entries = props
            .iter()
            .map(|prop| {
                let key = match &prop.key_expr {
                    Some(key_expr) => render_string_expr(key_expr, state)?,
                    None => go_string_literal(&prop.key),
                };
                let value = render_json_value_expr(&prop.value, state)?;
                Some(format!("[]any{{{key}, {value}}}"))
            })
            .collect::<Option<Vec<_>>>()?;
        return Some(format!("[]any{{{}}}", entries.join(", ")));
    }
    let value = render_dynamic_object_source_expr(object, state)
        .or_else(|| render_object_map_expr(object, state))?;
    Some(format!("tsgodownObjectEntries({value})"))
}

fn is_length_member_property(property: &str, property_expr: Option<&JsExpr>) -> bool {
    property == "length"
        || matches!(property_expr, Some(JsExpr::Ident { name }) if name == "length")
}

fn is_length_member_expr(expr: &JsExpr) -> bool {
    matches!(
        expr,
        JsExpr::Member {
            property,
            property_expr,
            ..
        } if is_length_member_property(property, property_expr.as_deref())
    )
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
    if matches!(object.as_ref(), JsExpr::Ident { name } if is_any_binding(name, state))
        && !is_known_numeric_member_index(property, Some(property_expr), state)
    {
        return None;
    }
    let value = render_string_expr(object, state)?;
    let index = render_numeric_expr(property_expr, state)?;
    Some(format!("tsgodownStringCharAt({value}, {index})"))
}

fn call_uses_strings_import(callee: &JsExpr) -> bool {
    matches!(
        string_method_name(callee),
        Some(
            "toLowerCase"
                | "toUpperCase"
                | "trim"
                | "trimStart"
                | "trimEnd"
                | "includes"
                | "startsWith"
                | "endsWith"
                | "indexOf"
                | "lastIndexOf"
                | "replaceAll"
                | "repeat"
        )
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
        "toLowerCase" | "toUpperCase" | "trim" | "trimStart" | "trimEnd" | "includes"
        | "startsWith" | "endsWith" | "indexOf" | "lastIndexOf" | "charAt" | "charCodeAt"
        | "replace" | "replaceAll" | "slice" | "substring" | "substr" | "repeat" => {
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
        "toLowerCase" | "toUpperCase" | "trim" | "trimStart" | "trimEnd" if args.is_empty() => {}
        "includes" if args.len() == 1 => {}
        "startsWith" | "endsWith" if args.len() == 1 => {}
        "indexOf" if matches!(args.len(), 1 | 2) => {}
        "lastIndexOf" if matches!(args.len(), 1 | 2) => {}
        "charAt" if args.len() == 1 => {}
        "charCodeAt" if args.len() == 1 => {}
        "replace" if args.len() == 2 => {}
        "replaceAll" if args.len() == 2 => {}
        "slice" if matches!(args.len(), 1 | 2) => {}
        "substring" if args.len() == 2 => {}
        "substr" if matches!(args.len(), 1 | 2) => {}
        "repeat" if args.len() == 1 => {}
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
    if let Some(object) = string_method_receiver(callee, "trimStart", args, state) {
        let object = render_string_expr(object, state)?;
        return Some(format!("strings.TrimLeftFunc({object}, unicode.IsSpace)"));
    }
    if let Some(object) = string_method_receiver(callee, "trimEnd", args, state) {
        let object = render_string_expr(object, state)?;
        return Some(format!("strings.TrimRightFunc({object}, unicode.IsSpace)"));
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
    if let Some(object) = string_method_receiver(callee, "substring", args, state) {
        let object = render_string_expr(object, state)?;
        let start = render_numeric_expr(args.first()?, state)?;
        let end = render_numeric_expr(args.get(1)?, state)?;
        return Some(format!("tsgodownStringSubstring({object}, {start}, {end})"));
    }
    if let Some(object) = string_method_receiver(callee, "substr", args, state) {
        let object = render_string_expr(object, state)?;
        let start = render_numeric_expr(args.first()?, state)?;
        if let Some(length) = args.get(1) {
            let length = render_numeric_expr(length, state)?;
            return Some(format!("tsgodownStringSubstr({object}, {start}, {length})"));
        }
        return Some(format!("tsgodownStringSubstr({object}, {start})"));
    }
    if let Some(object) = string_method_receiver(callee, "repeat", args, state) {
        let object = render_string_expr(object, state)?;
        let count = render_numeric_expr(args.first()?, state)?;
        return Some(format!("strings.Repeat({object}, int({count}))"));
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
                if let Some((pattern, global)) =
                    render_supported_regexp_backref_replace_pattern(args.first()?)
                {
                    return Some(format!(
                        "tsgodownRegexpReplaceBackref({object}, {}, {replacement}, {global})",
                        go_string_literal(&pattern)
                    ));
                }
                let (pattern, global) = render_supported_regexp_replace_pattern(args.first()?)?;
                return Some(format!(
                    "tsgodownRegexpReplace({object}, {}, {replacement}, {global})",
                    go_string_literal(&pattern)
                ));
            }
            _ => {}
        }
    }
    if let Some(object) = string_method_receiver(callee, "replaceAll", args, state) {
        let object = render_string_expr(object, state)?;
        let needle = render_string_expr(args.first()?, state)?;
        let replacement = render_string_expr(args.get(1)?, state)?;
        return Some(format!(
            "strings.ReplaceAll({object}, {needle}, {replacement})"
        ));
    }
    None
}

fn render_string_bool_method_call(
    callee: &JsExpr,
    args: &[JsExpr],
    state: &AotState,
) -> Option<String> {
    if let Some(object) = string_method_receiver(callee, "includes", args, state) {
        let object = render_string_expr(object, state)?;
        let needle = render_string_expr(args.first()?, state)?;
        return Some(format!("strings.Contains({object}, {needle})"));
    }
    if let Some(object) = string_method_receiver(callee, "startsWith", args, state) {
        let object = render_string_expr(object, state)?;
        let needle = render_string_expr(args.first()?, state)?;
        return Some(format!("strings.HasPrefix({object}, {needle})"));
    }
    if let Some(object) = string_method_receiver(callee, "endsWith", args, state) {
        let object = render_string_expr(object, state)?;
        let needle = render_string_expr(args.first()?, state)?;
        return Some(format!("strings.HasSuffix({object}, {needle})"));
    }
    None
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

fn render_string_array_includes_call(
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
    let values = render_string_array_expr(object, state)?;
    let needle = render_string_expr(args.first()?, state)?;
    Some(format!("tsgodownStringArrayIncludes({values}, {needle})"))
}

fn render_array_index_of_call(
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
    if property != "indexOf" {
        return None;
    }
    if let Some(values) = render_number_array_expr(object, state) {
        let needle = render_numeric_expr(args.first()?, state)?;
        return Some(format!("tsgodownNumberArrayIndexOf({values}, {needle})"));
    }
    None
}

fn render_array_includes_call(
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
    if property != "includes" {
        return None;
    }
    if let Some(values) = render_number_array_expr(object, state) {
        let needle = render_numeric_expr(args.first()?, state)?;
        return Some(format!("tsgodownNumberArrayIncludes({values}, {needle})"));
    }
    None
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

fn builtin_function_alias(expr: &JsExpr) -> Option<AotBuiltinFunctionAlias> {
    if is_array_is_array_ref(expr) {
        return Some(AotBuiltinFunctionAlias::ArrayIsArray);
    }
    if is_array_prototype_method_ref(expr, "concat") {
        return Some(AotBuiltinFunctionAlias::ArrayConcat);
    }
    if is_array_prototype_method_ref(expr, "join") {
        return Some(AotBuiltinFunctionAlias::ArrayJoin);
    }
    if is_array_prototype_push_ref(expr) {
        return Some(AotBuiltinFunctionAlias::ArrayPush);
    }
    if is_array_prototype_method_ref(expr, "slice") {
        return Some(AotBuiltinFunctionAlias::ArraySlice);
    }
    if is_date_prototype_to_iso_ref(expr) {
        return Some(AotBuiltinFunctionAlias::DateToISOString);
    }
    if is_object_has_own_property_ref(expr) {
        return Some(AotBuiltinFunctionAlias::ObjectHasOwnProperty);
    }
    if is_object_prototype_to_string_ref(expr) {
        return Some(AotBuiltinFunctionAlias::ObjectToString);
    }
    if is_regexp_prototype_test_ref(expr) {
        return Some(AotBuiltinFunctionAlias::RegExpTest);
    }
    None
}

fn is_date_prototype_to_iso_ref(expr: &JsExpr) -> bool {
    matches!(
        expr,
        JsExpr::Member {
            object,
            property,
            property_expr: None,
            optional: false,
        } if property == "toISOString" && is_date_prototype_expr(object)
    )
}

fn is_date_prototype_expr(expr: &JsExpr) -> bool {
    matches!(
        expr,
        JsExpr::Member {
            object,
            property,
            property_expr: None,
            optional: false,
        } if property == "prototype"
            && matches!(object.as_ref(), JsExpr::Ident { name } if name == "Date")
    )
}

fn is_array_prototype_method_ref(expr: &JsExpr, method: &str) -> bool {
    matches!(
        expr,
        JsExpr::Member {
            object,
            property,
            property_expr: None,
            optional: false,
        } if property == method && is_array_prototype_expr(object)
    )
}

fn is_array_is_array_ref(expr: &JsExpr) -> bool {
    matches!(
        expr,
        JsExpr::Member {
            object,
            property,
            property_expr: None,
            optional: false,
        } if matches!(object.as_ref(), JsExpr::Ident { name } if name == "Array")
            && property == "isArray"
    )
}

fn is_object_prototype_expr(expr: &JsExpr) -> bool {
    matches!(
        expr,
        JsExpr::Member {
            object,
            property,
            property_expr: None,
            optional: false,
        } if matches!(object.as_ref(), JsExpr::Ident { name } if name == "Object")
            && property == "prototype"
    )
}

fn is_array_prototype_expr(expr: &JsExpr) -> bool {
    matches!(
        expr,
        JsExpr::Member {
            object,
            property,
            property_expr: None,
            optional: false,
        } if matches!(object.as_ref(), JsExpr::Ident { name } if name == "Array")
            && property == "prototype"
    )
}

fn is_array_prototype_push_ref(expr: &JsExpr) -> bool {
    matches!(
        expr,
        JsExpr::Member {
            object,
            property,
            property_expr: None,
            optional: false,
        } if is_array_prototype_expr(object) && property == "push"
    )
}

fn is_object_has_own_property_ref(expr: &JsExpr) -> bool {
    matches!(
        expr,
        JsExpr::Member {
            object,
            property,
            property_expr: None,
            optional: false,
        } if is_object_prototype_expr(object) && property == "hasOwnProperty"
    )
}

fn is_array_is_array_call(callee: &JsExpr, args: &[JsExpr]) -> bool {
    args.len() == 1 && is_array_is_array_ref(callee)
}

fn is_array_is_array_alias_call(callee: &JsExpr, args: &[JsExpr], state: &AotState) -> bool {
    args.len() == 1
        && matches!(callee, JsExpr::Ident { name }
        if matches!(
            state.builtin_function_aliases.get(name),
            Some(AotBuiltinFunctionAlias::ArrayIsArray)
        ))
}

fn is_array_is_array_alias_call_in_context(
    callee: &JsExpr,
    args: &[JsExpr],
    builtin_aliases: &BTreeMap<String, AotBuiltinFunctionAlias>,
) -> bool {
    args.len() == 1
        && matches!(callee, JsExpr::Ident { name }
        if matches!(
            builtin_aliases.get(name),
            Some(AotBuiltinFunctionAlias::ArrayIsArray)
        ))
}

fn render_array_is_array_call(args: &[JsExpr], state: &AotState) -> Option<String> {
    if args.len() != 1 {
        return None;
    }
    let value = render_expr(args.first()?, state)?;
    Some(format!(
        "func() bool {{ switch any({value}).(type) {{ case []string, []any, []float64: return true; default: return false }} }}()"
    ))
}

fn is_array_push_apply_call(callee: &JsExpr, args: &[JsExpr], state: &AotState) -> bool {
    if args.len() != 2 {
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
    if property != "apply" {
        return false;
    }
    if is_array_prototype_push_ref(object) {
        return true;
    }
    if matches!(
        object.as_ref(),
        JsExpr::Member {
            property,
            property_expr: None,
            optional: false,
            ..
        } if property == "push"
    ) {
        return true;
    }
    matches!(object.as_ref(), JsExpr::Ident { name }
    if matches!(
        state.builtin_function_aliases.get(name),
        Some(AotBuiltinFunctionAlias::ArrayPush)
    ))
}

fn is_array_push_apply_call_in_context(
    callee: &JsExpr,
    args: &[JsExpr],
    builtin_aliases: &BTreeMap<String, AotBuiltinFunctionAlias>,
) -> bool {
    if args.len() != 2 {
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
    if property != "apply" {
        return false;
    }
    if is_array_prototype_push_ref(object) {
        return true;
    }
    if matches!(
        object.as_ref(),
        JsExpr::Member {
            property,
            property_expr: None,
            optional: false,
            ..
        } if property == "push"
    ) {
        return true;
    }
    matches!(object.as_ref(), JsExpr::Ident { name }
    if matches!(
        builtin_aliases.get(name),
        Some(AotBuiltinFunctionAlias::ArrayPush)
    ))
}

fn is_array_prototype_alias_call(
    callee: &JsExpr,
    args: &[JsExpr],
    state: &AotState,
    alias: AotBuiltinFunctionAlias,
) -> bool {
    if args.is_empty() {
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
    property == "call"
        && (is_array_prototype_alias_ref(object, alias)
            || matches!(object.as_ref(), JsExpr::Ident { name }
            if state.builtin_function_aliases.get(name).copied() == Some(alias)))
}

fn is_array_prototype_alias_call_in_context(
    callee: &JsExpr,
    args: &[JsExpr],
    builtin_aliases: &BTreeMap<String, AotBuiltinFunctionAlias>,
    alias: AotBuiltinFunctionAlias,
) -> bool {
    if args.is_empty() {
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
    property == "call"
        && (is_array_prototype_alias_ref(object, alias)
            || matches!(object.as_ref(), JsExpr::Ident { name }
            if builtin_aliases.get(name).copied() == Some(alias)))
}

fn is_array_prototype_alias_ref(expr: &JsExpr, alias: AotBuiltinFunctionAlias) -> bool {
    let method = match alias {
        AotBuiltinFunctionAlias::ArrayConcat => "concat",
        AotBuiltinFunctionAlias::ArrayJoin => "join",
        AotBuiltinFunctionAlias::ArrayPush => "push",
        AotBuiltinFunctionAlias::ArraySlice => "slice",
        _ => return false,
    };
    is_array_prototype_method_ref(expr, method)
}

fn render_any_array_prototype_concat_alias_call(
    callee: &JsExpr,
    args: &[JsExpr],
    state: &AotState,
) -> Option<String> {
    if !is_array_prototype_alias_call(callee, args, state, AotBuiltinFunctionAlias::ArrayConcat) {
        return None;
    }
    let receiver = render_json_value_expr(args.first()?, state)
        .or_else(|| render_expr(args.first()?, state))?;
    let base = format!("tsgodownAnyArrayConcatBase({receiver})");
    let values = args
        .iter()
        .skip(1)
        .map(|arg| render_json_value_expr(arg, state).or_else(|| render_expr(arg, state)))
        .collect::<Option<Vec<_>>>()?;
    if values.is_empty() {
        return Some(format!("tsgodownAnyArrayConcat({base})"));
    }
    Some(format!(
        "tsgodownAnyArrayConcat({base}, {})",
        values.join(", ")
    ))
}

fn render_any_array_prototype_slice_alias_call(
    callee: &JsExpr,
    args: &[JsExpr],
    state: &AotState,
) -> Option<String> {
    if !is_array_prototype_alias_call(callee, args, state, AotBuiltinFunctionAlias::ArraySlice)
        || !matches!(args.len(), 1..=3)
    {
        return None;
    }
    let values = render_any_array_from_any_expr(args.first()?, state)?;
    let start = args
        .get(1)
        .map(|expr| render_numeric_expr(expr, state))
        .unwrap_or_else(|| Some("0".to_string()))?;
    if let Some(end) = args.get(2) {
        let end = render_numeric_expr(end, state)?;
        return Some(format!("tsgodownAnyArraySlice({values}, {start}, {end})"));
    }
    Some(format!("tsgodownAnyArraySlice({values}, {start})"))
}

fn render_string_array_prototype_slice_alias_call(
    callee: &JsExpr,
    args: &[JsExpr],
    state: &AotState,
) -> Option<String> {
    if !is_array_prototype_alias_call(callee, args, state, AotBuiltinFunctionAlias::ArraySlice)
        || !matches!(args.len(), 1..=3)
    {
        return None;
    }
    let values = render_string_array_expr(args.first()?, state)?;
    let start = args
        .get(1)
        .map(|expr| render_numeric_expr(expr, state))
        .unwrap_or_else(|| Some("0".to_string()))?;
    if let Some(end) = args.get(2) {
        let end = render_numeric_expr(end, state)?;
        return Some(format!(
            "tsgodownStringArraySlice({values}, {start}, {end})"
        ));
    }
    Some(format!("tsgodownStringArraySlice({values}, {start})"))
}

fn render_array_prototype_join_alias_call(
    callee: &JsExpr,
    args: &[JsExpr],
    state: &AotState,
) -> Option<String> {
    if !is_array_prototype_alias_call(callee, args, state, AotBuiltinFunctionAlias::ArrayJoin)
        || !matches!(args.len(), 1 | 2)
    {
        return None;
    }
    let values = render_any_array_from_any_expr(args.first()?, state)?;
    let separator = args
        .get(1)
        .map(|expr| render_string_expr(expr, state))
        .unwrap_or_else(|| Some("\",\"".to_string()))?;
    Some(format!("tsgodownAnyArrayJoin({values}, {separator})"))
}

fn render_array_push_apply_stmt(
    callee: &JsExpr,
    args: &[JsExpr],
    state: &AotState,
) -> Option<String> {
    if !is_array_push_apply_call(callee, args, state) {
        return None;
    }
    let JsExpr::Ident { name } = args.first()? else {
        return None;
    };
    if state.string_array_bindings.contains(name) {
        let target = go_binding_ref(name, state);
        let values = render_string_array_expr(args.get(1)?, state)?;
        return Some(format!("{target} = append({target}, {values}...)"));
    }
    if !state.any_array_bindings.contains(name) {
        return None;
    }
    let target = go_binding_ref(name, state);
    let values = render_any_array_expr(args.get(1)?, state)?;
    Some(format!("{target} = append({target}, {values}...)"))
}

fn is_object_has_own_property_call(callee: &JsExpr, args: &[JsExpr], state: &AotState) -> bool {
    if !is_object_has_own_property_call_shape(callee, args) {
        return false;
    }
    let JsExpr::Member { object, .. } = callee else {
        return false;
    };
    if is_object_has_own_property_ref(object) {
        return true;
    }
    matches!(object.as_ref(), JsExpr::Ident { name }
    if matches!(
        state.builtin_function_aliases.get(name),
        Some(AotBuiltinFunctionAlias::ObjectHasOwnProperty)
    ))
}

fn is_object_has_own_property_call_shape(callee: &JsExpr, args: &[JsExpr]) -> bool {
    if args.len() != 2 {
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
    if property != "call" {
        return false;
    }
    is_object_has_own_property_ref(object) || matches!(object.as_ref(), JsExpr::Ident { .. })
}

fn is_object_has_own_property_call_in_context(
    callee: &JsExpr,
    args: &[JsExpr],
    builtin_aliases: &BTreeMap<String, AotBuiltinFunctionAlias>,
) -> bool {
    if args.len() != 2 {
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
    if property != "call" {
        return false;
    }
    if is_object_has_own_property_ref(object) {
        return true;
    }
    matches!(object.as_ref(), JsExpr::Ident { name }
    if matches!(
        builtin_aliases.get(name),
        Some(AotBuiltinFunctionAlias::ObjectHasOwnProperty)
    ))
}

fn render_object_has_own_property_call(
    callee: &JsExpr,
    args: &[JsExpr],
    state: &AotState,
) -> Option<String> {
    if !is_object_has_own_property_call(callee, args, state) {
        return None;
    }
    let receiver = args.first()?;
    let key = render_expr(args.get(1)?, state)?;
    if is_object_prototype_expr(receiver) {
        return Some(format!("tsgodownObjectPrototypeHasOwn({key})"));
    }
    if let JsExpr::Ident { name } = receiver {
        if state.array_property_bindings.contains(name) {
            let values = render_any_array_from_any_expr(receiver, state)?;
            let props = array_property_map_ref(name, state);
            return Some(format!(
                "tsgodownArrayHasOwn(len({values}), {props}, {key})"
            ));
        }
    }
    let object = render_expr(receiver, state)?;
    Some(format!("tsgodownObjectHasOwn({object}, {key})"))
}

fn is_object_has_own_call(callee: &JsExpr, args: &[JsExpr]) -> bool {
    if args.len() != 2 {
        return false;
    }
    matches!(
        callee,
        JsExpr::Member {
            object,
            property,
            property_expr: None,
            optional: false,
        } if property == "hasOwn"
            && matches!(object.as_ref(), JsExpr::Ident { name } if name == "Object")
    )
}

fn render_object_has_own_call(
    callee: &JsExpr,
    args: &[JsExpr],
    state: &AotState,
) -> Option<String> {
    if !is_object_has_own_call(callee, args) {
        return None;
    }
    if let Some(value) = render_function_static_has_own(args.first()?, args.get(1)?, state) {
        return Some(value);
    }
    let object = render_expr(args.first()?, state)?;
    let key = render_expr(args.get(1)?, state)?;
    Some(format!("tsgodownObjectHasOwn({object}, {key})"))
}

fn render_function_static_has_own(
    object: &JsExpr,
    key: &JsExpr,
    state: &AotState,
) -> Option<String> {
    let JsExpr::Ident { name } = object else {
        return None;
    };
    if !state.functions.contains_key(name) {
        return None;
    }
    let property = string_literal_value(key)?;
    Some(
        state
            .function_static_members
            .contains_key(&(name.clone(), property))
            .to_string(),
    )
}

fn is_object_prototype_to_string_call(callee: &JsExpr, args: &[JsExpr]) -> bool {
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
    property == "call" && is_object_prototype_to_string_ref(object)
}

fn is_object_to_string_alias_call(callee: &JsExpr, args: &[JsExpr], state: &AotState) -> bool {
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
    property == "call"
        && matches!(object.as_ref(), JsExpr::Ident { name }
        if matches!(
            state.builtin_function_aliases.get(name),
            Some(AotBuiltinFunctionAlias::ObjectToString)
        ))
}

fn is_object_to_string_alias_call_in_context(
    callee: &JsExpr,
    args: &[JsExpr],
    builtin_aliases: &BTreeMap<String, AotBuiltinFunctionAlias>,
) -> bool {
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
    property == "call"
        && matches!(object.as_ref(), JsExpr::Ident { name }
        if matches!(
            builtin_aliases.get(name),
            Some(AotBuiltinFunctionAlias::ObjectToString)
        ))
}

fn is_object_prototype_to_string_ref(expr: &JsExpr) -> bool {
    matches!(
        expr,
        JsExpr::Member {
            object,
            property,
            property_expr: None,
            optional: false,
        } if is_object_prototype_expr(object) && property == "toString"
    )
}

fn render_object_prototype_to_string_call(expr: &JsExpr, state: &AotState) -> Option<String> {
    match expr {
        JsExpr::Value {
            value: JsValue::RegExp { .. },
        } => Some(go_string_literal("[object RegExp]")),
        JsExpr::Value {
            value: JsValue::String { .. },
        } => Some(go_string_literal("[object String]")),
        JsExpr::Value {
            value: JsValue::Number { .. },
        } => Some(go_string_literal("[object Number]")),
        JsExpr::Value {
            value: JsValue::Bool { .. },
        } => Some(go_string_literal("[object Boolean]")),
        JsExpr::Value {
            value: JsValue::Null,
        } => Some(go_string_literal("[object Null]")),
        JsExpr::Value {
            value: JsValue::Undefined,
        } => Some(go_string_literal("[object Undefined]")),
        JsExpr::Ident { name } if name == "undefined" => {
            Some(go_string_literal("[object Undefined]"))
        }
        JsExpr::Array { .. } => Some(go_string_literal("[object Array]")),
        JsExpr::Ident { name } if state.regexp_bindings.contains(name) => {
            Some(go_string_literal("[object RegExp]"))
        }
        _ => {
            let value = render_json_value_expr(expr, state).or_else(|| render_expr(expr, state))?;
            Some(format!("tsgodownObjectToStringTag({value})"))
        }
    }
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

fn render_array_map_to_string_values_call(
    callee: &JsExpr,
    args: &[JsExpr],
    state: &AotState,
) -> Option<String> {
    if !is_array_map_call(callee, args) {
        return None;
    }
    let JsExpr::Member { object, .. } = callee else {
        return None;
    };
    let mapper = render_array_string_mapper_expr(args.first()?, state)?;
    if let Some(values) = render_any_array_expr(object, state) {
        return Some(format!(
            "tsgodownStringArrayFromAny(tsgodownAnyArrayMap({values}, {mapper}))"
        ));
    }
    let values = render_string_array_expr(object, state)?;
    Some(format!(
        "tsgodownStringArrayFromAny(tsgodownStringArrayMap({values}, {mapper}))"
    ))
}

fn render_array_string_mapper_expr(expr: &JsExpr, state: &AotState) -> Option<String> {
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
    if params.len() > 2 {
        return None;
    }
    let (last, prelude) = body.split_last()?;
    let JsStmt::Return { value: Some(value) } = last else {
        return None;
    };
    let mut mapper_state = clone_aot_state(state);
    if let Some(param) = params.first() {
        mapper_state.bind_slot(param, sanitize_go_identifier(param), AotSlotKind::Any);
    }
    if let Some(param) = params.get(1) {
        mapper_state.bind_slot(param, sanitize_go_identifier(param), AotSlotKind::Number);
    }
    mark_number_array_locals(body, &mut mapper_state);
    mark_string_array_locals(body, &mut mapper_state);
    mark_any_array_locals(body, &mut mapper_state);
    mark_array_property_locals(body, &mut mapper_state);
    mark_dynamic_object_locals(body, &mut mapper_state);
    let prelude = prelude
        .iter()
        .map(|stmt| render_stmt(stmt, &mut mapper_state))
        .collect::<Option<Vec<_>>>()?
        .join("\n");
    let value = render_string_expr(value, &mapper_state)?;
    let value_param = params
        .first()
        .map(|param| sanitize_go_identifier(param))
        .unwrap_or_else(|| "_".to_string());
    let index_param = params
        .get(1)
        .map(|param| sanitize_go_identifier(param))
        .unwrap_or_else(|| "_".to_string());
    let body = if prelude.is_empty() {
        format!("return {value}")
    } else {
        format!("{prelude}\nreturn {value}")
    };
    Some(format!(
        "func({value_param} any, {index_param} float64) any {{\n{}\n}}",
        indent_lines(&body)
    ))
}

fn is_array_sort_call(callee: &JsExpr, args: &[JsExpr]) -> bool {
    args.is_empty()
        && matches!(
            callee,
            JsExpr::Member {
                property,
                property_expr: None,
                optional: false,
                ..
            } if property == "sort"
        )
}

fn render_string_array_sort_call(
    callee: &JsExpr,
    args: &[JsExpr],
    state: &AotState,
) -> Option<String> {
    if !is_array_sort_call(callee, args) {
        return None;
    }
    let JsExpr::Member { object, .. } = callee else {
        return None;
    };
    let values = render_string_array_expr(object, state)?;
    Some(format!("tsgodownStringArraySort({values})"))
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

fn is_string_split_call(callee: &JsExpr, args: &[JsExpr]) -> bool {
    matches!(args.len(), 1)
        && matches!(
            callee,
            JsExpr::Member {
                property,
                property_expr: None,
                optional: false,
                ..
            } if property == "split"
        )
}

fn render_string_split_call(callee: &JsExpr, args: &[JsExpr], state: &AotState) -> Option<String> {
    if !is_string_split_call(callee, args) {
        return None;
    }
    let JsExpr::Member { object, .. } = callee else {
        return None;
    };
    let value = render_string_expr(object, state)?;
    let separator = render_string_expr(args.first()?, state)?;
    Some(format!("strings.Split({value}, {separator})"))
}

fn render_any_array_join_call(
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
    let values = render_any_array_from_any_expr(object, state)?;
    let separator = args
        .first()
        .map(|expr| render_string_expr(expr, state))
        .unwrap_or_else(|| Some("\",\"".to_string()))?;
    Some(format!("tsgodownAnyArrayJoin({values}, {separator})"))
}

fn render_string_method_alias_call(
    callee: &JsExpr,
    args: &[JsExpr],
    state: &AotState,
) -> Option<String> {
    let (method, object) = string_method_alias_call_parts(callee, args, state)?;
    match method.as_str() {
        "toLowerCase" if args.len() == 1 => {
            let object = render_string_expr(object, state)?;
            Some(format!("strings.ToLower({object})"))
        }
        "toUpperCase" if args.len() == 1 => {
            let object = render_string_expr(object, state)?;
            Some(format!("strings.ToUpper({object})"))
        }
        "trim" if args.len() == 1 => {
            let object = render_string_expr(object, state)?;
            Some(format!("strings.TrimSpace({object})"))
        }
        "trimStart" if args.len() == 1 => {
            let object = render_string_expr(object, state)?;
            Some(format!("strings.TrimLeftFunc({object}, unicode.IsSpace)"))
        }
        "trimEnd" if args.len() == 1 => {
            let object = render_string_expr(object, state)?;
            Some(format!("strings.TrimRightFunc({object}, unicode.IsSpace)"))
        }
        "slice" if matches!(args.len(), 2 | 3) => {
            let object = render_string_expr(object, state)?;
            let start = render_numeric_expr(args.get(1)?, state)?;
            if let Some(end) = args.get(2) {
                let end = render_numeric_expr(end, state)?;
                return Some(format!("tsgodownStringSlice({object}, {start}, {end})"));
            }
            Some(format!("tsgodownStringSlice({object}, {start})"))
        }
        "substring" if args.len() == 3 => {
            let object = render_string_expr(object, state)?;
            let start = render_numeric_expr(args.get(1)?, state)?;
            let end = render_numeric_expr(args.get(2)?, state)?;
            Some(format!("tsgodownStringSubstring({object}, {start}, {end})"))
        }
        "substr" if matches!(args.len(), 2 | 3) => {
            let object = render_string_expr(object, state)?;
            let start = render_numeric_expr(args.get(1)?, state)?;
            if let Some(length) = args.get(2) {
                let length = render_numeric_expr(length, state)?;
                return Some(format!("tsgodownStringSubstr({object}, {start}, {length})"));
            }
            Some(format!("tsgodownStringSubstr({object}, {start})"))
        }
        "repeat" if args.len() == 2 => {
            let object = render_string_expr(object, state)?;
            let count = render_numeric_expr(args.get(1)?, state)?;
            Some(format!("strings.Repeat({object}, int({count}))"))
        }
        "charAt" if args.len() == 2 => {
            let object = render_string_expr(object, state)?;
            let index = render_numeric_expr(args.get(1)?, state)?;
            Some(format!("tsgodownStringCharAt({object}, {index})"))
        }
        "replace" => render_string_replace_alias_call(args, state),
        "replaceAll" if args.len() == 3 => {
            let object = render_string_expr(object, state)?;
            let needle = render_string_expr(args.get(1)?, state)?;
            let replacement = render_string_expr(args.get(2)?, state)?;
            Some(format!(
                "strings.ReplaceAll({object}, {needle}, {replacement})"
            ))
        }
        _ => None,
    }
}

fn render_string_bool_method_alias_call(
    callee: &JsExpr,
    args: &[JsExpr],
    state: &AotState,
) -> Option<String> {
    let (method, object) = string_method_alias_call_parts(callee, args, state)?;
    match method.as_str() {
        "includes" if args.len() == 2 => {
            let object = render_string_expr(object, state)?;
            let needle = render_string_expr(args.get(1)?, state)?;
            Some(format!("strings.Contains({object}, {needle})"))
        }
        "startsWith" if args.len() == 2 => {
            let object = render_string_expr(object, state)?;
            let needle = render_string_expr(args.get(1)?, state)?;
            Some(format!("strings.HasPrefix({object}, {needle})"))
        }
        "endsWith" if args.len() == 2 => {
            let object = render_string_expr(object, state)?;
            let needle = render_string_expr(args.get(1)?, state)?;
            Some(format!("strings.HasSuffix({object}, {needle})"))
        }
        _ => None,
    }
}

fn render_string_numeric_method_alias_call(
    callee: &JsExpr,
    args: &[JsExpr],
    state: &AotState,
) -> Option<String> {
    let (method, object) = string_method_alias_call_parts(callee, args, state)?;
    match method.as_str() {
        "indexOf" if matches!(args.len(), 2 | 3) => {
            let object = render_string_expr(object, state)?;
            let needle = render_string_expr(args.get(1)?, state)?;
            if let Some(start) = args.get(2) {
                let start = render_numeric_expr(start, state)?;
                return Some(format!(
                    "func() float64 {{ value := {object}; offset := int({start}); if offset < 0 {{ offset = 0 }}; if offset > len(value) {{ offset = len(value) }}; found := strings.Index(value[offset:], {needle}); if found < 0 {{ return -1 }}; return float64(offset + found) }}()"
                ));
            }
            Some(format!("float64(strings.Index({object}, {needle}))"))
        }
        "lastIndexOf" if matches!(args.len(), 2 | 3) => {
            let object = render_string_expr(object, state)?;
            let needle = render_string_expr(args.get(1)?, state)?;
            if let Some(start) = args.get(2) {
                let start = render_numeric_expr(start, state)?;
                return Some(format!(
                    "tsgodownStringLastIndexOf({object}, {needle}, {start})"
                ));
            }
            Some(format!("float64(strings.LastIndex({object}, {needle}))"))
        }
        "charCodeAt" if args.len() == 2 => {
            let object = render_string_expr(object, state)?;
            let index = render_numeric_expr(args.get(1)?, state)?;
            Some(format!("tsgodownStringCharCodeAt({object}, {index})"))
        }
        _ => None,
    }
}

fn string_method_alias_call_parts<'a>(
    callee: &'a JsExpr,
    args: &'a [JsExpr],
    state: &AotState,
) -> Option<(String, &'a JsExpr)> {
    let JsExpr::Member {
        object,
        property,
        property_expr: None,
        optional: false,
    } = callee
    else {
        return None;
    };
    if property != "call" {
        return None;
    }
    let JsExpr::Ident { name } = object.as_ref() else {
        return None;
    };
    let method = state.string_method_aliases.get(name)?.clone();
    let receiver = args.first()?;
    Some((method, receiver))
}

fn is_string_replace_alias_call_shape(callee: &JsExpr, args: &[JsExpr]) -> bool {
    args.len() == 3
        && matches!(
            args.get(1),
            Some(
                JsExpr::Value {
                    value: JsValue::String { .. },
                } | JsExpr::Value {
                    value: JsValue::RegExp { .. },
                } | JsExpr::Ident { .. }
            )
        )
        && matches!(
            callee,
            JsExpr::Member {
                object,
                property,
                property_expr: None,
                optional: false,
            } if property == "call" && matches!(object.as_ref(), JsExpr::Ident { .. })
        )
}

fn string_method_alias_call_uses_regexp(callee: &JsExpr, args: &[JsExpr]) -> bool {
    if !is_string_replace_alias_call_shape(callee, args)
        || !matches!(
            args.get(1),
            Some(JsExpr::Value {
                value: JsValue::RegExp { .. }
            }) | Some(JsExpr::Ident { .. })
        )
    {
        return false;
    }
    true
}

fn render_string_replace_alias_call(args: &[JsExpr], state: &AotState) -> Option<String> {
    if args.len() != 3 {
        return None;
    }
    let object = render_string_expr(args.first()?, state)?;
    let replacement = render_string_expr(args.get(2)?, state)?;
    match args.get(1)? {
        JsExpr::Value {
            value: JsValue::String { value },
        } => Some(format!(
            "strings.Replace({object}, {}, {replacement}, 1)",
            go_string_literal(value)
        )),
        JsExpr::Value {
            value: JsValue::RegExp { .. },
        } => {
            let (pattern, global) = render_supported_regexp_replace_pattern(args.get(1)?)?;
            Some(format!(
                "tsgodownRegexpReplace({object}, {}, {replacement}, {global})",
                go_string_literal(&pattern)
            ))
        }
        JsExpr::Ident { name } => {
            let (_pattern, global) = state.regexp_replace_bindings.get(name)?;
            Some(format!(
                "tsgodownRegexpReplace({object}, {}, {replacement}, {global})",
                go_binding_ref(name, state)
            ))
        }
        _ => None,
    }
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

fn is_regexp_exec_call(callee: &JsExpr, args: &[JsExpr]) -> bool {
    args.len() == 1
        && matches!(
            callee,
            JsExpr::Member {
                object,
                property,
                property_expr: None,
                optional: false,
            } if property == "exec"
                && matches!(
                    object.as_ref(),
                    JsExpr::Ident { .. }
                        | JsExpr::Value {
                            value: JsValue::RegExp { .. },
                        }
                )
        )
}

fn is_value_to_string_call(callee: &JsExpr, args: &[JsExpr]) -> bool {
    args.is_empty()
        && matches!(
            callee,
            JsExpr::Member {
                property,
                property_expr: None,
                optional: false,
                ..
            } if property == "toString"
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
    ) || matches!(
        object.as_ref(),
        JsExpr::Ident { .. } | JsExpr::Member { .. }
    )
}

fn is_regexp_prototype_test_ref(expr: &JsExpr) -> bool {
    matches!(
        expr,
        JsExpr::Member {
            object,
            property,
            property_expr: None,
            optional: false,
        } if property == "test" && is_regexp_prototype_expr(object)
    )
}

fn is_regexp_prototype_expr(expr: &JsExpr) -> bool {
    matches!(
        expr,
        JsExpr::Member {
            object,
            property,
            property_expr: None,
            optional: false,
        } if property == "prototype"
            && matches!(object.as_ref(), JsExpr::Ident { name } if name == "RegExp")
    )
}

fn is_regexp_test_alias_call(callee: &JsExpr, args: &[JsExpr], state: &AotState) -> bool {
    if args.len() != 2 {
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
    property == "call"
        && matches!(object.as_ref(), JsExpr::Ident { name }
        if matches!(
            state.builtin_function_aliases.get(name),
            Some(AotBuiltinFunctionAlias::RegExpTest)
        ))
}

fn is_regexp_test_alias_call_in_context(
    callee: &JsExpr,
    args: &[JsExpr],
    builtin_aliases: &BTreeMap<String, AotBuiltinFunctionAlias>,
) -> bool {
    if args.len() != 2 {
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
    property == "call"
        && matches!(object.as_ref(), JsExpr::Ident { name }
        if matches!(
            builtin_aliases.get(name),
            Some(AotBuiltinFunctionAlias::RegExpTest)
        ))
}

fn render_regexp_pattern_expr(expr: &JsExpr, state: &AotState) -> Option<String> {
    if let Some(pattern) = render_supported_regexp_pattern(expr) {
        return Some(go_string_literal(&pattern));
    }
    match expr {
        JsExpr::Ident { name } if state.regexp_bindings.contains(name) => {
            Some(go_binding_ref(name, state))
        }
        expr if render_string_array_index_expr(expr, state).is_some() => {
            render_string_array_index_expr(expr, state)
        }
        _ => None,
    }
}

fn render_regexp_test_alias_call(
    callee: &JsExpr,
    args: &[JsExpr],
    state: &AotState,
) -> Option<String> {
    if !is_regexp_test_alias_call(callee, args, state) {
        return None;
    }
    let pattern = render_regexp_pattern_expr(args.first()?, state)?;
    let value = render_regexp_test_value_expr(args.get(1)?, state)?;
    Some(format!(
        "regexp.MustCompile({pattern}).MatchString({value})"
    ))
}

fn render_regexp_test_call(callee: &JsExpr, args: &[JsExpr], state: &AotState) -> Option<String> {
    let JsExpr::Member { object, .. } = callee else {
        return None;
    };
    let pattern = render_regexp_pattern_expr(object, state)?;
    let value = render_regexp_test_value_expr(args.first()?, state)?;
    Some(format!(
        "regexp.MustCompile({pattern}).MatchString({value})"
    ))
}

fn render_supported_regexp_pattern(expr: &JsExpr) -> Option<String> {
    let JsExpr::Value {
        value: JsValue::RegExp { pattern, flags },
    } = expr
    else {
        return None;
    };
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
    Some(if prefix.is_empty() {
        pattern.clone()
    } else {
        format!("(?{prefix}){pattern}")
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

fn render_supported_regexp_backref_replace_pattern(expr: &JsExpr) -> Option<(String, bool)> {
    let JsExpr::Value {
        value: JsValue::RegExp { pattern, flags },
    } = expr
    else {
        return None;
    };
    if !pattern.contains("\\1") || !is_supported_anchored_delimiter_backref_pattern(pattern) {
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

fn is_supported_anchored_delimiter_backref_pattern(pattern: &str) -> bool {
    const PREFIX: &str = "^([";
    const SUFFIX: &str = "])([\\s\\S]*)\\1$";
    pattern.starts_with(PREFIX)
        && pattern.ends_with(SUFFIX)
        && pattern.len() > PREFIX.len() + SUFFIX.len()
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
                || is_node_fs_mkdtemp_sync_call(callee, args)
                || is_node_fs_read_file_sync_call(callee, args)
                || is_node_buffer_from_call(callee, args)
                || is_node_buffer_alloc_call(callee, args)
                || is_node_buffer_is_buffer_call(callee, args)
                || is_node_path_bool_call(callee, args)
                || is_node_path_parse_call(callee, args)
                || is_crypto_hash_bytes_digest_call(callee, args)
                || is_crypto_hash_hex_digest_call(callee, args)
                || is_crypto_random_fill_sync_call_shape(callee, args)
                || is_crypto_random_uuid_call_shape(callee, args)
    )
}

fn is_node_path_string_call(callee: &JsExpr, args: &[JsExpr]) -> bool {
    if args.is_empty() {
        return false;
    }
    matches!(callee, JsExpr::Ident { name } if matches!(
        name.as_str(),
        "basename" | "dirname" | "join" | "normalize" | "relative" | "resolve"
    )) || matches!(
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
    let direct_property = if let JsExpr::Ident { name } = callee {
        if !state.builtin_bindings.contains(name) {
            return None;
        }
        Some(name.as_str())
    } else {
        None
    };
    let member_property = if let JsExpr::Member {
        object,
        property,
        property_expr: None,
        optional: false,
    } = callee
    {
        if !state.builtin_bindings.contains("path") || !is_node_path_call_receiver(object) {
            return None;
        }
        Some(property.as_str())
    } else {
        None
    };
    let property = direct_property.or(member_property)?;
    let rendered_args = args
        .iter()
        .map(|arg| render_string_expr(arg, state))
        .collect::<Option<Vec<_>>>()?;
    if direct_property.is_none() && is_node_path_posix_call(callee) {
        return match property {
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
    match property {
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
        && (matches!(callee, JsExpr::Ident { name } if name == "homedir")
            || matches!(
                callee,
                JsExpr::Member {
                    object,
                    property,
                    property_expr: None,
                    optional: false,
                } if matches!(object.as_ref(), JsExpr::Ident { name } if name == "os")
                    && property == "homedir"
            ))
}

fn render_node_os_homedir_call(
    callee: &JsExpr,
    args: &[JsExpr],
    state: &AotState,
) -> Option<String> {
    let binding = match callee {
        JsExpr::Ident { name } => name.as_str(),
        JsExpr::Member { object, .. } => match object.as_ref() {
            JsExpr::Ident { name } => name.as_str(),
            _ => return None,
        },
        _ => return None,
    };
    if state.builtin_bindings.contains(binding) && is_node_os_homedir_call(callee, args) {
        return Some("tsgodownOsHomedir()".to_string());
    }
    None
}

fn is_node_os_tmpdir_call(callee: &JsExpr, args: &[JsExpr]) -> bool {
    args.is_empty()
        && (matches!(callee, JsExpr::Ident { name } if name == "tmpdir")
            || matches!(
                callee,
                JsExpr::Member {
                    object,
                    property,
                    property_expr: None,
                    optional: false,
                } if matches!(object.as_ref(), JsExpr::Ident { name } if name == "os")
                    && property == "tmpdir"
            ))
}

fn render_node_os_tmpdir_call(
    callee: &JsExpr,
    args: &[JsExpr],
    state: &AotState,
) -> Option<String> {
    let binding = match callee {
        JsExpr::Ident { name } => name.as_str(),
        JsExpr::Member { object, .. } => match object.as_ref() {
            JsExpr::Ident { name } => name.as_str(),
            _ => return None,
        },
        _ => return None,
    };
    if state.builtin_bindings.contains(binding) && is_node_os_tmpdir_call(callee, args) {
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
        && (matches!(callee, JsExpr::Ident { name } if name == "mkdtempSync")
            || matches!(
                callee,
                JsExpr::Member {
                    object,
                    property,
                    property_expr: None,
                    optional: false,
                } if matches!(object.as_ref(), JsExpr::Ident { name } if name == "fs")
                    && property == "mkdtempSync"
            ))
}

fn is_node_fs_write_file_sync_call(callee: &JsExpr, args: &[JsExpr]) -> bool {
    matches!(args.len(), 2 | 3)
        && (matches!(callee, JsExpr::Ident { name } if name == "writeFileSync")
            || matches!(
                callee,
                JsExpr::Member {
                    object,
                    property,
                    property_expr: None,
                    optional: false,
                } if matches!(object.as_ref(), JsExpr::Ident { name } if name == "fs")
                    && property == "writeFileSync"
            ))
}

fn is_node_fs_read_file_sync_call(callee: &JsExpr, args: &[JsExpr]) -> bool {
    matches!(args.len(), 1 | 2)
        && (matches!(callee, JsExpr::Ident { name } if name == "readFileSync")
            || matches!(
                callee,
                JsExpr::Member {
                    object,
                    property,
                    property_expr: None,
                    optional: false,
                } if matches!(object.as_ref(), JsExpr::Ident { name } if name == "fs")
                    && property == "readFileSync"
            ))
}

fn is_node_fs_rm_sync_call(callee: &JsExpr, args: &[JsExpr]) -> bool {
    matches!(args.len(), 1 | 2)
        && (matches!(callee, JsExpr::Ident { name } if name == "rmSync")
            || matches!(
                callee,
                JsExpr::Member {
                    object,
                    property,
                    property_expr: None,
                    optional: false,
                } if matches!(object.as_ref(), JsExpr::Ident { name } if name == "fs")
                    && property == "rmSync"
            ))
}

fn node_fs_builtin_receiver(callee: &JsExpr, state: &AotState) -> Option<()> {
    let binding = match callee {
        JsExpr::Ident { name } => name.as_str(),
        JsExpr::Member { object, .. } => match object.as_ref() {
            JsExpr::Ident { name } => name.as_str(),
            _ => return None,
        },
        _ => return None,
    };
    state.builtin_bindings.contains(binding).then_some(())
}

fn node_fs_promises_method<'a>(callee: &JsExpr, state: &'a AotState) -> Option<&'a str> {
    let JsExpr::Ident { name } = callee else {
        return None;
    };
    state.fs_promises_bindings.get(name).map(String::as_str)
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
        .and_then(|expr| render_node_fs_encoding_arg(expr, state))
        .unwrap_or_else(|| "\"utf8\"".to_string());
    Some(format!("tsgodownFsReadFileSync({path}, {encoding})"))
}

fn render_node_fs_promises_read_file_call(
    callee: &JsExpr,
    args: &[JsExpr],
    state: &AotState,
) -> Option<String> {
    if node_fs_promises_method(callee, state)? != "readFile" || !matches!(args.len(), 1 | 2) {
        return None;
    }
    let path = render_string_expr(args.first()?, state)?;
    let encoding = args
        .get(1)
        .and_then(|expr| render_node_fs_encoding_arg(expr, state))
        .unwrap_or_else(|| "\"utf8\"".to_string());
    Some(format!("tsgodownFsReadFileSync({path}, {encoding})"))
}

fn render_node_fs_promises_readdir_call(
    callee: &JsExpr,
    args: &[JsExpr],
    state: &AotState,
) -> Option<String> {
    if node_fs_promises_method(callee, state)? != "readdir" || args.len() != 1 {
        return None;
    }
    let path = render_string_expr(args.first()?, state)?;
    Some(format!("tsgodownFsReaddirSync({path})"))
}

fn render_node_fs_encoding_arg(expr: &JsExpr, state: &AotState) -> Option<String> {
    if let Some(encoding) = render_string_expr(expr, state) {
        return Some(encoding);
    }
    let JsExpr::Object { props } = expr else {
        return None;
    };
    props
        .iter()
        .find(|prop| !prop.spread && prop.key_expr.is_none() && prop.key == "encoding")
        .and_then(|prop| render_string_expr(&prop.value, state))
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

fn render_node_fs_promises_write_file_stmt(
    callee: &JsExpr,
    args: &[JsExpr],
    state: &AotState,
) -> Option<String> {
    if node_fs_promises_method(callee, state)? != "writeFile" || !matches!(args.len(), 2 | 3) {
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
        JsExpr::Ident { name } if is_any_binding(name, state) => {
            let value = go_binding_ref(name, state);
            let encoding = args
                .get(1)
                .and_then(string_literal_value)
                .unwrap_or_else(|| "utf8".to_string());
            Some(format!(
                "tsgodownBufferFromAny({value}, {})",
                go_string_literal(&encoding)
            ))
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

fn crypto_hash_bytes_digest_source_expr<'a>(
    callee: &'a JsExpr,
    args: &[JsExpr],
) -> Option<(&'static str, &'a JsExpr, &'a JsExpr)> {
    if !args.is_empty() {
        return None;
    }
    let JsExpr::Member {
        object: update_call,
        property,
        property_expr: None,
        optional: false,
    } = callee
    else {
        return None;
    };
    if property != "digest" {
        return None;
    }
    let JsExpr::Call {
        callee: update_callee,
        args: update_args,
        optional: false,
    } = update_call.as_ref()
    else {
        return None;
    };
    let JsExpr::Member {
        object: create_hash_call,
        property,
        property_expr: None,
        optional: false,
    } = update_callee.as_ref()
    else {
        return None;
    };
    if property != "update" || update_args.len() != 1 {
        return None;
    }
    let JsExpr::Call {
        callee: create_hash_callee,
        args: create_hash_args,
        optional: false,
    } = create_hash_call.as_ref()
    else {
        return None;
    };
    if create_hash_args.len() != 1 {
        return None;
    }
    let algorithm = match create_hash_args
        .first()
        .and_then(string_literal_value)?
        .as_str()
    {
        "md5" => "md5",
        "sha1" => "sha1",
        _ => return None,
    };
    Some((algorithm, create_hash_callee, update_args.first()?))
}

fn crypto_hash_bytes_digest_algorithm(callee: &JsExpr, args: &[JsExpr]) -> Option<&'static str> {
    let (algorithm, _, _) = crypto_hash_bytes_digest_source_expr(callee, args)?;
    Some(algorithm)
}

fn crypto_hash_hex_digest_source_expr<'a>(
    callee: &'a JsExpr,
    args: &[JsExpr],
) -> Option<(&'static str, &'a JsExpr, &'a JsExpr)> {
    if args.len() != 1
        || !matches!(
            args.first().and_then(string_literal_value).as_deref(),
            Some("hex")
        )
    {
        return None;
    }
    let JsExpr::Member {
        object: update_call,
        property,
        property_expr: None,
        optional: false,
    } = callee
    else {
        return None;
    };
    if property != "digest" {
        return None;
    }
    let JsExpr::Call {
        callee: update_callee,
        args: update_args,
        optional: false,
    } = update_call.as_ref()
    else {
        return None;
    };
    let JsExpr::Member {
        object: create_hash_call,
        property,
        property_expr: None,
        optional: false,
    } = update_callee.as_ref()
    else {
        return None;
    };
    if property != "update" || update_args.len() != 1 {
        return None;
    }
    let JsExpr::Call {
        callee: create_hash_callee,
        args: create_hash_args,
        optional: false,
    } = create_hash_call.as_ref()
    else {
        return None;
    };
    if create_hash_args.len() != 1 {
        return None;
    }
    let algorithm = match create_hash_args
        .first()
        .and_then(string_literal_value)?
        .as_str()
    {
        "md5" => "md5",
        "sha1" => "sha1",
        "sha256" => "sha256",
        _ => return None,
    };
    Some((algorithm, create_hash_callee, update_args.first()?))
}

fn crypto_hash_hex_digest_algorithm(callee: &JsExpr, args: &[JsExpr]) -> Option<&'static str> {
    let (algorithm, _, _) = crypto_hash_hex_digest_source_expr(callee, args)?;
    Some(algorithm)
}

fn is_crypto_hash_bytes_digest_call(callee: &JsExpr, args: &[JsExpr]) -> bool {
    crypto_hash_bytes_digest_source_expr(callee, args).is_some()
}

fn is_crypto_hash_hex_digest_call(callee: &JsExpr, args: &[JsExpr]) -> bool {
    crypto_hash_hex_digest_source_expr(callee, args).is_some()
}

fn render_crypto_hash_bytes_digest_call(
    callee: &JsExpr,
    args: &[JsExpr],
    state: &AotState,
) -> Option<String> {
    let (algorithm, create_hash_callee, source) =
        crypto_hash_bytes_digest_source_expr(callee, args)?;
    if !is_crypto_create_hash_ref(create_hash_callee, state) {
        return None;
    }
    let source = render_bytes_expr(source, state)
        .or_else(|| {
            if let JsExpr::Ident { name } = source {
                if is_any_binding(name, state) {
                    return Some(format!(
                        "tsgodownBufferFromAny({}, \"utf8\")",
                        go_binding_ref(name, state)
                    ));
                }
            }
            None
        })
        .or_else(|| render_string_expr(source, state).map(|value| format!("[]byte({value})")))?;
    let package = match algorithm {
        "md5" => "tsgodownmd5",
        "sha1" => "tsgodownsha1",
        _ => return None,
    };
    Some(format!(
        "func() []byte {{ sum := {package}.Sum({source}); return sum[:] }}()"
    ))
}

fn render_crypto_hash_hex_digest_call(
    callee: &JsExpr,
    args: &[JsExpr],
    state: &AotState,
) -> Option<String> {
    let (algorithm, create_hash_callee, source) = crypto_hash_hex_digest_source_expr(callee, args)?;
    if !is_crypto_create_hash_ref(create_hash_callee, state) {
        return None;
    }
    let source = render_bytes_expr(source, state)
        .or_else(|| {
            if let JsExpr::Ident { name } = source {
                if is_any_binding(name, state) {
                    return Some(format!(
                        "tsgodownBufferFromAny({}, \"utf8\")",
                        go_binding_ref(name, state)
                    ));
                }
            }
            None
        })
        .or_else(|| render_string_expr(source, state).map(|value| format!("[]byte({value})")))?;
    let package = match algorithm {
        "md5" => "tsgodownmd5",
        "sha1" => "tsgodownsha1",
        "sha256" => "tsgodownsha256",
        _ => return None,
    };
    Some(format!(
        "func() string {{ sum := {package}.Sum({source}); return hex.EncodeToString(sum[:]) }}()"
    ))
}

fn is_crypto_create_hash_ref(expr: &JsExpr, state: &AotState) -> bool {
    match expr {
        JsExpr::Ident { name } => name == "createHash" && state.builtin_bindings.contains(name),
        JsExpr::Member {
            object,
            property,
            property_expr: None,
            optional: false,
        } if property == "createHash" => {
            matches!(object.as_ref(), JsExpr::Ident { name } if name == "crypto" && state.builtin_bindings.contains(name))
        }
        _ => false,
    }
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
    if let Some(object) = string_method_receiver(callee, "lastIndexOf", args, state) {
        let object = render_string_expr(object, state)?;
        let needle = render_string_expr(args.first()?, state)?;
        if let Some(start) = args.get(1) {
            let start = render_numeric_expr(start, state)?;
            return Some(format!(
                "tsgodownStringLastIndexOf({object}, {needle}, {start})"
            ));
        }
        return Some(format!("float64(strings.LastIndex({object}, {needle}))"));
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

fn is_number_cast_call(callee: &JsExpr, args: &[JsExpr]) -> bool {
    args.len() == 1 && matches!(callee, JsExpr::Ident { name } if name == "Number")
}

fn is_number_is_integer_call(callee: &JsExpr, args: &[JsExpr]) -> bool {
    args.len() == 1
        && matches!(
            callee,
            JsExpr::Member {
                object,
                property,
                property_expr: None,
                optional: false,
            } if matches!(object.as_ref(), JsExpr::Ident { name } if name == "Number")
                && property == "isInteger"
        )
}

fn is_number_is_finite_call(callee: &JsExpr, args: &[JsExpr]) -> bool {
    args.len() == 1
        && matches!(
            callee,
            JsExpr::Member {
                object,
                property,
                property_expr: None,
                optional: false,
            } if matches!(object.as_ref(), JsExpr::Ident { name } if name == "Number")
                && property == "isFinite"
        )
}

fn is_number_is_safe_integer_call(callee: &JsExpr, args: &[JsExpr]) -> bool {
    args.len() == 1
        && matches!(
            callee,
            JsExpr::Member {
                object,
                property,
                property_expr: None,
                optional: false,
            } if matches!(object.as_ref(), JsExpr::Ident { name } if name == "Number")
                && property == "isSafeInteger"
        )
}

fn is_uri_string_call(callee: &JsExpr, args: &[JsExpr]) -> bool {
    args.len() == 1
        && matches!(
            callee,
            JsExpr::Ident { name } if matches!(name.as_str(), "encodeURIComponent" | "unescape")
        )
}

fn render_uri_string_call(callee: &JsExpr, args: &[JsExpr], state: &AotState) -> Option<String> {
    if !is_uri_string_call(callee, args) {
        return None;
    }
    let JsExpr::Ident { name } = callee else {
        return None;
    };
    let value = render_string_expr(args.first()?, state)?;
    match name.as_str() {
        "encodeURIComponent" => Some(format!("tsgodownEncodeURIComponent({value})")),
        "unescape" => Some(format!("tsgodownUnescape({value})")),
        _ => None,
    }
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

fn is_global_is_finite_call(callee: &JsExpr, args: &[JsExpr]) -> bool {
    args.len() == 1 && matches!(callee, JsExpr::Ident { name } if name == "isFinite")
}

fn is_parse_int_call(callee: &JsExpr, args: &[JsExpr]) -> bool {
    matches!(args.len(), 1 | 2) && matches!(callee, JsExpr::Ident { name } if name == "parseInt")
}

fn is_parse_float_call(callee: &JsExpr, args: &[JsExpr]) -> bool {
    args.len() == 1 && matches!(callee, JsExpr::Ident { name } if name == "parseFloat")
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

fn render_string_function_expr(expr: &JsExpr, state: &AotState) -> Option<String> {
    if is_process_cwd_ref(expr) {
        return Some("tsgodownProcessCwd".to_string());
    }
    if is_crypto_random_uuid_ref(expr, state) {
        return Some("tsgodownCryptoRandomUUID".to_string());
    }
    match expr {
        JsExpr::Function {
            params,
            rest_param: None,
            r#async: false,
            generator: false,
            body,
            ..
        } if params.is_empty() => {
            let value = render_string_expr(single_return_expr(body)?, state)?;
            Some(format!("func() string {{ return {value} }}"))
        }
        JsExpr::Member {
            object,
            property,
            property_expr: None,
            optional: false,
        } if static_member_kind(object, property, state) == Some(AotSlotKind::StringFunction) => {
            render_static_member_expr(object, property, state)
        }
        _ => None,
    }
}

fn render_any_function_expr(expr: &JsExpr, state: &AotState) -> Option<String> {
    match expr {
        JsExpr::Function { .. } => render_variadic_any_function_expr(expr, state),
        JsExpr::Conditional {
            test,
            consequent,
            alternate,
        } => {
            let test = render_bool_expr(test, state)?;
            let consequent = render_variadic_any_function_expr(consequent, state)?;
            let alternate = render_variadic_any_function_expr(alternate, state)?;
            Some(format!(
                "func() any {{ if {test} {{ return {consequent} }}; return {alternate} }}()"
            ))
        }
        _ => None,
    }
}

fn render_variadic_any_function_expr(expr: &JsExpr, state: &AotState) -> Option<String> {
    let JsExpr::Function {
        params,
        rest_param,
        r#async: false,
        generator: false,
        body,
        ..
    } = expr
    else {
        return None;
    };
    if !params.is_empty() {
        return None;
    }
    if body.is_empty() {
        return Some("func(args ...any) any { return nil }".to_string());
    }
    let rest_param = rest_param.as_deref()?;
    let [JsStmt::Return { value: Some(value) }] = body.as_slice() else {
        return None;
    };
    let console = render_variadic_console_call(value, rest_param, state)?;
    Some(format!(
        "func({rest_param} ...any) any {{ {console}; return nil }}"
    ))
}

fn render_variadic_console_call(
    expr: &JsExpr,
    rest_param: &str,
    state: &AotState,
) -> Option<String> {
    let JsExpr::Call { callee, args, .. } = expr else {
        return None;
    };
    let is_error = is_console_error(callee);
    if !is_error && !is_console_log(callee) {
        return None;
    }
    let mut fixed = Vec::new();
    let mut saw_rest = false;
    for arg in args {
        match arg {
            JsExpr::Spread { arg } if matches!(arg.as_ref(), JsExpr::Ident { name } if name == rest_param) =>
            {
                saw_rest = true;
            }
            _ => fixed.push(render_console_arg_expr(arg, state)?),
        }
    }
    if !saw_rest {
        return None;
    }
    let prefix = if fixed.is_empty() {
        "[]any{}".to_string()
    } else {
        format!("[]any{{{}}}", fixed.join(", "))
    };
    if is_error {
        Some(format!(
            "fmt.Fprintln(os.Stderr, append({prefix}, {rest_param}...)...)"
        ))
    } else {
        Some(format!("fmt.Println(append({prefix}, {rest_param}...)...)"))
    }
}

fn is_crypto_random_uuid_ref(expr: &JsExpr, state: &AotState) -> bool {
    matches!(expr, JsExpr::Ident { name } if name == "randomUUID" && state.builtin_bindings.contains(name))
        || matches!(
            expr,
            JsExpr::Member {
                object,
                property,
                property_expr: None,
                optional: false,
            } if property == "randomUUID"
                && matches!(object.as_ref(), JsExpr::Ident { name } if name == "crypto" && state.builtin_bindings.contains(name))
        )
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

fn render_optional_call_expr(callee: &JsExpr, args: &[JsExpr], state: &AotState) -> Option<String> {
    let JsExpr::Member {
        object,
        property,
        property_expr,
        optional: false,
    } = callee
    else {
        return None;
    };
    let object = render_expr(object, state)?;
    let key = render_dynamic_object_property_key_expr(property, property_expr.as_deref(), state)?;
    let args = args
        .iter()
        .map(|arg| render_expr(arg, state))
        .collect::<Option<Vec<_>>>()?;
    if args.is_empty() {
        return Some(format!("tsgodownOptionalCallMember({object}, {key})"));
    }
    Some(format!(
        "tsgodownOptionalCallMember({object}, {key}, {})",
        args.join(", ")
    ))
}

fn render_call_expr(callee: &JsExpr, args: &[JsExpr], state: &AotState) -> Option<String> {
    if is_array_is_array_call(callee, args) || is_array_is_array_alias_call(callee, args, state) {
        return render_array_is_array_call(args, state);
    }
    if is_object_has_own_property_call(callee, args, state) {
        return render_object_has_own_property_call(callee, args, state);
    }
    if let Some(value) = render_regexp_test_alias_call(callee, args, state) {
        return Some(value);
    }
    if let Some(value) = render_string_bool_method_alias_call(callee, args, state) {
        return Some(value);
    }
    if let Some(value) = render_string_numeric_method_alias_call(callee, args, state) {
        return Some(value);
    }
    if let Some(value) = render_string_method_alias_call(callee, args, state) {
        return Some(value);
    }
    if let Some(value) = render_array_prototype_join_alias_call(callee, args, state) {
        return Some(value);
    }
    if let Some(value) = render_string_array_join_call(callee, args, state) {
        return Some(value);
    }
    if let Some(value) = render_string_split_call(callee, args, state) {
        return Some(value);
    }
    if let Some(value) = render_any_array_prototype_concat_alias_call(callee, args, state) {
        return Some(value);
    }
    if let Some(value) = render_any_array_prototype_slice_alias_call(callee, args, state) {
        return Some(value);
    }
    if is_object_to_string_alias_call(callee, args, state) {
        return render_object_prototype_to_string_call(args.first()?, state);
    }
    if let Some(value) = render_any_array_push_call_expr(callee, args, state) {
        return Some(value);
    }
    if let Some(value) = render_any_array_fill_call(callee, args, state) {
        return Some(value);
    }
    if let Some(value) = render_event_emitter_call_expr(callee, args, state) {
        return Some(value);
    }
    if let Some(value) = render_map_call_expr(callee, args, state) {
        return Some(value);
    }
    if let Some(value) = render_set_call_expr(callee, args, state) {
        return Some(value);
    }
    if let Some(value) = render_array_at_call(callee, args, state) {
        return Some(value);
    }
    if let Some(value) = render_array_find_call(callee, args, state) {
        return Some(value);
    }
    if let Some(value) = render_array_reduce_call(callee, args, state) {
        return Some(value);
    }
    if is_json_stringify(callee) {
        return render_json_stringify_call(args, state);
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
    if is_crypto_random_uuid_call(callee, args, state) {
        return Some("tsgodownCryptoRandomUUID()".to_string());
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
        if args.is_empty()
            && static_member_kind(object, property, state) == Some(AotSlotKind::BoolFunction)
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
    let rendered_args = render_call_args_for_function(args, function, state)?;
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

fn render_json_stringify_call(args: &[JsExpr], state: &AotState) -> Option<String> {
    let value_expr = args.first()?;
    let value = render_json_value_expr(value_expr, state)?;
    if let Some(space) = args.get(2).and_then(render_json_stringify_space_expr) {
        return Some(format!("tsgodownJSONStringifyIndent({value}, {space})"));
    }
    if let JsExpr::Ident { name } = value_expr {
        if state.ordered_dynamic_object_bindings.contains(name) {
            let object = render_dynamic_object_source_expr(value_expr, state)
                .or_else(|| render_object_map_expr(value_expr, state))?;
            let order = dynamic_object_order_ref(name, state);
            return Some(format!("tsgodownJSONStringifyOrdered({object}, {order})"));
        }
    }
    Some(format!("tsgodownJSONStringify({value})"))
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
    let mut rendered = Vec::new();
    for (index, kind) in param_kinds.iter().enumerate() {
        if let Some(arg) = args.get(index) {
            rendered.push(render_arg_for_kind(arg, *kind, state)?);
        } else if *kind == AotSlotKind::Any {
            rendered.push("nil".to_string());
        } else if *kind == AotSlotKind::Number {
            rendered.push("0".to_string());
        } else {
            return None;
        }
    }
    Some(rendered)
}

fn render_call_args_for_function(
    args: &[JsExpr],
    function: &AotFunction,
    state: &AotState,
) -> Option<Vec<String>> {
    if function.rest_param.is_none() {
        return render_call_args(args, &function.param_kinds, state);
    }
    if args.len() < function.params.len() {
        return None;
    }
    let mut rendered =
        render_call_args(&args[..function.params.len()], &function.param_kinds, state)?;
    let mut spread_rest = "[]any{}".to_string();
    let mut saw_spread = false;
    let mut fixed_rest = Vec::new();
    for arg in args.iter().skip(function.params.len()) {
        match arg {
            JsExpr::Spread { arg } => {
                saw_spread = true;
                let value = render_any_array_from_any_expr(arg, state)?;
                spread_rest = format!("append({spread_rest}, {value}...)");
            }
            _ => {
                let value =
                    render_json_value_expr(arg, state).or_else(|| render_expr(arg, state))?;
                if saw_spread {
                    spread_rest = format!("append({spread_rest}, {value})");
                } else {
                    fixed_rest.push(value);
                }
            }
        }
    }
    if saw_spread {
        if !fixed_rest.is_empty() {
            spread_rest = format!(
                "append([]any{{{}}}, {spread_rest}...)",
                fixed_rest.join(", ")
            );
        }
        rendered.push(format!("{spread_rest}..."));
    } else {
        rendered.extend(fixed_rest);
    }
    Some(rendered)
}

fn render_arg_for_kind(expr: &JsExpr, kind: AotSlotKind, state: &AotState) -> Option<String> {
    match kind {
        AotSlotKind::Any => {
            render_json_value_expr(expr, state).or_else(|| render_expr(expr, state))
        }
        AotSlotKind::AnyArray => render_any_array_coerced_expr(expr, state),
        AotSlotKind::Bool => render_bool_expr(expr, state),
        AotSlotKind::Bytes => render_bytes_expr_with_any_cast(expr, state),
        AotSlotKind::Date => render_date_expr(expr, state),
        AotSlotKind::Number => render_numeric_expr(expr, state),
        AotSlotKind::NumberArray => render_number_array_expr(expr, state),
        AotSlotKind::RegExp => render_regexp_expr(expr, state),
        AotSlotKind::String => render_string_expr(expr, state),
        AotSlotKind::StringArray => render_string_array_expr(expr, state),
        AotSlotKind::BoolFunction => render_bool_function_expr(expr, state),
        AotSlotKind::StringFunction => render_string_function_expr(expr, state),
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
    if let Some(arg) = object_freeze_arg(expr) {
        return render_json_value_expr(arg, state);
    }
    if matches!(
        expr,
        JsExpr::Call { callee, args, .. }
            if is_object_create_null_call(callee, args) || is_object_assign_call(callee, args)
    ) {
        return render_object_map_expr(expr, state).map(|value| format!("any({value})"));
    }
    match expr {
        JsExpr::Value { value } => render_value(value),
        JsExpr::Ident { name } if name == "undefined" => Some("nil".to_string()),
        JsExpr::Function { .. } => render_inline_function_value_expr(expr, state),
        JsExpr::Ident { name } if state.functions.contains_key(name) => {
            render_function_reference_expr(name, state)
        }
        JsExpr::Ident { name } if state.bindings.contains(name) => {
            Some(go_binding_ref(name, state))
        }
        expr if render_any_array_index_expr(expr, state).is_some() => {
            render_any_array_index_expr(expr, state)
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
        JsExpr::Call { callee, args, .. } if is_array_map_call(callee, args) => {
            render_any_array_map_call(callee, args, state)
        }
        JsExpr::Array { items } => {
            let items = items
                .iter()
                .map(|item| render_json_value_expr(item, state))
                .collect::<Option<Vec<_>>>()?;
            Some(format!("[]any{{{}}}", items.join(", ")))
        }
        JsExpr::ArraySpread { items } if items.iter().all(|item| !item.spread) => {
            let items = items
                .iter()
                .map(|item| render_json_value_expr(&item.value, state))
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
        JsExpr::Binary { op, left, right } if op == "??" => {
            let left = render_json_value_expr(left, state).or_else(|| render_expr(left, state))?;
            let right =
                render_json_value_expr(right, state).or_else(|| render_expr(right, state))?;
            Some(format!("tsgodownNullish({left}, {right})"))
        }
        JsExpr::Binary { op, .. } if op == "+" => render_expr(expr, state),
        JsExpr::Binary { op, .. } if op == "instanceof" => render_bool_expr(expr, state),
        JsExpr::Binary { op, .. } if go_comparison_op(op).is_some() => {
            render_bool_expr(expr, state)
        }
        JsExpr::Binary { op, .. } if matches!(op.as_str(), "&&" | "||") => {
            render_bool_expr(expr, state).or_else(|| render_logical_value_expr(expr, state))
        }
        JsExpr::Unary { .. } => render_expr(expr, state),
        JsExpr::Call { callee, args, .. } if is_json_parse_call(callee, args) => {
            render_json_parse_call(callee, args, state)
        }
        JsExpr::Call { callee, args, .. } if is_querystring_parse_call(callee, args) => {
            render_querystring_parse_call(callee, args, state)
        }
        JsExpr::Call { callee, args, .. } if is_local_function_call(callee, state) => {
            render_call_expr(callee, args, state)
        }
        JsExpr::Call { callee, args, .. }
            if render_numeric_expr(expr, state)
                .or_else(|| render_string_expr(expr, state))
                .or_else(|| render_node_fs_mkdtemp_sync_call(callee, args, state))
                .is_some() =>
        {
            render_numeric_expr(expr, state)
                .or_else(|| render_string_expr(expr, state))
                .or_else(|| render_node_fs_mkdtemp_sync_call(callee, args, state))
        }
        expr if is_process_version_expr(expr) => render_process_version_expr(expr),
        expr if is_process_platform_expr(expr) => Some("tsgodownProcessPlatform()".to_string()),
        expr if is_process_argv_ref(expr) => render_process_argv_expr(state),
        expr if is_process_env_ref(expr) => Some("tsgodownProcessEnv()".to_string()),
        expr if is_process_versions_ref(expr) => Some(render_process_versions_expr()),
        expr if is_process_cwd_ref(expr) => render_string_function_expr(expr, state),
        JsExpr::Call { callee, args, .. } => {
            render_bool_expr(expr, state).or_else(|| render_call_expr(callee, args, state))
        }
        JsExpr::Member {
            object,
            property,
            property_expr,
            optional: true,
        } => {
            let object = render_expr(object, state)?;
            let key =
                render_dynamic_object_property_key_expr(property, property_expr.as_deref(), state)?;
            Some(format!("tsgodownObjectProp({object}, {key})"))
        }
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
        JsExpr::Member {
            object,
            property,
            property_expr: Some(property_expr),
            optional: false,
        } => render_dynamic_object_member_access_expr(object, property, Some(property_expr), state)
            .or_else(|| render_numeric_expr(expr, state)),
        _ => None,
    }
}

fn render_logical_value_expr(expr: &JsExpr, state: &AotState) -> Option<String> {
    let JsExpr::Binary { op, left, right } = expr else {
        return None;
    };
    let left_value = render_json_value_expr(left, state).or_else(|| render_expr(left, state))?;
    let mut right_state = clone_aot_state(state);
    if op == "&&" {
        if let JsExpr::Ident { name } = left.as_ref() {
            if is_any_binding(name, state) {
                right_state.any_array_bindings.insert(name.clone());
                right_state.narrowed_any_array_bindings.insert(name.clone());
            }
        }
    }
    let right =
        render_json_value_expr(right, &right_state).or_else(|| render_expr(right, &right_state))?;
    match op.as_str() {
        "&&" => Some(format!(
            "func() any {{ left := any({left_value}); if !tsgodownToBool(left) {{ return left }}; return any({right}) }}()"
        )),
        "||" => Some(format!(
            "func() any {{ left := any({left_value}); if tsgodownToBool(left) {{ return left }}; return any({right}) }}()"
        )),
        _ => None,
    }
}

fn is_local_function_call(callee: &JsExpr, state: &AotState) -> bool {
    matches!(callee, JsExpr::Ident { name } if state.functions.contains_key(name) && !is_any_binding(name, state))
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
        AotSlotKind::AnyArray => format!("tsgodownAnyArrayFromAny({name})"),
        AotSlotKind::Bool => format!("tsgodownToBool({name})"),
        AotSlotKind::Bytes => format!("{name}.([]byte)"),
        AotSlotKind::Date => format!("tsgodownToString({name})"),
        AotSlotKind::Number => format!("tsgodownToFloat64({name})"),
        AotSlotKind::NumberArray => format!("{name}.([]float64)"),
        AotSlotKind::RegExp => format!("tsgodownToString({name})"),
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
        && !state.date_bindings.contains(name)
        && !state.any_array_bindings.contains(name)
        && !state.number_array_bindings.contains(name)
        && !state.string_array_bindings.contains(name)
        && !state.map_bindings.contains(name)
        && !state.set_bindings.contains(name)
        && !state.url_bindings.contains(name)
        && !state.event_emitter_bindings.contains(name)
        && !state.number_closure_bindings.contains(name)
        && !state.string_function_bindings.contains(name)
        && !state.object_bindings.contains_key(name)
        && !state.class_instance_bindings.contains_key(name)
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
    let normalized = value.replace('_', "");
    if normalized.parse::<f64>().is_ok() {
        return Some(value.to_string());
    }
    let digits = normalized
        .strip_prefix(['+', '-'])
        .unwrap_or(normalized.as_str());
    let valid_radix = digits
        .strip_prefix("0x")
        .or_else(|| digits.strip_prefix("0X"))
        .is_some_and(|rest| !rest.is_empty() && rest.chars().all(|ch| ch.is_ascii_hexdigit()))
        || digits
            .strip_prefix("0o")
            .or_else(|| digits.strip_prefix("0O"))
            .is_some_and(|rest| !rest.is_empty() && rest.chars().all(|ch| matches!(ch, '0'..='7')))
        || digits
            .strip_prefix("0b")
            .or_else(|| digits.strip_prefix("0B"))
            .is_some_and(|rest| !rest.is_empty() && rest.chars().all(|ch| matches!(ch, '0' | '1')));
    valid_radix.then(|| value.to_string())
}

fn is_numeric_binary_op(op: &str) -> bool {
    matches!(op, "+" | "-" | "*" | "/" | "%")
}

fn is_bitwise_binary_op(op: &str) -> bool {
    matches!(op, ">>>" | ">>" | "<<" | "&" | "|" | "^")
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
