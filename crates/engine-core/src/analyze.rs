use std::path::PathBuf;

use crate::contract::{
    AnalyzeRequest, AnalyzeResponse, Diagnostic, DiagnosticLevel, DiagnosticSource, Import,
    IrDocument, JsExpr, JsObjectProp, JsStmt, JsSwitchCase, JsValue, Module, Route,
};

pub fn analyze(request: AnalyzeRequest) -> AnalyzeResponse {
    let entry = request.manifest.entry;
    let root = request
        .cwd
        .map(PathBuf::from)
        .unwrap_or_else(|| std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")));
    let compiler_ir = analyzer_rust::analyze_compiler_project(&root, &entry);

    AnalyzeResponse {
        ir: IrDocument {
            version: "0.1".to_string(),
            entry,
            modules: compiler_ir
                .modules
                .into_iter()
                .map(|module| Module {
                    id: module.id,
                    source_path: module.source_path,
                    exports: module.exports,
                    imports: module
                        .imports
                        .into_iter()
                        .map(|import| Import {
                            spec: import.spec,
                            kind: import.kind,
                            resolved: import.resolved,
                        })
                        .collect(),
                    executable: module.executable.map(map_executable_module),
                })
                .collect(),
            routes: compiler_ir
                .routes
                .into_iter()
                .map(|route| Route {
                    method: route.method,
                    path: route.path,
                })
                .collect(),
        },
        diagnostics: compiler_ir
            .diagnostics
            .into_iter()
            .map(|diagnostic| Diagnostic {
                level: match diagnostic.level.as_str() {
                    "error" => DiagnosticLevel::Error,
                    "warn" => DiagnosticLevel::Warning,
                    _ => DiagnosticLevel::Info,
                },
                code: diagnostic.code,
                message: diagnostic.message,
                source: diagnostic.source.map(|source| DiagnosticSource {
                    file: source.file,
                    line: source.line,
                    column: source.column,
                    via_source_map: source.via_source_map,
                }),
            })
            .collect(),
    }
}

fn map_executable_module(
    executable: analyzer_rust::ExecutableModuleIR,
) -> crate::contract::ExecutableModule {
    crate::contract::ExecutableModule {
        stmts: executable.stmts.into_iter().map(map_js_stmt).collect(),
    }
}

fn map_js_stmt(stmt: analyzer_rust::JsStmtIR) -> JsStmt {
    match stmt {
        analyzer_rust::JsStmtIR::Expr(expr) => JsStmt::Expr {
            expr: map_js_expr(expr),
        },
        analyzer_rust::JsStmtIR::FunctionDecl {
            name,
            params,
            r#async,
            body,
        } => JsStmt::FunctionDecl {
            name,
            params,
            r#async,
            body: body.into_iter().map(map_js_stmt).collect(),
        },
        analyzer_rust::JsStmtIR::If {
            test,
            consequent,
            alternate,
        } => JsStmt::If {
            test: map_js_expr(test),
            consequent: consequent.into_iter().map(map_js_stmt).collect(),
            alternate: alternate.into_iter().map(map_js_stmt).collect(),
        },
        analyzer_rust::JsStmtIR::For {
            init,
            test,
            update,
            body,
        } => JsStmt::For {
            init: init.into_iter().map(map_js_stmt).collect(),
            test: test.map(map_js_expr),
            update: update.map(map_js_expr),
            body: body.into_iter().map(map_js_stmt).collect(),
        },
        analyzer_rust::JsStmtIR::ForOf { left, right, body } => JsStmt::ForOf {
            left,
            right: map_js_expr(right),
            body: body.into_iter().map(map_js_stmt).collect(),
        },
        analyzer_rust::JsStmtIR::While { test, body } => JsStmt::While {
            test: map_js_expr(test),
            body: body.into_iter().map(map_js_stmt).collect(),
        },
        analyzer_rust::JsStmtIR::Switch {
            discriminant,
            cases,
        } => JsStmt::Switch {
            discriminant: map_js_expr(discriminant),
            cases: cases
                .into_iter()
                .map(|case| JsSwitchCase {
                    test: case.test.map(map_js_expr),
                    consequent: case.consequent.into_iter().map(map_js_stmt).collect(),
                })
                .collect(),
        },
        analyzer_rust::JsStmtIR::Try {
            body,
            catch_param,
            catch_body,
            finally_body,
        } => JsStmt::Try {
            body: body.into_iter().map(map_js_stmt).collect(),
            catch_param,
            catch_body: catch_body.into_iter().map(map_js_stmt).collect(),
            finally_body: finally_body.into_iter().map(map_js_stmt).collect(),
        },
        analyzer_rust::JsStmtIR::Break(label) => JsStmt::Break { label },
        analyzer_rust::JsStmtIR::Continue(label) => JsStmt::Continue { label },
        analyzer_rust::JsStmtIR::Return(value) => JsStmt::Return {
            value: value.map(map_js_expr),
        },
        analyzer_rust::JsStmtIR::Throw(value) => JsStmt::Throw {
            value: map_js_expr(value),
        },
        analyzer_rust::JsStmtIR::VarDecl { name, init } => JsStmt::VarDecl {
            name,
            init: init.map(map_js_expr),
        },
    }
}

fn map_js_expr(expr: analyzer_rust::JsExprIR) -> JsExpr {
    match expr {
        analyzer_rust::JsExprIR::Value(value) => JsExpr::Value {
            value: map_js_value(value),
        },
        analyzer_rust::JsExprIR::Ident(name) => JsExpr::Ident { name },
        analyzer_rust::JsExprIR::Array(items) => JsExpr::Array {
            items: items.into_iter().map(map_js_expr).collect(),
        },
        analyzer_rust::JsExprIR::Object(props) => JsExpr::Object {
            props: props
                .into_iter()
                .map(|prop| JsObjectProp {
                    key: prop.key,
                    value: map_js_expr(prop.value),
                })
                .collect(),
        },
        analyzer_rust::JsExprIR::Function {
            params,
            r#async,
            body,
        } => JsExpr::Function {
            params,
            r#async,
            body: body.into_iter().map(map_js_stmt).collect(),
        },
        analyzer_rust::JsExprIR::Unary { op, arg } => JsExpr::Unary {
            op,
            arg: Box::new(map_js_expr(*arg)),
        },
        analyzer_rust::JsExprIR::Binary { op, left, right } => JsExpr::Binary {
            op,
            left: Box::new(map_js_expr(*left)),
            right: Box::new(map_js_expr(*right)),
        },
        analyzer_rust::JsExprIR::Assign { op, left, right } => JsExpr::Assign {
            op,
            left: Box::new(map_js_expr(*left)),
            right: Box::new(map_js_expr(*right)),
        },
        analyzer_rust::JsExprIR::Update { op, arg, prefix } => JsExpr::Update {
            op,
            arg: Box::new(map_js_expr(*arg)),
            prefix,
        },
        analyzer_rust::JsExprIR::Call { callee, args } => JsExpr::Call {
            callee: Box::new(map_js_expr(*callee)),
            args: args.into_iter().map(map_js_expr).collect(),
        },
        analyzer_rust::JsExprIR::Member { object, property } => JsExpr::Member {
            object: Box::new(map_js_expr(*object)),
            property,
        },
    }
}

fn map_js_value(value: analyzer_rust::JsValueIR) -> JsValue {
    match value {
        analyzer_rust::JsValueIR::Undefined => JsValue::Undefined,
        analyzer_rust::JsValueIR::Null => JsValue::Null,
        analyzer_rust::JsValueIR::Bool(value) => JsValue::Bool { value },
        analyzer_rust::JsValueIR::Number(value) => JsValue::Number { value },
        analyzer_rust::JsValueIR::String(value) => JsValue::String { value },
    }
}
