use std::collections::{BTreeMap, BTreeSet};

use crate::contract::{AnalyzeResponse, IrDocument, JsExpr, JsStmt, JsValue, Module};
use crate::emit_go::{go_string_literal, sanitize_go_identifier};

pub(crate) fn render_aot_executable_program(
    package_name: &str,
    analyzed: &AnalyzeResponse,
) -> Option<String> {
    let module = entry_module(&analyzed.ir)?;
    if !can_aot_module_graph(&analyzed.ir) {
        return None;
    }
    let module_functions = collect_module_functions(&analyzed.ir);
    let declarations = render_module_function_decls(&analyzed.ir, &module_functions)?;
    let mut state = module_aot_state(module, &analyzed.ir, &module_functions)?;
    let mut body = Vec::new();
    for stmt in &module.executable.as_ref()?.stmts {
        if let JsStmt::FunctionDecl { .. } = stmt {
            continue;
        }
        body.push(render_stmt(stmt, &mut state)?);
    }
    Some(format!(
        r#"package {package_name}

import "fmt"

{declarations}
func main() {{
{body}
}}
"#,
        declarations = declarations.join("\n\n"),
        body = indent_lines(&body.join("\n"))
    ))
}

fn can_aot_module_graph(ir: &IrDocument) -> bool {
    ir.modules.iter().all(|module| {
        module.executable.is_some()
            && module
                .imports
                .iter()
                .all(|import| import.kind == "esm" && import.resolved.is_some())
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
            if let JsStmt::FunctionDecl { name, params, .. } = stmt {
                let go_name = if module.id == entry.id {
                    sanitize_go_identifier(name)
                } else {
                    format!(
                        "{}_{}",
                        module_go_prefix(module),
                        sanitize_go_identifier(name)
                    )
                };
                functions.insert(
                    (module.id.clone(), name.clone()),
                    AotFunction {
                        params: params.clone(),
                        go_name,
                    },
                );
            }
        }
    }
    functions
}

fn render_module_function_decls(
    ir: &IrDocument,
    module_functions: &BTreeMap<(String, String), AotFunction>,
) -> Option<Vec<String>> {
    let mut declarations = Vec::new();
    for module in &ir.modules {
        let state = module_aot_state(module, ir, module_functions)?;
        for stmt in &module.executable.as_ref()?.stmts {
            if let JsStmt::FunctionDecl { name, .. } = stmt {
                let function = module_functions.get(&(module.id.clone(), name.clone()))?;
                declarations.push(render_function_decl(stmt, &state, &function.go_name)?);
            }
        }
    }
    Some(declarations)
}

fn module_aot_state(
    module: &Module,
    ir: &IrDocument,
    module_functions: &BTreeMap<(String, String), AotFunction>,
) -> Option<AotState> {
    let mut state = AotState::default();
    for stmt in &module.executable.as_ref()?.stmts {
        if let JsStmt::FunctionDecl { name, .. } = stmt {
            let function = module_functions.get(&(module.id.clone(), name.clone()))?;
            state.functions.insert(name.clone(), function.clone());
        }
    }
    for import in &module.imports {
        let resolved = import.resolved.as_ref()?;
        let imported_module = ir
            .modules
            .iter()
            .find(|candidate| &candidate.id == resolved)?;
        for binding in &import.bindings {
            let imported = binding.imported.as_deref().unwrap_or(&binding.local);
            let function =
                module_functions.get(&(imported_module.id.clone(), imported.to_string()))?;
            state
                .functions
                .insert(binding.local.clone(), function.clone());
        }
    }
    Some(state)
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

#[derive(Default)]
struct AotState {
    bindings: BTreeSet<String>,
    numeric_bindings: BTreeSet<String>,
    string_bindings: BTreeSet<String>,
    bool_bindings: BTreeSet<String>,
    object_bindings: BTreeMap<String, AotObject>,
    functions: BTreeMap<String, AotFunction>,
}

#[derive(Clone)]
struct AotFunction {
    params: Vec<String>,
    go_name: String,
}

#[derive(Clone)]
struct AotObject {
    fields: BTreeMap<String, AotSlotKind>,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum AotSlotKind {
    Bool,
    Number,
    String,
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
                if let Some(value) = render_numeric_expr(expr, state) {
                    state.bindings.insert(name.clone());
                    state.numeric_bindings.insert(name.clone());
                    return Some(format!("var {ident} float64 = {value}"));
                }
                if let Some(value) = render_string_expr(expr, state) {
                    state.bindings.insert(name.clone());
                    state.string_bindings.insert(name.clone());
                    return Some(format!("var {ident} string = {value}"));
                }
                if let Some(value) = render_bool_expr(expr, state) {
                    state.bindings.insert(name.clone());
                    state.bool_bindings.insert(name.clone());
                    return Some(format!("var {ident} bool = {value}"));
                }
                if let Some((value, object)) = render_object_literal(expr, state) {
                    state.bindings.insert(name.clone());
                    state.object_bindings.insert(name.clone(), object);
                    return Some(format!("var {ident} = {value}"));
                }
                let value = render_expr(expr, state)?;
                state.bindings.insert(name.clone());
                return Some(format!("var {ident} any = {value}"));
            }
            state.bindings.insert(name.clone());
            Some(format!("var {ident} any = nil"))
        }
        JsStmt::Expr { expr } => render_expr_stmt(expr, state),
        JsStmt::If {
            test,
            consequent,
            alternate,
        } => {
            let test = render_bool_expr(test, state)?;
            let consequent = indent_lines(&render_stmt_block(consequent, state)?);
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
        bindings: state.bindings.clone(),
        numeric_bindings: state.numeric_bindings.clone(),
        string_bindings: state.string_bindings.clone(),
        bool_bindings: state.bool_bindings.clone(),
        object_bindings: state.object_bindings.clone(),
        functions: state.functions.clone(),
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
        bindings: state.bindings.clone(),
        numeric_bindings: state.numeric_bindings.clone(),
        string_bindings: state.string_bindings.clone(),
        bool_bindings: state.bool_bindings.clone(),
        object_bindings: state.object_bindings.clone(),
        functions: state.functions.clone(),
    };
    render_stmt_block_with_state(stmts, &block_state)
}

