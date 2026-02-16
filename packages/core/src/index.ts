import fs from "node:fs";
import path from "node:path";
import { analyzeFastifyEntry } from "@tsgodown/analyzer";
import type { UserConfig } from "@tsgodown/config";
import { loadUserConfig } from "@tsgodown/config";
import { runPipeline } from "@tsgodown/pipeline";

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

function resolvePlan(
  cwd: string,
  conf: UserConfig,
  configIndex: number,
): BuildTargetPlan {
  const entry = typeof conf.entry === "string" ? conf.entry : "src/index.ts";
  const outDir = conf.outDir ?? "dist-go";
  const resolvedOut = path.resolve(cwd, outDir);
  return {
    configIndex,
    entry: path.resolve(cwd, entry),
    outDir: resolvedOut,
    artifact: path.join(resolvedOut, "main.go"),
  };
}

async function run(
  cwd: string,
  command: "build" | "check" | "report",
): Promise<BuildSummary> {
  const configs = await loadUserConfig(cwd);
  const targets: BuildTargetResult[] = [];

  if (command === "build") {
    await runPipeline(cwd);
  }

  for (const [idx, conf] of configs.entries()) {
    const plan = resolvePlan(cwd, conf, idx);
    const ir = analyzeFastifyEntry(plan.entry);

    if (command === "build") {
      // Emission already handled by pipeline orchestration.
    }

    const artifactExists = fs.existsSync(plan.artifact);
    targets.push({
      ...plan,
      emitted: command === "build" ? true : artifactExists,
      diagnostics: {
        routes: ir.routes.length,
        warnings: ir.diagnostics
          .filter((d) => d.level === "warn")
          .map((d) => d.message),
      },
    });
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
      const plan = resolvePlan(cwd, conf, idx);
      return {
        ...plan,
        diagnostics: { routes: 0, warnings: [] },
        emitted: fs.existsSync(plan.artifact),
      };
    }),
  };
}
