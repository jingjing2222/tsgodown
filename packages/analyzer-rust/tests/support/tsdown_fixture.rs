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

pub fn build_inline_source(ts_source: &str) -> String {
    let project_dir = create_temp_project_dir();
    let src_dir = project_dir.join("src");
    fs::create_dir_all(&src_dir).expect("temp src dir must be creatable");
    fs::write(src_dir.join("index.ts"), ts_source).expect("fixture ts source must be writable");
    let bundled = build_with_tsdown(&project_dir, &src_dir.join("index.ts"));
    fs::remove_dir_all(&project_dir).expect("temp project cleanup must succeed");
    bundled
}

pub fn build_fixture_path(source_path: &Path) -> String {
    let project_dir = create_temp_project_dir();
    let src_dir = project_dir.join("src");
    fs::create_dir_all(&src_dir).expect("temp src dir must be creatable");

    let fixture_dir = source_path
        .parent()
        .expect("fixture source must have a parent directory");
    for entry in fs::read_dir(fixture_dir).expect("fixture dir must be readable") {
        let entry = entry.expect("fixture dir entry must be readable");
        let path = entry.path();
        if path.extension().and_then(|ext| ext.to_str()) != Some("ts") {
            continue;
        }
        let file_name = path
            .file_name()
            .expect("fixture source file name must exist");
        fs::copy(&path, src_dir.join(file_name)).expect("fixture file copy must succeed");
    }

    let entry_name = source_path
        .file_name()
        .expect("fixture source file name must exist");
    let bundled = build_with_tsdown(&project_dir, &src_dir.join(entry_name));
    fs::remove_dir_all(&project_dir).expect("temp project cleanup must succeed");
    bundled
}

fn build_with_tsdown(project_dir: &Path, entry_path: &Path) -> String {
    fs::write(
        project_dir.join("tsdown.config.ts"),
        format!(
            r#"
export default {{
  entry: {{ index: "{}" }},
  outDir: "{}",
  sourcemap: true,
  dts: true,
  external: ["./lazy"],
  format: ["esm"],
}};
"#,
            entry_path.display(),
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

    fs::read_to_string(project_dir.join("dist").join("index.mjs")).expect("bundled mjs missing")
}