fn render_stmt_block_with_state(stmts: &[JsStmt], state: &AotState) -> Option<String> {
    let mut block_state = AotState {
        bindings: state.bindings.clone(),
        numeric_bindings: state.numeric_bindings.clone(),
        string_bindings: state.string_bindings.clone(),
        bool_bindings: state.bool_bindings.clone(),
        object_bindings: state.object_bindings.clone(),
        functions: state.functions.clone(),
    };
    stmts
        .iter()
        .map(|stmt| render_stmt(stmt, &mut block_state))
        .collect::<Option<Vec<_>>>()
        .map(|stmts| stmts.join("\n"))
}

fn render_function_decl(stmt: &JsStmt, state: &AotState, go_name: &str) -> Option<String> {
    let JsStmt::FunctionDecl {
        name: _,
        params,
        rest_param,
        r#async,
        generator,
        body,
    } = stmt
    else {
        return None;
    };
    if rest_param.is_some() || *r#async || *generator {
        return None;
    }
    let mut function_state = AotState {
        functions: state.functions.clone(),
        ..AotState::default()
    };
    for param in params {
        function_state.numeric_bindings.insert(param.clone());
    }
    let rendered_params = params
        .iter()
        .map(|param| format!("{} float64", sanitize_go_identifier(param)))
        .collect::<Vec<_>>()
        .join(", ");
    let function_body = render_function_body(body, &function_state)?;
    Some(format!(
        "func {go_name}({rendered_params}) any {{\n{}\n}}",
        indent_lines(&function_body)
    ))
}

fn render_function_body(body: &[JsStmt], state: &AotState) -> Option<String> {
    body.iter()
        .map(|stmt| render_function_stmt(stmt, state))
        .collect::<Option<Vec<_>>>()
        .map(|stmts| stmts.join("\n"))
}

fn render_function_stmt(stmt: &JsStmt, state: &AotState) -> Option<String> {
    match stmt {
        JsStmt::Return { value: Some(value) } => {
            let returned = render_expr(value, state)?;
            Some(format!("return {returned}"))
        }
        JsStmt::If {
            test,
            consequent,
            alternate,
        } => {
            let test = render_bool_expr(test, state)?;
            let consequent = indent_lines(&render_function_body(consequent, state)?);
            if alternate.is_empty() {
                return Some(format!("if {test} {{\n{consequent}\n}}"));
            }
            let alternate = indent_lines(&render_function_body(alternate, state)?);
            Some(format!(
                "if {test} {{\n{consequent}\n}} else {{\n{alternate}\n}}"
            ))
        }
        _ => None,
    }
}

