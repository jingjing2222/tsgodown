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
  node: {
    status: "blocked",
    vectors: 0,
    requiredVectors: entry.vectors.expected,
    reason: "100 Vitest vectors not vendored yet",
  },
  go: {
    build: { status: "blocked", reason: "generated Go vector suite missing" },
    run: { status: "blocked", reason: "generated Go vector suite missing" },
  },
  parity: { status: "blocked" },
}));

const report = {
  version: "node-large-vector-parity.v1",
  status: "blocked",
  nodeLts: manifest.nodeLts,
  summary: {
    total: cases.length,
    vectorsRequired: cases.reduce(
      (sum, entry) => sum + entry.node.requiredVectors,
      0,
    ),
    vectorsPresent: 0,
    parityPassed: 0,
  },
  cases,
};

console.log(JSON.stringify(report, null, 2));
process.exit(1);
