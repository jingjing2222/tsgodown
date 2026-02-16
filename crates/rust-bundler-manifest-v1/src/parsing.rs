use crate::{ArtifactManifest, ManifestError};

pub fn parse_manifest(raw: &str) -> Result<ArtifactManifest, ManifestError> {
    Ok(serde_json::from_str(raw)?)
}
