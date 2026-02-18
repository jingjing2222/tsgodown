import path from "node:path";

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
      [
        "[tsdown-driver] rust engine failed",
        `source=${response.error.source}`,
        `cause=${response.error.cause}`,
        `guidance=${response.error.guidance}`,
      ].join("; "),
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

  if (Array.isArray(input.manifest?.entries)) {
    for (const entry of input.manifest.entries) {
      if (!isSafeRelativePath(entry)) {
        violations.push(`manifest.entries contains invalid path (${entry})`);
      }
    }
  }

  if (!Array.isArray(input.manifest?.bundles)) {
    violations.push("manifest.bundles must be an array");
  }

  for (const bundle of input.manifest?.bundles ?? []) {
    if (!isSafeRelativePath(bundle.file)) {
      violations.push(
        `manifest.bundles.file contains invalid path (${bundle.file})`,
      );
    }

    if (bundle.map !== undefined && !isSafeRelativePath(bundle.map)) {
      violations.push(
        `manifest.bundles.map contains invalid path (${bundle.map})`,
      );
    }
  }

  for (const typePath of input.manifest?.types ?? []) {
    if (!isSafeRelativePath(typePath)) {
      violations.push(`manifest.types contains invalid path (${typePath})`);
    }
  }

  if (
    input.manifest.tsconfigPath !== undefined &&
    !isSafeRelativePath(input.manifest.tsconfigPath)
  ) {
    violations.push("manifest.tsconfigPath must be a safe relative path");
  }

  if (violations.length > 0) {
    throw new Error(
      `[tsdown-driver] artifact contract violation: ${violations.join("; ")}`,
    );
  }
}

function isSafeRelativePath(value: string): boolean {
  if (!value?.trim()) {
    return false;
  }

  if (path.isAbsolute(value)) {
    return false;
  }

  const normalized = path.posix.normalize(value);
  if (
    normalized === ".." ||
    normalized.startsWith("../") ||
    normalized.includes("/../") ||
    normalized === "."
  ) {
    return false;
  }

  return true;
}
