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
const tsTestAbsPath = path.join(repoRoot, tsTestPath);
const tsdownOutDir = path.join(outDir, "packages", "cli", "test");
const jsTestPath = path.join(tsdownOutDir, "commands.e2e.test.js");
const fixtureSrcDir = path.join(
  repoRoot,
  "packages",
  "cli",
  "test",
  "fixtures",
);
const fixtureOutDir = path.join(outDir, "packages", "cli", "test", "fixtures");

fs.rmSync(outDir, { recursive: true, force: true });
fs.mkdirSync(outDir, { recursive: true });

const tsdownConfigPath = path.join(outDir, "tsdown.e2e.config.ts");
fs.writeFileSync(
  tsdownConfigPath,
  `export default {
  entry: { "commands.e2e.test": ${JSON.stringify(tsTestAbsPath)} },
  outDir: ${JSON.stringify(tsdownOutDir)},
  format: ["esm"],
  dts: false,
  sourcemap: false,
  clean: true,
  fixedExtension: false,
  outExtensions: ({ format }) => ({
    js: format === "es" ? ".js" : ".cjs",
  }),
};
`,
);

const tsdown = spawnSync(
  "pnpm",
  ["exec", "tsdown", "--config", tsdownConfigPath],
  {
    cwd: repoRoot,
    encoding: "utf8",
  },
);

if (!fs.existsSync(jsTestPath)) {
  process.stderr.write(tsdown.stderr || tsdown.stdout || "tsdown failed\n");
  process.exit(tsdown.status ?? 1);
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
