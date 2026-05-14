use std::{collections::BTreeMap, path::Path};

use swc_common::{sync::Lrc, FileName, SourceMap};
use swc_ecma_ast::{
    AssignTarget, Callee, Expr, Lit, MemberExpr, MemberProp, Prop, PropName, PropOrSpread,
    SimpleAssignTarget, Stmt, VarDeclOrExpr,
};
use swc_ecma_ast::{
    BlockStmt, BlockStmtOrExpr, Class, ClassMember, Decl, ExportSpecifier, FnDecl, Function,
    MethodKind, Module, ModuleDecl, ModuleItem, ParamOrTsParamProp, Pat,
};
use swc_ecma_parser::{lexer::Lexer, EsSyntax, Parser, StringInput, Syntax, TsSyntax};

use crate::{
    DiagnosticIR, DiagnosticSourceIR, ExecutableModuleIR, ImportIR, JsClassMethodIR, JsExprIR,
    JsObjectPropIR, JsStmtIR, JsSwitchCaseIR, JsValueIR,
};

#[derive(Debug)]
pub struct ParsedModule {
    pub imports: Vec<ImportIR>,
    pub exports: Vec<String>,
    pub executable: ExecutableModuleIR,
    pub diagnostics: Vec<DiagnosticIR>,
}

pub fn parse_js_module(file: &str, src: &str) -> ParsedModule {
    let cm: Lrc<SourceMap> = Default::default();
    let fm = cm.new_source_file(FileName::Custom(file.to_string()).into(), src.to_string());
    let lexer = Lexer::new(
        syntax_for_file(file),
        swc_ecma_ast::EsVersion::latest(),
        StringInput::from(&*fm),
        None,
    );
    let mut parser = Parser::new_from(lexer);
    let mut diagnostics = parser
        .take_errors()
        .into_iter()
        .map(|error| parser_diag(file, format!("recoverable parser error: {error:?}")))
        .collect::<Vec<_>>();

    match parser.parse_module() {
        Ok(module) => {
            diagnostics.extend(
                parser
                    .take_errors()
                    .into_iter()
                    .map(|error| parser_diag(file, format!("recoverable parser error: {error:?}"))),
            );
            ParsedModule {
                imports: collect_imports_from_ast(&module),
                exports: collect_exports_from_ast(&module),
                executable: collect_executable_from_ast(&module),
                diagnostics,
            }
        }
        Err(error) => {
            diagnostics.push(parser_diag(
                file,
                format!("unrecoverable parser error: {error:?}"),
            ));
            ParsedModule {
                imports: vec![],
                exports: vec![],
                executable: ExecutableModuleIR { stmts: vec![] },
                diagnostics,
            }
        }
    }
}

fn syntax_for_file(file: &str) -> Syntax {
    let path = Path::new(file);
    let extension = path
        .extension()
        .and_then(|value| value.to_str())
        .unwrap_or_default()
        .to_ascii_lowercase();
    let is_ts = matches!(
        extension.as_str(),
        "ts" | "tsx" | "mts" | "cts" | "mtsx" | "ctsx"
    );
    let is_jsx = matches!(
        extension.as_str(),
        "jsx" | "tsx" | "mjsx" | "cjsx" | "mtsx" | "ctsx"
    );
    let is_dts = is_ts
        && path
            .file_name()
            .and_then(|value| value.to_str())
            .is_some_and(|name| name.ends_with(".d.ts") || name.ends_with(".d.mts"));

    if is_ts {
        Syntax::Typescript(TsSyntax {
            tsx: is_jsx,
            dts: is_dts,
            ..Default::default()
        })
    } else {
        Syntax::Es(EsSyntax {
            jsx: is_jsx,
            ..Default::default()
        })
    }
}

fn parser_diag(file: &str, message: String) -> DiagnosticIR {
    DiagnosticIR {
        level: "error".to_string(),
        code: "PARSER_SYNTAX_ERROR".to_string(),
        message,
        source: Some(DiagnosticSourceIR {
            file: file.to_string(),
            line: None,
            column: None,
            via_source_map: None,
        }),
    }
}

