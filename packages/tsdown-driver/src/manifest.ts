import { promises as fs } from "node:fs";
import path from "node:path";

import { shapeArtifactManifest } from "./artifact-indexer/shaping.js";
import type { ArtifactManifest, TsdownBundleLike } from "./types.js";

export function buildManifestFromBundles(
  cwd: string,
  bundles: TsdownBundleLike[],
  configPath?: string,
): ArtifactManifest {
  return shapeArtifactManifest(cwd, bundles, configPath);
}

export async function writeManifest(
  cwd: string,
  manifest: ArtifactManifest,
): Promise<string> {
  const outDir = path.join(cwd, "artifacts", "manifests");
  await fs.mkdir(outDir, { recursive: true });

  const manifestPath = path.join(outDir, "manifest.json");
  await fs.writeFile(
    manifestPath,
    `${JSON.stringify(manifest, null, 2)}\n`,
    "utf8",
  );
  return manifestPath;
}
