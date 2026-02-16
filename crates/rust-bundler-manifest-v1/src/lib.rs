mod builder;
mod contract;
mod diagnostics;
mod ordering;
mod parsing;
mod validation;

pub use builder::build_manifest;
pub use contract::{
    ArtifactBundle, ArtifactManifest, BuildInput, BuildManifestOutput, BundleFormat,
    DiagnosticLevel, ManifestDiagnostic, ManifestError,
};
pub use parsing::parse_manifest;
