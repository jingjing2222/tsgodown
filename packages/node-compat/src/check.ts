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
import { CapabilityStatus } from "./types.js";

export function checkCapabilities(
  ir: ProgramIRLike,
  options: CapabilityCheckOptions = {},
): CapabilityCheckResult {
  const allowWip = options.allowWip ?? true;
  const failFast = options.failFast ?? true;
  const targetBackend = options.targetBackend ?? "go";

  const required = collectRequiredCapabilities(ir);
  const diagnostics: CapabilityDiagnostic[] = [];

  for (const req of required) {
    const rule = CAPABILITY_MATRIX[req.capability];
    const backendRule = rule.backends[targetBackend];
    const status = backendRule?.status ?? CapabilityStatus.TODO;
    const strategy = backendRule?.strategy ?? rule.strategy;
    if (isSupportedStatus(status, allowWip)) continue;

    const cause = `Capability status is ${status} for backend '${targetBackend}' required '${req.capability}' (${req.reason}).`;
    const guidance = buildGuidance({ ...rule, status, strategy });
    const sourceLocation = formatSource(req.source);

    diagnostics.push({
      level: "error",
      code: "CAPABILITY_UNMET",
      message:
        `Capability '${req.capability}' is not supported for backend '${targetBackend}' (status=${status}) at ${sourceLocation}. ` +
        `Cause: ${cause} Guidance: ${guidance}`,
      capability: req.capability,
      status,
      backend: targetBackend,
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
