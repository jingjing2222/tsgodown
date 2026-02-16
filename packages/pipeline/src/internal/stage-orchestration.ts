import type { UserConfig } from "@tsgodown/config";
import type { RunBuildResult } from "@tsgodown/tsdown-driver";

import { formatPipelineFailure, resolveEntry } from "./result-normalization.js";
import { runBuildArtifactsViaRustAdapter } from "./rust-adapter-boundary.js";

export interface StageOrchestrationOptions {
  cwd: string;
  configs: UserConfig[];
  log: (message: string) => void;
}

export async function orchestratePipelineStages({
  cwd,
  configs,
  log,
}: StageOrchestrationOptions): Promise<void> {
  for (const config of configs) {
    const entry = resolveEntry(config);

    try {
      log("[BUILD_ARTIFACTS] collecting build outputs");
      const buildResult = await runBuildArtifactsViaRustAdapter(cwd);
      assertBuildArtifactContract(buildResult);

      log(
        `[BUILD_IR] analyzing entry: ${entry} (delegated to rust engine, buildId=${buildResult.manifest.buildId})`,
      );
      log(
        "[CAPABILITY_GATE] validating required capabilities (delegated to rust engine)",
      );
      log("[EMIT_GO] writing Go scaffold (delegated to rust engine)");
      await config.onSuccess?.();
    } catch (cause) {
      throw formatPipelineFailure(entry, cause);
    }
  }
}

export function assertBuildArtifactContract(buildResult: RunBuildResult): void {
  const violations: string[] = [];

  if (!buildResult.manifestPath?.trim()) {
    violations.push("manifestPath must be a non-empty string");
  }

  if (!buildResult.manifestIndexPath?.trim()) {
    violations.push("manifestIndexPath must be a non-empty string");
  }

  if (!buildResult.manifest?.buildId?.trim()) {
    violations.push("manifest.buildId must be a non-empty string");
  }

  if (!Array.isArray(buildResult.manifest?.entries)) {
    violations.push("manifest.entries must be an array");
  }

  if (violations.length > 0) {
    throw new Error(
      `[pipeline] artifact contract violation: ${violations.join("; ")}`,
    );
  }
}
