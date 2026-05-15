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
                let resolved = if is_relative_spec(&import.spec) {
                    resolve_relative_module(&root, &module_path, &import.spec)
                } else if is_builtin_spec(&import.spec) {
                    None
                } else {
                    resolve_package_module(&root, &module_path, &import.spec)
                };

                match resolved {
                    Some(resolved) => {
                        import.resolved = Some(resolved.clone());
                        if !seen.contains(&resolved) {
                            pending.push_back(resolved);
                        }
                    }
                    None if is_relative_spec(&import.spec) => {
                        out.diagnostics.push(diag(
                            "ANALYZER_UNRESOLVED_MODULE",
                            &format!(
                                "cannot resolve module specifier {:?} from {module_path}",
                                import.spec
                            ),
                            &module_path,
                        ));
                    }
                    None => {}
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
    resolve_file_or_directory(root, &base)
}

fn resolve_package_module(root: &Path, from_module: &str, spec: &str) -> Option<String> {
    let (package_name, subpath) = package_spec_parts(spec)?;
    let mut current = Path::new(from_module).parent().unwrap_or(Path::new(""));
    loop {
        let package_dir = normalize_path_buf(current.join("node_modules").join(&package_name));
        if root.join(&package_dir).is_dir() {
            if let Some(subpath) = subpath.as_deref() {
                return resolve_package_subpath(root, &package_dir, subpath);
            }
            return resolve_package_entry(root, &package_dir);
        }
        let Some(parent) = current.parent() else {
            break;
        };
        current = parent;
    }
    let package_dir = PathBuf::from("node_modules").join(&package_name);
    if !root.join(&package_dir).is_dir() {
        return None;
    }
    if let Some(subpath) = subpath.as_deref() {
        return resolve_package_subpath(root, &package_dir, subpath);
    }
    resolve_package_entry(root, &package_dir)
}

fn resolve_package_subpath(root: &Path, package_dir: &Path, subpath: &Path) -> Option<String> {
    let package_json_path = root.join(package_dir).join("package.json");
    if let Ok(source) = fs::read_to_string(package_json_path) {
        if let Ok(package_json) = serde_json::from_str::<serde_json::Value>(&source) {
            let export_key = format!("./{}", to_posix_path(subpath.to_path_buf()));
            for entry in package_export_candidates(package_json.get("exports"), &export_key) {
                if let Some(resolved) = resolve_file_or_directory(root, &package_dir.join(entry)) {
                    return Some(resolved);
                }
            }
        }
    }
    resolve_file_or_directory(root, &package_dir.join(subpath))
}

fn resolve_package_entry(root: &Path, package_dir: &Path) -> Option<String> {
    let package_json_path = root.join(package_dir).join("package.json");
    if let Ok(source) = fs::read_to_string(package_json_path) {
        if let Ok(package_json) = serde_json::from_str::<serde_json::Value>(&source) {
            for entry in package_entry_candidates(&package_json) {
                if let Some(resolved) = resolve_file_or_directory(root, &package_dir.join(entry)) {
                    return Some(resolved);
                }
            }
        }
    }
    resolve_file_or_directory(root, &package_dir.join("index"))
}

fn package_entry_candidates(package_json: &serde_json::Value) -> Vec<PathBuf> {
    let mut out = Vec::new();
    collect_exports_entry(package_json.get("exports"), &mut out);
    for key in ["module", "main"] {
        if let Some(value) = package_json.get(key).and_then(|value| value.as_str()) {
            out.push(PathBuf::from(value));
        }
    }
    out
}

fn package_export_candidates(value: Option<&serde_json::Value>, export_key: &str) -> Vec<PathBuf> {
    let mut out = Vec::new();
    let Some(serde_json::Value::Object(map)) = value else {
        return out;
    };
    if let Some(entry) = map.get(export_key) {
        collect_exports_entry(Some(entry), &mut out);
    }
    out
}

fn collect_exports_entry(value: Option<&serde_json::Value>, out: &mut Vec<PathBuf>) {
    match value {
        Some(serde_json::Value::String(entry)) => out.push(PathBuf::from(entry)),
        Some(serde_json::Value::Object(map)) => {
            if let Some(entry) = map.get(".") {
                collect_exports_entry(Some(entry), out);
            }
            for key in ["import", "require", "default", "node"] {
                if let Some(entry) = map.get(key) {
                    collect_exports_entry(Some(entry), out);
                }
            }
        }
        _ => {}
    }
}

fn resolve_file_or_directory(root: &Path, base: &Path) -> Option<String> {
    if root.join(base).is_dir() {
        if let Some(resolved) = resolve_package_entry(root, base) {
            return Some(resolved);
        }
    }

    let base_path = base.to_path_buf();
    let candidates = [
        base_path.clone(),
        append_extension(base, "ts"),
        append_extension(base, "tsx"),
        append_extension(base, "js"),
        append_extension(base, "mjs"),
        append_extension(base, "cjs"),
        append_extension(base, "jsx"),
        with_extension(base, "ts"),
        with_extension(base, "tsx"),
        with_extension(base, "js"),
        with_extension(base, "mjs"),
        with_extension(base, "cjs"),
        with_extension(base, "jsx"),
        base_path.join("index.ts"),
        base_path.join("index.tsx"),
        base_path.join("index.js"),
        base_path.join("index.mjs"),
        base_path.join("index.cjs"),
        base_path.join("index.jsx"),
    ];

    candidates
        .into_iter()
        .find(|candidate| root.join(candidate).is_file())
        .map(to_posix_path)
}

fn package_spec_parts(spec: &str) -> Option<(String, Option<PathBuf>)> {
    let mut parts = spec.split('/').collect::<Vec<_>>();
    if parts.is_empty() || parts[0].is_empty() {
        return None;
    }
    if spec.starts_with('@') {
        if parts.len() < 2 {
            return None;
        }
        let package_name = format!("{}/{}", parts[0], parts[1]);
        let rest = parts.split_off(2);
        return Some((package_name, subpath_from_parts(rest)));
    }
    let package_name = parts.remove(0).to_string();
    Some((package_name, subpath_from_parts(parts)))
}

fn subpath_from_parts(parts: Vec<&str>) -> Option<PathBuf> {
    if parts.is_empty() {
        return None;
    }
    let mut out = PathBuf::new();
    for part in parts {
        out.push(part);
    }
    Some(out)
}

fn with_extension(path: &Path, extension: &str) -> PathBuf {
    let mut out = path.to_path_buf();
    out.set_extension(extension);
    out
}

fn append_extension(path: &Path, extension: &str) -> PathBuf {
    PathBuf::from(format!("{}.{}", path.to_string_lossy(), extension))
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

fn is_builtin_spec(spec: &str) -> bool {
    spec.starts_with("node:")
        || matches!(
            spec,
            "assert"
                | "buffer"
                | "child_process"
                | "crypto"
                | "events"
                | "fs"
                | "module"
                | "os"
                | "path"
                | "process"
                | "stream"
                | "string_decoder"
                | "tty"
                | "url"
                | "util"
                | "v8"
        )
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
