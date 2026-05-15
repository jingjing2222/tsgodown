#!/usr/bin/env node

import { spawnSync } from "node:child_process";
import fs from "node:fs";
import path from "node:path";

const repoRoot = path.resolve(import.meta.dirname, "..");
const corpusRoot = path.join(repoRoot, "test-corpus", "node-large");
const manifestPath = path.join(corpusRoot, "manifest.json");
const generatedRoot =
  process.env.TSGODOWN_NODE_LARGE_GO_ROOT ??
  path.join(corpusRoot, "generated-go");
const engineCoreBin =
  process.env.TSGODOWN_ENGINE_CORE_BIN ??
  path.join(repoRoot, "target", "debug", "engine-core");

function run(cmd, args, options = {}) {
  return spawnSync(cmd, args, {
    cwd: repoRoot,
    encoding: "utf8",
    maxBuffer: 256 * 1024 * 1024,
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
  process.stderr.write(`[node-large-go] ${message}\n`);
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

function analyzeEntry(entry, sourceEntry) {
  const analyze = run(engineCoreBin, ["analyze"], {
    input: JSON.stringify({
      manifest: { entry: sourceEntry },
      cwd: corpusRoot,
      config: {},
    }),
  });

  if (analyze.status !== 0) {
    fail(
      `engine-core analyze failed for ${entry.id}`,
      analyze.stderr || analyze.stdout,
    );
  }

  try {
    return JSON.parse(analyze.stdout);
  } catch (error) {
    fail(
      `engine-core analyze emitted invalid JSON for ${entry.id}`,
      error instanceof Error ? error.message : String(error),
    );
  }
}

function emitGoEntry(entry, sourceEntry, options = {}) {
  const emit = run(engineCoreBin, ["emit-go"], {
    input: JSON.stringify({
      analyze: {
        manifest: { entry: sourceEntry },
        cwd: corpusRoot,
        config: {},
      },
      packageName: "main",
      modulePath: `example.com/tsgodown-node-large/${entry.id}`,
      outputKind: options.outputKind ?? "main",
      ...(options.irSnapshot ? { irSnapshot: options.irSnapshot } : {}),
    }),
  });

  if (emit.status !== 0) {
    fail(
      `engine-core emit-go failed for ${entry.id}`,
      emit.stderr || emit.stdout,
    );
  }

  try {
    return JSON.parse(emit.stdout);
  } catch (error) {
    fail(
      `engine-core emit-go emitted invalid JSON for ${entry.id}`,
      error instanceof Error ? error.message : String(error),
    );
  }
}

function emittedFileContents(entry, emitGoJson, fileName) {
  const file = (emitGoJson?.files ?? []).find(
    (item) => item?.path === fileName,
  );
  if (typeof file?.contents !== "string" || file.contents.length === 0) {
    fail(`engine-core emit-go did not return ${fileName} for ${entry.id}`);
  }
  return file.contents;
}

function writeSelectedEmitGoFile(entry, outDir, emitGoJson, fileName) {
  const contents = emittedFileContents(entry, emitGoJson, fileName);
  const outPath = path.join(outDir, fileName);
  fs.mkdirSync(path.dirname(outPath), { recursive: true });
  fs.writeFileSync(outPath, contents, "utf8");
}

function writeEmitGoFiles(entry, outDir, emitGoJson, requiredFile) {
  emittedFileContents(entry, emitGoJson, requiredFile);
  for (const file of emitGoJson?.files ?? []) {
    if (typeof file?.path !== "string" || typeof file?.contents !== "string") {
      continue;
    }
    const outPath = path.join(outDir, file.path);
    fs.mkdirSync(path.dirname(outPath), { recursive: true });
    fs.writeFileSync(outPath, file.contents, "utf8");
  }
}

function executableIrStats(analyzeJson) {
  const modules = Array.isArray(analyzeJson?.ir?.modules)
    ? analyzeJson.ir.modules
    : [];
  let statements = 0;
  let functions = 0;
  let conditionals = 0;

  function visitStmt(stmt) {
    if (!stmt || typeof stmt !== "object") {
      return;
    }
    statements += 1;
    visitExpr(stmt.expr);
    visitExpr(stmt.value);
    visitExpr(stmt.init);
    visitExpr(stmt.test);
    if (stmt.kind === "function-decl") {
      functions += 1;
      for (const child of stmt.body ?? []) {
        visitStmt(child);
      }
    }
    if (stmt.kind === "if" || stmt.kind === "switch") {
      conditionals += 1;
    }
    for (const child of stmt.consequent ?? []) {
      visitStmt(child);
    }
    for (const child of stmt.alternate ?? []) {
      visitStmt(child);
    }
    for (const branch of stmt.cases ?? []) {
      for (const child of branch.consequent ?? []) {
        visitStmt(child);
      }
    }
  }

  function visitExpr(expr) {
    if (!expr || typeof expr !== "object") {
      return;
    }
    if (expr.kind === "function") {
      functions += 1;
      for (const child of expr.body ?? []) {
        visitStmt(child);
      }
      return;
    }
    for (const child of expr.args ?? []) {
      visitExpr(child);
    }
    for (const child of expr.items ?? []) {
      visitExpr(child?.value ?? child);
    }
    for (const prop of expr.props ?? []) {
      visitExpr(prop?.value);
    }
    visitExpr(expr.arg);
    visitExpr(expr.left);
    visitExpr(expr.right);
    visitExpr(expr.callee);
    visitExpr(expr.object);
  }

  for (const module of modules) {
    for (const stmt of module?.executable?.stmts ?? []) {
      visitStmt(stmt);
    }
  }

  return { statements, functions, conditionals };
}

function generateEntry(entry) {
  const sourceAnalyzeJson = analyzeEntry(entry, entry.entry);
  const vectorAnalyzeJson = analyzeEntry(entry, "tests/vector-suite-entry.mjs");
  const sourceSnapshotEmitGoJson = emitGoEntry(entry, entry.entry, {
    irSnapshot: {
      filePath: "source_ir.go",
      constName: "sourceIRJSON",
      description:
        "sourceIRJSON is the analyzer-lowered executable JS IR for this large corpus package entry.",
    },
  });
  const vectorEmitGoJson = emitGoEntry(entry, "tests/vector-suite-entry.mjs", {
    outputKind: "vectorSuite",
    irSnapshot: {
      filePath: "vector_ir.go",
      constName: "vectorIRJSON",
      description:
        "vectorIRJSON is the analyzer-lowered executable JS IR for this large corpus vector runner.",
    },
  });

  const outDir = path.join(generatedRoot, entry.id);
  fs.mkdirSync(outDir, { recursive: true });
  writeSelectedEmitGoFile(
    entry,
    outDir,
    sourceSnapshotEmitGoJson,
    "source_ir.go",
  );
  writeEmitGoFiles(entry, outDir, vectorEmitGoJson, "vector_suite.go");

  return {
    id: entry.id,
    path: path.relative(repoRoot, outDir),
    sourceModules: sourceAnalyzeJson?.ir?.modules?.length ?? 0,
    vectorModules: vectorAnalyzeJson?.ir?.modules?.length ?? 0,
    sourceDiagnostics: sourceAnalyzeJson?.diagnostics?.length ?? 0,
    vectorDiagnostics: vectorAnalyzeJson?.diagnostics?.length ?? 0,
    emitGoDiagnostics: vectorEmitGoJson?.diagnostics?.length ?? 0,
    sourceExecutableIr: executableIrStats(sourceAnalyzeJson),
    vectorExecutableIr: executableIrStats(vectorAnalyzeJson),
  };
}

function main() {
  ensureEngineCore();
  const manifest = readJson(manifestPath);
  fs.rmSync(generatedRoot, { recursive: true, force: true });
  fs.mkdirSync(generatedRoot, { recursive: true });

  const entries = manifest.entries.map(generateEntry);
  process.stdout.write(
    `${JSON.stringify({
      version: "node-large-go-generator.v1",
      generatedRoot: path.relative(repoRoot, generatedRoot),
      entries,
    })}\n`,
  );
}

main();
