mod builder;
mod ir;

pub use ir::{
    DiagnosticIR, DiagnosticSourceIR, HandlerIR, HandlerParamIR, HandlerSemanticsIR, ImportIR,
    ModuleIR, ProgramIR, RouteIR,
};

pub fn analyze_compiler_entry(file: &str, src: &str) -> ProgramIR {
    builder::build_program_ir(file, src)
}
