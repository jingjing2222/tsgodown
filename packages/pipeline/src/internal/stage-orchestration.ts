import http from "node:http";
import { spawn, spawnSync } from "node:child_process";
import path from "node:path";
import { setTimeout as sleep } from "node:timers/promises";

import type { UserConfig } from "@tsgodown/config";
import { emitGoProject } from "@tsgodown/emitter-go";
import type { RunBuildResult } from "@tsgodown/tsdown-driver";

import { buildProgramIrFromArtifacts } from "./artifact-to-ir.js";
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
  verifyGoRunnable?: boolean;
}

export async function orchestratePipelineStages({
  cwd,
  configs,
  log,
  onStage,
  runBuildArtifacts = runBuildArtifactsViaRustAdapter,
  verifyGoRunnable = process.env.TSGODOWN_VERIFY_GO_RUNNABLE === "1",
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
      const ir = buildProgramIrFromArtifacts(buildResult, entry);
      emitStage(
        "BUILD_IR",
        `[BUILD_IR] analyzing entry: ${entry} (delegated to rust engine, buildId=${buildResult.manifest.buildId})`,
      );
      const sourceMappedIr = buildProgramIrFromArtifacts(buildResult, entry, {
        cwd,
      });

      stage = "CAPABILITY_GATE";
      emitStage(
        "CAPABILITY_GATE",
        "[CAPABILITY_GATE] validating required capabilities (delegated to rust engine)",
      );

      stage = "EMIT_GO";
      const goOutDir = path.join(cwd, "dist-go");
      emitGoProject(sourceMappedIr.modules.length > 0 ? sourceMappedIr : ir, goOutDir);
      emitStage(
        "EMIT_GO",
        `[EMIT_GO] wrote Go scaffold to ${goOutDir}`,
      );
      if (verifyGoRunnable) {
        await verifyCompiledGoRuntime(goOutDir, emitStage);
      }

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

async function verifyCompiledGoRuntime(
  goOutDir: string,
  emitStage: (currentStage: PipelineStage, message: string) => void,
): Promise<void> {
  const version = spawnSync("go", ["version"], { encoding: "utf8" });
  if (version.status !== 0) {
    emitStage(
      "EMIT_GO",
      "[EMIT_GO] Go toolchain not available; skipped compile/runtime verification",
    );
    return;
  }

  const binaryName = process.platform === "win32" ? "tsgodown-local.exe" : "tsgodown-local";
  const binaryPath = path.join(goOutDir, binaryName);

  const build = spawnSync("go", ["build", "-o", binaryName, "."], {
    cwd: goOutDir,
    encoding: "utf8",
  });
  if (build.status !== 0) {
    throw new Error(
      `[pipeline] go build failed: ${(build.stderr || build.stdout || "").trim()}`,
    );
  }
  emitStage("EMIT_GO", `[EMIT_GO] go build succeeded: ${binaryPath}`);

  const port = String(19000 + Math.floor(Math.random() * 500));
  const runtime = spawn(binaryPath, [], {
    cwd: goOutDir,
    env: {
      ...process.env,
      PORT: port,
    },
    stdio: "ignore",
  });

  try {
    await waitForHealthRuntime(port);
    emitStage("EMIT_GO", `[EMIT_GO] go runtime health check passed on :${port}`);
  } finally {
    if (!runtime.killed) {
      runtime.kill("SIGTERM");
    }
  }
}

async function waitForHealthRuntime(port: string): Promise<void> {
  const startedAt = Date.now();
  const timeoutMs = 8_000;

  while (Date.now() - startedAt < timeoutMs) {
    const status = await requestStatusCode(port, "/health");
    if (status !== null && status >= 200 && status < 500) {
      return;
    }
    await sleep(200);
  }
  throw new Error("[pipeline] go runtime health check timed out");
}

function requestStatusCode(port: string, pathname: string): Promise<number | null> {
  return new Promise((resolve) => {
    const req = http.get(
      {
        hostname: "127.0.0.1",
        port: Number(port),
        path: pathname,
        timeout: 400,
      },
      (res) => {
        resolve(res.statusCode ?? null);
      },
    );
    req.on("timeout", () => {
      req.destroy();
      resolve(null);
    });
    req.on("error", () => resolve(null));
  });
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
