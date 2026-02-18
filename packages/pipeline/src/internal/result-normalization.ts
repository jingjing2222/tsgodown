import type { UserConfig } from "@tsgodown/config";

const DEFAULT_ENTRY = "src/index.ts";

export interface PipelineFailureDetails {
  source: string;
  stage: string;
  cause: string;
  guidance: string;
}

export function resolveEntry(config: UserConfig): string {
  return typeof config.entry === "string" ? config.entry : DEFAULT_ENTRY;
}

export function normalizePipelineCause(cause: unknown): string {
  if (cause === null || cause === undefined) {
    return "";
  }

  return cause instanceof Error ? cause.message : String(cause);
}

export function formatPipelineFailure(
  entry: string,
  detailsOrCause: PipelineFailureDetails | unknown,
): Error {
  const details = normalizePipelineFailureDetails(entry, detailsOrCause);
  return new Error(
    [
      `[pipeline] failed for entry \"${entry}\"`,
      `source: ${details.source}`,
      `stage: ${details.stage}`,
      `cause: ${details.cause}`,
      `guidance: ${details.guidance}`,
    ].join("; "),
  );
}

function normalizePipelineFailureDetails(
  entry: string,
  detailsOrCause: PipelineFailureDetails | unknown,
): PipelineFailureDetails {
  if (isPipelineFailureDetails(detailsOrCause)) {
    return {
      source: detailsOrCause.source.trim() || `pipeline-entry(${entry})`,
      stage: detailsOrCause.stage.trim() || "UNKNOWN",
      cause: detailsOrCause.cause.trim() || "unknown pipeline failure",
      guidance:
        detailsOrCause.guidance.trim() ||
        "Verify rust engine build/analyze contract and tsgodown.config.ts settings.",
    };
  }

  const message =
    normalizePipelineCause(detailsOrCause).trim() || "unknown pipeline failure";
  return {
    source: `pipeline-entry(${entry})`,
    stage: "UNKNOWN",
    cause: message,
    guidance:
      "Verify rust engine build/analyze contract and tsgodown.config.ts settings.",
  };
}

function isPipelineFailureDetails(
  value: unknown,
): value is PipelineFailureDetails {
  if (!value || typeof value !== "object") {
    return false;
  }

  const candidate = value as Partial<PipelineFailureDetails>;
  return (
    typeof candidate.source === "string" &&
    typeof candidate.stage === "string" &&
    typeof candidate.cause === "string" &&
    typeof candidate.guidance === "string"
  );
}
