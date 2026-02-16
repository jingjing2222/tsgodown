import path from "node:path";
import type { UserConfig } from "@tsgodown/config";
import type { BuildTargetPlan } from "../index.js";

export function resolveTargetPlan(
  cwd: string,
  conf: UserConfig,
  configIndex: number,
): BuildTargetPlan {
  const entry = typeof conf.entry === "string" ? conf.entry : "src/index.ts";
  const outDir = conf.outDir ?? "dist-go";

  return {
    configIndex,
    entry: path.resolve(cwd, entry),
    outDir: path.resolve(cwd, outDir),
    artifact: path.join(cwd, "artifacts", "manifests", "manifest.json"),
  };
}
