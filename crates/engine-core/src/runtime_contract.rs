use crate::contract::{
    Diagnostic, DiagnosticLevel, IrDocument, JsArrayElement, JsExpr, JsObjectProp, JsStmt, Module,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProgramPurpose {
    Main,
    VectorSuite,
}

pub fn unsupported_codegen_diagnostic(features: &[String]) -> Diagnostic {
    let detail = if features.is_empty() {
        "unknown unsupported executable feature".to_string()
    } else {
        format!("unsupported features: {}", features.join(", "))
    };
    Diagnostic {
        level: DiagnosticLevel::Error,
        code: "EXECUTABLE_JS_CODEGEN_NOT_IMPLEMENTED".to_string(),
        message: format!(
            "Executable JS semantics lowering is not implemented yet; failing closed ({detail})."
        ),
        source: None,
    }
}

pub fn unsupported_executable_features(ir: &IrDocument) -> Vec<String> {
    if entry_module(ir).is_none() {
        return vec!["entry module not found".to_string()];
    };
    let mut unsupported = Vec::new();
    for module in &ir.modules {
        for import in &module.imports {
            if import.resolved.is_none() {
                unsupported.push(format!("external module import {}", import.spec));
            }
        }
        for stmt in module
            .executable
            .as_ref()
            .map(|executable| executable.stmts.as_slice())
            .unwrap_or(&[])
        {
            collect_unsupported_stmt(stmt, &mut unsupported);
        }
    }
    unsupported.sort();
    unsupported.dedup();
    unsupported
}

fn entry_module(ir: &IrDocument) -> Option<&Module> {
    ir.modules
        .iter()
        .find(|module| module.source_path == ir.entry || module.id == ir.entry)
        .or_else(|| ir.modules.first())
}

fn collect_unsupported_stmt(stmt: &JsStmt, unsupported: &mut Vec<String>) {
    match stmt {
        JsStmt::Expr { expr } => collect_unsupported_expr(expr, unsupported),
        JsStmt::VarDecl { init, .. } => {
            if let Some(init) = init {
                collect_unsupported_expr(init, unsupported);
            }
        }
        JsStmt::FunctionDecl { body, .. } => {
            for stmt in body {
                collect_unsupported_stmt_in_function(stmt, unsupported);
            }
        }
        JsStmt::ClassDecl { .. } => unsupported.push("class declarations".to_string()),
        JsStmt::If {
            test,
            consequent,
            alternate,
        } => {
            collect_unsupported_expr(test, unsupported);
            collect_unsupported_stmt_list(consequent, false, unsupported);
            collect_unsupported_stmt_list(alternate, false, unsupported);
        }
        JsStmt::For {
            init,
            test,
            update,
            body,
        } => {
            collect_unsupported_stmt_list(init, false, unsupported);
            if let Some(test) = test {
                collect_unsupported_expr(test, unsupported);
            }
            if let Some(update) = update {
                collect_unsupported_expr(update, unsupported);
            }
            collect_unsupported_stmt_list(body, false, unsupported);
        }
        JsStmt::ForOf { right, body, .. } => {
            collect_unsupported_expr(right, unsupported);
            collect_unsupported_stmt_list(body, false, unsupported);
        }
        JsStmt::While { test, body } => {
            collect_unsupported_expr(test, unsupported);
            collect_unsupported_stmt_list(body, false, unsupported);
        }
        JsStmt::Switch {
            discriminant,
            cases,
        } => {
            collect_unsupported_expr(discriminant, unsupported);
            for case in cases {
                if let Some(test) = &case.test {
                    collect_unsupported_expr(test, unsupported);
                }
                collect_unsupported_stmt_list(&case.consequent, false, unsupported);
            }
        }
        JsStmt::Try { .. } => unsupported.push("try statements".to_string()),
        JsStmt::Break { label } => {
            if label.is_some() {
                unsupported.push("labeled break".to_string());
            }
        }
        JsStmt::Continue { label } => {
            if label.is_some() {
                unsupported.push("labeled continue".to_string());
            }
        }
        JsStmt::Return { .. } => unsupported.push("top-level return".to_string()),
        JsStmt::Throw { .. } => unsupported.push("throw statements".to_string()),
    }
}

fn collect_unsupported_stmt_in_function(stmt: &JsStmt, unsupported: &mut Vec<String>) {
    match stmt {
        JsStmt::Return { value } => collect_unsupported_return(value, unsupported),
        JsStmt::Expr { expr } => collect_unsupported_expr(expr, unsupported),
        JsStmt::VarDecl { init, .. } => {
            if let Some(init) = init {
                collect_unsupported_expr(init, unsupported);
            }
        }
        JsStmt::If {
            test,
            consequent,
            alternate,
        } => {
            collect_unsupported_expr(test, unsupported);
            collect_unsupported_stmt_list(consequent, true, unsupported);
            collect_unsupported_stmt_list(alternate, true, unsupported);
        }
        JsStmt::ForOf { right, body, .. } => {
            collect_unsupported_expr(right, unsupported);
            collect_unsupported_stmt_list(body, true, unsupported);
        }
        JsStmt::For {
            init,
            test,
            update,
            body,
        } => {
            collect_unsupported_stmt_list(init, true, unsupported);
            if let Some(test) = test {
                collect_unsupported_expr(test, unsupported);
            }
            if let Some(update) = update {
                collect_unsupported_expr(update, unsupported);
            }
            collect_unsupported_stmt_list(body, true, unsupported);
        }
        JsStmt::While { test, body } => {
            collect_unsupported_expr(test, unsupported);
            collect_unsupported_stmt_list(body, true, unsupported);
        }
        JsStmt::Switch {
            discriminant,
            cases,
        } => {
            collect_unsupported_expr(discriminant, unsupported);
            for case in cases {
                if let Some(test) = &case.test {
                    collect_unsupported_expr(test, unsupported);
                }
                collect_unsupported_stmt_list(&case.consequent, true, unsupported);
            }
        }
        other => collect_unsupported_stmt(other, unsupported),
    }
}

fn collect_unsupported_stmt_list(
    stmts: &[JsStmt],
    allow_return: bool,
    unsupported: &mut Vec<String>,
) {
    for stmt in stmts {
        if allow_return {
            collect_unsupported_stmt_in_function(stmt, unsupported);
        } else {
            collect_unsupported_stmt(stmt, unsupported);
        }
    }
}

fn collect_unsupported_return(value: &Option<JsExpr>, unsupported: &mut Vec<String>) {
    if let Some(value) = value {
        collect_unsupported_expr(value, unsupported);
    }
}

fn collect_unsupported_expr(expr: &JsExpr, unsupported: &mut Vec<String>) {
    match expr {
        JsExpr::Value { .. } | JsExpr::Ident { .. } | JsExpr::This => {}
        JsExpr::Array { items } => {
            for item in items {
                collect_unsupported_expr(item, unsupported);
            }
        }
        JsExpr::ArraySpread { items } => {
            for JsArrayElement { value, .. } in items {
                collect_unsupported_expr(value, unsupported);
            }
        }
        JsExpr::Object { props } => {
            for JsObjectProp { value, .. } in props {
                collect_unsupported_expr(value, unsupported);
            }
        }
        JsExpr::Unary { op, arg } => {
            if !matches!(op.as_str(), "!" | "+" | "-" | "typeof" | "void") {
                unsupported.push(format!("unary {op}"));
            }
            collect_unsupported_expr(arg, unsupported);
        }
        JsExpr::Binary { op, left, right } => {
            if !matches!(
                op.as_str(),
                "+" | "-"
                    | "*"
                    | "/"
                    | "%"
                    | "==="
                    | "!=="
                    | "=="
                    | "!="
                    | "<"
                    | "<="
                    | ">"
                    | ">="
                    | "&&"
                    | "||"
                    | "??"
                    | "&"
                    | "|"
                    | "<<"
                    | ">>"
                    | ">>>"
                    | "in"
            ) {
                unsupported.push(format!("binary {op}"));
            }
            collect_unsupported_expr(left, unsupported);
            collect_unsupported_expr(right, unsupported);
        }
        JsExpr::Conditional {
            test,
            consequent,
            alternate,
        } => {
            collect_unsupported_expr(test, unsupported);
            collect_unsupported_expr(consequent, unsupported);
            collect_unsupported_expr(alternate, unsupported);
        }
        JsExpr::Call { callee, args } => {
            if !is_supported_member_call(callee) && !matches!(callee.as_ref(), JsExpr::Ident { .. })
            {
                unsupported.push("function calls".to_string());
            }
            for arg in args {
                collect_unsupported_expr(arg, unsupported);
            }
        }
        JsExpr::Member { object, .. } => collect_unsupported_expr(object, unsupported),
        JsExpr::Template { exprs, .. } => {
            for expr in exprs {
                collect_unsupported_expr(expr, unsupported);
            }
        }
        JsExpr::Sequence { exprs } => {
            for expr in exprs {
                collect_unsupported_expr(expr, unsupported);
            }
        }
        JsExpr::Function { .. } => unsupported.push("function expressions".to_string()),
        JsExpr::Class { .. } => unsupported.push("class expressions".to_string()),
        JsExpr::Await { arg } => collect_unsupported_expr(arg, unsupported),
        JsExpr::Assign { op, left, right } => {
            if !matches!(op.as_str(), "=" | "+=" | "??=") {
                unsupported.push(format!("assignment {op}"));
            }
            if !matches!(left.as_ref(), JsExpr::Ident { .. } | JsExpr::Member { .. }) {
                unsupported.push("assignment targets".to_string());
            }
            collect_unsupported_expr(left, unsupported);
            collect_unsupported_expr(right, unsupported);
        }
        JsExpr::Update { arg, .. } => {
            if !matches!(arg.as_ref(), JsExpr::Ident { .. } | JsExpr::Member { .. }) {
                unsupported.push("update targets".to_string());
            }
            collect_unsupported_expr(arg, unsupported);
        }
        JsExpr::New { .. } => unsupported.push("new expressions".to_string()),
    }
}

fn is_supported_member_call(callee: &JsExpr) -> bool {
    matches!(
        callee,
        JsExpr::Member { object, property }
            if (property == "log" && matches!(object.as_ref(), JsExpr::Ident { name } if name == "console"))
                || property == "push"
    )
}

pub fn fail_closed_report_version(purpose: ProgramPurpose) -> &'static str {
    match purpose {
        ProgramPurpose::Main => "engine-core.fail-closed.main.v1",
        ProgramPurpose::VectorSuite => "engine-core.fail-closed.vector-suite.v1",
    }
}
