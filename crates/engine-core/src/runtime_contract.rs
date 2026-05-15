use crate::contract::{
    Diagnostic, DiagnosticLevel, IrDocument, JsArrayElement, JsExpr, JsObjectProp, JsStmt, Module,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProgramPurpose {
    Main,
    VectorSuite,
}

pub fn unsupported_codegen_diagnostic() -> Diagnostic {
    Diagnostic {
        level: DiagnosticLevel::Error,
        code: "EXECUTABLE_JS_CODEGEN_NOT_IMPLEMENTED".to_string(),
        message: "Executable JS semantics lowering is not implemented yet; failing closed."
            .to_string(),
        source: None,
    }
}

pub fn unsupported_executable_features(ir: &IrDocument) -> Vec<String> {
    let Some(entry) = entry_module(ir) else {
        return vec!["entry module not found".to_string()];
    };
    let mut unsupported = Vec::new();
    if !entry.imports.is_empty() {
        unsupported.push("module imports".to_string());
    }
    for stmt in entry
        .executable
        .as_ref()
        .map(|executable| executable.stmts.as_slice())
        .unwrap_or(&[])
    {
        collect_unsupported_stmt(stmt, &mut unsupported);
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
        JsStmt::If { .. } => unsupported.push("if statements".to_string()),
        JsStmt::For { .. } => unsupported.push("for statements".to_string()),
        JsStmt::ForOf { .. } => unsupported.push("for-of statements".to_string()),
        JsStmt::While { .. } => unsupported.push("while statements".to_string()),
        JsStmt::Switch { .. } => unsupported.push("switch statements".to_string()),
        JsStmt::Try { .. } => unsupported.push("try statements".to_string()),
        JsStmt::Break { .. } => unsupported.push("break statements".to_string()),
        JsStmt::Continue { .. } => unsupported.push("continue statements".to_string()),
        JsStmt::Return { .. } => unsupported.push("top-level return".to_string()),
        JsStmt::Throw { .. } => unsupported.push("throw statements".to_string()),
    }
}

fn collect_unsupported_stmt_in_function(stmt: &JsStmt, unsupported: &mut Vec<String>) {
    match stmt {
        JsStmt::Return { value } => {
            if let Some(value) = value {
                collect_unsupported_expr(value, unsupported);
            }
        }
        JsStmt::Expr { expr } => collect_unsupported_expr(expr, unsupported),
        JsStmt::VarDecl { init, .. } => {
            if let Some(init) = init {
                collect_unsupported_expr(init, unsupported);
            }
        }
        other => collect_unsupported_stmt(other, unsupported),
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
            for JsArrayElement { spread, value } in items {
                if *spread {
                    unsupported.push("array spread".to_string());
                }
                collect_unsupported_expr(value, unsupported);
            }
        }
        JsExpr::Object { props } => {
            for JsObjectProp { value, .. } in props {
                collect_unsupported_expr(value, unsupported);
            }
        }
        JsExpr::Unary { op, arg } => {
            if !matches!(op.as_str(), "!" | "+" | "-") {
                unsupported.push(format!("unary {op}"));
            }
            collect_unsupported_expr(arg, unsupported);
        }
        JsExpr::Binary { op, left, right } => {
            if !matches!(
                op.as_str(),
                "+" | "-" | "*" | "/" | "%" | "===" | "!==" | "==" | "!=" | "<" | "<=" | ">" | ">="
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
            if !is_console_log_call(callee) && !matches!(callee.as_ref(), JsExpr::Ident { .. }) {
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
        JsExpr::Await { .. } => unsupported.push("await expressions".to_string()),
        JsExpr::Assign { .. } => unsupported.push("assignments".to_string()),
        JsExpr::Update { .. } => unsupported.push("updates".to_string()),
        JsExpr::New { .. } => unsupported.push("new expressions".to_string()),
    }
}

fn is_console_log_call(callee: &JsExpr) -> bool {
    matches!(
        callee,
        JsExpr::Member { object, property }
            if property == "log" && matches!(object.as_ref(), JsExpr::Ident { name } if name == "console")
    )
}

pub fn fail_closed_report_version(purpose: ProgramPurpose) -> &'static str {
    match purpose {
        ProgramPurpose::Main => "engine-core.fail-closed.main.v1",
        ProgramPurpose::VectorSuite => "engine-core.fail-closed.vector-suite.v1",
    }
}
