use std::{fs, path::PathBuf};

use analyzer_rust::{analyze_compiler_entry, ProgramIR};

fn fixture_path(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("fixtures")
        .join(name)
}

fn render_ir(ir: &ProgramIR) -> String {
    let mut out = String::new();

    out.push_str("modules:\n");
    for module in &ir.modules {
        out.push_str(&format!(
            "  - id={} source_path={}\n",
            module.id, module.source_path
        ));
        out.push_str("    exports:\n");
        for export in &module.exports {
            out.push_str(&format!("      - {}\n", export));
        }
        out.push_str("    imports:\n");
        for import in &module.imports {
            out.push_str(&format!(
                "      - spec={} kind={} resolved={}\n",
                import.spec,
                import.kind,
                import.resolved.as_deref().unwrap_or("<none>"),
            ));
        }
    }

    out.push_str("routes:\n");
    for route in &ir.routes {
        out.push_str(&format!(
            "  - method={} path={} handler_ref={}\n",
            route.method, route.path, route.handler_ref
        ));
    }

    out.push_str("handlers:\n");
    for handler in &ir.handlers {
        let params = if handler.params.is_empty() {
            "0".to_string()
        } else {
            let joined = handler
                .params
                .iter()
                .map(|p| format!("{}:{}", p.name, p.role))
                .collect::<Vec<_>>()
                .join(",");
            format!("[{joined}]")
        };
        let semantics = if let Some(semantics) = &handler.semantics {
            format!(
                "mode={} req={} res={} status={} body={} headers={} json={}",
                semantics.response_mode,
                semantics.request_param.as_deref().unwrap_or("<none>"),
                semantics.response_param.as_deref().unwrap_or("<none>"),
                semantics.uses_status,
                semantics.uses_body,
                semantics.uses_headers,
                semantics.uses_json,
            )
        } else {
            "none".to_string()
        };

        out.push_str(&format!(
            "  - id={} async={} params={} semantics={}\n",
            handler.id, handler.r#async, params, semantics
        ));
    }

    out.push_str("diagnostics:\n");
    for diag in &ir.diagnostics {
        out.push_str(&format!(
            "  - level={} code={} message={} source={}\n",
            diag.level,
            diag.code,
            diag.message,
            diag.source
                .as_ref()
                .map(|s| s.file.as_str())
                .unwrap_or("<none>"),
        ));
    }

    out
}

fn assert_fixture(fixture_name: &str, golden_name: &str) {
    let source = fs::read_to_string(fixture_path(fixture_name)).unwrap();
    let ir = analyze_compiler_entry(fixture_name, &source);
    let actual = render_ir(&ir);
    let expected = fs::read_to_string(fixture_path(golden_name)).unwrap();
    assert_eq!(actual, expected, "golden drift for fixture={fixture_name}");
}

#[test]
fn supported_shorthand_routes_are_lowered_deterministically() {
    assert_fixture("supported-shorthand.ts", "supported-shorthand.golden.txt");
}

#[test]
fn route_object_literal_is_lowered_deterministically() {
    assert_fixture("route-object-literal.ts", "route-object-literal.golden.txt");
}

#[test]
fn route_object_reference_is_lowered_deterministically() {
    assert_fixture(
        "route-object-reference.ts",
        "route-object-reference.golden.txt",
    );
}

#[test]
fn unsupported_patterns_emit_deterministic_diagnostics() {
    assert_fixture("unsupported-dynamic.ts", "unsupported-dynamic.golden.txt");
}

#[test]
fn semantic_patterns_are_lowered_deterministically() {
    assert_fixture("semantic-patterns.ts", "semantic-patterns.golden.txt");
}

#[test]
fn template_literal_paths_keep_static_literals_and_reject_interpolated_paths() {
    assert_fixture(
        "template-literal-paths.ts",
        "template-literal-paths.golden.txt",
    );
}

#[test]
fn unsupported_register_boundaries_emit_spec_mapped_diagnostics() {
    assert_fixture(
        "unsupported-register-boundaries.ts",
        "unsupported-register-boundaries.golden.txt",
    );
}

#[test]
fn conditional_routes_emit_unsupported_diagnostic() {
    assert_fixture("conditional-route.ts", "conditional-route.golden.txt");
}

#[test]
fn single_line_conditional_routes_emit_unsupported_diagnostic() {
    assert_fixture(
        "conditional-route-single-line.ts",
        "conditional-route-single-line.golden.txt",
    );
}

#[test]
fn conditional_map_delete_does_not_emit_route_diagnostic() {
    let source = r#"
const map = new Map();
if (map.size > 1) {
  map.delete("");
}
"#;
    let ir = analyze_compiler_entry("not-a-route.js", source);

    assert!(
        ir.diagnostics
            .iter()
            .all(|diag| diag.code != "ANALYZER_UNSUPPORTED_CONDITIONAL_ROUTE"),
        "Map.delete(string) must not be mistaken for DELETE route registration"
    );
}

#[test]
fn static_builtin_dynamic_import_is_tracked_without_unsupported_diagnostic() {
    let source = r#"
import('node:diagnostics_channel').then((dc) => dc.channel('x')).catch(() => {});
"#;
    let ir = analyze_compiler_entry("static-builtin-dynamic-import.js", source);

    assert!(
        ir.diagnostics
            .iter()
            .all(|diag| diag.code != "DYNAMIC_IMPORT_DETECTED"),
        "static node: builtin dynamic import should not block corpus graph analysis"
    );
    assert_eq!(ir.modules[0].imports.len(), 1);
    assert_eq!(ir.modules[0].imports[0].spec, "node:diagnostics_channel");
    assert_eq!(ir.modules[0].imports[0].kind, "dynamic");
}
