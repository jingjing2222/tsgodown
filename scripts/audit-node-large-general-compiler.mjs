#!/usr/bin/env node

import fs from "node:fs";
import path from "node:path";

const repoRoot = path.resolve(import.meta.dirname, "..");
const manifest = JSON.parse(
  fs.readFileSync(
    path.join(repoRoot, "test-corpus", "node-large", "manifest.json"),
    "utf8",
  ),
);

const targetRoots = [
  "scripts/generate-node-corpus-go.mjs",
  "packages/config/src",
  "packages/core/src",
  "packages/pipeline/src",
  "packages/tsdown-driver/src",
  "packages/emitter-go/src",
  "packages/cli/src",
  "packages/analyzer-rust/src",
  "crates/engine-core/src",
].map((target) => path.join(repoRoot, target));

const packageTokens = manifest.entries
  .flatMap((entry) => [
    entry.id,
    entry.package,
    entry.package.replace(/^@/, "").replace("/", "-"),
  ])
  .filter((token) => token !== "next");

const forbidden = new RegExp(
  `(?:case\\s+["'](?:${packageTokens.map(escapeRegExp).join("|")})["']\\s*:|["'](?:${packageTokens
    .map(escapeRegExp)
    .join("|")})["']\\s*(?:=>|:))`,
);

const findings = [];

for (const filePath of listTargetFiles()) {
  const source = fs.readFileSync(filePath, "utf8");
  const lines = source.split("\n");
  for (const [index, line] of lines.entries()) {
    if (forbidden.test(line)) {
      findings.push({
        code: "LARGE_CORPUS_PACKAGE_BRANCH",
        file: path.relative(repoRoot, filePath),
        line: index + 1,
        reason:
          "compiler/codegen/runtime path contains large corpus package identifier",
        source: line.trim(),
      });
    }
  }
}

const report = {
  version: "node-large-general-compiler-audit.v1",
  status: findings.length === 0 ? "passed" : "failed",
  cases: manifest.entries.length,
  findings,
};

console.log(JSON.stringify(report, null, 2));

if (findings.length > 0) {
  process.exit(1);
}

function listTargetFiles() {
  return targetRoots.flatMap((target) => {
    if (!fs.existsSync(target)) {
      return [];
    }
    const stat = fs.statSync(target);
    return stat.isFile() ? [target] : walk(target);
  });
}

function walk(dir) {
  return fs.readdirSync(dir, { withFileTypes: true }).flatMap((entry) => {
    const child = path.join(dir, entry.name);
    if (entry.isDirectory()) {
      if (
        entry.name === "dist" ||
        entry.name === "test" ||
        entry.name === "tests"
      ) {
        return [];
      }
      return walk(child);
    }
    return entry.isFile() && /\.(mjs|js|ts|tsx|rs)$/.test(entry.name)
      ? [child]
      : [];
  });
}

function escapeRegExp(value) {
  return value.replace(/[.*+?^${}()|[\]\\]/g, "\\$&");
}
