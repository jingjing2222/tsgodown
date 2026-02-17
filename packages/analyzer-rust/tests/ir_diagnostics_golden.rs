use std::fs;
use std::path::{Path, PathBuf};

use analyzer_rust::analyze_fastify_entry;
use pretty_assertions::assert_eq;

fn fixture_path(name: &str) -> PathBuf {
    let mut path = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    path.push("tests/fixtures");
    path.push(name);
    path
}

fn fixture(name: &str) -> (String, String) {
    let path = fixture_path(name);
    let src = fs::read_to_string(&path).expect("fixture must exist");
    (path.to_string_lossy().to_string(), src)
}

fn render_contract(name: &str) -> String {
    let (file, src) = fixture(name);
    let ir = analyze_fastify_entry(&file, &src);

    let routes = if ir.routes.is_empty() {
        "routes=[]".to_string()
    } else {
        let body = ir
            .routes
            .iter()
            .map(|r| format!("{} {} -> {}", r.method, r.path, r.handler_ref))
            .collect::<Vec<_>>()
            .join(", ");
        format!("routes=[{}]", body)
    };

    let handlers = if ir.handlers.is_empty() {
        "handlers=[]".to_string()
    } else {
        let body = ir
            .handlers
            .iter()
            .map(|h| {
                format!(
                    "{}(req={:?};async={};mode={})",
                    h.id,
                    h.params
                        .iter()
                        .map(|p| (&p.name, &p.role))
                        .collect::<Vec<_>>(),
                    h.r#async,
                    h.semantics
                        .as_ref()
                        .map(|s| s.response_mode.as_str())
                        .unwrap_or("<missing>"),
                )
            })
            .collect::<Vec<_>>()
            .join(", ");
        format!("handlers=[{}]", body)
    };

    let mut out = vec![routes, handlers, "diagnostics=[".to_string()];
    for d in &ir.diagnostics {
        let file_name = d
            .source
            .as_ref()
            .and_then(|s| Path::new(&s.file).file_name())
            .and_then(|v| v.to_str())
            .unwrap_or("<missing>");
        out.push(format!(
            "  {{level={},code={},message={},source.file={}}},",
            d.level, d.code, d.message, file_name
        ));
    }
    out.push("]".to_string());
    out.push(String::new());
    out.join("\n")
}

#[test]
fn diagnostics_contract_golden_duplicate_unsupported_patterns() {
    let actual = render_contract("duplicate-diagnostics-fastify.fixture.txt");
    let expected = fs::read_to_string(fixture_path("duplicate-diagnostics-fastify.golden.txt"))
        .expect("golden file must exist");
    assert_eq!(actual, expected);
}

#[test]
fn diagnostics_contract_golden_dynamic_import_warning() {
    let actual = render_contract("dynamic-import-fastify.fixture.txt");
    let expected = fs::read_to_string(fixture_path("dynamic-import-fastify.golden.txt"))
        .expect("golden file must exist");
    assert_eq!(actual, expected);
}
