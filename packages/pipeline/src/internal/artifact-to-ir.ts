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

  const sortedModuleEntries = [...moduleEntries].sort((a, b) =>
    a.localeCompare(b),
  );

  const modules = sortedModuleEntries.map((manifestEntry, index) => ({
    id: `module_${index}`,
    sourcePath: manifestEntry,
    exports: typedExports,
    imports: [],
  }));

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
  const typePaths = (buildResult.manifest.types ?? []).filter((typePath) =>
    typePath?.trim(),
  );
  if (typePaths.length === 0) {
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

  const exportedNames = new Set<string>();

  for (const typePath of typePaths) {
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
      continue;
    }

    for (const line of sourceText.split(/\r?\n/)) {
      const trimmed = line.trim();
      const match = trimmed.match(
        /^export\s+(?:declare\s+)?(?:async\s+)?(?:function|const|let|var|class|type|interface|enum)\s+([A-Za-z_$][A-Za-z0-9_$]*)/,
      );
      if (match?.[1]) {
        exportedNames.add(match[1]);
      }

      const braceExportMatch = trimmed.match(
        /^export\s+(?:type\s+)?\{([^}]+)\}/,
      );
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
  }

  if (exportedNames.size === 0) {
    diagnostics.push({
      level: "warn",
      code: "PIPELINE_INVALID_TYPES_METADATA",
      message: "no named exports could be parsed from .d.ts metadata",
      source: {
        file: typePaths[0] ?? "manifest.types",
      },
    });
  }

  return [...exportedNames].sort((a, b) => a.localeCompare(b));
}

function collectDeclarationLinkedSourceEntries(
  buildResult: RunBuildResult,
  cwd: string,
  diagnostics: DiagnosticIR[],
): Set<string> {
  const linkedSources = new Set<string>();

  for (const typePath of buildResult.manifest.types ?? []) {
    if (!typePath?.trim()) {
      continue;
    }

    const sourceMapResult = collectSourceMapEntriesFromArtifact({
      cwd,
      artifactPath: typePath,
      diagnostics,
    });
    for (const sourceEntry of sourceMapResult.entries) {
      linkedSources.add(sourceEntry);
    }
  }

  return linkedSources;
}

function collectSourceEntriesFromSourceMap(
  buildResult: RunBuildResult,
  cwd: string,
  diagnostics: DiagnosticIR[],
): string[] {
  const normalized = new Set<string>();

  const bundleFilePath = buildResult.manifest.bundles[0]?.file;
  const typePath = buildResult.manifest.types?.[0];

  const sourceMapTargets: Array<{
    artifactPath: string | undefined;
    mapPath?: string;
  }> = [
    ...buildResult.manifest.bundles.map((bundle) => ({
      artifactPath: bundle.file,
      mapPath: bundle.map,
    })),
    ...(buildResult.manifest.types ?? []).map((declaredTypePath) => ({
      artifactPath: declaredTypePath,
    })),
  ];

  let foundAnySourceMap = false;
  for (const target of sourceMapTargets) {
    if (!target.artifactPath?.trim()) {
      continue;
    }

    const sourceMapResult = collectSourceMapEntriesFromArtifact({
      cwd,
      artifactPath: target.artifactPath,
      declaredMapPath: target.mapPath,
      diagnostics,
    });

    if (sourceMapResult.foundSourceMap) {
      foundAnySourceMap = true;
    }

    for (const sourceEntry of sourceMapResult.entries) {
      normalized.add(sourceEntry);
    }
  }

  if (normalized.size > 0) {
    return canonicalizeDeclarationCompanionPaths([...normalized]).sort((a, b) =>
      a.localeCompare(b),
    );
  }

  if (!foundAnySourceMap) {
    diagnostics.push({
      level: "warn",
      code: "PIPELINE_MISSING_SOURCEMAP_MAPPING",
      message:
        "missing sourcemap path required for deterministic source mapping",
      source: {
        file: bundleFilePath ?? typePath ?? "manifest.bundles[0]",
        viaSourceMap: true,
        ...DETERMINISTIC_SOURCE_LOCATION,
      },
    });
  }

  return [];
}

