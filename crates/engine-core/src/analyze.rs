use crate::contract::{AnalyzeRequest, AnalyzeResponse, Diagnostic, DiagnosticLevel, IrDocument};
use crate::normalize::framework_label;

pub fn analyze(request: AnalyzeRequest) -> AnalyzeResponse {
    let framework = framework_label(request.manifest.framework.as_deref());

    AnalyzeResponse {
        ir: IrDocument {
            version: "0.1".to_string(),
            entry: request.manifest.entry,
            routes: vec![],
        },
        diagnostics: vec![Diagnostic {
            level: DiagnosticLevel::Info,
            code: "ENGINE_CORE_BOOTSTRAP".to_string(),
            message: format!("engine-core analyze bootstrap executed (framework={framework})"),
        }],
    }
}
