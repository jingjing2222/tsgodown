mod ast;
mod defs;
mod diagnostics;
mod ir;
mod register;
mod routes;
mod traversal;

pub use ir::{
    DiagnosticIR, DiagnosticSourceIR, HandlerIR, HandlerParamIR, HandlerSemanticsIR, ImportIR,
    ModuleIR, ProgramIR, RouteIR,
};

use defs::{collect_handler_definitions, collect_plugin_definitions, detect_root_instance_name};
use diagnostics::{dedupe_diagnostics, diag};
use traversal::analyze_scope;

pub fn analyze_fastify_entry(file: &str, src: &str) -> ProgramIR {
    let mut diagnostics = vec![];
    let mut routes = vec![];
    let mut handlers = vec![];

    let plugin_defs = collect_plugin_definitions(src);
    let handler_defs = collect_handler_definitions(src);

    let instance_name = detect_root_instance_name(src).unwrap_or_else(|| "fastify".to_string());
    analyze_scope(
        src,
        file,
        &instance_name,
        "",
        &plugin_defs,
        &handler_defs,
        &mut routes,
        &mut handlers,
        &mut diagnostics,
    );

    if src.contains("import(") {
        diagnostics.push(diag(
            file,
            "DYNAMIC_IMPORT_DETECTED",
            "dynamic import detected; use static import declarations for deterministic IR extraction.",
        ));
    }

    ProgramIR {
        modules: vec![],
        routes,
        handlers,
        diagnostics: dedupe_diagnostics(diagnostics),
    }
}
