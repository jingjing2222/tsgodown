mod ir;

pub use ir::{
    DiagnosticIR, DiagnosticSourceIR, HandlerIR, HandlerParamIR, HandlerSemanticsIR, ImportIR,
    ModuleIR, ProgramIR, RouteIR,
};

pub fn analyze_compiler_entry(_file: &str, _src: &str) -> ProgramIR {
    ProgramIR {
        modules: vec![],
        routes: vec![],
        handlers: vec![],
        diagnostics: vec![],
    }
    .normalize()
}
