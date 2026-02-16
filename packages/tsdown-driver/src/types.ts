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

interface BuildChunkLike {
  fileName?: string;
}

export interface TsdownBundleLike {
  chunks: BuildChunkLike[];
  config?: {
    entry?: Record<string, string>;
    tsconfig?: false | string;
  };
}

export interface RustEngineRequest {
  action: "build";
  cwd: string;
  configPath?: string;
}

export type RustEngineResponse =
  | {
      ok: true;
      manifest: ArtifactManifest;
      diagnostics?: string[];
    }
  | {
      ok: false;
      error: {
        source: string;
        cause: string;
        guidance: string;
      };
    }
  | {
      status: "ok" | "success";
      manifest: ArtifactManifest;
      diagnostics?: unknown[];
    }
  | {
      status: "error" | "failed";
      error?: {
        source?: unknown;
        cause?: unknown;
        guidance?: unknown;
      };
      source?: unknown;
      cause?: unknown;
      guidance?: unknown;
    };

export interface ArtifactManifestIndex {
  buildId: string;
  manifest: string;
  generatedAt: string;
}

export interface RunBuildResult {
  mode: "rust-engine-adapter";
  manifestPath: string;
  manifestIndexPath: string;
  manifest: ArtifactManifest;
  diagnostics: string[];
}

export interface RunBuildOptions {
  executeRustEngine?: (
    request: RustEngineRequest,
  ) => Promise<RustEngineResponse>;
}
