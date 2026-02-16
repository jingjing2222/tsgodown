import { createHash } from "node:crypto";
import { promises as fs } from "node:fs";
import path from "node:path";

import type { ArtifactManifest, TsdownBundleLike } from "./types.js";

export function buildManifestFromBundles(
  cwd: string,
  bundles: TsdownBundleLike[],
  configPath?: string,
): ArtifactManifest {
  const chunkFiles = uniqueSorted(
    bundles
      .flatMap((bundle) => bundle.chunks ?? [])
      .map((chunk) => normalizeChunkFile(cwd, chunk.fileName))
      .filter((file): file is string => Boolean(file)),
  );

  const chunkSet = new Set(chunkFiles);
  const bundleFiles = chunkFiles.filter(isBundleFile);
  const bundlesOut = bundleFiles.map((file) => ({
    file,
    map: chunkSet.has(`${file}.map`) ? `${file}.map` : undefined,
    format: file.endsWith(".cjs") ? ("cjs" as const) : ("esm" as const),
    exports: [],
  }));

  const typeFiles = chunkFiles.filter(isTypeFile);

  const entries = uniqueSorted(
    bundles.flatMap((bundle) => {
      const entryMap = bundle.config?.entry;
      if (!entryMap) return [];
      return Object.values(entryMap)
        .map((entryPath) => normalizeChunkFile(cwd, entryPath))
        .filter((entry): entry is string => Boolean(entry));
    }),
  );

  const tsconfigPath = resolveTsconfigPath(cwd, bundles, configPath);

  const manifestBase = {
    entries,
    bundles: bundlesOut,
    types: typeFiles,
    tsconfigPath,
  };

  return {
    buildId: createBuildId(manifestBase),
    ...manifestBase,
  };
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

function resolveTsconfigPath(
  cwd: string,
  bundles: TsdownBundleLike[],
  configPath?: string,
): string {
  if (configPath) return toPosix(configPath);

  for (const bundle of bundles) {
    const tsconfig = bundle.config?.tsconfig;
    if (typeof tsconfig === "string") {
      return normalizeChunkFile(cwd, tsconfig) ?? toPosix(tsconfig);
    }
  }

  return "tsconfig.json";
}

function createBuildId(input: Omit<ArtifactManifest, "buildId">): string {
  const normalized = JSON.stringify(input);
  return createHash("sha256").update(normalized).digest("hex").slice(0, 16);
}

function isBundleFile(relFile: string): boolean {
  if (relFile.endsWith(".js.map")) return false;
  return (
    relFile.endsWith(".js") ||
    relFile.endsWith(".mjs") ||
    relFile.endsWith(".cjs")
  );
}

function isTypeFile(relFile: string): boolean {
  return (
    relFile.endsWith(".d.ts") ||
    relFile.endsWith(".d.mts") ||
    relFile.endsWith(".d.cts")
  );
}

function normalizeChunkFile(
  cwd: string,
  fileName?: string,
): string | undefined {
  if (!fileName) return undefined;
  const normalized = path.isAbsolute(fileName)
    ? path.relative(cwd, fileName)
    : fileName;
  return toPosix(normalized);
}

function uniqueSorted(values: string[]): string[] {
  return [...new Set(values)].sort((a, b) => a.localeCompare(b));
}

function toPosix(p: string): string {
  return p.split(path.sep).join("/");
}