fn collect_imports_from_ast(module: &Module) -> Vec<ImportIR> {
    let mut imports = Vec::new();
    for item in &module.body {
        let ModuleItem::ModuleDecl(ModuleDecl::Import(import)) = item else {
            if let ModuleItem::ModuleDecl(ModuleDecl::ExportNamed(named)) = item {
                if let Some(src) = &named.src {
                    imports.push(ImportIR {
                        spec: src.value.to_string_lossy().to_string(),
                        kind: "esm".to_string(),
                        resolved: None,
                    });
                }
            }
            if let ModuleItem::ModuleDecl(ModuleDecl::ExportAll(export_all)) = item {
                imports.push(ImportIR {
                    spec: export_all.src.value.to_string_lossy().to_string(),
                    kind: "esm".to_string(),
                    resolved: None,
                });
            }
            continue;
        };
        imports.push(ImportIR {
            spec: import.src.value.to_string_lossy().to_string(),
            kind: "esm".to_string(),
            resolved: None,
        });
    }

    for item in &module.body {
        collect_cjs_imports_from_item(&mut imports, item);
    }

    imports.sort_by(|a, b| a.spec.cmp(&b.spec).then_with(|| a.kind.cmp(&b.kind)));
    imports
}

fn collect_exports_from_ast(module: &Module) -> Vec<String> {
    let mut exports = Vec::new();
    let object_defs = collect_top_level_object_defs(module);
    for item in &module.body {
        match item {
            ModuleItem::ModuleDecl(ModuleDecl::ExportDecl(export)) => {
                push_decl_export(&mut exports, &export.decl);
            }
            ModuleItem::ModuleDecl(ModuleDecl::ExportNamed(named)) => {
                for specifier in &named.specifiers {
                    if let Some(name) = export_specifier_name(specifier) {
                        exports.push(name);
                    }
                }
            }
            ModuleItem::ModuleDecl(ModuleDecl::ExportDefaultDecl(_))
            | ModuleItem::ModuleDecl(ModuleDecl::ExportDefaultExpr(_)) => {
                exports.push("default".to_string());
            }
            ModuleItem::ModuleDecl(ModuleDecl::ExportAll(_)) => {
                exports.push("*".to_string());
            }
            _ => {}
        }

        collect_cjs_exports_from_item(&mut exports, &object_defs, item);
    }
    exports.sort();
    exports.dedup();
    exports
}

fn push_decl_export(exports: &mut Vec<String>, decl: &Decl) {
    match decl {
        Decl::Fn(function) => exports.push(function.ident.sym.to_string()),
        Decl::Class(class) => exports.push(class.ident.sym.to_string()),
        Decl::Var(var_decl) => {
            for decl in &var_decl.decls {
                if let Some(name) = pat_name(&decl.name) {
                    exports.push(name);
                }
            }
        }
        Decl::TsInterface(interface) => exports.push(interface.id.sym.to_string()),
        Decl::TsTypeAlias(type_alias) => exports.push(type_alias.id.sym.to_string()),
        Decl::TsEnum(ts_enum) => exports.push(ts_enum.id.sym.to_string()),
        _ => {}
    }
}

fn export_specifier_name(specifier: &ExportSpecifier) -> Option<String> {
    match specifier {
        ExportSpecifier::Named(named) => Some(
            named
                .exported
                .as_ref()
                .unwrap_or(&named.orig)
                .atom()
                .to_string(),
        ),
        ExportSpecifier::Default(default) => Some(default.exported.sym.to_string()),
        ExportSpecifier::Namespace(namespace) => Some(namespace.name.atom().to_string()),
    }
}

fn pat_name(pat: &Pat) -> Option<String> {
    match pat {
        Pat::Ident(binding) => Some(binding.id.sym.to_string()),
        _ => None,
    }
}

fn collect_executable_from_ast(module: &Module) -> ExecutableModuleIR {
    let mut stmts = Vec::new();
    for item in &module.body {
        collect_executable_from_item(&mut stmts, item);
    }
    ExecutableModuleIR { stmts }
}

