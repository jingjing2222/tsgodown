import assert from "node:assert/strict";
import { spawnSync } from "node:child_process";
import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import { test } from "node:test";

const repoRoot = path.resolve(import.meta.dirname, "..", "..", "..");
const ratchetPath = path.join(
  repoRoot,
  "scripts",
  "check-differential-coverage-ratchet.mjs",
);

function runRatchet(baseline: object) {
  const tempDir = fs.mkdtempSync(path.join(os.tmpdir(), "tsgodown-ratchet-"));
  const baselinePath = path.join(tempDir, "baseline.json");
  fs.writeFileSync(baselinePath, `${JSON.stringify(baseline, null, 2)}\n`);

  const result = spawnSync(process.execPath, [ratchetPath], {
    cwd: repoRoot,
    encoding: "utf8",
    env: {
      ...process.env,
      TSGODOWN_COVERAGE_BASELINE_PATH: baselinePath,
    },
  });

  fs.rmSync(tempDir, { recursive: true, force: true });
  return result;
}

test("coverage ratchet passes when baseline matches current harness floor", () => {
  const result = runRatchet({
    version: "m4-differential-harness.v1",
    minimumScenarios: 3,
    minimumTotalCases: 3,
    requiredScenarios: [
      "fastify-scaffold-real-get-health",
      "hono-scaffold-real-get-health",
      "generic-simple-cli-get-health",
    ],
  });

  assert.equal(result.status, 0, result.stderr);
  assert.match(result.stdout, /\[coverage-ratchet\] ok:/);
});

test("coverage ratchet fails closed when required scenario is missing from baseline contract", () => {
  const result = runRatchet({
    version: "m4-differential-harness.v1",
    minimumScenarios: 3,
    minimumTotalCases: 3,
    requiredScenarios: ["non-existent-scenario"],
  });

  assert.equal(result.status, 1);
  assert.match(
    result.stderr,
    /missing required scenarios: non-existent-scenario/,
  );
});
