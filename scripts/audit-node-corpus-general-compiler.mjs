#!/usr/bin/env node

import fs from "node:fs";
import path from "node:path";

const repoRoot = path.resolve(import.meta.dirname, "..");
const targetRoots = [
  "scripts/generate-node-corpus-go.mjs",
  "packages/config/src",
  "packages/core/src",
  "packages/pipeline/src",
  "packages/tsdown-driver/src",
  "packages/emitter-go/src",
  "packages/cli/src",
  "packages/analyzer-rust/src",
  "crates/engine-core/src/analyze.rs",
  "crates/engine-core/src/cli.rs",
  "crates/engine-core/src/contract.rs",
  "crates/engine-core/src/emit_go.rs",
  "crates/engine-core/src/error.rs",
  "crates/engine-core/src/main.rs",
  "crates/engine-core/src/runtime_contract.rs",
].map((target) => path.join(repoRoot, target));

const corpusPackageNames = [
  "semver",
  "minimatch",
  "qs",
  "dotenv",
  "yargs-parser",
  "js-yaml",
  "lru-cache",
  "uuid",
  "fs-extra",
  "execa",
];

const forbidden = [
  {
    pattern:
      /\b(?:if|while)\s*\([^)]*\btestCase\.id\b|\bswitch\s*\(\s*testCase\.id\s*\)|\btestCase\.id\s*(?:={2,3}|!==?|\?)/,
    code: "CORPUS_ID_BRANCH",
    reason: "compiler/codegen path branches on corpus id",
  },
  {
    pattern: new RegExp(
      `(?:case\\s+["'](?:${corpusPackageNames.join("|")})["']\\s*:|["'](?:${corpusPackageNames.join("|")})["']\\s*(?:=>|:)|\\b(?:${corpusPackageNames.join("|")})\\b)`,
    ),
    code: "CORPUS_PACKAGE_NAME_IN_COMPILER",
    reason:
      "compiler/codegen/runtime path contains vendored corpus package name",
  },
  {
    pattern: /\brender[A-Z][A-Za-z0-9]+IrHelpers\b/,
    code: "CORPUS_HELPER_RENDERER",
    reason: "compiler/codegen path injects package-specific helper source",
  },
  {
    pattern:
      /\bexternalNamespaces\b|\bexternalFunctions\b|\bexternalConstructors\b/,
    code: "EXTERNAL_SEMANTIC_STUB",
    reason: "compiler/codegen path relies on external semantic stubs",
  },
];

const findings = [];

for (const filePath of listTargetFiles()) {
  const source = fs.readFileSync(filePath, "utf8");
  const lines = source.split("\n");
  for (const [index, line] of lines.entries()) {
    for (const rule of forbidden) {
      if (rule.pattern.test(line)) {
        findings.push({
          code: rule.code,
          file: path.relative(repoRoot, filePath),
          line: index + 1,
          reason: rule.reason,
          source: line.trim(),
        });
      }
    }
  }
}

const report = {
  version: "node-corpus-general-compiler-audit.v1",
  status: findings.length === 0 ? "passed" : "failed",
  findings,
};

console.log(JSON.stringify(report, null, 2));

if (findings.length > 0) {
  process.exit(1);
}

function listTargetFiles() {
  const files = [];
  for (const target of targetRoots) {
    if (!fs.existsSync(target)) {
      continue;
    }
    const stat = fs.statSync(target);
    if (stat.isFile()) {
      files.push(target);
      continue;
    }
    files.push(...walk(target));
  }
  return files.filter((filePath) =>
    /\.(mjs|js|ts|tsx|rs)$/.test(path.basename(filePath)),
  );
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
    return entry.isFile() ? [child] : [];
  });
}
