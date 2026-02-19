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

function compareDiagnostics(a: DiagnosticIR, b: DiagnosticIR): number {
  const fileA = normalizeDiagnosticPath(a.source?.file ?? "");
  const fileB = normalizeDiagnosticPath(b.source?.file ?? "");
  const lineA = a.source?.line ?? Number.MAX_SAFE_INTEGER;
  const lineB = b.source?.line ?? Number.MAX_SAFE_INTEGER;
  const columnA = a.source?.column ?? Number.MAX_SAFE_INTEGER;
  const columnB = b.source?.column ?? Number.MAX_SAFE_INTEGER;

  return (
    fileA.localeCompare(fileB) ||
    lineA - lineB ||
    columnA - columnB ||
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
