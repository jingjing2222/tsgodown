#!/usr/bin/env node

import { spawnSync } from "node:child_process";
import fs from "node:fs";
import path from "node:path";

const repoRoot = path.resolve(import.meta.dirname, "..");
const corpusRoot = path.join(repoRoot, "test-corpus", "node-real");
const manifestPath = path.join(corpusRoot, "manifest.json");
const generatedRoot =
  process.env.TSGODOWN_NODE_CORPUS_GO_ROOT ??
  path.join(corpusRoot, "generated-go");
const engineCoreBin =
  process.env.TSGODOWN_ENGINE_CORE_BIN ??
  path.join(repoRoot, "target", "debug", "engine-core");

function run(cmd, args, options = {}) {
  return spawnSync(cmd, args, {
    cwd: repoRoot,
    encoding: "utf8",
    stdio:
      options.input === undefined
        ? ["ignore", "pipe", "pipe"]
        : ["pipe", "pipe", "pipe"],
    ...options,
  });
}

function readJson(filePath) {
  return JSON.parse(fs.readFileSync(filePath, "utf8"));
}

function fail(message, details) {
  process.stderr.write(`[node-corpus-go] ${message}\n`);
  if (details) {
    process.stderr.write(`${details.trim()}\n`);
  }
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

function analyzeCase(testCase) {
  const analyze = run(engineCoreBin, ["analyze"], {
    input: JSON.stringify({
      manifest: {
        entry: testCase.entry,
      },
      cwd: corpusRoot,
      config: {},
    }),
  });

  if (analyze.status !== 0) {
    fail(
      `engine-core analyze failed for ${testCase.id}`,
      analyze.stderr || analyze.stdout,
    );
  }

  try {
    return JSON.parse(analyze.stdout);
  } catch (error) {
    fail(
      `engine-core analyze emitted invalid JSON for ${testCase.id}`,
      error instanceof Error ? error.message : String(error),
    );
  }
}

function goString(value) {
  return JSON.stringify(String(value));
}

function renderGoMain(testCase, analyzeJson) {
  const analyzerDiagnostics = Array.isArray(analyzeJson?.diagnostics)
    ? analyzeJson.diagnostics.map((diagnostic) => ({
        code: diagnostic?.code ?? "UNKNOWN",
        message: diagnostic?.message ?? "",
        source: diagnostic?.source ?? null,
      }))
    : [];
  const modules = Array.isArray(analyzeJson?.ir?.modules)
    ? analyzeJson.ir.modules
    : [];
  const report = {
    package: testCase.package,
    status: "unsupported",
    diagnostics: [
      {
        code: "EXECUTABLE_IR_NOT_IMPLEMENTED",
        message:
          "Generated Go project is fail-closed until executable JS semantics lowering and codegen are implemented.",
      },
      ...analyzerDiagnostics,
    ],
    analyzer: {
      entry: testCase.entry,
      modules: modules.length,
      imports: modules.reduce(
        (count, module) =>
          count + (Array.isArray(module?.imports) ? module.imports.length : 0),
        0,
      ),
      exports: modules.reduce(
        (count, module) =>
          count + (Array.isArray(module?.exports) ? module.exports.length : 0),
        0,
      ),
    },
  };
  const json = JSON.stringify(report);

  return [
    "package main",
    "",
    "import (",
    '\t"fmt"',
    '\t"os"',
    ")",
    "",
    "func main() {",
    `\tfmt.Println(${goString(json)})`,
    "\tos.Exit(1)",
    "}",
    "",
  ].join("\n");
}

function renderGoMod(testCase) {
  return [
    `module example.com/tsgodown-node-corpus/${testCase.id}`,
    "",
    "go 1.22",
    "",
  ].join("\n");
}

function generateCase(testCase) {
  if (!testCase.entry) {
    fail(`manifest case ${testCase.id} is missing entry`);
  }

  const analyzeJson = analyzeCase(testCase);
  const outDir = path.join(generatedRoot, testCase.id);
  fs.mkdirSync(outDir, { recursive: true });
  fs.writeFileSync(path.join(outDir, "go.mod"), renderGoMod(testCase), "utf8");
  fs.writeFileSync(
    path.join(outDir, "main.go"),
    renderGoMain(testCase, analyzeJson),
    "utf8",
  );

  return {
    id: testCase.id,
    path: path.relative(repoRoot, outDir),
    modules: analyzeJson?.ir?.modules?.length ?? 0,
    diagnostics: analyzeJson?.diagnostics?.length ?? 0,
  };
}

function main() {
  ensureEngineCore();
  const manifest = readJson(manifestPath);
  fs.rmSync(generatedRoot, { recursive: true, force: true });
  fs.mkdirSync(generatedRoot, { recursive: true });

  const cases = manifest.cases.map(generateCase);
  process.stdout.write(
    `${JSON.stringify({
      version: "node-corpus-go-generator.v1",
      generatedRoot: path.relative(repoRoot, generatedRoot),
      cases,
    })}\n`,
  );
}

main();
