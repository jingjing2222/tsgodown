export type BundleFormat = "esm" | "cjs";
export interface ArtifactBundle {
    file: string;
    map?: string;
    format: BundleFormat;
    exports: string[];
}
export interface ArtifactManifest {
    buildId: string;
    entries: string[];
    bundles: ArtifactBundle[];
    types: string[];
    tsconfigPath: string;
}
export interface RunBuildResult {
    mode: "fallback-adapter";
    manifestPath: string;
    manifest: ArtifactManifest;
    diagnostics: string[];
}
export declare function runBuild(cwd: string, configPath?: string): Promise<RunBuildResult>;
export declare function collectArtifacts(cwd: string, configPath?: string): Promise<ArtifactManifest>;
export declare function writeManifest(cwd: string, manifest: ArtifactManifest): Promise<string>;
