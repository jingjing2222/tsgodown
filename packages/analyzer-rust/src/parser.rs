use std::{collections::BTreeMap, path::Path};

use swc_common::{sync::Lrc, FileName, SourceMap};
use swc_ecma_ast::{
    AssignTarget, Callee, Expr, Lit, MemberExpr, MemberProp, OptChainBase, Prop, PropName,
    PropOrSpread, SimpleAssignTarget, Stmt, VarDeclOrExpr,
};
use swc_ecma_ast::{
    BlockStmt, BlockStmtOrExpr, Class, ClassMember, Decl, ExportSpecifier, FnDecl, Function,
    ImportSpecifier, MethodKind, Module, ModuleDecl, ModuleItem, ParamOrTsParamProp, Pat,
};
use swc_ecma_parser::{lexer::Lexer, EsSyntax, Parser, StringInput, Syntax, TsSyntax};

use crate::{
    DiagnosticIR, DiagnosticSourceIR, ExecutableModuleIR, ImportBindingIR, ImportIR,
    JsArrayElementIR, JsClassMethodIR, JsExprIR, JsObjectPropIR, JsStmtIR, JsSwitchCaseIR,
    JsValueIR,
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
                        bindings: named
                            .specifiers
                            .iter()
                            .filter_map(export_named_import_binding)
                            .collect(),
                    });
                }
            }
            if let ModuleItem::ModuleDecl(ModuleDecl::ExportAll(export_all)) = item {
                imports.push(ImportIR {
                    spec: export_all.src.value.to_string_lossy().to_string(),
                    kind: "esm".to_string(),
                    resolved: None,
                    bindings: Vec::new(),
                });
            }
            continue;
        };
        imports.push(ImportIR {
            spec: import.src.value.to_string_lossy().to_string(),
            kind: "esm".to_string(),
            resolved: None,
            bindings: import
                .specifiers
                .iter()
                .filter_map(import_binding)
                .collect(),
        });
    }

    for item in &module.body {
        collect_cjs_imports_from_item(&mut imports, item);
    }

    for import in &mut imports {
        import.bindings.sort_by(|a, b| {
            (&a.local, &a.imported, &a.kind).cmp(&(&b.local, &b.imported, &b.kind))
        });
    }
    imports.sort_by(|a, b| {
        a.spec
            .cmp(&b.spec)
            .then_with(|| a.kind.cmp(&b.kind))
            .then_with(|| a.bindings.cmp(&b.bindings))
    });
    imports
}

fn import_binding(specifier: &ImportSpecifier) -> Option<ImportBindingIR> {
    match specifier {
        ImportSpecifier::Default(default) => Some(ImportBindingIR {
            local: default.local.sym.to_string(),
            imported: Some("default".to_string()),
            kind: "default".to_string(),
        }),
        ImportSpecifier::Namespace(namespace) => Some(ImportBindingIR {
            local: namespace.local.sym.to_string(),
            imported: Some("*".to_string()),
            kind: "namespace".to_string(),
        }),
        ImportSpecifier::Named(named) => Some(ImportBindingIR {
            local: named.local.sym.to_string(),
            imported: Some(
                named
                    .imported
                    .as_ref()
                    .map(module_export_name)
                    .unwrap_or_else(|| named.local.sym.to_string()),
            ),
            kind: "named".to_string(),
        }),
    }
}

fn export_named_import_binding(specifier: &ExportSpecifier) -> Option<ImportBindingIR> {
    match specifier {
        ExportSpecifier::Named(named) => Some(ImportBindingIR {
            local: named
                .exported
                .as_ref()
                .unwrap_or(&named.orig)
                .atom()
                .to_string(),
            imported: Some(named.orig.atom().to_string()),
            kind: "named".to_string(),
        }),
        ExportSpecifier::Default(default) => Some(ImportBindingIR {
            local: default.exported.sym.to_string(),
            imported: Some("default".to_string()),
            kind: "named".to_string(),
        }),
        ExportSpecifier::Namespace(namespace) => Some(ImportBindingIR {
            local: namespace.name.atom().to_string(),
            imported: Some("*".to_string()),
            kind: "namespace".to_string(),
        }),
    }
}

fn module_export_name(name: &swc_ecma_ast::ModuleExportName) -> String {
    match name {
        swc_ecma_ast::ModuleExportName::Ident(ident) => ident.sym.to_string(),
        swc_ecma_ast::ModuleExportName::Str(str) => str.value.to_string_lossy().to_string(),
    }
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
                exports.extend(pat_names(&decl.name));
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
        Pat::Assign(assign) => pat_name(&assign.left),
        _ => None,
    }
}

