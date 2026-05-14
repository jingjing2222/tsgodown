use std::path::PathBuf;

use crate::contract::{
    AnalyzeRequest, AnalyzeResponse, Diagnostic, DiagnosticLevel, DiagnosticSource, Import,
    IrDocument, Module, Route,
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