fn collect_executable_from_item(stmts: &mut Vec<JsStmtIR>, item: &ModuleItem) {
    match item {
        ModuleItem::ModuleDecl(ModuleDecl::ExportDecl(export))
            if matches!(export.decl, Decl::Fn(_)) =>
        {
            let Decl::Fn(function) = &export.decl else {
                unreachable!("matches! guarded function decl")
            };
            lower_fn_decl_stmt(stmts, function);
        }
        ModuleItem::ModuleDecl(ModuleDecl::ExportDecl(export))
            if matches!(export.decl, Decl::Var(_)) =>
        {
            let Decl::Var(var_decl) = &export.decl else {
                unreachable!("matches! guarded var decl")
            };
            lower_var_decl_stmts(stmts, var_decl);
        }
        ModuleItem::ModuleDecl(ModuleDecl::ExportDecl(export))
            if matches!(export.decl, Decl::Class(_)) =>
        {
            let Decl::Class(class_decl) = &export.decl else {
                unreachable!("matches! guarded class decl")
            };
            lower_class_decl_stmt(stmts, class_decl);
        }
        ModuleItem::Stmt(Stmt::Decl(Decl::Var(var_decl))) => {
            lower_var_decl_stmts(stmts, var_decl);
        }
        ModuleItem::Stmt(Stmt::Decl(Decl::Fn(function))) => {
            lower_fn_decl_stmt(stmts, function);
        }
        ModuleItem::Stmt(Stmt::Decl(Decl::Class(class_decl))) => {
            lower_class_decl_stmt(stmts, class_decl);
        }
        ModuleItem::Stmt(Stmt::Expr(expr_stmt)) => {
            if let Some(expr) = lower_js_expr(&expr_stmt.expr) {
                stmts.push(JsStmtIR::Expr(expr));
            }
        }
        ModuleItem::Stmt(Stmt::Return(return_stmt)) => {
            stmts.push(JsStmtIR::Return(
                return_stmt.arg.as_deref().and_then(lower_js_expr),
            ));
        }
        ModuleItem::Stmt(Stmt::Throw(throw_stmt)) => {
            if let Some(expr) = lower_js_expr(&throw_stmt.arg) {
                stmts.push(JsStmtIR::Throw(expr));
            }
        }
        ModuleItem::Stmt(stmt) => collect_executable_from_stmt(stmts, stmt),
        _ => {}
    }
}

fn collect_executable_from_stmt(stmts: &mut Vec<JsStmtIR>, stmt: &Stmt) {
    match stmt {
        Stmt::Decl(Decl::Var(var_decl)) => lower_var_decl_stmts(stmts, var_decl),
        Stmt::Decl(Decl::Fn(function)) => lower_fn_decl_stmt(stmts, function),
        Stmt::Decl(Decl::Class(class_decl)) => lower_class_decl_stmt(stmts, class_decl),
        Stmt::Expr(expr_stmt) => {
            if let Some(expr) = lower_js_expr(&expr_stmt.expr) {
                stmts.push(JsStmtIR::Expr(expr));
            }
        }
        Stmt::If(if_stmt) => {
            if let Some(test) = lower_js_expr(&if_stmt.test) {
                stmts.push(JsStmtIR::If {
                    test,
                    consequent: lower_stmt_as_block(&if_stmt.cons),
                    alternate: if_stmt
                        .alt
                        .as_deref()
                        .map(lower_stmt_as_block)
                        .unwrap_or_default(),
                });
            }
        }
        Stmt::For(for_stmt) => {
            stmts.push(JsStmtIR::For {
                init: for_stmt
                    .init
                    .as_ref()
                    .map(lower_for_init)
                    .unwrap_or_default(),
                test: for_stmt.test.as_deref().and_then(lower_js_expr),
                update: for_stmt.update.as_deref().and_then(lower_js_expr),
                body: lower_stmt_as_block(&for_stmt.body),
            });
        }
        Stmt::ForOf(for_of) => {
            if let Some(left) = for_head_name(&for_of.left) {
                if let Some(right) = lower_js_expr(&for_of.right) {
                    stmts.push(JsStmtIR::ForOf {
                        left,
                        right,
                        body: lower_stmt_as_block(&for_of.body),
                    });
                }
            }
        }
        Stmt::While(while_stmt) => {
            if let Some(test) = lower_js_expr(&while_stmt.test) {
                stmts.push(JsStmtIR::While {
                    test,
                    body: lower_stmt_as_block(&while_stmt.body),
                });
            }
        }
        Stmt::Switch(switch_stmt) => {
            if let Some(discriminant) = lower_js_expr(&switch_stmt.discriminant) {
                let cases = switch_stmt
                    .cases
                    .iter()
                    .map(|case| JsSwitchCaseIR {
                        test: case.test.as_deref().and_then(lower_js_expr),
                        consequent: case
                            .cons
                            .iter()
                            .flat_map(|stmt| lower_stmt_as_block(stmt).into_iter())
                            .collect(),
                    })
                    .collect();
                stmts.push(JsStmtIR::Switch {
                    discriminant,
                    cases,
                });
            }
        }
        Stmt::Try(try_stmt) => {
            stmts.push(JsStmtIR::Try {
                body: lower_block_stmt(&try_stmt.block),
                catch_param: try_stmt
                    .handler
                    .as_ref()
                    .and_then(|handler| handler.param.as_ref())
                    .and_then(pat_name),
                catch_body: try_stmt
                    .handler
                    .as_ref()
                    .map(|handler| lower_block_stmt(&handler.body))
                    .unwrap_or_default(),
                finally_body: try_stmt
                    .finalizer
                    .as_ref()
                    .map(lower_block_stmt)
                    .unwrap_or_default(),
            });
        }
        Stmt::Break(break_stmt) => {
            stmts.push(JsStmtIR::Break(
                break_stmt.label.as_ref().map(|label| label.sym.to_string()),
            ));
        }
        Stmt::Continue(continue_stmt) => {
            stmts.push(JsStmtIR::Continue(
                continue_stmt
                    .label
                    .as_ref()
                    .map(|label| label.sym.to_string()),
            ));
        }
        Stmt::Return(return_stmt) => {
            stmts.push(JsStmtIR::Return(
                return_stmt.arg.as_deref().and_then(lower_js_expr),
            ));
        }
        Stmt::Throw(throw_stmt) => {
            if let Some(expr) = lower_js_expr(&throw_stmt.arg) {
                stmts.push(JsStmtIR::Throw(expr));
            }
        }
        _ => {}
    }
}

