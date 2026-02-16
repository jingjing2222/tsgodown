use std::collections::BTreeSet;

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum BundleFormat {
    Esm,
    Cjs,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ArtifactBundle {
    pub file: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub map: Option<String>,
    pub format: BundleFormat,
    pub exports: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ArtifactManifest {
    pub build_id: String,
    pub entries: Vec<String>,
    pub bundles: Vec<ArtifactBundle>,
    pub types: Vec<String>,
    pub tsconfig_path: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BuildInput {
    pub entries: Vec<String>,
    pub chunks: Vec<String>,
    pub tsconfig_path: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum DiagnosticLevel {
    Error,
    Warn,
    Info,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ManifestDiagnostic {
    pub level: DiagnosticLevel,
    pub code: String,
    pub message: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BuildManifestOutput {
    pub manifest: ArtifactManifest,
    pub diagnostics: Vec<ManifestDiagnostic>,
}

#[derive(thiserror::Error, Debug)]
pub enum ManifestError {
    #[error("failed to parse manifest json: {0}")]
    Parse(#[from] serde_json::Error),
}

pub fn parse_manifest(raw: &str) -> Result<ArtifactManifest, ManifestError> {
    Ok(serde_json::from_str(raw)?)
}

pub fn build_manifest(input: BuildInput) -> BuildManifestOutput {
    let all_chunks = sorted_unique(input.chunks);
    let chunk_set: BTreeSet<String> = all_chunks.iter().cloned().collect();

    let entries = sorted_unique(input.entries);
    let type_files = all_chunks
        .iter()
        .filter(|path| is_type_file(path))
        .cloned()
        .collect::<Vec<_>>();

    let bundle_files = all_chunks
        .iter()
        .filter(|path| is_bundle_file(path))
        .cloned()
        .collect::<Vec<_>>();

    let mut diagnostics = Vec::new();

    let orphan_maps = all_chunks
        .iter()
        .filter(|path| path.ends_with(".map"))
        .filter(|map| {
            let bundle = map.strip_suffix(".map").unwrap_or(map.as_str()).to_string();
            !chunk_set.contains(&bundle)
        })
        .cloned()
        .collect::<Vec<_>>();

    for map in orphan_maps {
        diagnostics.push(ManifestDiagnostic {
            level: DiagnosticLevel::Error,
            code: "MISSING_BUNDLE_LINK".to_string(),
            message: format!(
                "sourcemap '{}' does not have a matching bundle artifact",
                map
            ),
        });
    }

    let mut bundles = Vec::new();
    for file in bundle_files {
        let map_link = format!("{file}.map");
        let map = if chunk_set.contains(&map_link) {
            Some(map_link)
        } else {
            diagnostics.push(ManifestDiagnostic {
                level: DiagnosticLevel::Error,
                code: "MISSING_SOURCEMAP_LINK".to_string(),
                message: format!(
                    "bundle '{}' is missing sourcemap link '{}'.",
                    file, map_link
                )
                .trim_end_matches('.')
                .to_string(),
            });
            None
        };

        if !has_matching_types(&file, &chunk_set) {
            let base = bundle_base_no_ext(&file);
            diagnostics.push(ManifestDiagnostic {
                level: DiagnosticLevel::Error,
                code: "MISSING_TYPES_LINK".to_string(),
                message: format!(
                    "bundle '{}' is missing declaration link (expected one of: {}.d.ts, {}.d.mts, {}.d.cts)",
                    file, base, base, base
                ),
            });
        }

        bundles.push(ArtifactBundle {
            format: if file.ends_with(".cjs") {
                BundleFormat::Cjs
            } else {
                BundleFormat::Esm
            },
            file,
            map,
            exports: vec![],
        });
    }

    diagnostics.sort_by(|a, b| {
        a.code
            .cmp(&b.code)
            .then_with(|| a.message.cmp(&b.message))
            .then_with(|| level_rank(&a.level).cmp(&level_rank(&b.level)))
    });

    let mut manifest = ArtifactManifest {
        build_id: String::new(),
        entries,
        bundles,
        types: type_files,
        tsconfig_path: input.tsconfig_path,
    };
    manifest.build_id = create_build_id(&manifest);

    BuildManifestOutput {
        manifest,
        diagnostics,
    }
}

fn create_build_id(manifest: &ArtifactManifest) -> String {
    let serialized = serde_json::to_vec(manifest).expect("manifest should serialize");
    let digest = stable_hash_64(&serialized);
    format!("{digest:016x}")
}

fn stable_hash_64(bytes: &[u8]) -> u64 {
    const FNV_OFFSET: u64 = 0xcbf29ce484222325;
    const FNV_PRIME: u64 = 0x100000001b3;

    let mut hash = FNV_OFFSET;
    for byte in bytes {
        hash ^= *byte as u64;
        hash = hash.wrapping_mul(FNV_PRIME);
    }
    hash
}

fn has_matching_types(bundle: &str, chunk_set: &BTreeSet<String>) -> bool {
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

fn bundle_base_no_ext(path: &str) -> String {
    for ext in [".mjs", ".cjs", ".js"] {
        if let Some(trimmed) = path.strip_suffix(ext) {
            return trimmed.to_string();
        }
    }
    path.to_string()
}

fn is_bundle_file(path: &str) -> bool {
    !path.ends_with(".js.map")
        && (path.ends_with(".js") || path.ends_with(".mjs") || path.ends_with(".cjs"))
}

fn is_type_file(path: &str) -> bool {
    path.ends_with(".d.ts") || path.ends_with(".d.mts") || path.ends_with(".d.cts")
}

fn sorted_unique(values: Vec<String>) -> Vec<String> {
    let mut set = BTreeSet::new();
    for value in values {
        set.insert(value);
    }
    set.into_iter().collect()
}

fn level_rank(level: &DiagnosticLevel) -> u8 {
    match level {
        DiagnosticLevel::Error => 0,
        DiagnosticLevel::Warn => 1,
        DiagnosticLevel::Info => 2,
    }
}
