#!/usr/bin/env node
import { execSync } from "node:child_process";
import fs from "node:fs";

const forbiddenPaths = [
  "packages/analyzer",
  "docs/specs/FASTIFY_SUPPORT_MATRIX.md",
  "docs/specs/FASTIFY_UNSUPPORTED_INVENTORY.md",
  "scripts/check-fastify-diagnostics-sync.mjs",
  "scripts/guard-no-legacy-ts-analyzer.mjs",
];

const forbiddenReferences = [
  "docs/specs/FASTIFY_SUPPORT_MATRIX.md",
  "docs/specs/FASTIFY_UNSUPPORTED_INVENTORY.md",
  "scripts/check-fastify-diagnostics-sync.mjs",
  "pnpm run docs:diagnostics:sync",
];

const scanRoots = [
  "README.md",
  "docs",
  ".github/workflows",
  "scripts",
  "package.json",
];
const skipFiles = new Set(["scripts/guard-compiler-mode-only.mjs"]);

let hasFailure = false;

function isTrackedPath(path) {
  try {
    execSync(`git ls-files --error-unmatch -- "${path}"`, {
      stdio: "ignore",
    });
    return true;
  } catch {
    try {
      const out = execSync(`git ls-tree -r --name-only HEAD -- "${path}"`, {
        stdio: "pipe",
      })
        .toString()
        .trim();
      return out.length > 0;
    } catch {
      return false;
    }
  }
}

for (const path of forbiddenPaths) {
  if (fs.existsSync(path) && isTrackedPath(path)) {
    hasFailure = true;
    console.error(
      `[compiler-mode-guard] blocked: forbidden legacy path exists -> ${path}`,
    );
  }
}

function walk(targetPath) {
  const stat = fs.statSync(targetPath);
  if (stat.isFile()) return [targetPath];
  if (!stat.isDirectory()) return [];

  const files = [];
  for (const entry of fs.readdirSync(targetPath)) {
    if (entry === "node_modules" || entry === ".git") continue;
    files.push(...walk(`${targetPath}/${entry}`));
  }
  return files;
}

const filesToScan = scanRoots.flatMap((root) => {
  if (!fs.existsSync(root)) return [];
  return walk(root);
});

for (const file of filesToScan) {
  if (skipFiles.has(file)) continue;
  const body = fs.readFileSync(file, "utf8");
  for (const needle of forbiddenReferences) {
    if (body.includes(needle)) {
      hasFailure = true;
      console.error(
        `[compiler-mode-guard] blocked: forbidden reference '${needle}' found in ${file}`,
      );
    }
  }
}

if (hasFailure) {
  process.exit(1);
}

console.log("[compiler-mode-guard] ok: compiler-mode-only guardrails intact");
