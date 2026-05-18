#!/usr/bin/env node

import { spawnSync } from "node:child_process";
import fs from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";

const __filename = fileURLToPath(import.meta.url);
const __dirname = path.dirname(__filename);
const repoRoot = path.resolve(__dirname, "..");
const defaultEngineCoreBin = path.join(
  repoRoot,
  "target",
  "debug",
  "engine-core",
);
const generatedModulePath = "example.com/tsgodown-generated";

function fail(message, details, fixHint) {
  process.stderr.write(`[rust-engine-launcher] cause: ${message}\n`);
  if (details) {
    process.stderr.write(`[rust-engine-launcher] details: ${details}\n`);
  }
  if (fixHint) {
    process.stderr.write(`[rust-engine-launcher] fix: ${fixHint}\n`);
  }
  process.exit(1);
}

async function readStdinText() {
  const chunks = [];
  for await (const chunk of process.stdin) {
    chunks.push(typeof chunk === "string" ? Buffer.from(chunk) : chunk);
  }
  return Buffer.concat(chunks).toString("utf8").trim();
}

function ensureExecutable(filePath, label) {
  if (!fs.existsSync(filePath)) {
    fail(
      `${label} not found at: ${filePath}`,
      "required executable does not exist",
      "Build it first: cargo build -p engine-core (or set TSGODOWN_ENGINE_CORE_BIN to a valid binary)",
    );
  }
  try {
    fs.accessSync(filePath, fs.constants.X_OK);
  } catch {
    fail(
      `${label} is not executable: ${filePath}`,
      "found path but execute permission check failed",
      `Fix permissions: chmod +x ${JSON.stringify(filePath)}`,
    );
  }
}

const stdin = await readStdinText();
if (!stdin) {
  fail(
    "expected JSON request on stdin",
    "launcher stdin was empty",
    "Ensure CLI invokes this script via TSGODOWN_RUST_ENGINE_BIN and does not swallow stdin",
  );
}

let request;
try {
  request = JSON.parse(stdin);
} catch (error) {
  fail(
    "invalid JSON on stdin",
    error instanceof Error ? error.message : String(error),
    "Pass a valid JSON object. Expected shape: { action: 'build', cwd: '<project-root>' }",
  );
}

if (!request || request.action !== "build" || typeof request.cwd !== "string") {
  fail(
    "invalid request envelope",
    "Expected: { action: 'build', cwd: '<project-root>', configPath?: string }",
    "If you are calling the launcher manually, include action='build' and cwd=<absolute-or-relative-project-path>",
  );
}

const engineCoreBin =
  process.env.TSGODOWN_ENGINE_CORE_BIN || defaultEngineCoreBin;
ensureExecutable(engineCoreBin, "engine-core binary");

const emit = spawnSync(engineCoreBin, ["emit-go"], {
  input: JSON.stringify({
    analyze: {
      manifest: {
        entry: "src/index.ts",
      },
      cwd: request.cwd,
      config: {},
    },
    packageName: "main",
    modulePath: generatedModulePath,
    outputKind: "main",
  }),
  encoding: "utf8",
  maxBuffer: 64 * 1024 * 1024,
});

if (emit.status !== 0) {
  fail(
    `engine-core emit-go failed (exit=${emit.status ?? "null"})`,
    emit.stderr?.trim() ||
      emit.stdout?.trim() ||
      "No error output from engine-core",
    "Run cargo build -p engine-core, then retry. If it still fails, run the same command manually to inspect emitter diagnostics",
  );
}

let emitJson;
try {
  emitJson = JSON.parse(emit.stdout || "{}");
} catch (error) {
  fail(
    "engine-core emit-go stdout is not valid JSON",
    error instanceof Error ? error.message : String(error),
    "Verify engine-core emit-go prints JSON to stdout and logs diagnostics to stderr",
  );
}

const outDir = path.join(request.cwd, "dist-go");
fs.mkdirSync(outDir, { recursive: true });
for (const file of emitJson?.files ?? []) {
  if (typeof file?.path !== "string" || typeof file?.contents !== "string") {
    continue;
  }
  const outputPath = path.join(outDir, file.path);
  fs.mkdirSync(path.dirname(outputPath), { recursive: true });
  fs.writeFileSync(outputPath, file.contents, "utf8");
}

const diagnostics = [
  "engine=rust-engine-core",
  ...(Array.isArray(emitJson?.diagnostics)
    ? emitJson.diagnostics
        .map((entry) =>
          entry && typeof entry.code === "string"
            ? `engine-core:${entry.code}`
            : null,
        )
        .filter(Boolean)
    : []),
];

process.stdout.write(
  JSON.stringify({
    ok: true,
    diagnostics,
    manifest: {
      buildId: "1122334455667788",
      entries: ["src/index.ts"],
      bundles: [
        {
          file: "dist/index.mjs",
          map: "dist/index.mjs.map",
          format: "esm",
          exports: [],
        },
      ],
      types: ["dist/index.d.ts"],
      tsconfigPath: "tsconfig.json",
    },
  }),
);
