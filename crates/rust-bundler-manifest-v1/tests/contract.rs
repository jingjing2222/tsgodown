use std::fs;
use std::path::PathBuf;

use pretty_assertions::assert_eq;
use rust_bundler_manifest_v1::{
    build_manifest, parse_manifest, BuildInput, DiagnosticLevel, ManifestDiagnostic,
};

fn fixture_path(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("fixtures")
        .join(name)
}

#[test]
fn builds_manifest_with_deterministic_ordering_from_fixture() {
    let input_raw = fs::read_to_string(fixture_path("build-input.json")).unwrap();
    let input: BuildInput = serde_json::from_str(&input_raw).unwrap();

    let output = build_manifest(input);
    let actual = serde_json::to_value(&output.manifest).unwrap();

    let expected_raw = fs::read_to_string(fixture_path("manifest-valid.json")).unwrap();
    let expected: serde_json::Value = serde_json::from_str(&expected_raw).unwrap();

    assert_eq!(actual["entries"], expected["entries"]);
    assert_eq!(actual["bundles"], expected["bundles"]);
    assert_eq!(actual["types"], expected["types"]);
    assert_eq!(actual["tsconfigPath"], expected["tsconfigPath"]);

    let build_id = actual["buildId"].as_str().unwrap();
    assert_eq!(build_id.len(), 16);
    assert!(build_id.chars().all(|c| c.is_ascii_hexdigit()));

    assert!(output.diagnostics.is_empty());
}

#[test]
fn emits_clear_diagnostics_for_missing_bundle_sourcemap_and_types_links() {
    let input_raw = fs::read_to_string(fixture_path("build-input-missing-links.json")).unwrap();
    let input: BuildInput = serde_json::from_str(&input_raw).unwrap();

    let output = build_manifest(input);

    let expected = vec![
        ManifestDiagnostic {
            level: DiagnosticLevel::Error,
            code: "MISSING_BUNDLE_LINK".to_string(),
            message: "sourcemap 'dist/orphan.mjs.map' does not have a matching bundle artifact".to_string(),
        },
        ManifestDiagnostic {
            level: DiagnosticLevel::Error,
            code: "MISSING_SOURCEMAP_LINK".to_string(),
            message: "bundle 'dist/index.mjs' is missing sourcemap link 'dist/index.mjs.map'".to_string(),
        },
        ManifestDiagnostic {
            level: DiagnosticLevel::Error,
            code: "MISSING_TYPES_LINK".to_string(),
            message: "bundle 'dist/index.mjs' is missing declaration link (expected one of: dist/index.d.ts, dist/index.d.mts, dist/index.d.cts)".to_string(),
        },
    ];

    assert_eq!(output.diagnostics, expected);
}

#[test]
fn parses_manifest_json_compatible_with_current_schema() {
    let manifest_raw = fs::read_to_string(fixture_path("manifest-valid.json")).unwrap();
    let parsed = parse_manifest(&manifest_raw).unwrap();

    assert_eq!(parsed.entries, vec!["src/admin.ts", "src/index.ts"]);
    assert_eq!(parsed.bundles.len(), 2);
    assert_eq!(parsed.types, vec!["dist/admin.d.ts", "dist/index.d.ts"]);
}
