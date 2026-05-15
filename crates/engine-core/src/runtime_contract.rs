use crate::contract::{
    Diagnostic, DiagnosticLevel, IrDocument, JsArrayElement, JsClassMethod, JsExpr, JsObjectProp,
    JsStmt, Module,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProgramPurpose {
    Main,
    VectorSuite,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RuntimeContract {
    pub version: &'static str,
    pub operations: &'static [RuntimeOperation],
    pub node_builtins: &'static [&'static str],
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RuntimeOperation {
    pub key: &'static str,
    pub owner: RuntimeOperationOwner,
    pub status: RuntimeOperationStatus,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RuntimeOperationOwner {
    Contract,
    BackendRuntime,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RuntimeOperationStatus {
    Done,
    Wip,
    FailClosed,
}

pub const RUNTIME_OPERATIONS: &[RuntimeOperation] = &[
    RuntimeOperation {
        key: "js.value-model",
        owner: RuntimeOperationOwner::Contract,
        status: RuntimeOperationStatus::Wip,
    },
    RuntimeOperation {
        key: "js.property-access",
        owner: RuntimeOperationOwner::Contract,
        status: RuntimeOperationStatus::Wip,
    },
    RuntimeOperation {
        key: "js.call-construct-this",
        owner: RuntimeOperationOwner::Contract,
        status: RuntimeOperationStatus::Wip,
    },
    RuntimeOperation {
        key: "js.completion-records",
        owner: RuntimeOperationOwner::Contract,
        status: RuntimeOperationStatus::Wip,
    },
    RuntimeOperation {
        key: "js.module-cache",
        owner: RuntimeOperationOwner::Contract,
        status: RuntimeOperationStatus::Wip,
    },
    RuntimeOperation {
        key: "js.async-queue",
        owner: RuntimeOperationOwner::Contract,
        status: RuntimeOperationStatus::Wip,
    },
    RuntimeOperation {
        key: "node.process",
        owner: RuntimeOperationOwner::Contract,
        status: RuntimeOperationStatus::Wip,
    },
    RuntimeOperation {
        key: "node.fs",
        owner: RuntimeOperationOwner::Contract,
        status: RuntimeOperationStatus::Wip,
    },
    RuntimeOperation {
        key: "node.child-process",
        owner: RuntimeOperationOwner::Contract,
        status: RuntimeOperationStatus::Wip,
    },
    RuntimeOperation {
        key: "backend.runtime-rendering",
        owner: RuntimeOperationOwner::BackendRuntime,
        status: RuntimeOperationStatus::Done,
    },
];

pub const SUPPORTED_NODE_BUILTINS: &[&str] = &[
    "assert",
    "node:assert",
    "util",
    "node:util",
    "path",
    "node:path",
    "os",
    "node:os",
    "node:diagnostics_channel",
    "process",
    "node:process",
    "buffer",
    "node:buffer",
    "child_process",
    "node:child_process",
    "events",
    "node:events",
    "crypto",
    "node:crypto",
    "constants",
    "node:constants",
    "stream",
    "node:stream",
    "node:stream/promises",
    "fs",
    "node:fs",
    "string_decoder",
    "node:string_decoder",
    "node:timers/promises",
    "tty",
    "node:tty",
    "url",
    "node:url",
    "v8",
    "node:v8",
    "node:module",
];

pub fn runtime_contract() -> RuntimeContract {
    RuntimeContract {
        version: "runtime-contract.v1",
        operations: RUNTIME_OPERATIONS,
        node_builtins: SUPPORTED_NODE_BUILTINS,
    }
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

pub fn is_codegen_blocking_diagnostic(diagnostic: &Diagnostic) -> bool {
    !matches!(
        diagnostic.code.as_str(),
        "ANALYZER_UNSUPPORTED_ROUTE_OBJECT_SHAPE"
            | "ANALYZER_UNSUPPORTED_ROUTE_OBJECT_METHOD"
            | "ANALYZER_UNSUPPORTED_DYNAMIC_PATH"
            | "ANALYZER_UNSUPPORTED_INLINE_HANDLER"
            | "ANALYZER_UNSUPPORTED_CONDITIONAL_ROUTE"
            | "ANALYZER_UNSUPPORTED_REGISTER_CALLBACK"
            | "ANALYZER_UNSUPPORTED_REGISTER_BOUNDARY"
    )
}

pub fn unsupported_executable_features(ir: &IrDocument) -> Vec<String> {
    if entry_module(ir).is_none() {
        return vec!["entry module not found".to_string()];
    };
    let mut unsupported = Vec::new();
    for module in &ir.modules {
        for import in &module.imports {
            if import.resolved.is_none() && !is_supported_builtin_import(&import.spec) {
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

fn is_supported_builtin_import(spec: &str) -> bool {
    runtime_contract().node_builtins.contains(&spec)
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
        JsStmt::ClassDecl {
            super_class,
            methods,
            ..
        } => collect_unsupported_class(super_class.as_ref(), methods, unsupported),
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
        JsStmt::Try {
            body,
            catch_body,
            finally_body,
            ..
        } => {
            collect_unsupported_stmt_list(body, false, unsupported);
            collect_unsupported_stmt_list(catch_body, false, unsupported);
            collect_unsupported_stmt_list(finally_body, false, unsupported);
        }
        JsStmt::Label { body, .. } => collect_unsupported_stmt_list(body, false, unsupported),
        JsStmt::Break { .. } => {}
        JsStmt::Continue { .. } => {}
        JsStmt::Return { .. } => unsupported.push("top-level return".to_string()),
        JsStmt::Throw { value } => collect_unsupported_expr(value, unsupported),
        JsStmt::Yield { value, .. } => {
            if let Some(value) = value {
                collect_unsupported_expr(value, unsupported);
            }
        }
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
        JsStmt::Try {
            body,
            catch_body,
            finally_body,
            ..
        } => {
            collect_unsupported_stmt_list(body, true, unsupported);
            collect_unsupported_stmt_list(catch_body, true, unsupported);
            collect_unsupported_stmt_list(finally_body, true, unsupported);
        }
        JsStmt::Label { body, .. } => collect_unsupported_stmt_list(body, true, unsupported),
        JsStmt::Throw { value } => collect_unsupported_expr(value, unsupported),
        JsStmt::Yield { value, .. } => {
            if let Some(value) = value {
                collect_unsupported_expr(value, unsupported);
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
        JsExpr::ObjectRest { object, .. } => {
            collect_unsupported_expr(object, unsupported);
        }
        JsExpr::Unary { op, arg } => {
            if !matches!(
                op.as_str(),
                "!" | "+" | "-" | "~" | "typeof" | "void" | "delete"
            ) {
                unsupported.push(format!("unary {op}"));
            }
            if op == "delete" && !matches!(arg.as_ref(), JsExpr::Member { .. }) {
                unsupported.push("delete target".to_string());
            }
            collect_unsupported_expr(arg, unsupported);
        }
        JsExpr::Binary { op, left, right } => {
            if !matches!(
                op.as_str(),
                "+" | "-"
                    | "*"
                    | "**"
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
                    | "^"
                    | "<<"
                    | ">>"
                    | ">>>"
                    | "in"
                    | "instanceof"
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
        JsExpr::Call { callee, args, .. } => {
            collect_unsupported_expr(callee, unsupported);
            for arg in args {
                collect_unsupported_expr(arg, unsupported);
            }
        }
        JsExpr::Spread { arg } => collect_unsupported_expr(arg, unsupported),
        JsExpr::Member {
            object,
            property_expr,
            ..
        } => {
            collect_unsupported_expr(object, unsupported);
            if let Some(property_expr) = property_expr {
                collect_unsupported_expr(property_expr, unsupported);
            }
        }
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
        JsExpr::Function { body, .. } => {
            collect_unsupported_stmt_list(body, true, unsupported);
        }
        JsExpr::Class {
            super_class,
            methods,
        } => collect_unsupported_class(super_class.as_deref(), methods, unsupported),
        JsExpr::Await { arg } => collect_unsupported_expr(arg, unsupported),
        JsExpr::Assign { op, left, right } => {
            if !matches!(
                op.as_str(),
                "=" | "+="
                    | "-="
                    | "*="
                    | "/="
                    | "%="
                    | "**="
                    | "&="
                    | "|="
                    | "^="
                    | "<<="
                    | ">>="
                    | ">>>="
                    | "&&="
                    | "||="
                    | "??="
            ) {
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
        JsExpr::New { callee, args } => {
            collect_unsupported_expr(callee, unsupported);
            for arg in args {
                collect_unsupported_expr(arg, unsupported);
            }
        }
    }
}

fn collect_unsupported_class(
    super_class: Option<&JsExpr>,
    methods: &[JsClassMethod],
    unsupported: &mut Vec<String>,
) {
    if let Some(super_class) = super_class {
        collect_unsupported_expr(super_class, unsupported);
    }
    for method in methods {
        if method.kind != "constructor" && method.kind != "method" && method.kind != "getter" {
            unsupported.push(format!("class {} methods", method.kind));
        }
        collect_unsupported_stmt_list(&method.body, true, unsupported);
    }
}

pub fn fail_closed_report_version(purpose: ProgramPurpose) -> &'static str {
    match purpose {
        ProgramPurpose::Main => "engine-core.fail-closed.main.v1",
        ProgramPurpose::VectorSuite => "engine-core.fail-closed.vector-suite.v1",
    }
}