fn render_bool_expr(expr: &JsExpr, state: &AotState) -> Option<String> {
    match expr {
        JsExpr::Value {
            value: JsValue::Bool { value },
        } => Some(value.to_string()),
        JsExpr::Ident { name } if state.bool_bindings.contains(name) => {
            Some(sanitize_go_identifier(name))
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
            let left = render_numeric_expr(left, state)?;
            let right = render_numeric_expr(right, state)?;
            let op = go_comparison_op(op)?;
            Some(format!("({left} {op} {right})"))
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
        _ => None,
    }
}

fn render_expr_stmt(expr: &JsExpr, state: &mut AotState) -> Option<String> {
    match expr {
        JsExpr::Call { callee, args, .. } if is_console_log(callee) => {
            let args = args
                .iter()
                .map(|arg| render_expr(arg, state))
                .collect::<Option<Vec<_>>>()?;
            Some(format!("fmt.Println({})", args.join(", ")))
        }
        JsExpr::Assign { op, left, right } => render_assignment_stmt(op, left, right, state),
        JsExpr::Update { op, arg, .. } => render_update_stmt(op, arg, state),
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

fn render_update_stmt(op: &str, arg: &JsExpr, state: &AotState) -> Option<String> {
    let JsExpr::Ident { name } = arg else {
        return None;
    };
    if !state.numeric_bindings.contains(name) || !matches!(op, "++" | "--") {
        return None;
    }
    Some(format!("{}{}", sanitize_go_identifier(name), op))
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

fn render_expr(expr: &JsExpr, state: &AotState) -> Option<String> {
    match expr {
        JsExpr::Value { value } => render_value(value),
        JsExpr::Ident { name } if state.bindings.contains(name) => {
            Some(sanitize_go_identifier(name))
        }
        JsExpr::Binary { op, left, right } if is_numeric_binary_op(op) => {
            let left = render_numeric_expr(left, state)?;
            let right = render_numeric_expr(right, state)?;
            Some(format!("({left} {op} {right})"))
        }
        JsExpr::Binary { op, .. } if op == "+" => render_string_expr(expr, state),
        JsExpr::Call { callee, args, .. } => render_call_expr(callee, args, state),
        JsExpr::Member {
            object,
            property,
            property_expr: None,
            optional: false,
        } => render_static_member_expr(object, property, state),
        JsExpr::Template { quasis, exprs } if exprs.is_empty() && quasis.len() == 1 => {
            Some(go_string_literal(&quasis[0]))
        }
        _ => None,
    }
}

fn render_numeric_expr(expr: &JsExpr, state: &AotState) -> Option<String> {
    match expr {
        JsExpr::Value {
            value: JsValue::Number { value },
        } => number_literal(value),
        JsExpr::Ident { name } if state.numeric_bindings.contains(name) => {
            Some(sanitize_go_identifier(name))
        }
        JsExpr::Member {
            object,
            property,
            property_expr: None,
            optional: false,
        } if static_member_kind(object, property, state) == Some(AotSlotKind::Number) => {
            render_static_member_expr(object, property, state)
        }
        JsExpr::Binary { op, left, right } if is_numeric_binary_op(op) => {
            let left = render_numeric_expr(left, state)?;
            let right = render_numeric_expr(right, state)?;
            Some(format!("({left} {op} {right})"))
        }
        _ => None,
    }
}

fn render_string_expr(expr: &JsExpr, state: &AotState) -> Option<String> {
    match expr {
        JsExpr::Value {
            value: JsValue::String { value },
        } => Some(go_string_literal(value)),
        JsExpr::Ident { name } if state.string_bindings.contains(name) => {
            Some(sanitize_go_identifier(name))
        }
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
        JsExpr::Template { quasis, exprs } if exprs.is_empty() && quasis.len() == 1 => {
            Some(go_string_literal(&quasis[0]))
        }
        _ => None,
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
    None
}

fn render_static_member_expr(object: &JsExpr, property: &str, state: &AotState) -> Option<String> {
    static_member_kind(object, property, state)?;
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
    let JsExpr::Ident { name } = object else {
        return None;
    };
    let object = state.object_bindings.get(name)?;
    object.fields.get(property).copied()
}

fn render_call_expr(callee: &JsExpr, args: &[JsExpr], state: &AotState) -> Option<String> {
    let JsExpr::Ident { name } = callee else {
        return None;
    };
    let function = state.functions.get(name)?;
    if function.params.len() != args.len() {
        return None;
    }
    let rendered_args = args
        .iter()
        .map(|arg| render_numeric_expr(arg, state))
        .collect::<Option<Vec<_>>>()?;
    Some(format!(
        "{}({})",
        function.go_name,
        rendered_args.join(", ")
    ))
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