fn lower_for_init(init: &VarDeclOrExpr) -> Vec<JsStmtIR> {
    match init {
        VarDeclOrExpr::VarDecl(var_decl) => {
            let mut stmts = Vec::new();
            lower_var_decl_stmts(&mut stmts, var_decl);
            stmts
        }
        VarDeclOrExpr::Expr(expr) => lower_js_expr(expr)
            .map(JsStmtIR::Expr)
            .into_iter()
            .collect(),
    }
}

fn for_head_name(head: &swc_ecma_ast::ForHead) -> Option<String> {
    match head {
        swc_ecma_ast::ForHead::VarDecl(var_decl) => var_decl
            .decls
            .first()?
            .name
            .as_ident()
            .map(|ident| ident.id.sym.to_string()),
        swc_ecma_ast::ForHead::Pat(pat) => pat_name(pat),
        swc_ecma_ast::ForHead::UsingDecl(_) => None,
    }
}

fn lower_fn_decl_stmt(stmts: &mut Vec<JsStmtIR>, function: &FnDecl) {
    let Some(body) = &function.function.body else {
        return;
    };
    stmts.push(JsStmtIR::FunctionDecl {
        name: function.ident.sym.to_string(),
        params: function
            .function
            .params
            .iter()
            .filter_map(|param| pat_name(&param.pat))
            .collect(),
        r#async: function.function.is_async,
        body: lower_block_stmt(body),
    });
}

fn lower_class_decl_stmt(stmts: &mut Vec<JsStmtIR>, class_decl: &swc_ecma_ast::ClassDecl) {
    stmts.push(JsStmtIR::ClassDecl {
        name: class_decl.ident.sym.to_string(),
        super_class: class_decl
            .class
            .super_class
            .as_deref()
            .and_then(lower_js_expr),
        methods: lower_class_methods(&class_decl.class),
    });
}

fn lower_block_stmt(block: &BlockStmt) -> Vec<JsStmtIR> {
    let mut stmts = Vec::new();
    for stmt in &block.stmts {
        collect_executable_from_stmt(&mut stmts, stmt);
    }
    stmts
}

fn lower_stmt_as_block(stmt: &Stmt) -> Vec<JsStmtIR> {
    match stmt {
        Stmt::Block(block) => lower_block_stmt(block),
        stmt => {
            let mut stmts = Vec::new();
            collect_executable_from_stmt(&mut stmts, stmt);
            stmts
        }
    }
}

