import { createHash } from "node:crypto";

import type {
  ArtifactManifest,
  BundleFormat,
  TsdownBundleLike,
} from "../types.js";
import {
  indexArtifacts,
  isBundleFile,
  isTypeFile,
  normalizeTsconfigPath,
} from "./internal/index.js";

export function shapeArtifactManifest(
  cwd: string,
  bundles: TsdownBundleLike[],
  configPath?: string,
): ArtifactManifest {
  const indexed = indexArtifacts(cwd, bundles);

  const chunkSet = new Set(indexed.chunkFiles);
  const bundleFiles = indexed.chunkFiles.filter(isBundleFile);
  const bundlesOut = bundleFiles.map((file) => ({
    file,
    map: chunkSet.has(`${file}.map`) ? `${file}.map` : undefined,
    format: inferBundleFormat(file),
    exports: [],
  }));

  const manifestBase = {
    entries: indexed.entries,
    bundles: bundlesOut,
    types: indexed.chunkFiles.filter(isTypeFile),
    tsconfigPath: normalizeTsconfigPath(cwd, bundles, configPath),
  };

  return {
    buildId: createBuildId(manifestBase),
    ...manifestBase,
  };
}

function createBuildId(input: Omit<ArtifactManifest, "buildId">): string {
  const normalized = JSON.stringify(input);
  return createHash("sha256").update(normalized).digest("hex").slice(0, 16);
}

function inferBundleFormat(file: string): BundleFormat {
  return file.endsWith(".cjs") ? "cjs" : "esm";
}
