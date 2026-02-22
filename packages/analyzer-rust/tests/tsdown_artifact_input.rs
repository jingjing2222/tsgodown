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

fn build_with_tsdown(ts_source: &str) -> String {
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

    let bundled_source =
        fs::read_to_string(project_dir.join("dist").join("index.mjs")).expect("bundled mjs missing");
    fs::remove_dir_all(&project_dir).expect("temp project cleanup must succeed");
    bundled_source
}

#[test]
fn detects_generator_reentry_from_tsdown_bundled_output() {
    let bundled = build_with_tsdown(
        r#"
function* task() {
  task.next();
  yield 1;
}
export { task };
"#,
    );

    let ir = analyze_compiler_entry("dist/index.mjs", &bundled);
    let codes = ir
        .diagnostics
        .iter()
        .map(|diag| diag.code.as_str())
        .collect::<Vec<_>>();
    assert!(codes.contains(&"ANALYZER_POTENTIAL_GENERATOR_REENTRY"));
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

    let ir = analyze_compiler_entry("dist/index.mjs", &bundled);
    let codes = ir
        .diagnostics
        .iter()
        .map(|diag| diag.code.as_str())
        .collect::<Vec<_>>();
    assert!(!codes.contains(&"ANALYZER_DUPLICATE_PRAGMA"));
}
