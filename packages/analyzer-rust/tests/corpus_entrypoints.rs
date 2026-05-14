use std::{fs, path::PathBuf};

use analyzer_rust::analyze_compiler_entry;

fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(|path| path.parent())
        .expect("analyzer-rust package should live under packages/")
        .to_path_buf()
}

fn fixture_path(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("fixtures")
        .join(name)
}

#[test]
fn vendored_node_corpus_entrypoints_parse_as_ast_without_diagnostics() {
    let root = repo_root();
    let entries = [
        "test-corpus/node-real/packages/dotenv/lib/main.js",
        "test-corpus/node-real/packages/execa/index.js",
        "test-corpus/node-real/packages/fs-extra/lib/index.js",
        "test-corpus/node-real/packages/js-yaml/dist/js-yaml.mjs",
        "test-corpus/node-real/packages/lru-cache/dist/esm/index.js",
        "test-corpus/node-real/packages/minimatch/dist/esm/index.js",
        "test-corpus/node-real/packages/qs/lib/index.js",
        "test-corpus/node-real/packages/semver/index.js",
        "test-corpus/node-real/packages/uuid/dist-node/index.js",
        "test-corpus/node-real/packages/yargs-parser/build/lib/index.js",
    ];

    let mut actual = String::new();
    for entry in entries {
        let source = fs::read_to_string(root.join(entry)).expect("corpus entry should exist");
        let ir = analyze_compiler_entry(entry, &source);
        actual.push_str(&format!(
            "{entry} imports={} exports={} diagnostics={}\n",
            ir.modules[0].imports.len(),
            ir.modules[0].exports.len(),
            ir.diagnostics.len(),
        ));
    }

    let expected = fs::read_to_string(fixture_path("corpus-entrypoints.golden.txt"))
        .expect("golden should exist");
    assert_eq!(actual, expected);
}
