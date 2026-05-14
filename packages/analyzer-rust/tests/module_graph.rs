use std::{fs, path::PathBuf};

use analyzer_rust::analyze_compiler_project;

#[test]
fn resolves_relative_esm_and_cjs_module_graph() {
    let root = temp_project("module-graph");
    write(
        &root,
        "src/index.js",
        r#"
import { parse } from "./parse.js";
const format = require("./format");
export { parse, format };
"#,
    );
    write(
        &root,
        "src/parse.js",
        r#"
export function parse(value) {
  return value.trim();
}
"#,
    );
    write(
        &root,
        "src/format/index.js",
        r#"
module.exports = {
  format(value) {
    return String(value);
  },
};
"#,
    );

    let ir = analyze_compiler_project(&root, "src/index.js");

    assert_eq!(ir.diagnostics, vec![]);
    assert_eq!(
        ir.modules
            .iter()
            .map(|module| module.source_path.as_str())
            .collect::<Vec<_>>(),
        vec!["src/format/index.js", "src/index.js", "src/parse.js"]
    );

    let entry = ir
        .modules
        .iter()
        .find(|module| module.source_path == "src/index.js")
        .expect("entry module");
    assert_eq!(
        entry
            .imports
            .iter()
            .map(|import| (
                import.spec.as_str(),
                import.kind.as_str(),
                import.resolved.as_deref()
            ))
            .collect::<Vec<_>>(),
        vec![
            ("./format", "cjs", Some("src/format/index.js")),
            ("./parse.js", "esm", Some("src/parse.js")),
        ]
    );
}

#[test]
fn reports_unresolved_relative_import_without_silent_compile() {
    let root = temp_project("module-graph-unresolved");
    write(
        &root,
        "src/index.js",
        r#"
const missing = require("./missing");
module.exports = missing;
"#,
    );

    let ir = analyze_compiler_project(&root, "src/index.js");

    assert_eq!(ir.modules.len(), 1);
    assert_eq!(ir.diagnostics.len(), 1);
    assert_eq!(ir.diagnostics[0].code, "ANALYZER_UNRESOLVED_MODULE");
    assert_eq!(
        ir.diagnostics[0].source.as_ref().unwrap().file,
        "src/index.js"
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
