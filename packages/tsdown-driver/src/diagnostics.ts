export function normalizeDiagnostics(value: unknown): string[] {
  if (!Array.isArray(value)) return [];

  return value
    .filter((diag): diag is string => typeof diag === "string")
    .map((diag) => diag.trim())
    .filter((diag) => diag.length > 0);
}
