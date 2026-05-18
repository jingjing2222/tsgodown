#!/usr/bin/env node

import { spawnSync } from "node:child_process";
import fs from "node:fs";
import path from "node:path";
import { differentialFuzzCases } from "./differential-fuzz-cases.mjs";

const repoRoot = path.resolve(import.meta.dirname, "..");
const corpusRoot = path.join(repoRoot, ".tmp", "differential-fuzz-cases");
const generatedRoot =
  process.env.TSGODOWN_DIFFERENTIAL_FUZZ_GO_ROOT ??
  path.join(repoRoot, "test-corpus", "differential-fuzz", "generated-go");
const engineCoreBin =
  process.env.TSGODOWN_ENGINE_CORE_BIN ??
  path.join(repoRoot, "target", "debug", "engine-core");

function run(cmd, args, options = {}) {
  return spawnSync(cmd, args, {
    cwd: repoRoot,
    encoding: "utf8",
    maxBuffer: 64 * 1024 * 1024,
    stdio:
      options.input === undefined
        ? ["ignore", "pipe", "pipe"]
        : ["pipe", "pipe", "pipe"],
    ...options,
  });
}

function fail(message, details) {
  process.stderr.write(`[differential-fuzz-go] ${message}\n`);
  if (details) process.stderr.write(`${details.trim()}\n`);
  process.exit(1);
}

function ensureEngineCore() {
  if (process.env.TSGODOWN_ENGINE_CORE_BIN && fs.existsSync(engineCoreBin)) {
    return;
  }
  const build = run("cargo", ["build", "-p", "engine-core"]);
  if (build.status !== 0) {
    fail("failed to build engine-core", build.stderr || build.stdout);
  }
  if (!fs.existsSync(engineCoreBin)) {
    fail(`engine-core binary was not produced at ${engineCoreBin}`);
  }
}

function writeSource(testCase) {
  const outDir = path.join(corpusRoot, testCase.id);
  fs.mkdirSync(outDir, { recursive: true });
  fs.writeFileSync(path.join(outDir, "index.mjs"), testCase.source, "utf8");
}

function emitGo(testCase) {
  const emit = run(engineCoreBin, ["emit-go"], {
    input: JSON.stringify({
      analyze: {
        manifest: {
          entry: `${testCase.id}/index.mjs`,
        },
        cwd: corpusRoot,
        config: {},
      },
      packageName: "main",
      modulePath: `example.com/tsgodown-differential-fuzz/${testCase.id}`,
      outputKind: "main",
    }),
  });
  if (emit.status !== 0) {
    fail(
      `engine-core emit-go failed for ${testCase.id}`,
      emit.stderr || emit.stdout,
    );
  }
  try {
    return JSON.parse(emit.stdout);
  } catch (error) {
    fail(
      `engine-core emit-go emitted invalid JSON for ${testCase.id}`,
      error instanceof Error ? error.message : String(error),
    );
  }
}

function writeEmitGoFiles(testCase, emitGoJson) {
  const outDir = path.join(generatedRoot, testCase.id);
  const files = Array.isArray(emitGoJson?.files) ? emitGoJson.files : [];
  if (!files.some((file) => file?.path === "main.go")) {
    fail(`engine-core emit-go did not return main.go for ${testCase.id}`);
  }
  fs.mkdirSync(outDir, { recursive: true });
  for (const file of files) {
    if (typeof file?.path !== "string" || typeof file?.contents !== "string") {
      continue;
    }
    const outPath = path.join(outDir, file.path);
    fs.mkdirSync(path.dirname(outPath), { recursive: true });
    fs.writeFileSync(outPath, file.contents, "utf8");
  }
}

function main() {
  ensureEngineCore();
  fs.rmSync(corpusRoot, { recursive: true, force: true });
  fs.rmSync(generatedRoot, { recursive: true, force: true });
  fs.mkdirSync(corpusRoot, { recursive: true });
  fs.mkdirSync(generatedRoot, { recursive: true });

  const cases = differentialFuzzCases().map((testCase) => {
    writeSource(testCase);
    const emitGoJson = emitGo(testCase);
    writeEmitGoFiles(testCase, emitGoJson);
    return {
      id: testCase.id,
      group: testCase.group,
      capability: testCase.capability,
      diagnostics: emitGoJson?.diagnostics?.length ?? 0,
      path: path.relative(repoRoot, path.join(generatedRoot, testCase.id)),
    };
  });

  process.stdout.write(
    `${JSON.stringify({
      version: "differential-fuzz-go-generator.v1",
      generatedRoot: path.relative(repoRoot, generatedRoot),
      cases,
    })}\n`,
  );
  fs.rmSync(corpusRoot, { recursive: true, force: true });
}

main();
