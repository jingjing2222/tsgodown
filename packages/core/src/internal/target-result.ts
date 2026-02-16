import type {
  BuildTargetDiagnostics,
  BuildTargetPlan,
  BuildTargetResult,
} from "../index.js";

const TS_CORE_DEPRECATION_WARNING =
  "DEPRECATED: TS core analyzer diagnostics are disabled after Rust cutover; use IR diagnostics from the Rust engine.";

export function buildTargetDiagnostics(): BuildTargetDiagnostics {
  return {
    routes: 0,
    warnings: [TS_CORE_DEPRECATION_WARNING],
  };
}

export function buildTargetResult(
  plan: BuildTargetPlan,
  emitted: boolean,
): BuildTargetResult {
  return {
    ...plan,
    emitted,
    diagnostics: buildTargetDiagnostics(),
  };
}
