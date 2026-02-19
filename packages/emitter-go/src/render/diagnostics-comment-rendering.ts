import type { DiagnosticIR } from "@tsgodown/ir-core";

function formatDiagnosticSource(diagnostic: DiagnosticIR): string | undefined {
  const source = diagnostic.source;
  if (!source) return undefined;

  if (
    diagnostic.code === "PIPELINE_SOURCEMAP_SPARSE_MAPPING" ||
    (source.line == null && source.column == null)
  ) {
    return source.file;
  }

  if (source.line != null && source.column != null) {
    return `${source.file}:${source.line}:${source.column}`;
  }

  if (source.line != null) {
    return `${source.file}:${source.line}`;
  }

  return source.file;
}

function compareDiagnostics(a: DiagnosticIR, b: DiagnosticIR): number {
  const sourceA = a.source;
  const sourceB = b.source;

  const fileOrder = (sourceA?.file ?? "").localeCompare(sourceB?.file ?? "");
  if (fileOrder !== 0) {
    return fileOrder;
  }

  const lineA = sourceA?.line ?? -1;
  const lineB = sourceB?.line ?? -1;
  if (lineA !== lineB) {
    return lineA - lineB;
  }

  const columnA = sourceA?.column ?? -1;
  const columnB = sourceB?.column ?? -1;
  if (columnA !== columnB) {
    return columnA - columnB;
  }

  return (
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
