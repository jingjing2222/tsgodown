import assert from "node:assert/strict";
import { spawnSync } from "node:child_process";
import path from "node:path";
import { test } from "node:test";

const repoRoot = path.resolve(import.meta.dirname, "..", "..", "..");
const harnessPath = path.join(repoRoot, "scripts", "differential-harness.mjs");

function runHarness(
  scenario = "fastify-min-get-health",
  env: NodeJS.ProcessEnv = {},
) {
  return spawnSync(process.execPath, [harnessPath, "--scenario", scenario], {
    cwd: repoRoot,
    encoding: "utf8",
    env: {
      ...process.env,
      ...env,
    },
  });
}

test("differential harness emits deterministic report format for supported-subset scenario", () => {
  const result = runHarness();
  assert.equal(result.status, 0, result.stderr);

  const report = JSON.parse(result.stdout) as {
    version: string;
    scenario: string;
    subset: string;
    description: string;
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
  assert.equal(report.subset, "fastify.get + json response");
  assert.match(report.description, /Representative supported-subset scenario/);
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

test("differential harness covers deterministic scaffold-real style endpoint matrix", () => {
  const result = runHarness("fastify-scaffold-real-routes");
  assert.equal(result.status, 0, result.stderr);

  const report = JSON.parse(result.stdout) as {
    scenario: string;
    summary: {
      total: number;
      matched: number;
      mismatched: number;
      pass: boolean;
    };
    cases: Array<{ id: string; request: { method: string; path: string } }>;
  };

  assert.equal(report.scenario, "fastify-scaffold-real-routes");
  assert.deepEqual(report.summary, {
    total: 5,
    matched: 5,
    mismatched: 0,
    pass: true,
  });
  assert.deepEqual(
    report.cases.map((entry) => entry.id),
    [
      "health-get-200",
      "missing-get-404",
      "users-get-405",
      "users-post-200",
      "users-put-200",
    ],
  );
});

test("differential harness fails closed on runtime mismatch", () => {
  const result = runHarness("fastify-min-get-health", {
    TSGODOWN_DIFF_FORCE_MISMATCH: "1",
  });
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

test("differential harness gate catches each deterministic drift type", () => {
  const driftCases: Array<{ mode: string; diff: string }> = [
    { mode: "status", diff: "status:200!=501" },
    { mode: "headers", diff: "headers-mismatch" },
    { mode: "body", diff: "body-mismatch" },
    { mode: "missing-go", diff: "missing-go-case" },
    { mode: "missing-ts", diff: "missing-ts-case" },
  ];

  for (const driftCase of driftCases) {
    const result = runHarness("fastify-scaffold-real-routes", {
      TSGODOWN_DIFF_FORCE_DRIFT: driftCase.mode,
    });
    assert.equal(result.status, 1, `expected ${driftCase.mode} drift to fail`);

    const report = JSON.parse(result.stdout) as {
      summary: {
        mismatched: number;
        pass: boolean;
      };
      cases: Array<{ diffs: string[] }>;
    };

    assert.equal(report.summary.pass, false);
    assert.equal(report.summary.mismatched, 1);
    assert.ok(
      report.cases.some((entry) => entry.diffs.includes(driftCase.diff)),
      `missing expected diff marker ${driftCase.diff}`,
    );
  }
});
