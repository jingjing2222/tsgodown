import assert from "node:assert/strict";
import { spawnSync } from "node:child_process";
import path from "node:path";
import { test } from "node:test";

const repoRoot = path.resolve(import.meta.dirname, "..", "..", "..");
const harnessPath = path.join(repoRoot, "scripts", "differential-harness.mjs");

const scenarios = [
  "fastify-scaffold-real-get-health",
  "hono-scaffold-real-get-health",
  "generic-simple-cli-get-health",
] as const;

function runHarness(
  scenario: (typeof scenarios)[number],
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

for (const scenario of scenarios) {
  test(`differential harness emits deterministic report format for ${scenario}`, () => {
    const result = runHarness(scenario);
    assert.equal(result.status, 0, result.stderr);

    const report = JSON.parse(result.stdout) as {
      version: string;
      scenario: string;
      semanticsSurface: string;
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
        ts: { status: number; headers: Record<string, string>; body: unknown };
        go: { status: number; headers: Record<string, string>; body: unknown };
      }>;
    };

    assert.equal(report.version, "m4-differential-harness.v1");
    assert.equal(report.scenario, scenario);
    assert.equal(report.deterministic, true);
    assert.equal(typeof report.semanticsSurface, "string");
    assert.equal(typeof report.description, "string");
    assert.deepEqual(report.summary, {
      total: 1,
      matched: 1,
      mismatched: 0,
      pass: true,
    });

    assert.equal(report.cases[0]?.id, "health-get-501");
    assert.deepEqual(report.cases[0]?.request, {
      method: "GET",
      path: "/health",
    });
    assert.equal(report.cases[0]?.match, true);
    assert.deepEqual(report.cases[0]?.diffs, []);
    assert.equal(report.cases[0]?.ts.status, 501);
    assert.equal(report.cases[0]?.go.status, 501);
    assert.equal(
      report.cases[0]?.ts.headers["content-type"],
      "text/plain; charset=utf-8",
    );
    assert.equal(
      report.cases[0]?.go.headers["content-type"],
      "text/plain; charset=utf-8",
    );
    assert.equal(
      report.cases[0]?.ts.body,
      "TODO implement handler health for GET /health\n",
    );
    assert.equal(
      report.cases[0]?.go.body,
      "TODO implement handler health for GET /health\n",
    );
  });

  test(`differential harness fails closed on runtime mismatch for ${scenario}`, () => {
    const result = runHarness(scenario, { TSGODOWN_DIFF_FORCE_MISMATCH: "1" });
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
    assert.deepEqual(report.cases[0]?.diffs, ["status:501!=503"]);
  });
}
