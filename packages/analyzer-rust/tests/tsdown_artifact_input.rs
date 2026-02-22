use analyzer_rust::analyze_compiler_entry;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(2)
        .expect("repo root must exist")
        .to_path_buf()
}

fn create_temp_project_dir() -> PathBuf {
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("system time should be after unix epoch")
        .as_nanos();
    std::env::temp_dir().join(format!(
        "tsgodown-analyzer-rust-artifact-test-{}-{nonce}",
        std::process::id()
    ))
}

struct TsdownArtifacts {
    js_source: String,
    dts_source: String,
    sourcemap_source: String,
}

fn read_first_existing(paths: &[PathBuf], label: &str) -> String {
    for path in paths {
        if path.exists() {
            return fs::read_to_string(path)
                .unwrap_or_else(|err| panic!("failed to read {label} {}: {err}", path.display()));
        }
    }
    let checked = paths
        .iter()
        .map(|path| path.display().to_string())
        .collect::<Vec<_>>()
        .join(", ");
    panic!("{label} missing (checked: {checked})");
}

fn build_with_tsdown(ts_source: &str) -> TsdownArtifacts {
    let project_dir = create_temp_project_dir();
    let src_dir = project_dir.join("src");
    fs::create_dir_all(&src_dir).expect("temp src dir must be creatable");
    fs::write(src_dir.join("index.ts"), ts_source).expect("fixture ts source must be writable");
    fs::write(
        project_dir.join("tsdown.config.ts"),
        format!(
            r#"
export default {{
  entry: {{ index: "{}" }},
  outDir: "{}",
  sourcemap: true,
  dts: true,
  format: ["esm"],
}};
"#,
            src_dir.join("index.ts").display(),
            project_dir.join("dist").display()
        ),
    )
    .expect("tsdown config must be writable");

    let output = Command::new("pnpm")
        .arg("--dir")
        .arg(repo_root())
        .arg("--filter")
        .arg("@tsgodown/tsdown-driver")
        .arg("exec")
        .arg("tsdown")
        .arg("--config")
        .arg(project_dir.join("tsdown.config.ts"))
        .output()
        .expect("tsdown command must be runnable");

    assert!(
        output.status.success(),
        "tsdown build failed\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );

    let dist_dir = project_dir.join("dist");
    let bundled_source = read_first_existing(
        &[
            dist_dir.join("index.mjs"),
            dist_dir.join("index.js"),
            dist_dir.join("index.cjs"),
        ],
        "bundled javascript artifact",
    );
    let dts_source = read_first_existing(
        &[
            dist_dir.join("index.d.ts"),
            dist_dir.join("index.d.mts"),
            dist_dir.join("index.d.cts"),
        ],
        "bundled d.ts artifact",
    );
    let sourcemap_source = read_first_existing(
        &[
            dist_dir.join("index.mjs.map"),
            dist_dir.join("index.js.map"),
            dist_dir.join("index.cjs.map"),
        ],
        "bundled sourcemap artifact",
    );
    fs::remove_dir_all(&project_dir).expect("temp project cleanup must succeed");
    TsdownArtifacts {
        js_source: bundled_source,
        dts_source,
        sourcemap_source,
    }
}

#[test]
fn detects_computed_static_member_from_tsdown_bundled_output() {
    let bundled = build_with_tsdown(
        r#"
export class Cache {
  static [nameExpr] = 1;
}
"#,
    );

    assert!(!bundled.dts_source.trim().is_empty());
    assert!(!bundled.sourcemap_source.trim().is_empty());
    let ir = analyze_compiler_entry("dist/index.mjs", &bundled.js_source);
    let codes = ir
        .diagnostics
        .iter()
        .map(|diag| diag.code.as_str())
        .collect::<Vec<_>>();
    assert!(codes.contains(&"ANALYZER_UNSUPPORTED_STATIC_CLASS_MEMBER"));
}

#[test]
fn duplicate_pragma_is_not_reported_after_tsdown_bundle_normalization() {
    let bundled = build_with_tsdown(
        r#"
'use strict';
'use strict';
export const answer = 42;
"#,
    );

    assert!(!bundled.dts_source.trim().is_empty());
    assert!(!bundled.sourcemap_source.trim().is_empty());
    let ir = analyze_compiler_entry("dist/index.mjs", &bundled.js_source);
    let codes = ir
        .diagnostics
        .iter()
        .map(|diag| diag.code.as_str())
        .collect::<Vec<_>>();
    assert!(!codes.contains(&"ANALYZER_DUPLICATE_PRAGMA"));
}

#[test]
fn supports_class_constructor_from_tsdown_js_dts_sourcemap_artifacts() {
    let bundled = build_with_tsdown(
        r#"
export default class HealthController {
  constructor(service: string) {
    void service;
  }
}
"#,
    );

    assert!(!bundled.dts_source.trim().is_empty());
    assert!(!bundled.sourcemap_source.trim().is_empty());
    let ir = analyze_compiler_entry("dist/index.mjs", &bundled.js_source);
    assert!(ir.diagnostics.is_empty());
}

#[test]
fn supports_class_extends_from_tsdown_js_dts_sourcemap_artifacts() {
    let bundled = build_with_tsdown(
        r#"
class BaseController {
  handle() {
    return 1;
  }
}
export class ApiController extends BaseController {
  constructor() {
    super();
  }
}
"#,
    );

    assert!(!bundled.dts_source.trim().is_empty());
    assert!(!bundled.sourcemap_source.trim().is_empty());
    let ir = analyze_compiler_entry("dist/index.mjs", &bundled.js_source);
    assert!(
        ir.diagnostics.is_empty(),
        "unexpected diagnostics: {:?}\njs:\n{}",
        ir.diagnostics,
        bundled.js_source
    );
}

#[test]
fn supports_class_declaration_from_tsdown_js_dts_sourcemap_artifacts() {
    let bundled = build_with_tsdown(
        r#"
export class HealthController {
  handle() {
    return { ok: true };
  }
}
"#,
    );

    assert!(!bundled.dts_source.trim().is_empty());
    assert!(!bundled.sourcemap_source.trim().is_empty());
    let ir = analyze_compiler_entry("dist/index.mjs", &bundled.js_source);
    assert!(ir.diagnostics.is_empty());
}
