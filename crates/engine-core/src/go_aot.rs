use std::collections::BTreeSet;

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
    let mut body = Vec::new();
    for stmt in &executable.stmts {
        body.push(render_stmt(stmt, &mut state)?);
    }
    Some(format!(
        r#"package {package_name}

import "fmt"

func main() {{
{body}
}}
"#,
        body = indent_lines(&body.join("\n"))
    ))
}

#[derive(Default)]
struct AotState {
    bindings: BTreeSet<String>,
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
            state.bindings.insert(name.clone());
            let value = init
                .as_ref()
                .map(|expr| render_expr(expr, state))
                .unwrap_or_else(|| Some("nil".to_string()))?;
            Some(format!("var {ident} any = {value}"))
        }
        JsStmt::Expr { expr } => render_expr_stmt(expr, state),
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
        JsExpr::Ident { name } if state.bindings.contains(name) => {
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

fn indent_lines(value: &str) -> String {
    value
        .lines()
        .map(|line| format!("\t{line}"))
        .collect::<Vec<_>>()
        .join("\n")
}
