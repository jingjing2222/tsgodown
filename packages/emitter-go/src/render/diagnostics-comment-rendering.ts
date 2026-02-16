import type { DiagnosticIR } from "@tsgodown/ir-core";

function formatDiagnosticSource(diagnostic: DiagnosticIR): string | undefined {
  const source = diagnostic.source;
  if (!source) return undefined;

  const line = source.line ?? "?";
  const column = source.column ?? "?";
  return `${source.file}:${line}:${column}`;
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

  for (const diagnostic of diagnostics) {
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
