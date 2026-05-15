export {
  CAPABILITY_BACKENDS,
  CAPABILITY_KEYS,
  CAPABILITY_MATRIX,
  CapabilityStatus,
  collectRequiredCapabilities,
  checkCapabilities,
} from "./internal/index.js";

export type {
  CapabilityCheckOptions,
  CapabilityCheckResult,
  CapabilityBackend,
  CapabilityDiagnostic,
  CapabilityKey,
  CapabilityRequirement,
  CapabilityRule,
  CapabilitySource,
  ProgramIRLike,
} from "./types.js";