fn lower_var_decl_stmts(stmts: &mut Vec<JsStmtIR>, var_decl: &swc_ecma_ast::VarDecl) {
    for decl in &var_decl.decls {
        let Some(name) = pat_name(&decl.name) else {
            continue;
        };
        stmts.push(JsStmtIR::VarDecl {
            name,
            init: decl.init.as_deref().and_then(lower_js_expr),
        });
    }
}

fn lower_js_expr(expr: &Expr) -> Option<JsExprIR> {
    match expr {
        Expr::Lit(lit) => lower_js_lit(lit).map(JsExprIR::Value),
        Expr::Ident(ident) => Some(JsExprIR::Ident(ident.sym.to_string())),
        Expr::This(_) => Some(JsExprIR::This),
        Expr::Array(array) => {
            let mut items = Vec::new();
            for elem in &array.elems {
                let Some(elem) = elem else {
                    return None;
                };
                if elem.spread.is_some() {
                    return None;
                }
                items.push(lower_js_expr(&elem.expr)?);
            }
            Some(JsExprIR::Array(items))
        }
        Expr::Object(object) => {
            let mut props = Vec::new();
            for prop in &object.props {
                props.push(lower_js_object_prop(prop)?);
            }
            Some(JsExprIR::Object(props))
        }
        Expr::Fn(function) => lower_function_expr(&function.function),
        Expr::Class(class) => Some(JsExprIR::Class {
            super_class: class
                .class
                .super_class
                .as_deref()
                .and_then(lower_js_expr)
                .map(Box::new),
            methods: lower_class_methods(&class.class),
        }),
        Expr::Arrow(arrow) => Some(JsExprIR::Function {
            params: arrow.params.iter().filter_map(pat_name).collect(),
            r#async: arrow.is_async,
            body: lower_arrow_body(&arrow.body)?,
        }),
        Expr::Unary(unary) => Some(JsExprIR::Unary {
            op: unary.op.to_string(),
            arg: Box::new(lower_js_expr(&unary.arg)?),
        }),
        Expr::Await(await_expr) => Some(JsExprIR::Await {
            arg: Box::new(lower_js_expr(&await_expr.arg)?),
        }),
        Expr::Bin(binary) => Some(JsExprIR::Binary {
            op: binary.op.to_string(),
            left: Box::new(lower_js_expr(&binary.left)?),
            right: Box::new(lower_js_expr(&binary.right)?),
        }),
        Expr::Cond(cond) => Some(JsExprIR::Conditional {
            test: Box::new(lower_js_expr(&cond.test)?),
            consequent: Box::new(lower_js_expr(&cond.cons)?),
            alternate: Box::new(lower_js_expr(&cond.alt)?),
        }),
        Expr::Assign(assign) => Some(JsExprIR::Assign {
            op: assign.op.to_string(),
            left: Box::new(lower_assign_target_expr(&assign.left)?),
            right: Box::new(lower_js_expr(&assign.right)?),
        }),
        Expr::Update(update) => Some(JsExprIR::Update {
            op: update.op.to_string(),
            arg: Box::new(lower_js_expr(&update.arg)?),
            prefix: update.prefix,
        }),
        Expr::Call(call) => {
            let callee = match &call.callee {
                Callee::Expr(callee) => lower_js_expr(callee)?,
                Callee::Import(_) => JsExprIR::Ident("import".to_string()),
                Callee::Super(_) => return None,
            };
            let args = call
                .args
                .iter()
                .filter_map(|arg| lower_js_expr(&arg.expr))
                .collect();
            Some(JsExprIR::Call {
                callee: Box::new(callee),
                args,
            })
        }
        Expr::New(new_expr) => {
            let args = new_expr
                .args
                .as_ref()
                .map(|args| {
                    args.iter()
                        .filter_map(|arg| lower_js_expr(&arg.expr))
                        .collect::<Vec<_>>()
                })
                .unwrap_or_default();
            Some(JsExprIR::New {
                callee: Box::new(lower_js_expr(&new_expr.callee)?),
                args,
            })
        }
        Expr::Member(member) => {
            let property = match &member.prop {
                MemberProp::Ident(ident) => ident.sym.to_string(),
                MemberProp::Computed(computed) => match &*computed.expr {
                    Expr::Lit(Lit::Str(str)) => str.value.to_string_lossy().to_string(),
                    _ => return None,
                },
                MemberProp::PrivateName(_) => return None,
            };
            Some(JsExprIR::Member {
                object: Box::new(lower_js_expr(&member.obj)?),
                property,
            })
        }
        Expr::Tpl(template) => Some(JsExprIR::Template {
            quasis: template
                .quasis
                .iter()
                .map(|quasi| quasi.raw.to_string())
                .collect(),
            exprs: template
                .exprs
                .iter()
                .filter_map(|expr| lower_js_expr(expr))
                .collect(),
        }),
        Expr::Seq(sequence) => Some(JsExprIR::Sequence(
            sequence
                .exprs
                .iter()
                .filter_map(|expr| lower_js_expr(expr))
                .collect(),
        )),
        Expr::Paren(paren) => lower_js_expr(&paren.expr),
        _ => None,
    }
}

