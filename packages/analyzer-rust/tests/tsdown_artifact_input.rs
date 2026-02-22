use analyzer_rust::analyze_compiler_entry;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

static TEMP_SEQ: AtomicU64 = AtomicU64::new(1);

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
    let seq = TEMP_SEQ.fetch_add(1, Ordering::Relaxed);
    std::env::temp_dir().join(format!(
        "tsgodown-analyzer-rust-artifact-test-{}-{nonce}-{seq}",
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
    build_with_tsdown_target(ts_source, None)
}

fn build_with_tsdown_target(ts_source: &str, target: Option<&str>) -> TsdownArtifacts {
    let project_dir = create_temp_project_dir();
    let src_dir = project_dir.join("src");
    fs::create_dir_all(&src_dir).expect("temp src dir must be creatable");
    fs::write(src_dir.join("index.ts"), ts_source).expect("fixture ts source must be writable");
    let target_line = if let Some(target) = target {
        format!("  target: \"{target}\",\n")
    } else {
        String::new()
    };
    fs::write(
        project_dir.join("tsdown.config.ts"),
        format!(
            r#"
export default {{
  entry: {{ index: "{}" }},
  outDir: "{}",
  sourcemap: true,
  dts: true,
{}
  format: ["esm"],
}};
"#,
            src_dir.join("index.ts").display(),
            project_dir.join("dist").display(),
            target_line
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

fn build_with_tsdown_error(ts_source: &str) -> String {
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

    assert!(!output.status.success(), "tsdown build unexpectedly succeeded");
    let stderr = String::from_utf8_lossy(&output.stderr).to_string();
    let stdout = String::from_utf8_lossy(&output.stdout).to_string();
    let combined = format!("{stdout}\n{stderr}");
    fs::remove_dir_all(&project_dir).expect("temp project cleanup must succeed");
    combined
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
fn supports_class_extends_member_path_from_tsdown_artifacts() {
    let bundled = build_with_tsdown(
        r#"
class BaseController {}
const Framework = { BaseController };
export class ApiController extends Framework.BaseController {
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
        !ir.diagnostics
            .iter()
            .any(|diag| diag.code == "ANALYZER_UNSUPPORTED_CLASS_EXTENDS_EXPRESSION"),
        "unexpected extends diagnostics: {:?}\njs:\n{}",
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

#[test]
fn collects_class_exported_via_export_list_from_tsdown_artifacts() {
    let bundled = build_with_tsdown(
        r#"
class HealthController {
  handle() {
    return { ok: true };
  }
}
export { HealthController };
"#,
    );

    assert!(!bundled.dts_source.trim().is_empty());
    assert!(!bundled.sourcemap_source.trim().is_empty());
    let ir = analyze_compiler_entry("dist/index.mjs", &bundled.js_source);
    assert!(ir.diagnostics.is_empty());
    assert!(
        ir.modules[0].exports.iter().any(|name| name == "HealthController"),
        "missing export symbol; exports={:?}\njs:\n{}",
        ir.modules[0].exports,
        bundled.js_source
    );
}

#[test]
fn supports_class_private_elements_from_tsdown_js_dts_sourcemap_artifacts() {
    let bundled = build_with_tsdown_target(
        r#"
export class Counter {
  #count = 0;
  increment() {
    this.#count += 1;
    return this.#count;
  }
}
"#,
        Some("esnext"),
    );

    assert!(bundled.js_source.contains("#count"));
    assert!(!bundled.dts_source.trim().is_empty());
    assert!(!bundled.sourcemap_source.trim().is_empty());
    let ir = analyze_compiler_entry("dist/index.mjs", &bundled.js_source);
    assert!(
        !ir.diagnostics
            .iter()
            .any(|diag| diag.code == "ANALYZER_UNSUPPORTED_CLASS_PRIVATE_ELEMENTS"),
        "unexpected private-element diagnostic: {:?}\njs:\n{}",
        ir.diagnostics,
        bundled.js_source
    );
}

#[test]
fn supports_class_public_fields_from_tsdown_js_dts_sourcemap_artifacts() {
    let bundled = build_with_tsdown(
        r#"
export class ProfileController {
  retries = 3;
  label = "ok";
}
"#,
    );

    assert!(!bundled.dts_source.trim().is_empty());
    assert!(!bundled.sourcemap_source.trim().is_empty());
    let ir = analyze_compiler_entry("dist/index.mjs", &bundled.js_source);
    assert!(ir.diagnostics.is_empty());
}

#[test]
fn supports_class_static_members_from_tsdown_js_dts_sourcemap_artifacts() {
    let bundled = build_with_tsdown(
        r#"
export class Cache {
  static VERSION = 1;
  static reset() {}
}
"#,
    );

    assert!(!bundled.dts_source.trim().is_empty());
    assert!(!bundled.sourcemap_source.trim().is_empty());
    let ir = analyze_compiler_entry("dist/index.mjs", &bundled.js_source);
    assert!(ir.diagnostics.is_empty());
}

#[test]
fn supports_class_static_initialization_blocks_from_tsdown_artifacts() {
    let bundled = build_with_tsdown(
        r#"
export class Cache {
  static {
    const seed = 1;
    void seed;
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
fn detects_deprecated_and_obsolete_features_from_tsdown_artifacts() {
    let bundled = build_with_tsdown(
        r#"
const text = escape("a b");
const raw = unescape(text);
obj.__defineGetter__("x", () => 1);
void raw;
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
    assert!(codes.contains(&"ANALYZER_DEPRECATED_ESCAPE_API"));
    assert!(codes.contains(&"ANALYZER_DEPRECATED_UNESCAPE_API"));
    assert!(codes.contains(&"ANALYZER_DEPRECATED_LEGACY_ACCESSOR_API"));
}

#[test]
fn handles_already_executing_generator_pattern_from_tsdown_artifacts() {
    let bundled = build_with_tsdown(
        r#"
function* task() {
  task.next();
  yield 1;
}
export const marker = 1;
"#,
    );

    assert!(!bundled.dts_source.trim().is_empty());
    assert!(!bundled.sourcemap_source.trim().is_empty());
    let ir = analyze_compiler_entry("dist/index.mjs", &bundled.js_source);
    assert!(ir.diagnostics.is_empty());
}

#[test]
fn handles_already_has_pragma_pattern_from_tsdown_artifacts() {
    let bundled = build_with_tsdown(
        r#"
'use strict';
'use strict';
export const marker = 1;
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
fn reports_arguments_not_allowed_error_from_tsdown_parser() {
    let output = build_with_tsdown_error(
        r#"
export class BadField {
  value = arguments;
}
"#,
    );

    assert!(output.contains("arguments"));
    assert!(output.contains("not allowed in class field initializer"));
}

#[test]
fn handles_array_sort_argument_pattern_from_tsdown_artifacts() {
    let bundled = build_with_tsdown(
        r#"
const values = [3, 1, 2];
values.sort(123);
export const marker = values.length;
"#,
    );

    assert!(!bundled.dts_source.trim().is_empty());
    assert!(!bundled.sourcemap_source.trim().is_empty());
    let ir = analyze_compiler_entry("dist/index.mjs", &bundled.js_source);
    assert!(ir.diagnostics.is_empty());
}

#[test]
fn reports_await_yield_in_parameter_error_from_tsdown_parser() {
    let output = build_with_tsdown_error(
        r#"
async function f(await x) {
  return x;
}
export const marker = 1;
"#,
    );

    assert!(output.contains("await"));
    assert!(output.contains("Cannot use `await` as an identifier in an async context"));
}

#[test]
fn reports_bad_await_error_from_tsdown_parser() {
    let output = build_with_tsdown_error(
        r#"
function f() {
  await 1;
}
export const marker = 1;
"#,
    );

    assert!(output.contains("await"));
    assert!(output.contains("only allowed within async functions"));
}

#[test]
fn emits_diagnostic_for_computed_constructor_name_from_tsdown_artifacts() {
    let bundled = build_with_tsdown(
        r#"
export class WeirdConstructor {
  ["constructor"]() {}
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
    assert!(codes.contains(&"ANALYZER_UNSUPPORTED_COMPUTED_CONSTRUCTOR"));
}
