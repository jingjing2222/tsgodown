use std::collections::HashSet;

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

pub(crate) fn dedupe_diagnostics(diagnostics: Vec<DiagnosticIR>) -> Vec<DiagnosticIR> {
    let mut seen = HashSet::new();
    let mut deduped = Vec::with_capacity(diagnostics.len());

    for diagnostic in diagnostics {
        let source_file = diagnostic
            .source
            .as_ref()
            .map(|source| source.file.as_str())
            .unwrap_or("");
        let key = format!(
            "{}\u{001F}{}\u{001F}{}\u{001F}{}",
            diagnostic.level, diagnostic.code, diagnostic.message, source_file
        );

        if seen.insert(key) {
            deduped.push(diagnostic);
        }
    }

    deduped
}
