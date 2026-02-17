export interface PerfScenario {
  id: string;
  fixture: string;
  command: "build" | "check" | "report" | "stages";
  warmupRuns: number;
  sampleRuns: number;
  thresholdMs: number;
  regressionTolerancePct: number;
}

export interface PerfStats {
  meanMs: number;
  medianMs: number;
  p95Ms: number;
  minMs: number;
  maxMs: number;
}

export interface RegressionCheck {
  ok: boolean;
  deltaMs: number;
  deltaPct: number;
}

export const PERF_SCENARIOS: PerfScenario[] = [
  {
    id: "cli-build-fastify-min",
    fixture: "fastify-min",
    command: "build",
    warmupRuns: 1,
    sampleRuns: 5,
    thresholdMs: 1500,
    regressionTolerancePct: 20,
  },
  {
    id: "cli-check-multi-file",
    fixture: "multi-file",
    command: "check",
    warmupRuns: 1,
    sampleRuns: 5,
    thresholdMs: 1200,
    regressionTolerancePct: 20,
  },
  {
    id: "cli-report-route-object-variants",
    fixture: "route-object-variants",
    command: "report",
    warmupRuns: 1,
    sampleRuns: 5,
    thresholdMs: 1200,
    regressionTolerancePct: 20,
  },
  {
    id: "cli-stages-nested-register-prefix",
    fixture: "nested-register-prefix",
    command: "stages",
    warmupRuns: 1,
    sampleRuns: 5,
    thresholdMs: 1200,
    regressionTolerancePct: 20,
  },
];

export function summarizeMs(samples: number[]): PerfStats {
  if (samples.length === 0) {
    throw new Error("samples must not be empty");
  }

  const sorted = [...samples].sort((a, b) => a - b);
  const meanMs =
    samples.reduce((acc, value) => acc + value, 0) / samples.length;
  const medianMs = percentile(sorted, 0.5);
  const p95Ms = percentile(sorted, 0.95);

  return {
    meanMs,
    medianMs,
    p95Ms,
    minMs: sorted[0],
    maxMs: sorted[sorted.length - 1],
  };
}

export function evaluateRegression(
  baselineMedianMs: number,
  currentMedianMs: number,
  tolerancePct: number,
): RegressionCheck {
  if (baselineMedianMs <= 0) {
    return { ok: true, deltaMs: 0, deltaPct: 0 };
  }

  const deltaMs = currentMedianMs - baselineMedianMs;
  const deltaPct = (deltaMs / baselineMedianMs) * 100;

  return {
    ok: deltaPct <= tolerancePct,
    deltaMs,
    deltaPct,
  };
}

function percentile(sorted: number[], p: number): number {
  if (sorted.length === 1) {
    return sorted[0];
  }

  const idx = (sorted.length - 1) * p;
  const lo = Math.floor(idx);
  const hi = Math.ceil(idx);
  if (lo === hi) {
    return sorted[lo];
  }

  const weight = idx - lo;
  return sorted[lo] * (1 - weight) + sorted[hi] * weight;
}
