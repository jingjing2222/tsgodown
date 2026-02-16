import type { UserConfig } from "@tsgodown/config";

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
      if (!buildResult.manifestPath || !buildResult.manifestIndexPath) {
        throw new Error(
          "rust adapter contract violation: missing manifest or manifest index path",
        );
      }

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
