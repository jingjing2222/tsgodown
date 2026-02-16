import { promises as fs } from "node:fs";
import path from "node:path";

import { shapeArtifactManifest } from "./artifact-indexer/shaping.js";
import type {
  ArtifactManifest,
  ArtifactManifestIndex,
  TsdownBundleLike,
} from "./types.js";

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
  return (await writeManifestArtifacts(cwd, manifest)).manifestPath;
}

export async function writeManifestArtifacts(
  cwd: string,
  manifest: ArtifactManifest,
): Promise<{ manifestPath: string; manifestIndexPath: string }> {
  const outDir = path.join(cwd, "artifacts", "manifests");
  await fs.mkdir(outDir, { recursive: true });

  const manifestPath = path.join(outDir, "manifest.json");
  await fs.writeFile(
    manifestPath,
    `${JSON.stringify(manifest, null, 2)}\n`,
    "utf8",
  );

  const manifestIndexPath = path.join(outDir, "index.json");
  const manifestIndex: ArtifactManifestIndex = {
    buildId: manifest.buildId,
    manifest: "manifest.json",
    generatedAt: new Date().toISOString(),
  };

  await fs.writeFile(
    manifestIndexPath,
    `${JSON.stringify(manifestIndex, null, 2)}\n`,
    "utf8",
  );

  return {
    manifestPath,
    manifestIndexPath,
  };
}
