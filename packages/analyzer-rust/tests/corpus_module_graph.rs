use std::path::PathBuf;

use analyzer_rust::analyze_compiler_project;

#[test]
fn vendored_node_corpus_relative_module_graphs_are_deterministic() {
    let root = repo_root().join("test-corpus/node-real");
    let cases = [
        ("semver", "packages/semver/index.js", 46, 0),
        ("minimatch", "packages/minimatch/dist/esm/index.js", 8, 0),
        ("qs", "packages/qs/lib/index.js", 46, 0),
        ("dotenv", "packages/dotenv/lib/main.js", 1, 0),
        (
            "yargs-parser",
            "packages/yargs-parser/build/lib/index.js",
            5,
            0,
        ),
        ("js-yaml", "packages/js-yaml/dist/js-yaml.mjs", 1, 0),
        ("lru-cache", "packages/lru-cache/dist/esm/index.js", 3, 0),
        ("uuid", "packages/uuid/dist-node/index.js", 21, 0),
        ("fs-extra", "packages/fs-extra/lib/index.js", 35, 0),
        ("execa", "packages/execa/index.js", 150, 0),
    ];

    let actual = cases
        .iter()
        .map(|(id, entry, _, _)| {
            let ir = analyze_compiler_project(&root, entry);
            format!(
                "{id} modules={} diagnostics={}",
                ir.modules.len(),
                ir.diagnostics.len()
            )
        })
        .collect::<Vec<_>>();
    let expected = cases
        .iter()
        .map(|(id, _, modules, diagnostics)| {
            format!("{id} modules={modules} diagnostics={diagnostics}")
        })
        .collect::<Vec<_>>();

    assert_eq!(actual, expected);
}

fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("packages dir")
        .parent()
        .expect("repo root")
        .to_path_buf()
}
