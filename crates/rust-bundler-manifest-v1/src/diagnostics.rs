use crate::{ordering::level_rank, DiagnosticLevel, ManifestDiagnostic};

pub fn missing_bundle_link(map: &str) -> ManifestDiagnostic {
    ManifestDiagnostic {
        level: DiagnosticLevel::Error,
        code: "MISSING_BUNDLE_LINK".to_string(),
        message: format!(
            "sourcemap '{}' does not have a matching bundle artifact",
            map
        ),
    }
}

pub fn missing_sourcemap_link(file: &str, map_link: &str) -> ManifestDiagnostic {
    ManifestDiagnostic {
        level: DiagnosticLevel::Error,
        code: "MISSING_SOURCEMAP_LINK".to_string(),
        message: format!(
            "bundle '{}' is missing sourcemap link '{}'.",
            file, map_link
        )
        .trim_end_matches('.')
        .to_string(),
    }
}

pub fn missing_types_link(file: &str, base: &str) -> ManifestDiagnostic {
    ManifestDiagnostic {
        level: DiagnosticLevel::Error,
        code: "MISSING_TYPES_LINK".to_string(),
        message: format!(
            "bundle '{}' is missing declaration link (expected one of: {}.d.ts, {}.d.mts, {}.d.cts)",
            file, base, base, base
        ),
    }
}

pub fn sort_diagnostics(diagnostics: &mut [ManifestDiagnostic]) {
    diagnostics.sort_by(|a, b| {
        a.code
            .cmp(&b.code)
            .then_with(|| a.message.cmp(&b.message))
            .then_with(|| level_rank(&a.level).cmp(&level_rank(&b.level)))
    });
}
