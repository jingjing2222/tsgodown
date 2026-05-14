mod builder;
mod graph;
mod ir;
mod parser;

pub use ir::{
    DiagnosticIR, DiagnosticSourceIR, ExecutableModuleIR, HandlerIR, HandlerParamIR,
    HandlerSemanticsIR, ImportIR, JsExprIR, JsStmtIR, JsValueIR, ModuleIR, ProgramIR, RouteIR,
};

pub fn analyze_compiler_entry(file: &str, src: &str) -> ProgramIR {
    builder::build_program_ir(file, src)
}

pub fn analyze_compiler_project(root: &std::path::Path, entry: &str) -> ProgramIR {
    graph::build_program_graph(root, entry)
}
