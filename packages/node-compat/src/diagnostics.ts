import {
  type CapabilityRule,
  type CapabilitySource,
  CapabilityStatus,
} from "./types.js";

export function isSupportedStatus(
  status: CapabilityStatus,
  allowWip: boolean,
): boolean {
  if (status === CapabilityStatus.DONE) return true;
  if (status === CapabilityStatus.WIP && allowWip) return true;
  return false;
}

export function formatSource(source?: CapabilitySource): string {
  if (!source) return "unknown";
  const line = source.line ?? "?";
  const column = source.column ?? "?";
  const suffix = source.viaSourceMap ? " (via source map)" : "";
  return `${source.file}:${line}:${column}${suffix}`;
}

export function buildGuidance(rule: CapabilityRule): string {
  return `Update source to avoid '${rule.key}' usage, or implement ${rule.scope} strategy '${rule.strategy}'. See docs/specs/CAPABILITY_MATRIX.md.`;
}
