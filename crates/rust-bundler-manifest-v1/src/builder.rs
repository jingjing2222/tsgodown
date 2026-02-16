use std::collections::BTreeSet;

use crate::{
    diagnostics::{
        missing_bundle_link, missing_sourcemap_link, missing_types_link, sort_diagnostics,
    },
    ordering::{sorted_unique, stable_hash_64},
    validation::{bundle_base_no_ext, has_matching_types, is_bundle_file, is_type_file},
    ArtifactBundle, ArtifactManifest, BuildInput, BuildManifestOutput, BundleFormat,
};

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
        diagnostics.push(missing_bundle_link(&map));
    }

    let mut bundles = Vec::new();
    for file in bundle_files {
        let map_link = format!("{file}.map");
        let map = if chunk_set.contains(&map_link) {
            Some(map_link)
        } else {
            diagnostics.push(missing_sourcemap_link(&file, &map_link));
            None
        };

        if !has_matching_types(&file, &chunk_set) {
            let base = bundle_base_no_ext(&file);
            diagnostics.push(missing_types_link(&file, &base));
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

    sort_diagnostics(&mut diagnostics);

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
