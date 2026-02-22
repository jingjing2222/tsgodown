#!/usr/bin/env node

import { spawnSync } from "node:child_process";
import fs from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";

const __filename = fileURLToPath(import.meta.url);
const __dirname = path.dirname(__filename);
const repoRoot = path.resolve(__dirname, "..");

const pattern = process.argv[2];
if (!pattern) {
  console.error(
    "usage: node scripts/run-cli-e2e-gate.mjs '<node-test-name-pattern>'",
  );
  process.exit(2);
}

const outDir = path.join(repoRoot, ".tmp", "cli-e2e-gate");
const tsTestPath = path.join("packages", "cli", "test", "commands.e2e.test.ts");
const jsTestPath = path.join(
  outDir,
  "packages",
  "cli",
  "test",
  "commands.e2e.test.js",
);
const fixtureSrcDir = path.join(repoRoot, "packages", "cli", "test", "fixtures");
const fixtureOutDir = path.join(
  outDir,
  "packages",
  "cli",
  "test",
  "fixtures",
);

fs.rmSync(outDir, { recursive: true, force: true });
fs.mkdirSync(outDir, { recursive: true });

const tsc = spawnSync(
  "pnpm",
  [
    "exec",
    "tsc",
    "--pretty",
    "false",
    "--module",
    "nodenext",
    "--moduleResolution",
    "nodenext",
    "--target",
    "ES2022",
    "--types",
    "node",
    "--skipLibCheck",
    "--esModuleInterop",
    "--noEmitOnError",
    "false",
    "--rootDir",
    ".",
    "--outDir",
    outDir,
    tsTestPath,
  ],
  {
    cwd: repoRoot,
    encoding: "utf8",
  },
);

if (!fs.existsSync(jsTestPath)) {
  process.stderr.write(tsc.stderr || tsc.stdout || "tsc failed\n");
  process.exit(tsc.status ?? 1);
}

if (tsc.status !== 0) {
  process.stderr.write(
    `[run-cli-e2e-gate] continuing with emitted JS despite TypeScript diagnostics\n`,
  );
  if (tsc.stderr) process.stderr.write(tsc.stderr);
}

fs.mkdirSync(path.dirname(fixtureOutDir), { recursive: true });
fs.cpSync(fixtureSrcDir, fixtureOutDir, { recursive: true });

const test = spawnSync(
  process.execPath,
  ["--test", "--test-name-pattern", pattern, jsTestPath],
  {
    cwd: repoRoot,
    encoding: "utf8",
    env: {
      ...process.env,
      TSGODOWN_REPO_ROOT: repoRoot,
    },
  },
);

if (test.stdout) process.stdout.write(test.stdout);
if (test.stderr) process.stderr.write(test.stderr);
process.exit(test.status ?? 1);
