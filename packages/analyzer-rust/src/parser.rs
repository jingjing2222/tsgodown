use std::{collections::BTreeMap, path::Path};

use swc_common::{sync::Lrc, FileName, SourceMap};
use swc_ecma_ast::{
    AssignTarget, Callee, Expr, Lit, MemberExpr, MemberProp, Prop, PropName, PropOrSpread,
    SimpleAssignTarget, Stmt,
};
use swc_ecma_ast::{Decl, ExportSpecifier, Module, ModuleDecl, ModuleItem, Pat};
use swc_ecma_parser::{lexer::Lexer, EsSyntax, Parser, StringInput, Syntax, TsSyntax};

use crate::{
    DiagnosticIR, DiagnosticSourceIR, ExecutableModuleIR, ImportIR, JsExprIR, JsStmtIR, JsValueIR,
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
        ModuleItem::Stmt(Stmt::Decl(Decl::Var(var_decl))) => {
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
        _ => {}
    }
}

fn lower_js_expr(expr: &Expr) -> Option<JsExprIR> {
    match expr {
        Expr::Lit(lit) => lower_js_lit(lit).map(JsExprIR::Value),
        Expr::Ident(ident) => Some(JsExprIR::Ident(ident.sym.to_string())),
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
        _ => None,
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
