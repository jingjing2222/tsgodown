#!/usr/bin/env node

import fs from "node:fs";
import path from "node:path";

const repoRoot = path.resolve(import.meta.dirname, "..");
const corpusRoot = path.join(repoRoot, "test-corpus", "node-large");
const manifest = JSON.parse(
  fs.readFileSync(path.join(corpusRoot, "manifest.json"), "utf8"),
);

const cases = manifest.entries.map((entry) => ({
  id: entry.id,
  package: entry.package,
  version: entry.version,
  node: { status: "blocked", reason: "large corpus package not vendored yet" },
  go: {
    build: {
      status: "blocked",
      reason: "generated Go project not available yet",
    },
    run: { status: "blocked", reason: "generated Go binary not available yet" },
  },
  parity: {
    status: "blocked",
    reason: "100-vector parity suite not vendored yet",
  },
}));

const report = {
  version: "node-large-parity.v1",
  status: "blocked",
  nodeLts: manifest.nodeLts,
  summary: {
    total: cases.length,
    nodePassed: 0,
    goBuildPassed: 0,
    goRunPassed: 0,
    parityPassed: 0,
    requiredVectors: cases.length * manifest.policy.vectorsPerEntry,
  },
  cases,
};

console.log(JSON.stringify(report, null, 2));
process.exit(1);
