import assert from "node:assert/strict";
import { spawnSync } from "node:child_process";
import path from "node:path";
import { test } from "node:test";

const repoRoot = path.resolve(import.meta.dirname, "..", "..", "..");
const harnessPath = path.join(repoRoot, "scripts", "differential-harness.mjs");

function runHarness(env: NodeJS.ProcessEnv = {}) {
  return spawnSync(
    process.execPath,
    [harnessPath, "--scenario", "fastify-min-get-health"],
    {
      cwd: repoRoot,
      encoding: "utf8",
      env: {
        ...process.env,
        ...env,
      },
    },
  );
}

test("differential harness emits deterministic report format for supported-subset scenario", () => {
  const result = runHarness();
  assert.equal(result.status, 0, result.stderr);

  const report = JSON.parse(result.stdout) as {
    version: string;
    scenario: string;
    deterministic: boolean;
    summary: {
      total: number;
      matched: number;
      mismatched: number;
      pass: boolean;
    };
    cases: Array<{
      id: string;
      match: boolean;
      diffs: string[];
      request: { method: string; path: string };
    }>;
  };

  assert.equal(report.version, "m4-differential-harness.v1");
  assert.equal(report.scenario, "fastify-min-get-health");
  assert.equal(report.deterministic, true);
  assert.deepEqual(report.summary, {
    total: 1,
    matched: 1,
    mismatched: 0,
    pass: true,
  });
  assert.equal(report.cases[0]?.id, "health-get-200");
  assert.deepEqual(report.cases[0]?.request, {
    method: "GET",
    path: "/health",
  });
  assert.equal(report.cases[0]?.match, true);
  assert.deepEqual(report.cases[0]?.diffs, []);
});

test("differential harness fails closed on runtime mismatch", () => {
  const result = runHarness({ TSGODOWN_DIFF_FORCE_MISMATCH: "1" });
  assert.equal(result.status, 1, "expected mismatch to fail harness");

  const report = JSON.parse(result.stdout) as {
    summary: {
      total: number;
      matched: number;
      mismatched: number;
      pass: boolean;
    };
    cases: Array<{ match: boolean; diffs: string[] }>;
  };

  assert.equal(report.summary.total, 1);
  assert.equal(report.summary.matched, 0);
  assert.equal(report.summary.mismatched, 1);
  assert.equal(report.summary.pass, false);
  assert.equal(report.cases[0]?.match, false);
  assert.deepEqual(report.cases[0]?.diffs, ["status:200!=501"]);
});
