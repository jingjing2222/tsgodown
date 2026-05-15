#!/usr/bin/env node

import fs from "node:fs";
import path from "node:path";

const repoRoot = path.resolve(import.meta.dirname, "..");
const targets = [path.join(repoRoot, "scripts", "generate-node-corpus-go.mjs")];

const forbidden = [
  {
    pattern: /\btestCase\.id\s*(?:={2,3}|!==?)/,
    code: "CORPUS_ID_BRANCH",
    reason: "compiler/codegen path branches on corpus id",
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

for (const filePath of targets) {
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
