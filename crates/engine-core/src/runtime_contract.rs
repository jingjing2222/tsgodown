use crate::contract::{Diagnostic, DiagnosticLevel};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProgramPurpose {
    Main,
    VectorSuite,
}

pub fn unsupported_codegen_diagnostic() -> Diagnostic {
    Diagnostic {
        level: DiagnosticLevel::Error,
        code: "EXECUTABLE_JS_CODEGEN_NOT_IMPLEMENTED".to_string(),
        message: "Executable JS semantics lowering is not implemented yet; failing closed."
            .to_string(),
        source: None,
    }
}

pub fn fail_closed_report_version(purpose: ProgramPurpose) -> &'static str {
    match purpose {
        ProgramPurpose::Main => "engine-core.fail-closed.main.v1",
        ProgramPurpose::VectorSuite => "engine-core.fail-closed.vector-suite.v1",
    }
}
