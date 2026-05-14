use std::path::Path;

use swc_common::{sync::Lrc, FileName, SourceMap};
use swc_ecma_ast::{Decl, ExportSpecifier, Module, ModuleDecl, ModuleItem, Pat};
use swc_ecma_parser::{lexer::Lexer, EsSyntax, Parser, StringInput, Syntax, TsSyntax};

use crate::{DiagnosticIR, DiagnosticSourceIR, ImportIR};

#[derive(Debug)]
pub struct ParsedModule {
    pub imports: Vec<ImportIR>,
    pub exports: Vec<String>,
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
    imports.sort_by(|a, b| a.spec.cmp(&b.spec).then_with(|| a.kind.cmp(&b.kind)));
    imports
}

fn collect_exports_from_ast(module: &Module) -> Vec<String> {
    let mut exports = Vec::new();
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