fn pat_names(pat: &Pat) -> Vec<String> {
    match pat {
        Pat::Ident(binding) => vec![binding.id.sym.to_string()],
        Pat::Assign(assign) => pat_names(&assign.left),
        Pat::Rest(rest) => pat_names(&rest.arg),
        Pat::Array(array) => array.elems.iter().flatten().flat_map(pat_names).collect(),
        Pat::Object(object) => object
            .props
            .iter()
            .flat_map(|prop| match prop {
                swc_ecma_ast::ObjectPatProp::KeyValue(kv) => pat_names(&kv.value),
                swc_ecma_ast::ObjectPatProp::Assign(assign) => {
                    vec![assign.key.sym.to_string()]
                }
                swc_ecma_ast::ObjectPatProp::Rest(rest) => pat_names(&rest.arg),
            })
            .collect(),
        _ => Vec::new(),
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
        ModuleItem::ModuleDecl(ModuleDecl::ExportDefaultExpr(export)) => {
            if let Some(expr) = lower_js_expr(&export.expr) {
                stmts.push(JsStmtIR::VarDecl {
                    name: "default".to_string(),
                    init: Some(expr),
                });
            }
        }
        ModuleItem::ModuleDecl(ModuleDecl::ExportDefaultDecl(export)) => match &export.decl {
            swc_ecma_ast::DefaultDecl::Fn(function_expr) => {
                if let Some(ident) = &function_expr.ident {
                    let (params, rest_param, body) = lower_param_bound_body(
                        function_expr.function.params.iter().map(|param| &param.pat),
                        function_expr
                            .function
                            .body
                            .as_ref()
                            .map(lower_block_stmt)
                            .unwrap_or_default(),
                    );
                    stmts.push(JsStmtIR::FunctionDecl {
                        name: ident.sym.to_string(),
                        params,
                        rest_param,
                        r#async: function_expr.function.is_async,
                        generator: function_expr.function.is_generator,
                        body,
                    });
                    stmts.push(JsStmtIR::VarDecl {
                        name: "default".to_string(),
                        init: Some(JsExprIR::Ident(ident.sym.to_string())),
                    });
                } else if let Some(function) = lower_function_expr(&function_expr.function) {
                    stmts.push(JsStmtIR::VarDecl {
                        name: "default".to_string(),
                        init: Some(function),
                    });
                }
            }
            swc_ecma_ast::DefaultDecl::Class(class_expr) => {
                let class = JsExprIR::Class {
                    super_class: class_expr
                        .class
                        .super_class
                        .as_deref()
                        .and_then(lower_js_expr)
                        .map(Box::new),
                    methods: lower_class_methods(&class_expr.class),
                };
                if let Some(ident) = &class_expr.ident {
                    stmts.push(JsStmtIR::VarDecl {
                        name: ident.sym.to_string(),
                        init: Some(class.clone()),
                    });
                    stmts.push(JsStmtIR::VarDecl {
                        name: "default".to_string(),
                        init: Some(JsExprIR::Ident(ident.sym.to_string())),
                    });
                } else {
                    stmts.push(JsStmtIR::VarDecl {
                        name: "default".to_string(),
                        init: Some(class),
                    });
                }
            }
            _ => {}
        },
        ModuleItem::ModuleDecl(ModuleDecl::ExportNamed(named)) if named.src.is_none() => {
            for specifier in &named.specifiers {
                if let ExportSpecifier::Named(named_specifier) = specifier {
                    let local = named_specifier.orig.atom().to_string();
                    let exported = named_specifier
                        .exported
                        .as_ref()
                        .unwrap_or(&named_specifier.orig)
                        .atom()
                        .to_string();
                    if exported == local {
                        continue;
                    }
                    stmts.push(JsStmtIR::VarDecl {
                        name: exported,
                        init: Some(JsExprIR::Ident(local)),
                    });
                }
            }
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
            if let Expr::Yield(yield_expr) = &*expr_stmt.expr {
                stmts.push(JsStmtIR::Yield {
                    value: yield_expr.arg.as_deref().and_then(lower_js_expr),
                    delegate: yield_expr.delegate,
                });
            } else if let Some(expr) = lower_js_expr(&expr_stmt.expr) {
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
            if let Expr::Yield(yield_expr) = &*expr_stmt.expr {
                stmts.push(JsStmtIR::Yield {
                    value: yield_expr.arg.as_deref().and_then(lower_js_expr),
                    delegate: yield_expr.delegate,
                });
            } else if let Some(expr) = lower_js_expr(&expr_stmt.expr) {
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
            if let Some((left, mut prefix)) = lower_for_head_binding(&for_of.left) {
                if let Some(right) = lower_js_expr(&for_of.right) {
                    prefix.extend(lower_stmt_as_block(&for_of.body));
                    stmts.push(JsStmtIR::ForOf {
                        left,
                        right,
                        body: prefix,
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
        Stmt::DoWhile(do_while_stmt) => {
            if let Some(test) = lower_js_expr(&do_while_stmt.test) {
                stmts.push(JsStmtIR::DoWhile {
                    body: lower_stmt_as_block(&do_while_stmt.body),
                    test,
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
        Stmt::Labeled(labeled_stmt) => {
            stmts.push(JsStmtIR::Label {
                label: labeled_stmt.label.sym.to_string(),
                body: lower_stmt_as_block(&labeled_stmt.body),
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

fn lower_for_head_binding(head: &swc_ecma_ast::ForHead) -> Option<(String, Vec<JsStmtIR>)> {
    let pat = match head {
        swc_ecma_ast::ForHead::VarDecl(var_decl) => &var_decl.decls.first()?.name,
        swc_ecma_ast::ForHead::Pat(pat) => pat,
        swc_ecma_ast::ForHead::UsingDecl(_) => return None,
    };
    if let Some(ident) = pat.as_ident() {
        return Some((ident.id.sym.to_string(), Vec::new()));
    }
    let temp_name = "__tsgodown_forof_value".to_string();
    let mut prefix = Vec::new();
    lower_pat_decl_stmts(&mut prefix, pat, JsExprIR::Ident(temp_name.clone()));
    Some((temp_name, prefix))
}

fn lower_fn_decl_stmt(stmts: &mut Vec<JsStmtIR>, function: &FnDecl) {
    let Some(body) = &function.function.body else {
        return;
    };
    let (params, rest_param, body) = lower_param_bound_body(
        function.function.params.iter().map(|param| &param.pat),
        lower_block_stmt(body),
    );
    stmts.push(JsStmtIR::FunctionDecl {
        name: function.ident.sym.to_string(),
        params,
        rest_param,
        r#async: function.function.is_async,
        generator: function.function.is_generator,
        body,
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
        let init = decl
            .init
            .as_deref()
            .and_then(lower_js_expr)
            .unwrap_or(JsExprIR::Value(JsValueIR::Undefined));
        lower_pat_decl_stmts(stmts, &decl.name, init);
    }
}

fn lower_pat_decl_stmts(stmts: &mut Vec<JsStmtIR>, pat: &Pat, init: JsExprIR) {
    match pat {
        Pat::Ident(binding) => {
            stmts.push(JsStmtIR::VarDecl {
                name: binding.id.sym.to_string(),
                init: Some(init),
            });
        }
        Pat::Assign(assign) => {
            let Some(default_expr) = lower_js_expr(&assign.right) else {
                return;
            };
            lower_pat_decl_stmts(stmts, &assign.left, defaulted_expr(init, default_expr));
        }
        Pat::Array(array) => {
            let temp_name = format!("__tsgodown_destructure_{}", stmts.len());
            stmts.push(JsStmtIR::VarDecl {
                name: temp_name.clone(),
                init: Some(init),
            });
            for (index, elem) in array.elems.iter().enumerate() {
                let Some(elem) = elem else {
                    continue;
                };
                lower_pat_decl_stmts(
                    stmts,
                    elem,
                    JsExprIR::Member {
                        object: Box::new(JsExprIR::Ident(temp_name.clone())),
                        property: index.to_string(),
                        computed: None,
                        optional: false,
                    },
                );
            }
        }
        Pat::Object(object) => {
            let temp_name = format!("__tsgodown_destructure_{}", stmts.len());
            stmts.push(JsStmtIR::VarDecl {
                name: temp_name.clone(),
                init: Some(init),
            });
            let mut excluded = Vec::new();
            for prop in &object.props {
                match prop {
                    swc_ecma_ast::ObjectPatProp::KeyValue(kv) => {
                        let Some(property) = prop_name(&kv.key) else {
                            continue;
                        };
                        excluded.push(property.clone());
                        lower_pat_decl_stmts(
                            stmts,
                            &kv.value,
                            JsExprIR::Member {
                                object: Box::new(JsExprIR::Ident(temp_name.clone())),
                                property,
                                computed: None,
                                optional: false,
                            },
                        );
                    }
                    swc_ecma_ast::ObjectPatProp::Assign(assign) => {
                        let property = assign.key.sym.to_string();
                        excluded.push(property.clone());
                        let member = JsExprIR::Member {
                            object: Box::new(JsExprIR::Ident(temp_name.clone())),
                            property: property.clone(),
                            computed: None,
                            optional: false,
                        };
                        let init = match assign.value.as_deref().and_then(lower_js_expr) {
                            Some(default_expr) => defaulted_expr(member, default_expr),
                            None => member,
                        };
                        stmts.push(JsStmtIR::VarDecl {
                            name: property,
                            init: Some(init),
                        });
                    }
                    swc_ecma_ast::ObjectPatProp::Rest(rest) => {
                        lower_pat_decl_stmts(
                            stmts,
                            &rest.arg,
                            JsExprIR::ObjectRest {
                                object: Box::new(JsExprIR::Ident(temp_name.clone())),
                                excluded: excluded.clone(),
                            },
                        );
                    }
                }
            }
        }
        _ => {}
    }
}

fn defaulted_expr(value: JsExprIR, fallback: JsExprIR) -> JsExprIR {
    JsExprIR::Conditional {
        test: Box::new(JsExprIR::Binary {
            op: "===".to_string(),
            left: Box::new(value.clone()),
            right: Box::new(JsExprIR::Value(JsValueIR::Undefined)),
        }),
        consequent: Box::new(fallback),
        alternate: Box::new(value),
    }
}

fn lower_param_bound_body<'a>(
    params: impl IntoIterator<Item = &'a Pat>,
    body: Vec<JsStmtIR>,
) -> (Vec<String>, Option<String>, Vec<JsStmtIR>) {
    let mut bound_params = Vec::new();
    let mut rest_param = None;
    let mut prefix = Vec::new();

    for (index, param) in params.into_iter().enumerate() {
        if let Pat::Ident(binding) = param {
            bound_params.push(binding.id.sym.to_string());
            continue;
        }
        if let Pat::Rest(rest) = param {
            if let Pat::Ident(binding) = &*rest.arg {
                rest_param = Some(binding.id.sym.to_string());
            } else {
                let temp_name = format!("__tsgodown_rest_{index}");
                rest_param = Some(temp_name.clone());
                lower_pat_decl_stmts(&mut prefix, &rest.arg, JsExprIR::Ident(temp_name));
            }
            continue;
        }

        let temp_name = format!("__tsgodown_param_{index}");
        bound_params.push(temp_name.clone());
        lower_pat_decl_stmts(&mut prefix, param, JsExprIR::Ident(temp_name));
    }

    prefix.extend(body);
    (bound_params, rest_param, prefix)
}

fn lower_js_expr(expr: &Expr) -> Option<JsExprIR> {
    match expr {
        Expr::Lit(lit) => lower_js_lit(lit).map(JsExprIR::Value),
        Expr::Ident(ident) => Some(JsExprIR::Ident(ident.sym.to_string())),
        Expr::This(_) => Some(JsExprIR::This),
        Expr::Array(array) => {
            let mut items = Vec::new();
            let mut spread_items = Vec::new();
            let mut has_spread = false;
            for elem in &array.elems {
                let Some(elem) = elem else {
                    return None;
                };
                let value = lower_js_expr(&elem.expr)?;
                if elem.spread.is_some() {
                    has_spread = true;
                }
                spread_items.push(JsArrayElementIR {
                    spread: elem.spread.is_some(),
                    value: value.clone(),
                });
                items.push(value);
            }
            if has_spread {
                return Some(JsExprIR::ArraySpread(spread_items));
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
        Expr::Arrow(arrow) => {
            let (params, rest_param, body) =
                lower_param_bound_body(arrow.params.iter(), lower_arrow_body(&arrow.body)?);
            Some(JsExprIR::Function {
                params,
                rest_param,
                r#async: arrow.is_async,
                generator: false,
                lexical_this: true,
                body,
            })
        }
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
                Callee::Super(_) => JsExprIR::Super,
            };
            let args = lower_call_args(&call.args);
            Some(JsExprIR::Call {
                callee: Box::new(callee),
                args,
                optional: false,
            })
        }
        Expr::New(new_expr) => {
            let args = new_expr
                .args
                .as_ref()
                .map(|args| lower_call_args(args))
                .unwrap_or_default();
            Some(JsExprIR::New {
                callee: Box::new(lower_js_expr(&new_expr.callee)?),
                args,
            })
        }
        Expr::Member(member) => lower_member_expr(member, false),
        Expr::OptChain(chain) => match &*chain.base {
            OptChainBase::Member(member) => lower_member_expr(member, true),
            OptChainBase::Call(call) => Some(JsExprIR::Call {
                callee: Box::new(lower_js_expr(&call.callee)?),
                args: lower_call_args(&call.args),
                optional: true,
            }),
        },
        Expr::Tpl(template) => Some(JsExprIR::Template {
            quasis: template
                .quasis
                .iter()
                .map(|quasi| {
                    quasi
                        .cooked
                        .as_ref()
                        .map(|value| value.to_string_lossy().to_string())
                        .unwrap_or_else(|| quasi.raw.to_string())
                })
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

fn lower_member_expr(member: &MemberExpr, optional: bool) -> Option<JsExprIR> {
    let (property, computed) = match &member.prop {
        MemberProp::Ident(ident) => (ident.sym.to_string(), None),
        MemberProp::Computed(computed) => match &*computed.expr {
            Expr::Lit(Lit::Str(str)) => (str.value.to_string_lossy().to_string(), None),
            Expr::Lit(Lit::Num(num)) => (
                num.raw
                    .as_ref()
                    .map(ToString::to_string)
                    .unwrap_or_else(|| num.value.to_string()),
                None,
            ),
            expr => ("".to_string(), Some(Box::new(lower_js_expr(expr)?))),
        },
        MemberProp::PrivateName(private) => (private_name(private), None),
    };
    Some(JsExprIR::Member {
        object: Box::new(lower_js_expr(&member.obj)?),
        property,
        computed,
        optional,
    })
}

fn lower_class_methods(class: &Class) -> Vec<JsClassMethodIR> {
    let instance_fields = lower_class_instance_fields(class);
    let mut has_constructor = false;
    let mut methods = Vec::new();

    for member in &class.body {
        match member {
            ClassMember::Constructor(constructor) => {
                has_constructor = true;
                let mut body = instance_fields.clone();
                body.extend(
                    constructor
                        .body
                        .as_ref()
                        .map(lower_block_stmt)
                        .unwrap_or_default(),
                );
                let (params, rest_param, body) = lower_param_bound_body(
                    constructor.params.iter().filter_map(constructor_param_pat),
                    body,
                );
                methods.push(JsClassMethodIR {
                    name: "constructor".to_string(),
                    kind: "constructor".to_string(),
                    is_static: false,
                    params,
                    rest_param,
                    r#async: false,
                    generator: false,
                    body,
                });
            }
            ClassMember::Method(method) => {
                let Some(name) = prop_name(&method.key) else {
                    continue;
                };
                let (params, rest_param, body) = lower_param_bound_body(
                    method.function.params.iter().map(|param| &param.pat),
                    method
                        .function
                        .body
                        .as_ref()
                        .map(lower_block_stmt)
                        .unwrap_or_default(),
                );
                methods.push(JsClassMethodIR {
                    name,
                    kind: method_kind_name(&method.kind).to_string(),
                    is_static: method.is_static,
                    params,
                    rest_param,
                    r#async: method.function.is_async,
                    generator: method.function.is_generator,
                    body,
                });
            }
            ClassMember::PrivateMethod(method) => {
                let (params, rest_param, body) = lower_param_bound_body(
                    method.function.params.iter().map(|param| &param.pat),
                    method
                        .function
                        .body
                        .as_ref()
                        .map(lower_block_stmt)
                        .unwrap_or_default(),
                );
                methods.push(JsClassMethodIR {
                    name: private_name(&method.key),
                    kind: method_kind_name(&method.kind).to_string(),
                    is_static: method.is_static,
                    params,
                    rest_param,
                    r#async: method.function.is_async,
                    generator: method.function.is_generator,
                    body,
                });
            }
            _ => {}
        }
    }

    if !instance_fields.is_empty() && !has_constructor {
        methods.insert(
            0,
            JsClassMethodIR {
                name: "constructor".to_string(),
                kind: "constructor".to_string(),
                is_static: false,
                params: Vec::new(),
                rest_param: None,
                r#async: false,
                generator: false,
                body: instance_fields,
            },
        );
    }

    methods
}

fn lower_class_instance_fields(class: &Class) -> Vec<JsStmtIR> {
    let mut stmts = Vec::new();
    for member in &class.body {
        match member {
            ClassMember::ClassProp(prop) if !prop.is_static => {
                let Some(property) = prop_name(&prop.key) else {
                    continue;
                };
                let init = prop
                    .value
                    .as_deref()
                    .and_then(lower_js_expr)
                    .unwrap_or(JsExprIR::Value(JsValueIR::Undefined));
                stmts.push(class_field_initializer(property, init));
            }
            ClassMember::PrivateProp(prop) if !prop.is_static => {
                let init = prop
                    .value
                    .as_deref()
                    .and_then(lower_js_expr)
                    .unwrap_or(JsExprIR::Value(JsValueIR::Undefined));
                stmts.push(class_field_initializer(private_name(&prop.key), init));
            }
            _ => {}
        }
    }
    stmts
}

fn class_field_initializer(property: String, init: JsExprIR) -> JsStmtIR {
    JsStmtIR::Expr(JsExprIR::Assign {
        op: "=".to_string(),
        left: Box::new(JsExprIR::Member {
            object: Box::new(JsExprIR::This),
            property,
            computed: None,
            optional: false,
        }),
        right: Box::new(init),
    })
}

fn private_name(name: &swc_ecma_ast::PrivateName) -> String {
    format!("#{}", name.name)
}

fn method_kind_name(kind: &MethodKind) -> &'static str {
    match kind {
        MethodKind::Method => "method",
        MethodKind::Getter => "getter",
        MethodKind::Setter => "setter",
    }
}

fn constructor_param_pat(param: &ParamOrTsParamProp) -> Option<&Pat> {
    match param {
        ParamOrTsParamProp::Param(param) => Some(&param.pat),
        ParamOrTsParamProp::TsParamProp(_) => None,
    }
}

fn lower_assign_target_expr(target: &AssignTarget) -> Option<JsExprIR> {
    match target {
        AssignTarget::Simple(SimpleAssignTarget::Ident(ident)) => {
            Some(JsExprIR::Ident(ident.id.sym.to_string()))
        }
        AssignTarget::Simple(SimpleAssignTarget::Member(member)) => {
            lower_member_expr(member, false)
        }
        _ => None,
    }
}

fn lower_function_expr(function: &Function) -> Option<JsExprIR> {
    let (params, rest_param, body) = lower_param_bound_body(
        function.params.iter().map(|param| &param.pat),
        lower_block_stmt(function.body.as_ref()?),
    );
    Some(JsExprIR::Function {
        params,
        rest_param,
        r#async: function.is_async,
        generator: function.is_generator,
        lexical_this: false,
        body,
    })
}

fn lower_arrow_body(body: &BlockStmtOrExpr) -> Option<Vec<JsStmtIR>> {
    match body {
        BlockStmtOrExpr::BlockStmt(block) => Some(lower_block_stmt(block)),
        BlockStmtOrExpr::Expr(expr) => Some(vec![JsStmtIR::Return(Some(lower_js_expr(expr)?))]),
    }
}

fn lower_call_args(args: &[swc_ecma_ast::ExprOrSpread]) -> Vec<JsExprIR> {
    args.iter()
        .filter_map(|arg| {
            let expr = lower_js_expr(&arg.expr)?;
            if arg.spread.is_some() {
                Some(JsExprIR::Spread {
                    arg: Box::new(expr),
                })
            } else {
                Some(expr)
            }
        })
        .collect()
}

fn lower_js_object_prop(prop: &PropOrSpread) -> Option<JsObjectPropIR> {
    let PropOrSpread::Prop(prop) = prop else {
        if let PropOrSpread::Spread(spread) = prop {
            return Some(JsObjectPropIR {
                key: String::new(),
                key_expr: None,
                value: lower_js_expr(&spread.expr)?,
                spread: true,
            });
        }
        return None;
    };

    match &**prop {
        Prop::Shorthand(ident) => Some(JsObjectPropIR {
            key: ident.sym.to_string(),
            key_expr: None,
            value: JsExprIR::Ident(ident.sym.to_string()),
            spread: false,
        }),
        Prop::KeyValue(kv) => {
            let (key, key_expr) = lower_object_key(&kv.key)?;
            Some(JsObjectPropIR {
                key,
                key_expr,
                value: lower_js_expr(&kv.value)?,
                spread: false,
            })
        }
        Prop::Assign(assign) => Some(JsObjectPropIR {
            key: assign.key.sym.to_string(),
            key_expr: None,
            value: lower_js_expr(&assign.value)?,
            spread: false,
        }),
        Prop::Method(method) => {
            let (key, key_expr) = lower_object_key(&method.key)?;
            let (params, rest_param, body) = lower_param_bound_body(
                method.function.params.iter().map(|param| &param.pat),
                method
                    .function
                    .body
                    .as_ref()
                    .map(lower_block_stmt)
                    .unwrap_or_default(),
            );
            Some(JsObjectPropIR {
                key,
                key_expr,
                value: JsExprIR::Function {
                    params,
                    rest_param,
                    r#async: method.function.is_async,
                    generator: method.function.is_generator,
                    lexical_this: false,
                    body,
                },
                spread: false,
            })
        }
        Prop::Getter(_) | Prop::Setter(_) => None,
    }
}

fn lower_object_key(key: &PropName) -> Option<(String, Option<JsExprIR>)> {
    match key {
        PropName::Computed(computed) => {
            Some(("".to_string(), Some(lower_js_expr(&computed.expr)?)))
        }
        prop => Some((prop_name(prop)?, None)),
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
    if let ModuleItem::Stmt(stmt) = item {
        collect_cjs_imports_from_stmt(imports, stmt);
    }
}

fn collect_cjs_imports_from_stmt(imports: &mut Vec<ImportIR>, stmt: &Stmt) {
    match stmt {
        Stmt::Decl(Decl::Var(var_decl)) => {
            for decl in &var_decl.decls {
                if let Some(init) = &decl.init {
                    if let Some(spec) = require_spec(init) {
                        imports.push(ImportIR {
                            spec,
                            kind: "cjs".to_string(),
                            resolved: None,
                            bindings: cjs_require_bindings(&decl.name),
                        });
                        continue;
                    }
                    collect_cjs_imports_from_expr(imports, init);
                }
            }
        }
        Stmt::Decl(Decl::Fn(fn_decl)) => {
            collect_cjs_imports_from_function(imports, &fn_decl.function);
        }
        Stmt::Decl(Decl::Class(class_decl)) => {
            collect_cjs_imports_from_class(imports, &class_decl.class);
        }
        Stmt::Expr(expr_stmt) => {
            collect_cjs_imports_from_expr(imports, &expr_stmt.expr);
        }
        Stmt::If(if_stmt) => {
            collect_cjs_imports_from_expr(imports, &if_stmt.test);
            collect_cjs_imports_from_stmt(imports, &if_stmt.cons);
            if let Some(alt) = &if_stmt.alt {
                collect_cjs_imports_from_stmt(imports, alt);
            }
        }
        Stmt::Block(block) => {
            for stmt in &block.stmts {
                collect_cjs_imports_from_stmt(imports, stmt);
            }
        }
        Stmt::Return(return_stmt) => {
            if let Some(arg) = &return_stmt.arg {
                collect_cjs_imports_from_expr(imports, arg);
            }
        }
        Stmt::Throw(throw_stmt) => collect_cjs_imports_from_expr(imports, &throw_stmt.arg),
        Stmt::Try(try_stmt) => {
            let start = imports.len();
            collect_cjs_imports_from_block(imports, &try_stmt.block);
            if let Some(handler) = &try_stmt.handler {
                collect_cjs_imports_from_block(imports, &handler.body);
            }
            if let Some(finalizer) = &try_stmt.finalizer {
                collect_cjs_imports_from_block(imports, finalizer);
            }
            for import in &mut imports[start..] {
                if import.kind == "cjs" {
                    import.kind = "cjs-optional".to_string();
                }
            }
        }
        Stmt::For(for_stmt) => {
            if let Some(init) = &for_stmt.init {
                match init {
                    VarDeclOrExpr::VarDecl(var_decl) => collect_cjs_imports_from_stmt(
                        imports,
                        &Stmt::Decl(Decl::Var(var_decl.clone())),
                    ),
                    VarDeclOrExpr::Expr(expr) => collect_cjs_imports_from_expr(imports, expr),
                }
            }
            if let Some(test) = &for_stmt.test {
                collect_cjs_imports_from_expr(imports, test);
            }
            if let Some(update) = &for_stmt.update {
                collect_cjs_imports_from_expr(imports, update);
            }
            collect_cjs_imports_from_stmt(imports, &for_stmt.body);
        }
        Stmt::While(while_stmt) => {
            collect_cjs_imports_from_expr(imports, &while_stmt.test);
            collect_cjs_imports_from_stmt(imports, &while_stmt.body);
        }
        Stmt::DoWhile(do_while_stmt) => {
            collect_cjs_imports_from_stmt(imports, &do_while_stmt.body);
            collect_cjs_imports_from_expr(imports, &do_while_stmt.test);
        }
        _ => {}
    }
}

fn cjs_require_bindings(pat: &Pat) -> Vec<ImportBindingIR> {
    match pat {
        Pat::Ident(ident) => vec![ImportBindingIR {
            local: ident.id.sym.to_string(),
            imported: None,
            kind: "require".to_string(),
        }],
        Pat::Object(object) => object
            .props
            .iter()
            .filter_map(|prop| match prop {
                swc_ecma_ast::ObjectPatProp::KeyValue(kv) => Some(ImportBindingIR {
                    local: pat_name(&kv.value)?,
                    imported: Some(prop_name(&kv.key)?),
                    kind: "destructure".to_string(),
                }),
                swc_ecma_ast::ObjectPatProp::Assign(assign) => Some(ImportBindingIR {
                    local: assign.key.sym.to_string(),
                    imported: Some(assign.key.sym.to_string()),
                    kind: "destructure".to_string(),
                }),
                swc_ecma_ast::ObjectPatProp::Rest(_) => None,
            })
            .collect(),
        _ => Vec::new(),
    }
}

fn collect_cjs_imports_from_expr(imports: &mut Vec<ImportIR>, expr: &Expr) {
    if let Some(spec) = require_spec(expr) {
        imports.push(ImportIR {
            spec,
            kind: "cjs".to_string(),
            resolved: None,
            bindings: Vec::new(),
        });
        return;
    }
    if let Some(spec) = dynamic_import_spec(expr) {
        imports.push(ImportIR {
            spec,
            kind: "dynamic".to_string(),
            resolved: None,
            bindings: Vec::new(),
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
        Expr::Fn(function) => {
            collect_cjs_imports_from_function(imports, &function.function);
        }
        Expr::Arrow(arrow) => match &*arrow.body {
            BlockStmtOrExpr::BlockStmt(block) => collect_cjs_imports_from_block(imports, block),
            BlockStmtOrExpr::Expr(expr) => collect_cjs_imports_from_expr(imports, expr),
        },
        Expr::Class(class) => {
            collect_cjs_imports_from_class(imports, &class.class);
        }
        Expr::Cond(cond) => {
            collect_cjs_imports_from_expr(imports, &cond.test);
            collect_cjs_imports_from_expr(imports, &cond.cons);
            collect_cjs_imports_from_expr(imports, &cond.alt);
        }
        Expr::Await(await_expr) => {
            collect_cjs_imports_from_expr(imports, &await_expr.arg);
        }
        Expr::Unary(unary) => {
            collect_cjs_imports_from_expr(imports, &unary.arg);
        }
        Expr::Bin(binary) => {
            collect_cjs_imports_from_expr(imports, &binary.left);
            collect_cjs_imports_from_expr(imports, &binary.right);
        }
        Expr::Tpl(template) => {
            for expr in &template.exprs {
                collect_cjs_imports_from_expr(imports, expr);
            }
        }
        _ => {}
    }
}

fn collect_cjs_imports_from_function(imports: &mut Vec<ImportIR>, function: &Function) {
    if let Some(body) = &function.body {
        collect_cjs_imports_from_block(imports, body);
    }
}

fn collect_cjs_imports_from_class(imports: &mut Vec<ImportIR>, class: &Class) {
    if let Some(super_class) = &class.super_class {
        collect_cjs_imports_from_expr(imports, super_class);
    }
    for member in &class.body {
        match member {
            ClassMember::Constructor(constructor) => {
                if let Some(body) = &constructor.body {
                    collect_cjs_imports_from_block(imports, body);
                }
            }
            ClassMember::Method(method) => {
                collect_cjs_imports_from_function(imports, &method.function)
            }
            ClassMember::PrivateMethod(method) => {
                collect_cjs_imports_from_function(imports, &method.function)
            }
            ClassMember::ClassProp(prop) => {
                if let Some(value) = &prop.value {
                    collect_cjs_imports_from_expr(imports, value);
                }
            }
            ClassMember::PrivateProp(prop) => {
                if let Some(value) = &prop.value {
                    collect_cjs_imports_from_expr(imports, value);
                }
            }
            ClassMember::StaticBlock(block) => collect_cjs_imports_from_block(imports, &block.body),
            _ => {}
        }
    }
}

fn collect_cjs_imports_from_block(imports: &mut Vec<ImportIR>, block: &BlockStmt) {
    for stmt in &block.stmts {
        collect_cjs_imports_from_stmt(imports, stmt);
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
        MemberProp::PrivateName(private) => path.push(private_name(private)),
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
