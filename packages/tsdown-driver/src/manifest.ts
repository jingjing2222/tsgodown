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

  assertManifestIndexContract(manifest, manifestIndex);

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

export function assertManifestIndexContract(
  manifest: ArtifactManifest,
  manifestIndex: ArtifactManifestIndex,
): void {
  const violations: string[] = [];

  if (!manifest.buildId?.trim()) {
    violations.push("manifest.buildId must be a non-empty string");
  }

  if (!manifestIndex.buildId?.trim()) {
    violations.push("manifest index buildId must be a non-empty string");
  }

  if (
    manifest.buildId?.trim() &&
    manifestIndex.buildId?.trim() &&
    manifestIndex.buildId !== manifest.buildId
  ) {
    violations.push(
      `manifest index buildId mismatch (manifest=${manifest.buildId}, index=${manifestIndex.buildId})`,
    );
  }

  if (manifestIndex.manifest !== "manifest.json") {
    violations.push(
      `manifest index manifest must equal \"manifest.json\" (received=${manifestIndex.manifest})`,
    );
  }

  if (!manifestIndex.generatedAt?.trim()) {
    violations.push("manifest index generatedAt must be a non-empty string");
  } else if (Number.isNaN(Date.parse(manifestIndex.generatedAt))) {
    violations.push(
      `manifest index generatedAt must be ISO-8601 parseable (received=${manifestIndex.generatedAt})`,
    );
  }

  if (violations.length > 0) {
    throw new Error(
      `[tsdown-driver] artifact contract violation: ${violations.join("; ")}`,
    );
  }
}
