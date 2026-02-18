import path from "node:path";
import type { UserConfig } from "@tsgodown/config";
import { emitGoProject } from "@tsgodown/emitter-go";
import { checkCapabilities } from "@tsgodown/node-compat";
import type { RunBuildResult } from "@tsgodown/tsdown-driver";

import { buildProgramIrFromArtifacts } from "./artifact-to-ir.js";
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
    const outDir = path.resolve(cwd, config.outDir ?? "dist-go");
    let stage = "BUILD_ARTIFACTS";

    try {
      log("[BUILD_ARTIFACTS] collecting tsdown build outputs");
      const buildResult = await runBuildArtifactsViaRustAdapter(cwd);
      assertBuildArtifactContract(buildResult);

      stage = "BUILD_IR";
      log(
        `[BUILD_IR] deriving ProgramIR from artifacts (buildId=${buildResult.manifest.buildId})`,
      );
      const ir = buildProgramIrFromArtifacts(buildResult, entry);

      stage = "CAPABILITY_GATE";
      log("[CAPABILITY_GATE] checking required capabilities for ProgramIR");
      const capabilityCheck = checkCapabilities(ir, {
        allowWip: true,
        failFast: true,
      });
      if (!capabilityCheck.ok) {
        throw new Error(
          capabilityCheck.diagnostics[0]?.message ?? "capability gate failed",
        );
      }

      stage = "EMIT_GO";
      log(
        `[EMIT_GO] emitting Go project to ${path.relative(cwd, outDir) || "."}`,
      );
      emitGoProject(ir, outDir);

      stage = "ON_SUCCESS";
      await config.onSuccess?.();
    } catch (cause) {
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

  if (!Array.isArray(buildResult.manifest?.bundles)) {
    violations.push("manifest.bundles must be an array");
  }

  if (!Array.isArray(buildResult.manifest?.types)) {
    violations.push("manifest.types must be an array");
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
