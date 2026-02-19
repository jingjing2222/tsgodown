import fs from "node:fs";
import path from "node:path";

import type { DiagnosticIR } from "@tsgodown/ir-core";
import type { ArtifactManifest } from "@tsgodown/tsdown-driver";

export interface ArtifactMetadata {
  exports: string[];
  sourcePath?: string;
  diagnostics: DiagnosticIR[];
}

interface SourceMapLike {
  sources?: unknown;
}

export function ingestArtifactMetadata(
  repoRoot: string,
  manifest: ArtifactManifest,
): ArtifactMetadata {
  const exports = collectTypeExports(repoRoot, manifest.types ?? []);
  const sourceMapResult = resolveSourcePathFromMaps(repoRoot, manifest);

  return {
    exports,
    sourcePath: sourceMapResult.sourcePath,
    diagnostics: sourceMapResult.diagnostics,
  };
}

function collectTypeExports(repoRoot: string, typeFiles: string[]): string[] {
  const names = new Set<string>();

  for (const relPath of typeFiles) {
    const absPath = path.join(repoRoot, relPath);
    if (!fs.existsSync(absPath)) {
      continue;
    }

    const text = fs.readFileSync(absPath, "utf8");
    for (const line of text.split(/\r?\n/)) {
      const declareMatch =
        /^\s*export\s+declare\s+(?:const|let|var|function|class|interface|type|enum)\s+([A-Za-z_$][\w$]*)/.exec(
          line,
        );
      if (declareMatch?.[1]) {
        names.add(declareMatch[1]);
      }

      const braceExportMatch = /^\s*export\s*\{([^}]+)\}/.exec(line);
      if (braceExportMatch?.[1]) {
        for (const segment of braceExportMatch[1]
          .split(",")
          .map((value) => value.trim())) {
          if (!segment) {
            continue;
          }
          const asMatch =
            /^(?:type\s+)?([A-Za-z_$][\w$]*)(?:\s+as\s+([A-Za-z_$][\w$]*))?$/.exec(
              segment,
            );
          if (!asMatch) {
            continue;
          }
          names.add(asMatch[2] ?? asMatch[1]);
        }
      }
    }
  }

  return [...names].sort((a, b) => a.localeCompare(b));
}

function resolveSourcePathFromMaps(
  repoRoot: string,
  manifest: ArtifactManifest,
): { sourcePath?: string; diagnostics: DiagnosticIR[] } {
  const diagnostics: DiagnosticIR[] = [];
  const candidateSources: string[] = [];

  for (const bundle of manifest.bundles) {
    if (!bundle.map?.trim()) {
      diagnostics.push({
        level: "warn",
        code: "ARTIFACT_SOURCEMAP_MISSING",
        message: `bundle ${bundle.file} does not declare a sourcemap path`,
        source: {
          file: bundle.file,
          viaSourceMap: true,
        },
      });
      continue;
    }

    const mapPath = path.join(repoRoot, bundle.map);
    let mapText: string;
    try {
      mapText = fs.readFileSync(mapPath, "utf8");
    } catch {
      diagnostics.push({
        level: "warn",
        code: "ARTIFACT_SOURCEMAP_INVALID",
        message: `failed to read sourcemap ${bundle.map}`,
        source: {
          file: bundle.map,
          viaSourceMap: true,
        },
      });
      continue;
    }

    let parsed: SourceMapLike;
    try {
      parsed = JSON.parse(mapText) as SourceMapLike;
    } catch {
      diagnostics.push({
        level: "warn",
        code: "ARTIFACT_SOURCEMAP_INVALID",
        message: `failed to parse sourcemap ${bundle.map}`,
        source: {
          file: bundle.map,
          viaSourceMap: true,
        },
      });
      continue;
    }

    if (!Array.isArray(parsed.sources) || parsed.sources.length === 0) {
      diagnostics.push({
        level: "warn",
        code: "ARTIFACT_SOURCEMAP_INVALID",
        message: `sourcemap ${bundle.map} does not include sources[]`,
        source: {
          file: bundle.map,
          viaSourceMap: true,
        },
      });
      continue;
    }

    for (const source of parsed.sources) {
      if (typeof source === "string" && source.trim()) {
        candidateSources.push(normalizeSourcePath(source));
      }
    }
  }

  diagnostics.sort((a, b) => {
    const byCode = a.code.localeCompare(b.code);
    if (byCode !== 0) {
      return byCode;
    }
    return (a.source?.file ?? "").localeCompare(b.source?.file ?? "");
  });

  candidateSources.sort((a, b) => a.localeCompare(b));

  return {
    sourcePath: candidateSources[0],
    diagnostics,
  };
}

function normalizeSourcePath(source: string): string {
  return source.replace(/^\.\//, "");
}
