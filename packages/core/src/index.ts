import fs from "node:fs";
import { loadUserConfig } from "@tsgodown/config";
import { runPipeline } from "@tsgodown/pipeline";
import {
  buildTargetResult,
  resolveArtifactManifestPath,
  resolveTargetPlan,
} from "./internal/index.js";

export const ACTIVE_STAGES = [
  "load-config",
  "analyze",
  "emit",
  "onSuccess",
] as const;

export type BuildStage = (typeof ACTIVE_STAGES)[number];

export interface BuildTargetPlan {
  configIndex: number;
  entry: string;
  outDir: string;
  artifact: string;
}

export interface BuildTargetDiagnostics {
  routes: number;
  warnings: string[];
}

export interface BuildTargetResult extends BuildTargetPlan {
  diagnostics: BuildTargetDiagnostics;
  emitted: boolean;
}

export interface BuildSummary {
  ok: boolean;
  cwd: string;
  command: "build" | "check" | "report";
  stages: readonly BuildStage[];
  targets: BuildTargetResult[];
}

async function run(
  cwd: string,
  command: "build" | "check" | "report",
): Promise<BuildSummary> {
  const configs = await loadUserConfig(cwd);
  const targets: BuildTargetResult[] = [];

  const artifactPath = resolveArtifactManifestPath(cwd);
  if (command === "build" || !fs.existsSync(artifactPath)) {
    await runPipeline(cwd);
  }

  for (const [idx, conf] of configs.entries()) {
    const plan = resolveTargetPlan(cwd, conf, idx);
    const artifactExists = fs.existsSync(plan.artifact);
    targets.push(buildTargetResult(plan, artifactExists));
  }

  return {
    ok: true,
    cwd,
    command,
    stages: ACTIVE_STAGES,
    targets,
  };
}

export async function build(cwd: string): Promise<BuildSummary> {
  return run(cwd, "build");
}

export async function check(cwd: string): Promise<BuildSummary> {
  return run(cwd, "check");
}

export async function report(cwd: string): Promise<BuildSummary> {
  return run(cwd, "report");
}

export async function stages(
  cwd: string,
): Promise<Pick<BuildSummary, "cwd" | "stages" | "targets">> {
  const configs = await loadUserConfig(cwd);
  return {
    cwd,
    stages: ACTIVE_STAGES,
    targets: configs.map((conf, idx) => {
      const plan = resolveTargetPlan(cwd, conf, idx);
      return buildTargetResult(plan, fs.existsSync(plan.artifact));
    }),
  };
}
