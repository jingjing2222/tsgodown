import path from "node:path";

import type { TsdownBundleLike } from "../types.js";

export function isBundleFile(relFile: string): boolean {
  if (relFile.endsWith(".js.map")) return false;
  return (
    relFile.endsWith(".js") ||
    relFile.endsWith(".mjs") ||
    relFile.endsWith(".cjs")
  );
}

export function isTypeFile(relFile: string): boolean {
  return (
    relFile.endsWith(".d.ts") ||
    relFile.endsWith(".d.mts") ||
    relFile.endsWith(".d.cts")
  );
}

export function normalizeTsconfigPath(
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

export function normalizeChunkFile(
  cwd: string,
  fileName?: string,
): string | undefined {
  if (!fileName) return undefined;
  const normalized = path.isAbsolute(fileName)
    ? path.relative(cwd, fileName)
    : fileName;
  return toPosix(normalized);
}

export function uniqueSorted(values: string[]): string[] {
  return [...new Set(values)].sort((a, b) => a.localeCompare(b));
}

function toPosix(p: string): string {
  return p.split(path.sep).join("/");
}
