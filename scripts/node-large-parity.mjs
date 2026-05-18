#!/usr/bin/env node

import { spawnSync } from "node:child_process";
import path from "node:path";

const repoRoot = path.resolve(import.meta.dirname, "..");

const vector = spawnSync("node", ["scripts/node-large-vector-parity.mjs"], {
  cwd: repoRoot,
  encoding: "utf8",
  stdio: ["ignore", "pipe", "pipe"],
  maxBuffer: 256 * 1024 * 1024,
});

let vectorReport;
try {
  vectorReport = JSON.parse(vector.stdout);
} catch (error) {
  const report = {
    version: "node-large-parity.v1",
    status: "failed",
    reason: "node-large vector parity report was not valid JSON",
    error: error instanceof Error ? error.message : String(error),
    stdout: vector.stdout,
    stderr: vector.stderr,
  };
  console.log(JSON.stringify(report, null, 2));
  process.exit(1);
}

const report = {
  version: "node-large-parity.v1",
  status: vectorReport.status === "passed" ? "passed" : "blocked",
  nodeLts: vectorReport.nodeLts,
  summary: {
    total: vectorReport.summary.total,
    nodePassed: vectorReport.summary.nodePassed,
    goBuildPassed: vectorReport.summary.goBuildPassed,
    goRunPassed: vectorReport.summary.goRunPassed,
    parityPassed: vectorReport.summary.parityPassed,
    requiredVectors: vectorReport.summary.vectorsRequired,
  },
  cases: vectorReport.cases,
};

console.log(JSON.stringify(report, null, 2));
if (report.status !== "passed") {
  process.exit(1);
}
