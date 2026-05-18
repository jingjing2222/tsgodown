#!/usr/bin/env node

import fs from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";

const repoRoot = path.resolve(
  path.dirname(fileURLToPath(import.meta.url)),
  "..",
);

const checkedRoots = ["scripts", "packages"];
const sourceExtensions = new Set([".js", ".mjs", ".cjs", ".ts", ".tsx", ".sh"]);
const excludedPathParts = new Set(["dist", "node_modules", "test", "tests"]);
const excludedFiles = new Set(["scripts/guard-codegen-ownership.mjs"]);

const forbidden = [
  {
    name: "go-package-template",
    pattern: /["'`]package main["'`]/,
    reason: "non-Rust code must not template Go source packages",
  },
  {
    name: "go-main-template-constant",
    pattern: /\bGO_MAIN\b/,
    reason: "non-Rust code must not own Go source templates",
  },
  {
    name: "go-render-helper",
    pattern: /\brenderGo[A-Za-z0-9_]*\b/,
    reason: "Go backend rendering belongs in Rust engine code",
  },
  {
    name: "direct-go-file-write",
    pattern: /writeFileSync\([^)]*["'`][^"'`]*\.go["'`]/s,
    reason:
      "non-Rust code may copy files returned by engine-core, not synthesize Go files",
  },
];

function walk(dir) {
  const entries = fs.readdirSync(dir, { withFileTypes: true });
  const files = [];
  for (const entry of entries) {
    const fullPath = path.join(dir, entry.name);
    const relPath = path.relative(repoRoot, fullPath);
    if (entry.isDirectory()) {
      if (excludedPathParts.has(entry.name)) {
        continue;
      }
      files.push(...walk(fullPath));
      continue;
    }
    if (!sourceExtensions.has(path.extname(entry.name))) {
      continue;
    }
    if (excludedFiles.has(relPath)) {
      continue;
    }
    files.push(fullPath);
  }
  return files;
}

const findings = [];
for (const root of checkedRoots) {
  const rootPath = path.join(repoRoot, root);
  if (!fs.existsSync(rootPath)) {
    continue;
  }
  for (const file of walk(rootPath)) {
    const contents = fs.readFileSync(file, "utf8");
    for (const rule of forbidden) {
      const match = contents.match(rule.pattern);
      if (!match) {
        continue;
      }
      findings.push({
        file: path.relative(repoRoot, file),
        rule: rule.name,
        reason: rule.reason,
      });
    }
  }
}

if (findings.length > 0) {
  process.stderr.write(
    `${JSON.stringify(
      {
        version: "codegen-ownership-guard.v1",
        status: "failed",
        findings,
      },
      null,
      2,
    )}\n`,
  );
  process.exit(1);
}

process.stdout.write(
  `${JSON.stringify({
    version: "codegen-ownership-guard.v1",
    status: "passed",
    findings: [],
  })}\n`,
);
