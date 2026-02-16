use crate::ir::{DiagnosticIR, DiagnosticSourceIR};

pub(crate) fn diag(file: &str, code: &str, message: &str) -> DiagnosticIR {
    DiagnosticIR {
        level: "warn".to_string(),
        code: code.to_string(),
        message: message.to_string(),
        source: Some(DiagnosticSourceIR {
            file: file.to_string(),
        }),
    }
}
