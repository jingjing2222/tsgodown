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
    maxBuffer: 64 * 1024 * 1024,
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

function analyzeEntry(testCase, entry) {
  const analyze = run(engineCoreBin, ["analyze"], {
    input: JSON.stringify({
      manifest: {
        entry,
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

function emitGoEntry(testCase, entry, outputKind = "main") {
  const emit = run(engineCoreBin, ["emit-go"], {
    input: JSON.stringify({
      analyze: {
        manifest: {
          entry,
        },
        cwd: corpusRoot,
        config: {},
      },
      packageName: "main",
      modulePath: goModulePath(testCase),
      outputKind,
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

function emittedFileContents(testCase, emitGoJson, fileName) {
  const file = (emitGoJson?.files ?? []).find(
    (item) => item?.path === fileName,
  );
  if (typeof file?.contents !== "string" || file.contents.length === 0) {
    fail(`engine-core emit-go did not return ${fileName} for ${testCase.id}`);
  }
  return file.contents;
}

function goModulePath(testCase) {
  return `example.com/tsgodown-node-corpus/${testCase.id}`;
}

function renderGoMod(testCase) {
  return [goModulePath(testCase), "", "go 1.22", ""]
    .map((line, index) => (index === 0 ? `module ${line}` : line))
    .join("\n");
}

function renderSourceIrGo(analyzeJson) {
  return renderIrGoConst(
    analyzeJson,
    "sourceIRJSON",
    "sourceIRJSON is the analyzer-lowered executable JS IR for this corpus package entry.",
  );
}

function renderProbeIrGo(analyzeJson) {
  return renderIrGoConst(
    analyzeJson,
    "probeIRJSON",
    "probeIRJSON is the analyzer-lowered executable JS IR for this corpus probe app.",
  );
}

function renderIrGoConst(analyzeJson, constName, comment) {
  const ir = analyzeJson?.ir ?? {};
  const sourceIr = {
    version: ir.version ?? "unknown",
    entry: ir.entry ?? "",
    modules: Array.isArray(ir.modules)
      ? ir.modules.map((module) => ({
          id: module?.id ?? "",
          sourcePath: module?.sourcePath ?? "",
          imports: Array.isArray(module?.imports) ? module.imports : [],
          exports: Array.isArray(module?.exports) ? module.exports : [],
          executable: module?.executable ?? { stmts: [] },
        }))
      : [],
    diagnostics: Array.isArray(analyzeJson?.diagnostics)
      ? analyzeJson.diagnostics
      : [],
  };
  const json = JSON.stringify(sourceIr, null, 2);

  return [
    "package main",
    "",
    `// ${comment}`,
    "// It is emitted into generated Go projects so codegen can be driven by source semantics.",
    `const ${constName} = ${goRawString(json)}`,
    "",
  ].join("\n");
}

function goRawString(value) {
  return `\`${String(value).replaceAll("`", '` + "`" + `')}\``;
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
    if (stmt.kind === "if") {
      conditionals += 1;
      for (const child of stmt.consequent ?? []) {
        visitStmt(child);
      }
      for (const child of stmt.alternate ?? []) {
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
      visitExpr(child);
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

function generateCase(testCase) {
  if (!testCase.entry) {
    fail(`manifest case ${testCase.id} is missing entry`);
  }

  const analyzeJson = analyzeEntry(testCase, testCase.entry);
  const probeAnalyzeJson = analyzeEntry(testCase, testCase.probe);
  const emitGoJson = emitGoEntry(testCase, testCase.probe);
  const vectorEmitGoJson = emitGoEntry(
    testCase,
    "tests/vector-suite-entry.mjs",
    "vectorSuite",
  );
  const vectorAnalyzeJson = analyzeEntry(
    testCase,
    "tests/vector-suite-entry.mjs",
  );
  const outDir = path.join(generatedRoot, testCase.id);
  fs.mkdirSync(outDir, { recursive: true });
  fs.writeFileSync(path.join(outDir, "go.mod"), renderGoMod(testCase), "utf8");
  writeEmitGoFiles(testCase, outDir, emitGoJson, "main.go");
  fs.writeFileSync(
    path.join(outDir, "source_ir.go"),
    renderSourceIrGo(analyzeJson),
    "utf8",
  );
  fs.writeFileSync(
    path.join(outDir, "probe_ir.go"),
    renderProbeIrGo(probeAnalyzeJson),
    "utf8",
  );
  writeEmitGoFiles(testCase, outDir, vectorEmitGoJson, "vector_suite.go");

  return {
    id: testCase.id,
    path: path.relative(repoRoot, outDir),
    modules: analyzeJson?.ir?.modules?.length ?? 0,
    diagnostics: analyzeJson?.diagnostics?.length ?? 0,
    emitGoDiagnostics: emitGoJson?.diagnostics?.length ?? 0,
    vectorEmitGoDiagnostics: vectorEmitGoJson?.diagnostics?.length ?? 0,
    executableIr: executableIrStats(analyzeJson),
    probeExecutableIr: executableIrStats(probeAnalyzeJson),
    vectorExecutableIr: executableIrStats(vectorAnalyzeJson),
  };
}

function writeEmitGoFiles(testCase, outDir, emitGoJson, requiredFile) {
  emittedFileContents(testCase, emitGoJson, requiredFile);
  for (const file of emitGoJson?.files ?? []) {
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
