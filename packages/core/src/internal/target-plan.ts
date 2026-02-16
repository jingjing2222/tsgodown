import path from "node:path";
import type { UserConfig } from "@tsgodown/config";
import type { BuildTargetPlan } from "../index.js";

const ARTIFACT_MANIFEST_SEGMENTS = [
  "artifacts",
  "manifests",
  "manifest.json",
] as const;

export function resolveArtifactManifestPath(cwd: string): string {
  return path.join(cwd, ...ARTIFACT_MANIFEST_SEGMENTS);
}

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
    artifact: resolveArtifactManifestPath(cwd),
  };
}
