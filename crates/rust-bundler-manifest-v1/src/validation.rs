use std::collections::BTreeSet;

pub fn has_matching_types(bundle: &str, chunk_set: &BTreeSet<String>) -> bool {
    let base = bundle_base_no_ext(bundle);
    let candidates = [
        format!("{base}.d.ts"),
        format!("{base}.d.mts"),
        format!("{base}.d.cts"),
    ];

    candidates
        .iter()
        .any(|candidate| chunk_set.contains(candidate))
}

pub fn bundle_base_no_ext(path: &str) -> String {
    for ext in [".mjs", ".cjs", ".js"] {
        if let Some(trimmed) = path.strip_suffix(ext) {
            return trimmed.to_string();
        }
    }
    path.to_string()
}

pub fn is_bundle_file(path: &str) -> bool {
    !path.ends_with(".js.map")
        && (path.ends_with(".js") || path.ends_with(".mjs") || path.ends_with(".cjs"))
}

pub fn is_type_file(path: &str) -> bool {
    path.ends_with(".d.ts") || path.ends_with(".d.mts") || path.ends_with(".d.cts")
}
