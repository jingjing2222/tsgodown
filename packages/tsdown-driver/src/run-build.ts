import { normalizeRustEngineResponse } from "./contract.js";
import { writeManifest } from "./manifest.js";
import { invokeRustEngine } from "./process-adapter.js";
import type {
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

  const manifestPath = await writeManifest(cwd, response.manifest);
  return {
    mode: "rust-engine-adapter",
    manifestPath,
    manifest: response.manifest,
    diagnostics: response.diagnostics ?? [],
  };
}
