mod analyze;
mod contract;
mod normalize;

pub use analyze::analyze;
pub use contract::{
    AnalyzeConfig, AnalyzeRequest, AnalyzeResponse, Diagnostic, DiagnosticLevel, InputManifest,
    IrDocument, Route,
};

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn contract_roundtrip_request_json() {
        let request = AnalyzeRequest {
            manifest: InputManifest {
                entry: "src/server.ts".to_string(),
                framework: Some("compiler".to_string()),
            },
            config: AnalyzeConfig {
                profile: Some("default".to_string()),
            },
        };

        let encoded = serde_json::to_string(&request).expect("encode request");
        let decoded: AnalyzeRequest = serde_json::from_str(&encoded).expect("decode request");

        assert_eq!(decoded, request);
    }

    #[test]
    fn contract_roundtrip_response_json() {
        let response = AnalyzeResponse {
            ir: IrDocument {
                version: "0.1".to_string(),
                entry: "src/server.ts".to_string(),
                routes: vec![Route {
                    method: "GET".to_string(),
                    path: "/health".to_string(),
                }],
            },
            diagnostics: vec![Diagnostic {
                level: DiagnosticLevel::Info,
                code: "ENGINE_BOOTSTRAP".to_string(),
                message: "bootstrap analyzer executed".to_string(),
            }],
        };

        let encoded = serde_json::to_string(&response).expect("encode response");
        let decoded: AnalyzeResponse = serde_json::from_str(&encoded).expect("decode response");

        assert_eq!(decoded, response);
    }

    #[test]
    fn analyze_maps_manifest_to_ir_entry() {
        let request = AnalyzeRequest {
            manifest: InputManifest {
                entry: "src/server.ts".to_string(),
                framework: Some("compiler".to_string()),
            },
            config: AnalyzeConfig::default(),
        };

        let response = analyze(request);

        assert_eq!(response.ir.entry, "src/server.ts");
        assert_eq!(response.ir.version, "0.1");
    }
}
