import type { DiagnosticIR } from "@tsgodown/ir-core";

function normalizeDiagnosticPath(pathValue: string): string {
  return pathValue.replaceAll("\\", "/");
}

function formatDiagnosticSource(diagnostic: DiagnosticIR): string | undefined {
  const source = diagnostic.source;
  if (!source) return undefined;

  const normalizedFile = normalizeDiagnosticPath(source.file);

  if (
    diagnostic.code === "PIPELINE_SOURCEMAP_SPARSE_MAPPING" ||
    (source.line == null && source.column == null)
  ) {
    return normalizedFile;
  }

  if (source.line != null && source.column != null) {
    return `${normalizedFile}:${source.line}:${source.column}`;
  }

  if (source.line != null) {
    return `${normalizedFile}:${source.line}`;
  }

  return `${normalizedFile}:${source.column}`;
}

function renderedSourceScopeRank(diagnostic: DiagnosticIR): number {
  const source = diagnostic.source;
  if (!source) return 3;

  const normalizedFile = normalizeDiagnosticPath(source.file);
  const renderedSource = formatDiagnosticSource(diagnostic);

  if (!renderedSource || renderedSource === normalizedFile) {
    return 2;
  }

  if (source.line != null) {
    return 0;
  }

  return 1;
}

function compareDiagnostics(a: DiagnosticIR, b: DiagnosticIR): number {
  const fileA = normalizeDiagnosticPath(a.source?.file ?? "");
  const fileB = normalizeDiagnosticPath(b.source?.file ?? "");
  const scopeRankA = renderedSourceScopeRank(a);
  const scopeRankB = renderedSourceScopeRank(b);
  const lineA = a.source?.line ?? Number.MAX_SAFE_INTEGER;
  const lineB = b.source?.line ?? Number.MAX_SAFE_INTEGER;
  const columnA = a.source?.column ?? Number.MAX_SAFE_INTEGER;
  const columnB = b.source?.column ?? Number.MAX_SAFE_INTEGER;
  const sourceA = formatDiagnosticSource(a) ?? "";
  const sourceB = formatDiagnosticSource(b) ?? "";

  return (
    fileA.localeCompare(fileB) ||
    scopeRankA - scopeRankB ||
    lineA - lineB ||
    columnA - columnB ||
    sourceA.localeCompare(sourceB) ||
    a.level.localeCompare(b.level) ||
    a.code.localeCompare(b.code) ||
    a.message.localeCompare(b.message)
  );
}

export function renderDiagnosticsComments(
  diagnostics: DiagnosticIR[],
): string[] {
  if (diagnostics.length === 0) {
    return [];
  }

  const lines = [
    "// IR diagnostics carried from rust analyzer (SSoT):",
    "// Generated Go may be scaffold-only until these diagnostics are resolved.",
  ];

  const sortedDiagnostics = [...diagnostics].sort(compareDiagnostics);

  for (const diagnostic of sortedDiagnostics) {
    lines.push(
      `// [${diagnostic.level}] ${diagnostic.code}: ${diagnostic.message}`,
    );

    const source = formatDiagnosticSource(diagnostic);
    if (source) {
      lines.push(`//   at ${source}`);
    }
  }

  lines.push(
    "// Action: fix diagnostics in source and regenerate. Emitter does not own policy decisions.",
    "",
  );

  return lines;
}