fn lower_class_methods(class: &Class) -> Vec<JsClassMethodIR> {
    class
        .body
        .iter()
        .filter_map(|member| match member {
            ClassMember::Constructor(constructor) => Some(JsClassMethodIR {
                name: "constructor".to_string(),
                kind: "constructor".to_string(),
                is_static: false,
                params: constructor
                    .params
                    .iter()
                    .filter_map(class_constructor_param_name)
                    .collect(),
                r#async: false,
                body: constructor
                    .body
                    .as_ref()
                    .map(lower_block_stmt)
                    .unwrap_or_default(),
            }),
            ClassMember::Method(method) => Some(JsClassMethodIR {
                name: prop_name(&method.key)?,
                kind: method_kind_name(&method.kind).to_string(),
                is_static: method.is_static,
                params: method
                    .function
                    .params
                    .iter()
                    .filter_map(|param| pat_name(&param.pat))
                    .collect(),
                r#async: method.function.is_async,
                body: method
                    .function
                    .body
                    .as_ref()
                    .map(lower_block_stmt)
                    .unwrap_or_default(),
            }),
            _ => None,
        })
        .collect()
}

fn method_kind_name(kind: &MethodKind) -> &'static str {
    match kind {
        MethodKind::Method => "method",
        MethodKind::Getter => "getter",
        MethodKind::Setter => "setter",
    }
}

fn class_constructor_param_name(param: &ParamOrTsParamProp) -> Option<String> {
    match param {
        ParamOrTsParamProp::Param(param) => pat_name(&param.pat),
        ParamOrTsParamProp::TsParamProp(_) => None,
    }
}

fn lower_assign_target_expr(target: &AssignTarget) -> Option<JsExprIR> {
    match target {
        AssignTarget::Simple(SimpleAssignTarget::Ident(ident)) => {
            Some(JsExprIR::Ident(ident.id.sym.to_string()))
        }
        AssignTarget::Simple(SimpleAssignTarget::Member(member)) => {
            let mut parts = member_path(member)?;
            let property = parts.pop()?;
            let object = parts
                .into_iter()
                .fold(None, |object: Option<JsExprIR>, part| {
                    Some(match object {
                        Some(object) => JsExprIR::Member {
                            object: Box::new(object),
                            property: part,
                        },
                        None => JsExprIR::Ident(part),
                    })
                })?;
            Some(JsExprIR::Member {
                object: Box::new(object),
                property,
            })
        }
        _ => None,
    }
}

fn lower_function_expr(function: &Function) -> Option<JsExprIR> {
    Some(JsExprIR::Function {
        params: function
            .params
            .iter()
            .filter_map(|param| pat_name(&param.pat))
            .collect(),
        r#async: function.is_async,
        body: lower_block_stmt(function.body.as_ref()?),
    })
}

fn lower_arrow_body(body: &BlockStmtOrExpr) -> Option<Vec<JsStmtIR>> {
    match body {
        BlockStmtOrExpr::BlockStmt(block) => Some(lower_block_stmt(block)),
        BlockStmtOrExpr::Expr(expr) => Some(vec![JsStmtIR::Return(Some(lower_js_expr(expr)?))]),
    }
}

fn lower_js_object_prop(prop: &PropOrSpread) -> Option<JsObjectPropIR> {
    let PropOrSpread::Prop(prop) = prop else {
        return None;
    };

    match &**prop {
        Prop::Shorthand(ident) => Some(JsObjectPropIR {
            key: ident.sym.to_string(),
            value: JsExprIR::Ident(ident.sym.to_string()),
        }),
        Prop::KeyValue(kv) => Some(JsObjectPropIR {
            key: prop_name(&kv.key)?,
            value: lower_js_expr(&kv.value)?,
        }),
        Prop::Assign(assign) => Some(JsObjectPropIR {
            key: assign.key.sym.to_string(),
            value: lower_js_expr(&assign.value)?,
        }),
        Prop::Getter(_) | Prop::Setter(_) | Prop::Method(_) => None,
    }
}

