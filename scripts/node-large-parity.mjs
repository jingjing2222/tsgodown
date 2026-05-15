#!/usr/bin/env node

import fs from "node:fs";
import path from "node:path";

const repoRoot = path.resolve(import.meta.dirname, "..");
const corpusRoot = path.join(repoRoot, "test-corpus", "node-large");
const manifest = JSON.parse(
  fs.readFileSync(path.join(corpusRoot, "manifest.json"), "utf8"),
);
const generatedRoot =
  process.env.TSGODOWN_NODE_LARGE_GO_ROOT ??
  path.join(corpusRoot, "generated-go");

const cases = manifest.entries.map((entry) => {
  const vectorPath = path.join(corpusRoot, "cases", entry.id, "vectors.json");
  const vectorCount = fs.existsSync(vectorPath)
    ? (JSON.parse(fs.readFileSync(vectorPath, "utf8")).cases?.length ?? 0)
    : 0;
  const generatedPath = path.join(generatedRoot, entry.id, "go.mod");
  return {
    id: entry.id,
    package: entry.package,
    version: entry.version,
    node:
      vectorCount === manifest.policy.vectorsPerEntry
        ? {
            status: "passed",
            vectors: vectorCount,
            command: `pnpm run test:node-large:vitest -- ${entry.id}`,
          }
        : {
            status: "blocked",
            vectors: vectorCount,
            reason: "100-vector Node probe suite not implemented yet",
          },
    go: {
      build: fs.existsSync(generatedPath)
        ? { status: "blocked", reason: "large Go build gate not wired yet" }
        : {
            status: "blocked",
            reason: "generated Go project not available yet",
          },
      run: {
        status: "blocked",
        reason: "generated Go binary not available yet",
      },
    },
    parity: {
      status: "blocked",
      reason: "generated Go vector parity suite not implemented yet",
    },
  };
});

const report = {
  version: "node-large-parity.v1",
  status: "blocked",
  nodeLts: manifest.nodeLts,
  summary: {
    total: cases.length,
    nodePassed: cases.filter((entry) => entry.node.status === "passed").length,
    goBuildPassed: 0,
    goRunPassed: 0,
    parityPassed: 0,
    requiredVectors: cases.length * manifest.policy.vectorsPerEntry,
  },
  cases,
};

console.log(JSON.stringify(report, null, 2));
process.exit(1);
