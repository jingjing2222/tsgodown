import { normalizeRustEngineResponse } from "./contract.js";
import { writeManifestArtifacts } from "./manifest.js";
import { invokeRustEngine } from "./process-adapter.js";
import type {
  ArtifactManifest,
  RunBuildOptions,
  RunBuildResult,
  RustEngineRequest,
} from "./types.js";

export async function runBuild(
  cwd: string,
  configPath?: string,
  options: RunBuildOptions = {},
): Promise<RunBuildResult> {
  const request: RustEngineRequest = {
    action: "build",
    cwd,
    ...(configPath ? { configPath } : {}),
  };

  const executeRustEngine =
    options.executeRustEngine ?? ((req) => invokeRustEngine(req));
  const response = normalizeRustEngineResponse(
    await executeRustEngine(request),
    "<adapter-response>",
  );

  if (!response.ok) {
    throw new Error(
      `[tsdown-driver] rust engine failed source=${response.error.source} cause=${response.error.cause} guidance=${response.error.guidance}`,
    );
  }

  const { manifestPath, manifestIndexPath } = await writeManifestArtifacts(
    cwd,
    response.manifest,
  );

  assertRunBuildArtifactContract({
    manifestPath,
    manifestIndexPath,
    manifest: response.manifest,
  });

  return {
    mode: "rust-engine-adapter",
    manifestPath,
    manifestIndexPath,
    manifest: response.manifest,
    diagnostics: response.diagnostics ?? [],
  };
}

function assertRunBuildArtifactContract(input: {
  manifestPath: string;
  manifestIndexPath: string;
  manifest: ArtifactManifest;
}): void {
  const violations: string[] = [];

  if (!input.manifestPath?.trim()) {
    violations.push("manifestPath must be a non-empty string");
  }

  if (!input.manifestIndexPath?.trim()) {
    violations.push("manifestIndexPath must be a non-empty string");
  }

  if (!input.manifest?.buildId?.trim()) {
    violations.push("manifest.buildId must be a non-empty string");
  }

  if (!Array.isArray(input.manifest?.entries)) {
    violations.push("manifest.entries must be an array");
  }

  if (violations.length > 0) {
    throw new Error(
      `[tsdown-driver] artifact contract violation: ${violations.join("; ")}`,
    );
  }
}
