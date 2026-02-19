import fs from "node:fs";
import path from "node:path";

import type { DiagnosticIR, ProgramIR } from "@tsgodown/ir-core";
import type { RunBuildResult } from "@tsgodown/tsdown-driver";

interface ArtifactToIrOptions {
  cwd?: string;
}

export function buildProgramIrFromArtifacts(
  buildResult: RunBuildResult,
  entry: string,
  options: ArtifactToIrOptions = {},
): ProgramIR {
  const cwd = options.cwd ?? process.cwd();
  const diagnostics: DiagnosticIR[] = [];

  const manifestEntries =
    buildResult.manifest.entries.length > 0
      ? buildResult.manifest.entries
      : [entry];

  const sourceMappedEntries = collectSourceEntriesFromSourceMap(
    buildResult,
    cwd,
    diagnostics,
  );
  const moduleEntries =
    sourceMappedEntries.length > 0 ? sourceMappedEntries : manifestEntries;

  const typedExports = collectTypedExports(buildResult, cwd, diagnostics);

  const modules = moduleEntries
    .map((manifestEntry, index) => ({
      id: `module_${index}`,
      sourcePath: manifestEntry,
      exports: typedExports,
      imports: [],
    }))
    .sort((a, b) =>
      a.sourcePath === b.sourcePath
        ? a.id.localeCompare(b.id)
        : a.sourcePath.localeCompare(b.sourcePath),
    );

  const primaryHandlerId = "health";

  diagnostics.sort((a, b) =>
    `${a.level}:${a.code}:${a.message}:${a.source?.file ?? ""}`.localeCompare(
      `${b.level}:${b.code}:${b.message}:${b.source?.file ?? ""}`,
    ),
  );

  return {
    modules,
    routes: [
      {
        method: "GET",
        path: "/health",
        handlerRef: primaryHandlerId,
      },
    ],
    handlers: [
      {
        id: primaryHandlerId,
        params: [],
        async: false,
        bodyRef: entry,
        semantics: {
          responseMode: "unknown",
          usesStatus: false,
          usesBody: false,
          usesHeaders: false,
          usesJson: false,
        },
      },
    ],
    diagnostics,
  };
}

function collectTypedExports(
  buildResult: RunBuildResult,
  cwd: string,
  diagnostics: DiagnosticIR[],
): string[] {
  const typePath = buildResult.manifest.types?.[0];
  if (!typePath?.trim()) {
    diagnostics.push({
      level: "warn",
      code: "PIPELINE_MISSING_TYPES_METADATA",
      message:
        "missing .d.ts metadata required for typed artifact-to-ir mapping",
      source: {
        file: "manifest.types",
      },
    });
    return [];
  }

  const absoluteTypePath = path.join(cwd, typePath);
  let sourceText = "";
  try {
    sourceText = fs.readFileSync(absoluteTypePath, "utf8");
  } catch {
    diagnostics.push({
      level: "warn",
      code: "PIPELINE_MISSING_TYPES_METADATA",
      message: `declared types file is unreadable: ${typePath}`,
      source: {
        file: typePath,
      },
    });
    return [];
  }

  const exportedNames = new Set<string>();
  for (const line of sourceText.split(/\r?\n/)) {
    const trimmed = line.trim();
    const match = trimmed.match(
      /^export\s+(?:declare\s+)?(?:async\s+)?(?:function|const|let|var|class|type|interface|enum)\s+([A-Za-z_$][A-Za-z0-9_$]*)/,
    );
    if (match?.[1]) {
      exportedNames.add(match[1]);
    }

    const braceExportMatch = trimmed.match(/^export\s*\{([^}]+)\}/);
    if (braceExportMatch?.[1]) {
      for (const segment of braceExportMatch[1].split(",")) {
        const symbol = segment
          .trim()
          .match(
            /^(?:type\s+)?([A-Za-z_$][A-Za-z0-9_$]*)(?:\s+as\s+([A-Za-z_$][A-Za-z0-9_$]*))?$/,
          );
        if (symbol) {
          exportedNames.add(symbol[2] ?? symbol[1]);
        }
      }
    }
  }

  if (exportedNames.size === 0) {
    diagnostics.push({
      level: "warn",
      code: "PIPELINE_INVALID_TYPES_METADATA",
      message: "no named exports could be parsed from .d.ts metadata",
      source: {
        file: typePath,
      },
    });
  }

  return [...exportedNames].sort((a, b) => a.localeCompare(b));
}

function collectSourceEntriesFromSourceMap(
  buildResult: RunBuildResult,
  cwd: string,
  diagnostics: DiagnosticIR[],
): string[] {
  const mapPath = buildResult.manifest.bundles[0]?.map;
  if (!mapPath?.trim()) {
    diagnostics.push({
      level: "warn",
      code: "PIPELINE_MISSING_SOURCEMAP_MAPPING",
      message:
        "missing sourcemap path required for deterministic source mapping",
      source: {
        file: buildResult.manifest.bundles[0]?.file ?? "manifest.bundles[0]",
        viaSourceMap: true,
      },
    });
    return [];
  }

  let parsedMap: unknown;
  try {
    parsedMap = JSON.parse(fs.readFileSync(path.join(cwd, mapPath), "utf8"));
  } catch {
    diagnostics.push({
      level: "warn",
      code: "PIPELINE_INVALID_SOURCEMAP_MAPPING",
      message: `sourcemap metadata is missing or invalid JSON: ${mapPath}`,
      source: {
        file: mapPath,
        viaSourceMap: true,
      },
    });
    return [];
  }

  const sources =
    typeof parsedMap === "object" &&
    parsedMap !== null &&
    "sources" in parsedMap
      ? (parsedMap as { sources?: unknown[] }).sources
      : undefined;

  if (!Array.isArray(sources)) {
    diagnostics.push({
      level: "warn",
      code: "PIPELINE_INVALID_SOURCEMAP_MAPPING",
      message:
        "sourcemap metadata missing sources[] for artifact-to-ir mapping",
      source: {
        file: mapPath,
        viaSourceMap: true,
      },
    });
    return [];
  }

  const normalized = new Set<string>();
  for (const sourcePath of sources) {
    if (typeof sourcePath !== "string" || !sourcePath.trim()) {
      continue;
    }
    const resolved = path
      .normalize(path.join(path.dirname(mapPath), sourcePath))
      .replaceAll("\\", "/");
    const withoutDot = resolved.startsWith("./") ? resolved.slice(2) : resolved;
    if (!withoutDot.startsWith("../")) {
      normalized.add(withoutDot);
    }
  }

  return [...normalized].sort((a, b) => a.localeCompare(b));
}
