#!/usr/bin/env node

import fs from "node:fs";
import path from "node:path";

const repoRoot = path.resolve(import.meta.dirname, "..");

const targets = [
  "crates/engine-core/src/emit_go.rs",
  "test-corpus/node-real/generated-go",
].map((target) => path.join(repoRoot, target));

const forbidden = [
  {
    pattern: /\bTSGODOWN_NODE\b/,
    code: "NODE_ENV_FALLBACK",
    reason: "generated runtime must not be redirected to a Node executable",
  },
  {
    pattern: /\bfindExecutableOnPath\s*\(\s*["']node["']\s*\)/,
    code: "NODE_PATH_LOOKUP",
    reason: "generated runtime must not discover Node from PATH",
  },
  {
    pattern: /\bexec\.Command\s*\(\s*["']node["']/,
    code: "NODE_EXEC_COMMAND",
    reason: "generated Go must not shell out to Node",
  },
  {
    pattern: /\bexec\.Command\s*\([^)]*nodeExecutablePath\s*\(/,
    code: "NODE_EXEC_PATH_COMMAND",
    reason: "generated Go must not shell out through process.execPath",
  },
  {
    pattern: /\bsyscall\.Exec\s*\([^)]*node/,
    code: "NODE_SYSCALL_EXEC",
    reason: "generated Go must not exec Node",
  },
  {
    pattern:
      /["'](?:node-addon-api|node_api|napi)["']|\b(?:v8::|node::|napi_)/i,
    code: "NODE_NATIVE_FALLBACK",
    reason:
      "generated Go must not depend on V8, Node-API, N-API, or native addon fallback",
  },
];

const findings = [];

for (const filePath of listTargetFiles()) {
  const source = fs.readFileSync(filePath, "utf8");
  const lines = source.split("\n");
  for (const [index, line] of lines.entries()) {
    for (const rule of forbidden) {
      if (!rule.pattern.test(line)) {
        continue;
      }
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

const report = {
  version: "no-node-fallback-guard.v1",
  status: findings.length === 0 ? "passed" : "failed",
  findings,
};

console.log(JSON.stringify(report, null, 2));

if (findings.length > 0) {
  process.exit(1);
}

function listTargetFiles() {
  const files = [];
  for (const target of targets) {
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
  return files.filter((filePath) => /\.(go|rs)$/.test(path.basename(filePath)));
}

function walk(dir) {
  return fs.readdirSync(dir, { withFileTypes: true }).flatMap((entry) => {
    const child = path.join(dir, entry.name);
    if (entry.isDirectory()) {
      return walk(child);
    }
    if (!entry.isFile()) {
      return [];
    }
    if (
      entry.name === "source_ir.go" ||
      entry.name === "probe_ir.go" ||
      entry.name === "vector_suite.go"
    ) {
      return [];
    }
    return [child];
  });
}
