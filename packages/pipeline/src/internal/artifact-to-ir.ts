import fs from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";

import type { DiagnosticIR, ProgramIR } from "@tsgodown/ir-core";
import type { RunBuildResult } from "@tsgodown/tsdown-driver";

const DETERMINISTIC_SOURCE_LOCATION = {
  line: 1,
  column: 1,
} as const;

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
  const bundleFilePath = buildResult.manifest.bundles[0]?.file;
  let mapPath =
    buildResult.manifest.bundles[0]?.map ??
    discoverSourceMapPathFromBundle({
      cwd,
      bundlePath: bundleFilePath,
    });

  let parsedMap: unknown;
  if (mapPath?.trim()) {
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
          ...DETERMINISTIC_SOURCE_LOCATION,
        },
      });
      return [];
    }
  } else {
    parsedMap = readInlineSourceMapFromBundle({
      cwd,
      bundlePath: bundleFilePath,
    });
    if (parsedMap) {
      mapPath = bundleFilePath ?? "manifest.bundles[0]";
    }
  }

  if (!mapPath?.trim() || parsedMap === undefined) {
    diagnostics.push({
      level: "warn",
      code: "PIPELINE_MISSING_SOURCEMAP_MAPPING",
      message:
        "missing sourcemap path required for deterministic source mapping",
      source: {
        file: bundleFilePath ?? "manifest.bundles[0]",
        viaSourceMap: true,
        ...DETERMINISTIC_SOURCE_LOCATION,
      },
    });
    return [];
  }

  const sourceEntries = collectSourceMapEntries(parsedMap, {
    mapPath,
    diagnostics,
  });
  if (sourceEntries.length === 0) {
    diagnostics.push({
      level: "warn",
      code: "PIPELINE_INVALID_SOURCEMAP_MAPPING",
      message:
        "sourcemap metadata missing sources[] for artifact-to-ir mapping",
      source: {
        file: mapPath,
        viaSourceMap: true,
        ...DETERMINISTIC_SOURCE_LOCATION,
      },
    });
    return [];
  }

  const normalized = new Set<string>();
  let hasSparseEntries = false;
  for (const entry of sourceEntries) {
    if (typeof entry.sourcePath !== "string" || !entry.sourcePath.trim()) {
      hasSparseEntries = true;
      continue;
    }

    const normalizedSource = normalizeSourceMapSourcePath({
      cwd,
      mapPath,
      sourceRoot: entry.sourceRoot,
      sourcePath: entry.sourcePath,
    });
    if (normalizedSource) {
      normalized.add(normalizedSource);
    }
  }

  if (hasSparseEntries) {
    diagnostics.push({
      level: "warn",
      code: "PIPELINE_SOURCEMAP_SPARSE_MAPPING",
      message:
        "sourcemap sections include sparse source entries; positional metadata omitted deterministically",
      source: {
        file: mapPath,
        viaSourceMap: true,
      },
    });
  }

  return [...normalized].sort((a, b) => a.localeCompare(b));
}

function collectSourceMapEntries(
  parsedMap: unknown,
  context?: {
    mapPath: string;
    diagnostics: DiagnosticIR[];
  },
  inheritedSourceRoot = "",
): Array<{
  sourcePath: unknown;
  sourceRoot: string;
}> {
  if (typeof parsedMap !== "object" || parsedMap === null) {
    return [];
  }

  const directSources = (parsedMap as { sources?: unknown }).sources;
  const directSourceRoot =
    typeof (parsedMap as { sourceRoot?: unknown }).sourceRoot === "string"
      ? ((parsedMap as { sourceRoot: string }).sourceRoot ?? "")
      : inheritedSourceRoot;
  if (Array.isArray(directSources)) {
    return directSources.map((sourcePath) => ({
      sourcePath,
      sourceRoot: directSourceRoot,
    }));
  }

  const sections = (parsedMap as { sections?: unknown }).sections;
  if (!Array.isArray(sections)) {
    return [];
  }

  const entries: Array<{ sourcePath: unknown; sourceRoot: string }> = [];
  for (const section of sections) {
    if (typeof section !== "object" || section === null) {
      continue;
    }

    const sectionOffset = (section as { offset?: unknown }).offset;
    if (context && isSparseSourceMapOffset(sectionOffset)) {
      context.diagnostics.push({
        level: "warn",
        code: "PIPELINE_SOURCEMAP_POSITION_PARTIAL",
        message:
          "indexed sourcemap section offset is partial; diagnostics remain file-scoped for deterministic mapping",
        source: {
          file: context.mapPath,
          viaSourceMap: true,
          ...(deriveDeterministicLine(sectionOffset) ?? {}),
        },
      });
    }

    const sectionMap = (section as { map?: unknown }).map;
    const nestedEntries = collectSourceMapEntries(
      sectionMap,
      context,
      directSourceRoot,
    );
    if (
      context &&
      nestedEntries.length === 0 &&
      isMissingSectionSources(sectionMap)
    ) {
      context.diagnostics.push({
        level: "warn",
        code: "PIPELINE_INVALID_SOURCEMAP_MAPPING",
        message:
          "indexed sourcemap section map is missing sources[]; section ignored for deterministic mapping",
        source: {
          file: context.mapPath,
          viaSourceMap: true,
          ...(deriveDeterministicLine(sectionOffset) ?? {}),
        },
      });
    }

    for (const nestedEntry of nestedEntries) {
      entries.push(nestedEntry);
    }
  }

  return entries;
}

