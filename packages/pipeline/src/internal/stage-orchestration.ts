import type { UserConfig } from "@tsgodown/config";
import type { RunBuildResult } from "@tsgodown/tsdown-driver";

import { formatPipelineFailure, resolveEntry } from "./result-normalization.js";
import { runBuildArtifactsViaRustAdapter } from "./rust-adapter-boundary.js";

export type PipelineStage =
  | "BUILD_ARTIFACTS"
  | "BUILD_IR"
  | "CAPABILITY_GATE"
  | "EMIT_GO"
  | "ON_SUCCESS"
  | "ON_FAILURE";

export interface PipelineStageEvent {
  entry: string;
  stage: PipelineStage;
  message: string;
}

export interface StageOrchestrationOptions {
  cwd: string;
  configs: UserConfig[];
  log: (message: string) => void;
  onStage?: (event: PipelineStageEvent) => void;
  runBuildArtifacts?: (cwd: string) => Promise<RunBuildResult>;
}

export async function orchestratePipelineStages({
  cwd,
  configs,
  log,
  onStage,
  runBuildArtifacts = runBuildArtifactsViaRustAdapter,
}: StageOrchestrationOptions): Promise<void> {
  for (const config of configs) {
    const entry = resolveEntry(config);
    let stage: PipelineStage = "BUILD_ARTIFACTS";

    const emitStage = (currentStage: PipelineStage, message: string): void => {
      log(message);
      onStage?.({
        entry,
        stage: currentStage,
        message,
      });
    };

    try {
      emitStage(
        "BUILD_ARTIFACTS",
        "[BUILD_ARTIFACTS] collecting build outputs",
      );
      const buildResult = await runBuildArtifacts(cwd);
      assertBuildArtifactContract(buildResult);

      stage = "BUILD_IR";
      emitStage(
        "BUILD_IR",
        `[BUILD_IR] analyzing entry: ${entry} (delegated to rust engine, buildId=${buildResult.manifest.buildId})`,
      );

      stage = "CAPABILITY_GATE";
      emitStage(
        "CAPABILITY_GATE",
        "[CAPABILITY_GATE] validating required capabilities (delegated to rust engine)",
      );

      stage = "EMIT_GO";
      emitStage(
        "EMIT_GO",
        "[EMIT_GO] writing Go scaffold (delegated to rust engine)",
      );

      stage = "ON_SUCCESS";
      await config.onSuccess?.();
      emitStage("ON_SUCCESS", `[ON_SUCCESS] completed pipeline for ${entry}`);
    } catch (cause) {
      emitStage("ON_FAILURE", `[ON_FAILURE] ${entry} failed at stage ${stage}`);
      throw formatPipelineFailure(entry, {
        source: `pipeline-entry(${entry})`,
        stage,
        cause: cause instanceof Error ? cause.message : String(cause),
        guidance:
          "Verify rust engine build/analyze contract and tsgodown.config.ts settings.",
      });
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

  assertCompileInputContract(buildResult);
}

export function assertCompileInputContract(buildResult: RunBuildResult): void {
  const violations: string[] = [];
  const bundles = buildResult.manifest.bundles;

  if (bundles.length === 0) {
    violations.push("manifest.bundles must include at least one JS bundle");
  }

  const firstBundle = bundles[0];
  if (!firstBundle?.file?.trim()) {
    violations.push("manifest.bundles[0].file must be a non-empty string");
  }

  if (violations.length > 0) {
    throw new Error(
      `[pipeline] compile-input contract violation: ${violations.join("; ")}`,
    );
  }
}
