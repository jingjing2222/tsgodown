import { collectRequiredCapabilities } from "./collect.js";
import {
  buildGuidance,
  formatSource,
  isSupportedStatus,
} from "./diagnostics.js";
import { CAPABILITY_MATRIX } from "./matrix.js";
import type {
  CapabilityCheckOptions,
  CapabilityCheckResult,
  CapabilityDiagnostic,
  ProgramIRLike,
} from "./types.js";

export function checkCapabilities(
  ir: ProgramIRLike,
  options: CapabilityCheckOptions = {},
): CapabilityCheckResult {
  const allowWip = options.allowWip ?? true;
  const failFast = options.failFast ?? true;

  const required = collectRequiredCapabilities(ir);
  const diagnostics: CapabilityDiagnostic[] = [];

  for (const req of required) {
    const rule = CAPABILITY_MATRIX[req.capability];
    if (isSupportedStatus(rule.status, allowWip)) continue;

    const cause = `Capability status is ${rule.status} for required '${req.capability}' (${req.reason}).`;
    const guidance = buildGuidance(rule);
    const sourceLocation = formatSource(req.source);

    diagnostics.push({
      level: "error",
      code: "CAPABILITY_UNMET",
      message:
        `Capability '${req.capability}' is not supported (status=${rule.status}) at ${sourceLocation}. ` +
        `Cause: ${cause} Guidance: ${guidance}`,
      capability: req.capability,
      status: rule.status,
      source: req.source,
      cause,
      guidance,
    });

    if (failFast) break;
  }

  return {
    ok: diagnostics.length === 0,
    required,
    diagnostics,
  };
}
