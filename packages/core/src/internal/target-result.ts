import type {
  BuildTargetDiagnostics,
  BuildTargetPlan,
  BuildTargetResult,
} from "../index.js";

export function buildTargetDiagnostics(): BuildTargetDiagnostics {
  return {
    routes: 0,
    warnings: [],
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
