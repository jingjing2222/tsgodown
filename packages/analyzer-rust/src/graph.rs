use std::{
    collections::{BTreeSet, VecDeque},
    fs,
    path::{Path, PathBuf},
};

use crate::{builder::build_program_ir, DiagnosticIR, DiagnosticSourceIR, ProgramIR};

pub fn build_program_graph(root: &Path, entry: &str) -> ProgramIR {
    let root = root.to_path_buf();
    let mut pending = VecDeque::from([normalize_entry(entry)]);
    let mut seen = BTreeSet::new();
    let mut out = ProgramIR {
        modules: vec![],
        routes: vec![],
        handlers: vec![],
        diagnostics: vec![],
    };

    while let Some(module_path) = pending.pop_front() {
        if !seen.insert(module_path.clone()) {
            continue;
        }

        let abs_path = root.join(&module_path);
        let source = match fs::read_to_string(&abs_path) {
            Ok(source) => source,
            Err(error) => {
                out.diagnostics.push(diag(
                    "ANALYZER_READ_FAILED",
                    &format!("failed to read module {module_path}: {error}"),
                    &module_path,
                ));
                continue;
            }
        };

        let mut ir = build_program_ir(&module_path, &source);
        for module in &mut ir.modules {
            for import in &mut module.imports {
                if !is_relative_spec(&import.spec) {
                    continue;
                }

                match resolve_relative_module(&root, &module_path, &import.spec) {
                    Some(resolved) => {
                        import.resolved = Some(resolved.clone());
                        if !seen.contains(&resolved) {
                            pending.push_back(resolved);
                        }
                    }
                    None => out.diagnostics.push(diag(
                        "ANALYZER_UNRESOLVED_MODULE",
                        &format!(
                            "cannot resolve module specifier {:?} from {module_path}",
                            import.spec
                        ),
                        &module_path,
                    )),
                }
            }
        }

        out.modules.extend(ir.modules);
        out.routes.extend(ir.routes);
        out.handlers.extend(ir.handlers);
        out.diagnostics.extend(ir.diagnostics);
    }

    out.normalize()
}

fn resolve_relative_module(root: &Path, from_module: &str, spec: &str) -> Option<String> {
    let from_dir = Path::new(from_module).parent().unwrap_or(Path::new(""));
    let base = normalize_path_buf(from_dir.join(spec));
    let candidates = [
        base.clone(),
        with_extension(&base, "ts"),
        with_extension(&base, "tsx"),
        with_extension(&base, "js"),
        with_extension(&base, "mjs"),
        with_extension(&base, "cjs"),
        with_extension(&base, "jsx"),
        base.join("index.ts"),
        base.join("index.tsx"),
        base.join("index.js"),
        base.join("index.mjs"),
        base.join("index.cjs"),
        base.join("index.jsx"),
    ];

    candidates
        .into_iter()
        .find(|candidate| root.join(candidate).is_file())
        .map(to_posix_path)
}

fn with_extension(path: &Path, extension: &str) -> PathBuf {
    let mut out = path.to_path_buf();
    out.set_extension(extension);
    out
}

fn normalize_entry(entry: &str) -> String {
    to_posix_path(normalize_path_buf(PathBuf::from(entry)))
}

fn normalize_path_buf(path: PathBuf) -> PathBuf {
    let mut out = PathBuf::new();
    for component in path.components() {
        match component {
            std::path::Component::CurDir => {}
            std::path::Component::ParentDir => {
                out.pop();
            }
            other => out.push(other.as_os_str()),
        }
    }
    out
}

fn is_relative_spec(spec: &str) -> bool {
    spec.starts_with("./") || spec.starts_with("../")
}

fn to_posix_path(path: PathBuf) -> String {
    path.components()
        .map(|component| component.as_os_str().to_string_lossy())
        .collect::<Vec<_>>()
        .join("/")
}

fn diag(code: &str, message: &str, file: &str) -> DiagnosticIR {
    DiagnosticIR {
        level: "error".to_string(),
        code: code.to_string(),
        message: message.to_string(),
        source: Some(DiagnosticSourceIR {
            file: file.to_string(),
            line: None,
            column: None,
            via_source_map: Some(false),
        }),
    }
}