fn lower_js_lit(lit: &Lit) -> Option<JsValueIR> {
    match lit {
        Lit::Str(str) => Some(JsValueIR::String(str.value.to_string_lossy().to_string())),
        Lit::Bool(bool) => Some(JsValueIR::Bool(bool.value)),
        Lit::Null(_) => Some(JsValueIR::Null),
        Lit::Num(num) => Some(JsValueIR::Number(
            num.raw
                .as_ref()
                .map(ToString::to_string)
                .unwrap_or_else(|| num.value.to_string()),
        )),
        Lit::BigInt(bigint) => Some(JsValueIR::BigInt(
            bigint
                .raw
                .as_ref()
                .map(ToString::to_string)
                .unwrap_or_else(|| bigint.value.to_string()),
        )),
        Lit::Regex(regex) => Some(JsValueIR::RegExp {
            pattern: regex.exp.to_string(),
            flags: regex.flags.to_string(),
        }),
        _ => None,
    }
}

fn collect_cjs_imports_from_item(imports: &mut Vec<ImportIR>, item: &ModuleItem) {
    match item {
        ModuleItem::Stmt(Stmt::Decl(Decl::Var(var_decl))) => {
            for decl in &var_decl.decls {
                if let Some(init) = &decl.init {
                    collect_cjs_imports_from_expr(imports, init);
                }
            }
        }
        ModuleItem::Stmt(Stmt::Expr(expr_stmt)) => {
            collect_cjs_imports_from_expr(imports, &expr_stmt.expr);
        }
        _ => {}
    }
}

fn collect_cjs_imports_from_expr(imports: &mut Vec<ImportIR>, expr: &Expr) {
    if let Some(spec) = require_spec(expr) {
        imports.push(ImportIR {
            spec,
            kind: "cjs".to_string(),
            resolved: None,
        });
        return;
    }
    if let Some(spec) = dynamic_import_spec(expr) {
        imports.push(ImportIR {
            spec,
            kind: "dynamic".to_string(),
            resolved: None,
        });
        return;
    }

    match expr {
        Expr::Object(object) => {
            for prop in &object.props {
                if let PropOrSpread::Spread(spread) = prop {
                    collect_cjs_imports_from_expr(imports, &spread.expr);
                }
                if let PropOrSpread::Prop(prop) = prop {
                    match &**prop {
                        Prop::KeyValue(kv) => collect_cjs_imports_from_expr(imports, &kv.value),
                        Prop::Assign(assign) => {
                            collect_cjs_imports_from_expr(imports, &assign.value)
                        }
                        _ => {}
                    }
                }
            }
        }
        Expr::Call(call) => {
            if let Callee::Expr(callee) = &call.callee {
                collect_cjs_imports_from_expr(imports, callee);
            }
            for arg in &call.args {
                collect_cjs_imports_from_expr(imports, &arg.expr);
            }
        }
        Expr::Member(member) => {
            collect_cjs_imports_from_expr(imports, &member.obj);
        }
        Expr::Assign(assign) => {
            collect_cjs_imports_from_expr(imports, &assign.right);
        }
        _ => {}
    }
}

fn require_spec(expr: &Expr) -> Option<String> {
    let Expr::Call(call) = expr else {
        return None;
    };
    let Callee::Expr(callee) = &call.callee else {
        return None;
    };
    if !callee.is_ident_ref_to("require") {
        return None;
    }
    let first_arg = call.args.first()?;
    let Expr::Lit(Lit::Str(spec)) = &*first_arg.expr else {
        return None;
    };
    Some(spec.value.to_string_lossy().to_string())
}

fn dynamic_import_spec(expr: &Expr) -> Option<String> {
    let Expr::Call(call) = expr else {
        return None;
    };
    let Callee::Import(_) = &call.callee else {
        return None;
    };
    let first_arg = call.args.first()?;
    let Expr::Lit(Lit::Str(spec)) = &*first_arg.expr else {
        return None;
    };
    Some(spec.value.to_string_lossy().to_string())
}

