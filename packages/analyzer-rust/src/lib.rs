mod ir;

pub use ir::{
    DiagnosticIR, DiagnosticSourceIR, HandlerIR, HandlerParamIR, HandlerSemanticsIR, ImportIR,
    ModuleIR, ProgramIR, RouteIR,
};

pub fn analyze_fastify_entry(_file: &str, _src: &str) -> ProgramIR {
    // TODO(compiler-mode): replace this legacy Fastify pattern-matching analyzer entrypoint
    // with compiler-mode IR builder flow.
    ProgramIR {
        modules: vec![],
        routes: vec![],
        handlers: vec![],
        diagnostics: vec![],
    }
}
