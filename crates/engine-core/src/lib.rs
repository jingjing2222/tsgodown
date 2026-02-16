use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct InputManifest {
    pub entry: String,
    #[serde(default)]
    pub framework: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
pub struct AnalyzeConfig {
    #[serde(default)]
    pub profile: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct AnalyzeRequest {
    pub manifest: InputManifest,
    #[serde(default)]
    pub config: AnalyzeConfig,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct IrDocument {
    pub version: String,
    pub entry: String,
    #[serde(default)]
    pub routes: Vec<Route>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Route {
    pub method: String,
    pub path: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Diagnostic {
    pub level: DiagnosticLevel,
    pub code: String,
    pub message: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum DiagnosticLevel {
    Info,
    Warning,
    Error,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct AnalyzeResponse {
    pub ir: IrDocument,
    #[serde(default)]
    pub diagnostics: Vec<Diagnostic>,
}

pub fn analyze(request: AnalyzeRequest) -> AnalyzeResponse {
    let framework = request
        .manifest
        .framework
        .clone()
        .unwrap_or_else(|| "unknown".to_string());

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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn contract_roundtrip_request_json() {
        let request = AnalyzeRequest {
            manifest: InputManifest {
                entry: "src/server.ts".to_string(),
                framework: Some("fastify".to_string()),
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
                framework: Some("fastify".to_string()),
            },
            config: AnalyzeConfig::default(),
        };

        let response = analyze(request);

        assert_eq!(response.ir.entry, "src/server.ts");
        assert_eq!(response.ir.version, "0.1");
    }
}
