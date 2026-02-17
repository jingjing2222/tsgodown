#!/usr/bin/env node
import { spawnSync } from "node:child_process";
import fs from "node:fs";
import os from "node:os";
import path from "node:path";

const args = new Set(process.argv.slice(2));
const writeMode = args.has("--write");

const repoRoot = path.resolve(import.meta.dirname, "..");
const rustLauncher = path.join(repoRoot, "scripts", "rust-engine-launcher.sh");
const engineCoreBin = path.join(repoRoot, "target", "debug", "engine-core");
const cliBuiltEntry = path.join(
  repoRoot,
  "packages",
  "cli",
  "dist",
  "index.js",
);

function run(cmd, cmdArgs, opts = {}) {
  const res = spawnSync(cmd, cmdArgs, {
    cwd: repoRoot,
    encoding: "utf8",
    ...opts,
  });
  if (res.status !== 0) {
    const context = [
      `${cmd} ${cmdArgs.join(" ")} failed (exit=${res.status ?? "null"})`,
      res.stdout ? `stdout:\n${res.stdout}` : "",
      res.stderr ? `stderr:\n${res.stderr}` : "",
    ]
      .filter(Boolean)
      .join("\n\n");
    throw new Error(context);
  }
  return res.stdout;
}

function listTrackedDistGoFiles() {
  const out = run("git", ["ls-files", "**/dist-go/**"]);
  return out
    .split("\n")
    .map((line) => line.trim())
    .filter(Boolean);
}

function isScaffoldScope(file) {
  return (
    file.startsWith("examples/") ||
    file.includes("/fixtures/") ||
    file.startsWith("packages/cli/test/fixtures/")
  );
}

function copyDir(src, dest) {
  fs.mkdirSync(dest, { recursive: true });
  for (const entry of fs.readdirSync(src, { withFileTypes: true })) {
    const srcPath = path.join(src, entry.name);
    const destPath = path.join(dest, entry.name);
    if (entry.isDirectory()) {
      copyDir(srcPath, destPath);
    } else if (entry.isSymbolicLink()) {
      const target = fs.readlinkSync(srcPath);
      fs.symlinkSync(target, destPath);
    } else {
      fs.copyFileSync(srcPath, destPath);
    }
  }
}

function ensureEngineReady() {
  if (!fs.existsSync(cliBuiltEntry)) {
    throw new Error(
      `missing built CLI entry: ${path.relative(repoRoot, cliBuiltEntry)}\nHint: run \`pnpm run build\` before scaffold sync check.`,
    );
  }
  if (!fs.existsSync(rustLauncher)) {
    throw new Error(
      `missing launcher: ${path.relative(repoRoot, rustLauncher)}`,
    );
  }
  fs.chmodSync(rustLauncher, 0o755);
  if (!fs.existsSync(engineCoreBin)) {
    console.log(
      "[scaffold-sync] engine-core not found; building cargo target...",
    );
    run("cargo", ["build", "-p", "engine-core"]);
  }
}

function normalizeEol(text) {
  return text.replace(/\r\n/g, "\n");
}

function regenerateMainGo(projectRootRel) {
  const absSourceProject = path.join(repoRoot, projectRootRel);
  const tmpRoot = fs.mkdtempSync(
    path.join(os.tmpdir(), "tsgodown-scaffold-sync-"),
  );
  const absTmpProject = path.join(tmpRoot, path.basename(projectRootRel));

  try {
    copyDir(absSourceProject, absTmpProject);

    const env = {
      ...process.env,
      TSGODOWN_RUST_ENGINE_BIN: rustLauncher,
      TSGODOWN_ENGINE_CORE_BIN: engineCoreBin,
    };

    const build = spawnSync("node", [cliBuiltEntry, "build", "--json"], {
      cwd: absTmpProject,
      encoding: "utf8",
      env,
    });

    if (build.status !== 0) {
      throw new Error(
        [
          `failed to regenerate scaffold for ${projectRootRel} (exit=${build.status ?? "null"})`,
          build.stdout ? `stdout:\n${build.stdout}` : "",
          build.stderr ? `stderr:\n${build.stderr}` : "",
        ]
          .filter(Boolean)
          .join("\n\n"),
      );
    }

    const outPath = path.join(absTmpProject, "dist-go", "main.go");
    if (!fs.existsSync(outPath)) {
      throw new Error(
        `build did not emit dist-go/main.go for ${projectRootRel}`,
      );
    }
    return fs.readFileSync(outPath, "utf8");
  } finally {
    fs.rmSync(tmpRoot, { recursive: true, force: true });
  }
}

function main() {
  ensureEngineReady();

  const trackedDistGoFiles = listTrackedDistGoFiles().filter(isScaffoldScope);
  const disallowedTracked = trackedDistGoFiles.filter(
    (file) => path.basename(file) !== "main.go",
  );

  const failures = [];

  if (disallowedTracked.length > 0) {
    failures.push(
      [
        "Tracked dist-go artifacts must only include dist-go/main.go in scaffold-managed examples/fixtures.",
        ...disallowedTracked.map((file) => `  - ${file}`),
      ].join("\n"),
    );
  }

  const trackedMainGoFiles = trackedDistGoFiles.filter(
    (file) => path.basename(file) === "main.go",
  );

  for (const mainGoRel of trackedMainGoFiles) {
    const projectRootRel = path.dirname(path.dirname(mainGoRel));
    const configPath = path.join(
      repoRoot,
      projectRootRel,
      "tsgodown.config.ts",
    );
    if (!fs.existsSync(configPath)) {
      failures.push(
        `${mainGoRel}: expected project config missing (${path.relative(repoRoot, configPath)})`,
      );
      continue;
    }

    const expected = fs.readFileSync(path.join(repoRoot, mainGoRel), "utf8");
    const regenerated = regenerateMainGo(projectRootRel);
    if (normalizeEol(expected) === normalizeEol(regenerated)) {
      continue;
    }

    if (writeMode) {
      fs.writeFileSync(path.join(repoRoot, mainGoRel), regenerated, "utf8");
      console.log(`[scaffold-sync] updated ${mainGoRel}`);
      continue;
    }

    failures.push(
      [
        `${mainGoRel} is out of sync with regenerated scaffold output.`,
        "Hint: run `node scripts/check-scaffold-sync.mjs --write` and commit the updated scaffold.",
      ].join("\n"),
    );
  }

  if (failures.length > 0) {
    console.error("✖ Scaffold regeneration/sync check failed.\n");
    for (const failure of failures) {
      console.error(`- ${failure}\n`);
    }
    process.exit(1);
  }

  console.log(
    `✔ Scaffold sync is valid (${trackedMainGoFiles.length} tracked dist-go/main.go target${trackedMainGoFiles.length === 1 ? "" : "s"}).`,
  );
}

main();