function isMissingSectionSources(sectionMap: unknown): boolean {
  if (typeof sectionMap !== "object" || sectionMap === null) {
    return false;
  }

  const hasSourcesArray = Array.isArray(
    (sectionMap as { sources?: unknown }).sources,
  );
  if (hasSourcesArray) {
    return false;
  }

  const hasSectionsArray = Array.isArray(
    (sectionMap as { sections?: unknown }).sections,
  );
  return !hasSectionsArray;
}

function isSparseSourceMapOffset(offset: unknown): boolean {
  if (typeof offset !== "object" || offset === null) {
    return true;
  }

  const line = (offset as { line?: unknown }).line;
  const column = (offset as { column?: unknown }).column;
  const hasLine = Number.isInteger(line) && (line as number) >= 0;
  const hasColumn = Number.isInteger(column) && (column as number) >= 0;

  return !hasLine || !hasColumn;
}

function deriveDeterministicLine(
  offset: unknown,
): Pick<NonNullable<DiagnosticIR["source"]>, "line"> | undefined {
  if (typeof offset !== "object" || offset === null) {
    return undefined;
  }

  const line = (offset as { line?: unknown }).line;
  if (!Number.isInteger(line) || (line as number) < 0) {
    return undefined;
  }

  return {
    line: (line as number) + 1,
  };
}

function discoverSourceMapPathFromBundle(params: {
  cwd: string;
  bundlePath: string | undefined;
}): string | undefined {
  const bundlePath = params.bundlePath;
  if (!bundlePath?.trim()) {
    return undefined;
  }

  const sourceMapUrl = readSourceMapUrlFromBundle(params);
  if (!sourceMapUrl || sourceMapUrl.startsWith("data:")) {
    return undefined;
  }

  const bundleDir = path.dirname(bundlePath);
  const discoveredPath = path.isAbsolute(sourceMapUrl)
    ? path.normalize(sourceMapUrl)
    : path.normalize(path.join(bundleDir, sourceMapUrl));

  const normalized = discoveredPath.replaceAll("\\", "/");
  return normalized.startsWith("./") ? normalized.slice(2) : normalized;
}

function readInlineSourceMapFromBundle(params: {
  cwd: string;
  bundlePath: string | undefined;
}): unknown | undefined {
  const sourceMapUrl = readSourceMapUrlFromBundle(params);
  if (!sourceMapUrl?.startsWith("data:")) {
    return undefined;
  }

  const dataUrlMatch = sourceMapUrl.match(/^data:([^,]*),(.*)$/);
  if (!dataUrlMatch) {
    return undefined;
  }

  const mediaType = dataUrlMatch[1] ?? "";
  const payload = dataUrlMatch[2] ?? "";
  const mediaTypeParts = mediaType
    .split(";")
    .map((segment) => segment.trim().toLowerCase())
    .filter(Boolean);

  const mimeType = mediaTypeParts[0] ?? "";
  if (mimeType && mimeType !== "application/json") {
    return undefined;
  }

  const isBase64 = mediaTypeParts.includes("base64");

  try {
    const decoded = isBase64
      ? Buffer.from(payload, "base64").toString("utf8")
      : decodeURIComponent(payload);
    return JSON.parse(decoded);
  } catch {
    return undefined;
  }
}

function readSourceMapUrlFromBundle(params: {
  cwd: string;
  bundlePath: string | undefined;
}): string | undefined {
  if (!params.bundlePath?.trim()) {
    return undefined;
  }

  const absoluteBundlePath = path.join(params.cwd, params.bundlePath);
  let bundleText = "";
  try {
    bundleText = fs.readFileSync(absoluteBundlePath, "utf8");
  } catch {
    return undefined;
  }

  const sourceMapUrlMatch = bundleText.match(
    /\/\/\#\s*sourceMappingURL\s*=\s*(\S+)\s*$/m,
  );
  return sourceMapUrlMatch?.[1]?.trim();
}

function normalizeSourceMapSourcePath(params: {
  cwd: string;
  mapPath: string;
  sourceRoot: string;
  sourcePath: unknown;
}): string | undefined {
  if (typeof params.sourcePath !== "string" || !params.sourcePath.trim()) {
    return undefined;
  }

  const sourceRootPath = toFileSystemPath(params.sourceRoot);
  const sourcePath = toFileSystemPath(params.sourcePath);
  const rootedPath = sourceRootPath.trim()
    ? path.join(sourceRootPath, sourcePath)
    : sourcePath;

  const resolved = path.isAbsolute(rootedPath)
    ? path.normalize(rootedPath)
    : path.normalize(path.join(path.dirname(params.mapPath), rootedPath));

  const relativeToCwd = path.isAbsolute(resolved)
    ? path.relative(params.cwd, resolved)
    : resolved;
  const normalized = relativeToCwd.replaceAll("\\", "/");
  const withoutDot = normalized.startsWith("./")
    ? normalized.slice(2)
    : normalized;
  if (
    !withoutDot ||
    withoutDot.startsWith("../") ||
    path.isAbsolute(withoutDot)
  ) {
    return undefined;
  }
  return withoutDot;
}

function toFileSystemPath(value: unknown): string {
  if (typeof value !== "string" || !value.trim()) {
    return "";
  }

  if (value.startsWith("file://")) {
    try {
      return fileURLToPath(value);
    } catch {
      return value;
    }
  }

  return value;
}