function canonicalizeDeclarationCompanionPaths(entries: string[]): string[] {
  const entrySet = new Set(entries);

  for (const entry of entries) {
    if (entry.endsWith(".d.ts")) {
      const tsCandidate = `${entry.slice(0, -".d.ts".length)}.ts`;
      if (entrySet.has(tsCandidate)) {
        entrySet.delete(entry);
      }
    }

    if (!entry.startsWith("dist/src/")) {
      continue;
    }

    const sourceCandidate = entry.slice("dist/".length);
    if (entrySet.has(sourceCandidate)) {
      entrySet.delete(entry);
    }
  }

  return [...entrySet];
}

function collectSourceMapEntriesFromArtifact(params: {
  cwd: string;
  artifactPath: string;
  declaredMapPath?: string;
  diagnostics: DiagnosticIR[];
}): {
  entries: string[];
  foundSourceMap: boolean;
} {
  const mapPath =
    params.declaredMapPath?.trim() ||
    discoverSourceMapPathFromArtifact({
      cwd: params.cwd,
      artifactPath: params.artifactPath,
    });

  let parsedMap: unknown;
  let resolvedMapPath: string | undefined;

  if (mapPath?.trim()) {
    try {
      parsedMap = JSON.parse(
        fs.readFileSync(path.join(params.cwd, mapPath), "utf8"),
      );
      resolvedMapPath = mapPath;
    } catch {
      const discoveredMapPath = discoverSourceMapPathFromArtifact({
        cwd: params.cwd,
        artifactPath: params.artifactPath,
      });

      if (discoveredMapPath && discoveredMapPath !== mapPath) {
        try {
          parsedMap = JSON.parse(
            fs.readFileSync(path.join(params.cwd, discoveredMapPath), "utf8"),
          );
          resolvedMapPath = discoveredMapPath;
        } catch {
          // Fall through to inline sourcemap fallback below.
        }
      }

      if (parsedMap === undefined) {
        const inlineParsedMap = readInlineSourceMapFromArtifact({
          cwd: params.cwd,
          artifactPath: params.artifactPath,
        });

        if (inlineParsedMap !== undefined) {
          parsedMap = inlineParsedMap;
          resolvedMapPath = params.artifactPath;
        } else {
          if (hasInlineSourceMapDataUrl(params)) {
            params.diagnostics.push({
              level: "warn",
              code: "PIPELINE_INVALID_SOURCEMAP_MAPPING",
              message: `inline sourcemap data URL is malformed or invalid JSON: ${params.artifactPath}`,
              source: {
                file: params.artifactPath,
                viaSourceMap: true,
                ...DETERMINISTIC_SOURCE_LOCATION,
              },
            });
          }
          params.diagnostics.push({
            level: "warn",
            code: "PIPELINE_INVALID_SOURCEMAP_MAPPING",
            message: `sourcemap metadata is missing or invalid JSON: ${mapPath}`,
            source: {
              file: mapPath,
              viaSourceMap: true,
              ...DETERMINISTIC_SOURCE_LOCATION,
            },
          });
          return { entries: [], foundSourceMap: true };
        }
      }
    }
  } else {
    parsedMap = readInlineSourceMapFromArtifact({
      cwd: params.cwd,
      artifactPath: params.artifactPath,
    });
    if (parsedMap !== undefined) {
      resolvedMapPath = params.artifactPath;
    }
  }

  if (parsedMap === undefined || !resolvedMapPath?.trim()) {
    return { entries: [], foundSourceMap: false };
  }

  const sourceEntries = collectSourceMapEntries(parsedMap, {
    mapPath: resolvedMapPath,
    diagnostics: params.diagnostics,
  });
  if (sourceEntries.length === 0) {
    params.diagnostics.push({
      level: "warn",
      code: "PIPELINE_INVALID_SOURCEMAP_MAPPING",
      message:
        "sourcemap metadata missing sources[] for artifact-to-ir mapping",
      source: {
        file: resolvedMapPath,
        viaSourceMap: true,
        ...DETERMINISTIC_SOURCE_LOCATION,
      },
    });
    return { entries: [], foundSourceMap: true };
  }

  let hasSparseEntries = false;
  const normalized = new Set<string>();
  for (const entry of sourceEntries) {
    if (typeof entry.sourcePath !== "string" || !entry.sourcePath.trim()) {
      hasSparseEntries = true;
      continue;
    }

    const normalizedSource = normalizeSourceMapSourcePath({
      cwd: params.cwd,
      mapPath: resolvedMapPath,
      sourceRoot: entry.sourceRoot,
      sourcePath: entry.sourcePath,
    });
    if (normalizedSource) {
      normalized.add(normalizedSource);
    }
  }

  if (hasSparseEntries) {
    params.diagnostics.push({
      level: "warn",
      code: "PIPELINE_SOURCEMAP_SPARSE_MAPPING",
      message:
        "sourcemap sections include sparse source entries; positional metadata omitted deterministically",
      source: {
        file: resolvedMapPath,
        viaSourceMap: true,
      },
    });
  }

  return {
    entries: [...normalized].sort((a, b) => a.localeCompare(b)),
    foundSourceMap: true,
  };
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
          ...(deriveDeterministicLocation(sectionOffset) ?? {}),
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
          ...(deriveDeterministicLocation(sectionOffset) ?? {}),
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

function deriveDeterministicLocation(
  offset: unknown,
):
  | Pick<NonNullable<DiagnosticIR["source"]>, "line">
  | Pick<NonNullable<DiagnosticIR["source"]>, "line" | "column">
  | undefined {
  if (typeof offset !== "object" || offset === null) {
    return undefined;
  }

  const line = (offset as { line?: unknown }).line;
  if (!Number.isInteger(line) || (line as number) < 0) {
    return undefined;
  }

  const column = (offset as { column?: unknown }).column;
  if (Number.isInteger(column) && (column as number) >= 0) {
    return {
      line: (line as number) + 1,
      column: (column as number) + 1,
    };
  }

  return {
    line: (line as number) + 1,
  };
}

function discoverSourceMapPathFromArtifact(params: {
  cwd: string;
  artifactPath: string | undefined;
}): string | undefined {
  const artifactPath = params.artifactPath;
  if (!artifactPath?.trim()) {
    return undefined;
  }

  const sourceMapUrl = readSourceMapUrlFromArtifact(params);
  if (!sourceMapUrl || sourceMapUrl.startsWith("data:")) {
    return undefined;
  }

  const decodedSourceMapUrl = decodeSourceMapUrlPath(sourceMapUrl);
  const sanitizedSourceMapPath = decodedSourceMapUrl
    .replace(/[?#].*$/, "")
    .trim();
  if (!sanitizedSourceMapPath) {
    return undefined;
  }

  const normalizedSourceMapPath = toFileSystemPath(sanitizedSourceMapPath);

  const artifactDir = path.dirname(artifactPath);
  const discoveredPath = path.isAbsolute(normalizedSourceMapPath)
    ? path.normalize(normalizedSourceMapPath)
    : path.normalize(path.join(artifactDir, normalizedSourceMapPath));

  const relativeToCwd = path.isAbsolute(discoveredPath)
    ? path.relative(params.cwd, discoveredPath)
    : discoveredPath;
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

function readInlineSourceMapFromArtifact(params: {
  cwd: string;
  artifactPath: string | undefined;
}): unknown | undefined {
  const sourceMapUrl = readSourceMapUrlFromArtifact(params);
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

function hasInlineSourceMapDataUrl(params: {
  cwd: string;
  artifactPath: string | undefined;
}): boolean {
  const sourceMapUrl = readSourceMapUrlFromArtifact(params);
  return sourceMapUrl?.startsWith("data:") ?? false;
}

function readSourceMapUrlFromArtifact(params: {
  cwd: string;
  artifactPath: string | undefined;
}): string | undefined {
  if (!params.artifactPath?.trim()) {
    return undefined;
  }

  const absoluteArtifactPath = path.join(params.cwd, params.artifactPath);
  let artifactText = "";
  try {
    artifactText = fs.readFileSync(absoluteArtifactPath, "utf8");
  } catch {
    return undefined;
  }

  const sourceMapUrlMatches = [
    ...artifactText.matchAll(/\/\/[#@]\s*sourceMappingURL\s*=\s*(\S+)\s*$/gm),
  ];
  const sourceMapUrlMatch = sourceMapUrlMatches.at(-1);
  return unwrapQuotedValue(sourceMapUrlMatch?.[1]?.trim());
}

function unwrapQuotedValue(value: string | undefined): string | undefined {
  if (!value) {
    return value;
  }

  const singleQuoted = value.match(/^'([^']+)'$/);
  if (singleQuoted?.[1]) {
    return singleQuoted[1];
  }

  const doubleQuoted = value.match(/^"([^"]+)"$/);
  if (doubleQuoted?.[1]) {
    return doubleQuoted[1];
  }

  return value;
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

  const decodedSourceRootPath = normalizeDecodedSourceMapPathSegment(
    params.sourceRoot,
  );
  const sourceRootQueryIndex = decodedSourceRootPath.indexOf("?");
  const sourceRootPath =
    sourceRootQueryIndex >= 0
      ? decodedSourceRootPath.slice(0, sourceRootQueryIndex)
      : decodedSourceRootPath;
  const decodedSourcePathSegment = decodePathSegmentIfPossible(
    normalizeDecodedSourceMapPathSegment(params.sourcePath),
  );
  const sourcePath = String(stripQueryAndHash(decodedSourcePathSegment))
    .replace(/%2f/gi, "/")
    .replace(/%5c/gi, "/");
  if (!sourcePath.trim()) {
    return undefined;
  }
  const rootedPath = isAbsoluteFileSystemPathLike(sourcePath)
    ? sourcePath
    : sourceRootPath.trim()
      ? path.join(sourceRootPath, sourcePath)
      : sourcePath;

  const resolved = isAbsoluteFileSystemPathLike(rootedPath)
    ? path.normalize(rootedPath)
    : path.normalize(path.join(path.dirname(params.mapPath), rootedPath));

  const relativeToCwd = isAbsoluteFileSystemPathLike(resolved)
    ? path.relative(params.cwd, resolved)
    : resolved;
  const normalized = relativeToCwd.replaceAll("\\", "/");
  const withoutDot = normalized.startsWith("./")
    ? normalized.slice(2)
    : normalized;
  if (
    withoutDot &&
    !withoutDot.startsWith("../") &&
    !path.isAbsolute(withoutDot)
  ) {
    return withoutDot;
  }

  if (
    isUncFileUrlLike(params.sourcePath) ||
    isUncFileUrlLike(params.sourceRoot)
  ) {
    const uncPortable = normalizePathSeparators(resolved).replace(/^\/+/, "");
    if (
      uncPortable &&
      !uncPortable.startsWith("../") &&
      !path.isAbsolute(uncPortable)
    ) {
      return uncPortable;
    }
  }

  return undefined;
}

function normalizeDecodedSourceMapPathSegment(value: unknown): string {
  const strippedValue = stripQueryAndHash(value);
  const normalizedPath = toFileSystemPath(strippedValue);

  const queryIndex = normalizedPath.indexOf("?");
  if (queryIndex >= 0) {
    return normalizedPath.slice(0, queryIndex);
  }

  return normalizedPath;
}

function stripQueryAndHash(value: unknown): unknown {
  if (typeof value !== "string") {
    return value;
  }

  const queryIndex = value.indexOf("?");
  const withoutQuery = queryIndex >= 0 ? value.slice(0, queryIndex) : value;

  const hashIndex = withoutQuery.indexOf("#");
  if (hashIndex < 0) {
    return withoutQuery;
  }

  // Preserve literal `#` path segments for filesystem-style paths where `#` is
  // part of a real directory/file name, not a URL fragment marker.
  if (/^[A-Za-z]:[\\/]/.test(withoutQuery)) {
    return withoutQuery;
  }

  const afterHash = withoutQuery.slice(hashIndex + 1);
  if (
    afterHash.includes("/") ||
    afterHash.includes("\\") ||
    /^[^/\\]+\.[A-Za-z0-9]+$/.test(afterHash)
  ) {
    return withoutQuery;
  }

  return withoutQuery.slice(0, hashIndex);
}

function decodePathSegmentIfPossible(value: string): string {
  try {
    return decodeURIComponent(value);
  } catch {
    return value;
  }
}

function decodeSourceMapUrlPath(sourceMapUrl: string): string {
  try {
    return decodeURIComponent(sourceMapUrl);
  } catch {
    return sourceMapUrl;
  }
}

function isUncFileUrlLike(value: unknown): boolean {
  return typeof value === "string" && /^file:\/\/[^/]/i.test(value.trim());
}

function toFileSystemPath(value: unknown): string {
  if (typeof value !== "string" || !value.trim()) {
    return "";
  }

  const trimmed = value.trim();

  if (/^file:/i.test(trimmed)) {
    const normalizedFileUrl = normalizeDoubleEncodedFileUrlPathSeparators(
      /^file:\/\//i.test(trimmed)
        ? trimmed.replace(/^file:\/\//i, "file://")
        : trimmed.replace(/^file:/i, "file://"),
    );
    try {
      return normalizePathSeparators(fileURLToPath(normalizedFileUrl));
    } catch {
      try {
        const parsedUrl = new URL(normalizedFileUrl);
        let decodedPathname = decodeURIComponent(parsedUrl.pathname);
        const hashPayload = parsedUrl.hash.slice(1);
        if (hashPayload && /[./\\]/.test(hashPayload)) {
          decodedPathname = `${decodedPathname.replace(/\/?$/, "/")}${decodeURIComponent(hashPayload)}`;
        }
        const hostPrefix =
          parsedUrl.host && parsedUrl.hostname.toLowerCase() !== "localhost"
            ? `//${parsedUrl.host}`
            : "";
        return normalizePathSeparators(`${hostPrefix}${decodedPathname}`);
      } catch {
        return normalizePathSeparators(trimmed);
      }
    }
  }

  try {
    return normalizePathSeparators(decodeURIComponent(trimmed));
  } catch {
    return normalizePathSeparators(trimmed);
  }
}

function normalizeDoubleEncodedFileUrlPathSeparators(value: string): string {
  let normalized = value;
  for (let i = 0; i < 2; i += 1) {
    const next = normalized.replace(/%252f/gi, "%2F").replace(/%255c/gi, "%5C");
    if (next === normalized) {
      break;
    }
    normalized = next;
  }
  if (/^file:\/\/%2f/i.test(normalized)) {
    normalized = normalized.replace(/^file:\/\/%2f/i, "file:///");
  }

  return normalized;
}

function normalizePathSeparators(value: string): string {
  const normalized = value.replaceAll("\\", "/");
  const uncNormalized = normalizeWindowsUncAuthorityAndShareCase(normalized);
  return normalizeWindowsDrivePathToPortableAbsolute(uncNormalized);
}

function normalizeWindowsUncAuthorityAndShareCase(value: string): string {
  const uncMatch = value.match(/^\/\/([^/]+)\/([^/]+)(\/.*)?$/);
  if (!uncMatch) {
    return value;
  }

  const host = uncMatch[1]?.toLowerCase();
  const share = uncMatch[2]?.toLowerCase();
  const rest = uncMatch[3] ?? "";
  if (!host || !share) {
    return value;
  }

  return `//${host}/${share}${rest}`;
}

function normalizeWindowsDrivePathToPortableAbsolute(value: string): string {
  const withDrive = value.match(/^\/?[A-Za-z]:(\/.*)$/);
  if (!withDrive) {
    return value;
  }
  return withDrive[1] ?? value;
}

function isAbsoluteFileSystemPathLike(value: string): boolean {
  return (
    path.isAbsolute(value) ||
    /^[A-Za-z]:\//.test(value) ||
    /^\/\/[A-Za-z0-9._-]+\//.test(value)
  );
}
