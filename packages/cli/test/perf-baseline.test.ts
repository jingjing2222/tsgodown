import assert from "node:assert/strict";
import test from "node:test";

import {
  PERF_SCENARIOS,
  evaluateRegression,
  summarizeMs,
} from "../src/perf-baseline.js";

test("PERF_SCENARIOS defines stable scenario scaffolding", () => {
  assert.ok(PERF_SCENARIOS.length >= 4);

  const ids = new Set(PERF_SCENARIOS.map((scenario) => scenario.id));
  assert.equal(ids.size, PERF_SCENARIOS.length);

  for (const scenario of PERF_SCENARIOS) {
    assert.ok(scenario.thresholdMs > 0);
    assert.ok(scenario.sampleRuns > 0);
    assert.ok(scenario.regressionTolerancePct >= 0);
  }
});

test("summarizeMs calculates median and p95 from run samples", () => {
  const stats = summarizeMs([100, 110, 90, 130, 120]);

  assert.equal(stats.minMs, 90);
  assert.equal(stats.maxMs, 130);
  assert.equal(stats.medianMs, 110);
  assert.equal(stats.p95Ms, 128);
  assert.equal(stats.meanMs, 110);
});

test("evaluateRegression passes within tolerance and fails above it", () => {
  const pass = evaluateRegression(100, 119, 20);
  assert.equal(pass.ok, true);
  assert.equal(pass.deltaMs, 19);
  assert.equal(pass.deltaPct, 19);

  const fail = evaluateRegression(100, 125, 20);
  assert.equal(fail.ok, false);
  assert.equal(fail.deltaMs, 25);
  assert.equal(fail.deltaPct, 25);
});
