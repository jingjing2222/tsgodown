import type { UserConfig } from "@tsgodown/config";

const DEFAULT_ENTRY = "src/index.ts";

export function resolveEntry(config: UserConfig): string {
  return typeof config.entry === "string" ? config.entry : DEFAULT_ENTRY;
}

export function normalizePipelineCause(cause: unknown): string {
  return cause instanceof Error ? cause.message : String(cause);
}

export function formatPipelineFailure(entry: string, cause: unknown): Error {
  const message =
    normalizePipelineCause(cause).trim() || "unknown pipeline failure";
  return new Error(
    [
      `[pipeline] failed for entry \"${entry}\"`,
      `source: pipeline-entry(${entry})`,
      `cause: ${message}`,
      "guidance: Verify rust engine build/analyze contract and tsgodown.config.ts settings.",
    ].join("; "),
  );
}
