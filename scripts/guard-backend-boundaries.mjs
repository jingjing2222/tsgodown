#!/usr/bin/env node

import fs from "node:fs";
import path from "node:path";

const repoRoot = path.resolve(import.meta.dirname, "..");

const backendNeutralTargets = [
  "packages/analyzer-rust/src/ir.rs",
  "crates/engine-core/src/contract.rs",
  "packages/ir/src",
];

const forbiddenBackendTerms = [
  {
    pattern: /\bGo[A-Z_a-z0-9]*\b|\bgo[A-Z_][A-Za-z0-9]*\b|\bgolang\b/i,
    code: "GO_CONCEPT_IN_BACKEND_NEUTRAL_IR",
    reason: "backend-neutral IR/contract must not encode Go-specific concepts",
  },
  {
    pattern: /\btsgodownrt\b/,
    code: "GO_RUNTIME_NAME_IN_IR_CONTRACT",
    reason: "runtime package names belong to backend emitters, not IR contract",
  },
  {
    pattern: /\bfilepath\b|\bgoroutine\b|\bchan\b/,
    code: "GO_RUNTIME_POLICY_IN_IR_CONTRACT",
    reason:
      "JS semantics policy belongs to IR/runtime contract, backend mechanics belong to emitters",
  },
];

const findings = [];

for (const target of backendNeutralTargets) {
  const absolute = path.join(repoRoot, target);
  for (const filePath of listFiles(absolute)) {
    const source = fs.readFileSync(filePath, "utf8");
    const lines = source.split("\n");
    for (const [index, line] of lines.entries()) {
      const trimmed = line.trim();
      if (trimmed.startsWith("//")) {
        continue;
      }
      for (const rule of forbiddenBackendTerms) {
        if (rule.pattern.test(line)) {
          findings.push({
            code: rule.code,
            file: path.relative(repoRoot, filePath),
            line: index + 1,
            reason: rule.reason,
            source: trimmed,
          });
        }
      }
    }
  }
}

const report = {
  version: "backend-boundary-guard.v1",
  status: findings.length === 0 ? "passed" : "failed",
  findings,
};

console.log(JSON.stringify(report, null, 2));

if (findings.length > 0) {
  process.exit(1);
}

function listFiles(target) {
  const stat = fs.statSync(target);
  if (stat.isFile()) {
    return [target];
  }
  return fs.readdirSync(target, { withFileTypes: true }).flatMap((entry) => {
    const child = path.join(target, entry.name);
    if (entry.isDirectory()) {
      return listFiles(child);
    }
    return entry.isFile() && /\.(rs|ts|md)$/.test(entry.name) ? [child] : [];
  });
}