fn collect_top_level_object_defs(module: &Module) -> BTreeMap<String, Vec<String>> {
    let mut object_defs = BTreeMap::new();
    for item in &module.body {
        let ModuleItem::Stmt(Stmt::Decl(Decl::Var(var_decl))) = item else {
            continue;
        };
        for decl in &var_decl.decls {
            let Some(name) = pat_name(&decl.name) else {
                continue;
            };
            let Some(init) = &decl.init else {
                continue;
            };
            let Expr::Object(object) = &**init else {
                continue;
            };
            let names = object_export_names(object.props.iter());
            if !names.is_empty() {
                object_defs.insert(name, names);
            }
        }
    }
    object_defs
}

fn collect_cjs_exports_from_item(
    exports: &mut Vec<String>,
    object_defs: &BTreeMap<String, Vec<String>>,
    item: &ModuleItem,
) {
    let ModuleItem::Stmt(Stmt::Expr(expr_stmt)) = item else {
        return;
    };
    let Expr::Assign(assign) = &*expr_stmt.expr else {
        return;
    };

    let Some(path) = assign_target_path(&assign.left) else {
        return;
    };

    if path == ["module", "exports"] {
        match &*assign.right {
            Expr::Object(object) => exports.extend(object_export_names(object.props.iter())),
            Expr::Ident(ident) => {
                if let Some(names) = object_defs.get(ident.sym.as_ref()) {
                    exports.extend(names.iter().cloned());
                }
            }
            _ => exports.push("*".to_string()),
        }
        return;
    }

    if path.len() == 3 && path[0] == "module" && path[1] == "exports" {
        exports.push(path[2].clone());
        return;
    }

    if path.len() == 2 && path[0] == "exports" {
        exports.push(path[1].clone());
    }
}

fn object_export_names<'a>(props: impl Iterator<Item = &'a PropOrSpread>) -> Vec<String> {
    let mut names = Vec::new();
    for prop in props {
        match prop {
            PropOrSpread::Spread(_) => names.push("*".to_string()),
            PropOrSpread::Prop(prop) => match &**prop {
                Prop::Shorthand(ident) => names.push(ident.sym.to_string()),
                Prop::KeyValue(kv) => {
                    if let Some(name) = prop_name(&kv.key) {
                        names.push(name);
                    }
                }
                Prop::Assign(assign) => names.push(assign.key.sym.to_string()),
                Prop::Getter(getter) => {
                    if let Some(name) = prop_name(&getter.key) {
                        names.push(name);
                    }
                }
                Prop::Setter(setter) => {
                    if let Some(name) = prop_name(&setter.key) {
                        names.push(name);
                    }
                }
                Prop::Method(method) => {
                    if let Some(name) = prop_name(&method.key) {
                        names.push(name);
                    }
                }
            },
        }
    }
    names
}

fn prop_name(prop: &PropName) -> Option<String> {
    match prop {
        PropName::Ident(ident) => Some(ident.sym.to_string()),
        PropName::Str(str) => Some(str.value.to_string_lossy().to_string()),
        PropName::Num(num) => Some(num.value.to_string()),
        _ => None,
    }
}

fn assign_target_path(target: &AssignTarget) -> Option<Vec<String>> {
    match target {
        AssignTarget::Simple(SimpleAssignTarget::Ident(ident)) => {
            Some(vec![ident.id.sym.to_string()])
        }
        AssignTarget::Simple(SimpleAssignTarget::Member(member)) => member_path(member),
        _ => None,
    }
}

fn member_path(member: &MemberExpr) -> Option<Vec<String>> {
    let mut path = expr_path(&member.obj)?;
    match &member.prop {
        MemberProp::Ident(ident) => path.push(ident.sym.to_string()),
        MemberProp::Computed(computed) => {
            let Expr::Lit(Lit::Str(str)) = &*computed.expr else {
                return None;
            };
            path.push(str.value.to_string_lossy().to_string());
        }
        MemberProp::PrivateName(_) => return None,
    }
    Some(path)
}

fn expr_path(expr: &Expr) -> Option<Vec<String>> {
    match expr {
        Expr::Ident(ident) => Some(vec![ident.sym.to_string()]),
        Expr::Member(member) => member_path(member),
        _ => None,
    }
}
