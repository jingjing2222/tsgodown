use std::collections::{BTreeMap, BTreeSet};

use crate::contract::{AnalyzeResponse, IrDocument, JsExpr, JsStmt, JsValue, Module};
use crate::emit_go::{go_string_literal, sanitize_go_identifier};

pub(crate) fn render_aot_executable_program(
    package_name: &str,
    analyzed: &AnalyzeResponse,
) -> Option<String> {
    let module = entry_module(&analyzed.ir)?;
    if !module.imports.is_empty() {
        return None;
    }
    if analyzed
        .ir
        .modules
        .iter()
        .any(|candidate| !candidate.imports.is_empty())
    {
        return None;
    }
    let executable = module.executable.as_ref()?;
    let mut state = AotState::default();
    for stmt in &executable.stmts {
        if let JsStmt::FunctionDecl { name, params, .. } = stmt {
            state.functions.insert(
                name.clone(),
                AotFunction {
                    params: params.clone(),
                },
            );
        }
    }
    let mut declarations = Vec::new();
    let mut body = Vec::new();
    for stmt in &executable.stmts {
        if let JsStmt::FunctionDecl { .. } = stmt {
            declarations.push(render_function_decl(stmt, &state)?);
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

#[derive(Default)]
struct AotState {
    bindings: BTreeSet<String>,
    numeric_bindings: BTreeSet<String>,
    functions: BTreeMap<String, AotFunction>,
}

#[derive(Clone)]
struct AotFunction {
    params: Vec<String>,
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
    Some(format!("{} := {value}", sanitize_go_identifier(name)))
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
        functions: state.functions.clone(),
    };
    render_stmt_block_with_state(stmts, &block_state)
}

fn render_stmt_block_with_state(stmts: &[JsStmt], state: &AotState) -> Option<String> {
    let mut block_state = AotState {
        bindings: state.bindings.clone(),
        numeric_bindings: state.numeric_bindings.clone(),
        functions: state.functions.clone(),
    };
    stmts
        .iter()
        .map(|stmt| render_stmt(stmt, &mut block_state))
        .collect::<Option<Vec<_>>>()
        .map(|stmts| stmts.join("\n"))
}

fn render_function_decl(stmt: &JsStmt, state: &AotState) -> Option<String> {
    let JsStmt::FunctionDecl {
        name,
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
        "func {}({rendered_params}) any {{\n{}\n}}",
        sanitize_go_identifier(name),
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
    let JsExpr::Call { callee, args, .. } = expr else {
        return None;
    };
    if !is_console_log(callee) {
        return None;
    }
    let args = args
        .iter()
        .map(|arg| render_expr(arg, state))
        .collect::<Option<Vec<_>>>()?;
    Some(format!("fmt.Println({})", args.join(", ")))
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
        JsExpr::Call { callee, args, .. } => render_call_expr(callee, args, state),
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
        JsExpr::Binary { op, left, right } if is_numeric_binary_op(op) => {
            let left = render_numeric_expr(left, state)?;
            let right = render_numeric_expr(right, state)?;
            Some(format!("({left} {op} {right})"))
        }
        _ => None,
    }
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
        sanitize_go_identifier(name),
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
