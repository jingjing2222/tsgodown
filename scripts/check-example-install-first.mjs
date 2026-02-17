#!/usr/bin/env node
import { spawnSync } from "node:child_process";
import fs from "node:fs";
import path from "node:path";

const repoRoot = path.resolve(import.meta.dirname, "..");
const engineCoreBin = path.join(repoRoot, "target", "debug", "engine-core");
const rustLauncher = path.join(repoRoot, "scripts", "rust-engine-launcher.sh");

function run(cmd, args, opts = {}) {
  const res = spawnSync(cmd, args, {
    cwd: repoRoot,
    encoding: "utf8",
    stdio: ["inherit", "pipe", "pipe"],
    ...opts,
  });

  if (res.status !== 0) {
    const context = [
      `${cmd} ${args.join(" ")} failed (exit=${res.status ?? "null"})`,
      res.stdout ? `stdout:\n${res.stdout}` : "",
      res.stderr ? `stderr:\n${res.stderr}` : "",
    ]
      .filter(Boolean)
      .join("\n\n");
    throw new Error(context);
  }

  return res;
}

function listTrackedExamples() {
  const out = run("git", ["ls-files", "examples/*/tsgodown.config.ts"]);
  return out.stdout
    .split("\n")
    .map((line) => line.trim())
    .filter(Boolean)
    .map((configPath) => path.dirname(configPath));
}

function readScripts(projectDirRel) {
  const packageJsonPath = path.join(repoRoot, projectDirRel, "package.json");
  if (!fs.existsSync(packageJsonPath)) {
    return {};
  }
  const raw = JSON.parse(fs.readFileSync(packageJsonPath, "utf8"));
  return raw.scripts ?? {};
}

function main() {
  const examples = listTrackedExamples();

  if (examples.length === 0) {
    console.log(
      "[install-first] SKIP (no tracked examples with tsgodown.config.ts)",
    );
    return;
  }

  if (!fs.existsSync(rustLauncher)) {
    throw new Error(
      `[install-first] missing launcher: ${path.relative(repoRoot, rustLauncher)}`,
    );
  }

  if (!fs.existsSync(engineCoreBin)) {
    console.log(
      "[install-first] engine-core not found; building cargo target...",
    );
    run("cargo", ["build", "-p", "engine-core"]);
  }

  const env = {
    ...process.env,
    TSGODOWN_RUST_ENGINE_BIN: rustLauncher,
    TSGODOWN_ENGINE_CORE_BIN: engineCoreBin,
  };

  for (const projectDirRel of examples) {
    const scripts = readScripts(projectDirRel);
    if (!scripts["build:go"]) {
      throw new Error(
        [
          `[install-first] ${projectDirRel} is missing required script 'build:go'.`,
          "Hint: add a package.json script so CI can run `pnpm run build:go` after install.",
        ].join("\n"),
      );
    }

    console.log(`\n[install-first] checking ${projectDirRel}`);

    const cwd = path.join(repoRoot, projectDirRel);
    run("pnpm", ["install", "--lockfile=false", "--reporter=append-only"], {
      cwd,
      env,
    });
    run("pnpm", ["run", "build:go"], { cwd, env });

    console.log(`[install-first] PASS ${projectDirRel}`);
  }

  console.log(
    `\n[install-first] PASS (${examples.length} example${examples.length === 1 ? "" : "s"})`,
  );
}

main();
