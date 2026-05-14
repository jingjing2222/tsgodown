mod analyze;
mod contract;

pub use analyze::analyze;
pub use contract::{
    AnalyzeConfig, AnalyzeRequest, AnalyzeResponse, Diagnostic, DiagnosticLevel, DiagnosticSource,
    Import, InputManifest, IrDocument, Module, Route,
};

#[cfg(test)]
mod tests {
    use super::*;
    use std::{fs, path::PathBuf};

    #[test]
    fn contract_roundtrip_request_json() {
        let request = AnalyzeRequest {
            manifest: InputManifest {
                entry: "src/server.ts".to_string(),
                framework: Some("compiler".to_string()),
            },
            cwd: None,
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
                modules: vec![],
                routes: vec![Route {
                    method: "GET".to_string(),
                    path: "/health".to_string(),
                }],
            },
            diagnostics: vec![Diagnostic {
                level: DiagnosticLevel::Info,
                code: "ENGINE_BOOTSTRAP".to_string(),
                message: "bootstrap analyzer executed".to_string(),
                source: None,
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
            cwd: None,
            config: AnalyzeConfig::default(),
        };

        let response = analyze(request);

        assert_eq!(response.ir.entry, "src/server.ts");
        assert_eq!(response.ir.version, "0.1");
    }

    #[test]
    fn analyze_reads_project_modules_from_cwd() {
        let root = temp_project("engine-core-analyze");
        write(
            &root,
            "src/index.js",
            r#"
import { value } from "./value.js";
export { value };
"#,
        );
        write(&root, "src/value.js", "export const value = 1;");

        let response = analyze(AnalyzeRequest {
            manifest: InputManifest {
                entry: "src/index.js".to_string(),
                framework: None,
            },
            cwd: Some(root.to_string_lossy().to_string()),
            config: AnalyzeConfig::default(),
        });

        assert_eq!(response.diagnostics, vec![]);
        assert_eq!(
            response
                .ir
                .modules
                .iter()
                .map(|module| module.source_path.as_str())
                .collect::<Vec<_>>(),
            vec!["src/index.js", "src/value.js"]
        );
        assert_eq!(
            response.ir.modules[0].imports[0].resolved.as_deref(),
            Some("src/value.js")
        );
    }

    fn write(root: &std::path::Path, rel: &str, source: &str) {
        let path = root.join(rel);
        fs::create_dir_all(path.parent().expect("parent")).expect("create parent");
        fs::write(path, source).expect("write source");
    }

    fn temp_project(name: &str) -> PathBuf {
        let root = std::env::temp_dir().join(format!("tsgodown-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).expect("create temp project");
        root
    }
}
