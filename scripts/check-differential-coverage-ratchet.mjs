#!/usr/bin/env node

import { spawnSync } from "node:child_process";
import fs from "node:fs";
import path from "node:path";

const repoRoot = path.resolve(import.meta.dirname, "..");
const baselinePath =
  process.env.TSGODOWN_COVERAGE_BASELINE_PATH ??
  path.join(repoRoot, "profiles", "differential-coverage-baseline.json");
const harnessPath = path.join(repoRoot, "scripts", "differential-harness.mjs");

function fail(message) {
  console.error(`[coverage-ratchet] fail: ${message}`);
  process.exit(1);
}

function main() {
  const baseline = JSON.parse(fs.readFileSync(baselinePath, "utf8"));

  const harness = spawnSync(process.execPath, [harnessPath, "--all"], {
    cwd: repoRoot,
    encoding: "utf8",
  });

  if (harness.error) {
    fail(`unable to run differential harness: ${harness.error.message}`);
  }

  if (harness.status !== 0) {
    fail(`differential harness failed (exit=${harness.status})`);
  }

  const report = JSON.parse(harness.stdout);
  if (report.version !== baseline.version) {
    fail(
      `version mismatch: baseline=${baseline.version}, report=${report.version}`,
    );
  }

  const scenarios = Array.isArray(report.reports) ? report.reports : [];
  const totalScenarios = scenarios.length;
  if (totalScenarios < baseline.minimumScenarios) {
    fail(
      `scenario count regressed: expected >= ${baseline.minimumScenarios}, got ${totalScenarios}`,
    );
  }

  const totalCases = scenarios.reduce(
    (sum, scenario) => sum + Number(scenario?.summary?.total ?? 0),
    0,
  );

  if (totalCases < baseline.minimumTotalCases) {
    fail(
      `total case count regressed: expected >= ${baseline.minimumTotalCases}, got ${totalCases}`,
    );
  }

  const seenScenarios = new Set(
    scenarios.map((scenario) => String(scenario.scenario ?? "")),
  );
  const missingRequired = baseline.requiredScenarios.filter(
    (name) => !seenScenarios.has(name),
  );

  if (missingRequired.length > 0) {
    fail(`missing required scenarios: ${missingRequired.join(", ")}`);
  }

  console.log(
    `[coverage-ratchet] ok: scenarios=${totalScenarios} totalCases=${totalCases}`,
  );
}

main();
