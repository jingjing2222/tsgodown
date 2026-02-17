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

const GO_MAIN_SCAFFOLD = [
  "package main",
  "",
  "import (",
  '\t"fmt"',
  '\t"net/http"',
  ")",
  "",
  "func main() {",
  '\tfmt.Println("tsgodown-fastify-min-ready")',
  '\thttp.HandleFunc("GET /health", func(w http.ResponseWriter, _ *http.Request) {',
  "\t\tw.WriteHeader(http.StatusNotImplemented)",
  '\t\tfmt.Fprintln(w, "TODO implement handler health for GET /health")',
  "\t})",
  '\t_ = http.ListenAndServe(":8080", nil)',
  "}",
  "",
].join("\n");

function fail(message, details) {
  process.stderr.write(`[rust-engine-launcher] ${message}\n`);
  if (details) {
    process.stderr.write(`${details}\n`);
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
      "Build it first: cargo build -p engine-core",
    );
  }
  try {
    fs.accessSync(filePath, fs.constants.X_OK);
  } catch {
    fail(
      `${label} is not executable: ${filePath}`,
      `Fix permissions: chmod +x ${JSON.stringify(filePath)}`,
    );
  }
}

const stdin = await readStdinText();
if (!stdin) {
  fail(
    "expected JSON request on stdin",
    "Launcher contract requires a build request JSON payload.",
  );
}

let request;
try {
  request = JSON.parse(stdin);
} catch (error) {
  fail(
    "invalid JSON on stdin",
    error instanceof Error ? error.message : String(error),
  );
}

if (!request || request.action !== "build" || typeof request.cwd !== "string") {
  fail(
    "invalid request envelope",
    "Expected: { action: 'build', cwd: '<project-root>', configPath?: string }",
  );
}

const engineCoreBin =
  process.env.TSGODOWN_ENGINE_CORE_BIN || defaultEngineCoreBin;
ensureExecutable(engineCoreBin, "engine-core binary");

const analyzeRequest = {
  manifest: {
    entry: "src/index.ts",
    framework: "fastify",
  },
  config: {},
};

const analyze = spawnSync(engineCoreBin, ["analyze"], {
  input: JSON.stringify(analyzeRequest),
  encoding: "utf8",
});

if (analyze.status !== 0) {
  fail(
    `engine-core analyze failed (exit=${analyze.status ?? "null"})`,
    analyze.stderr?.trim() ||
      analyze.stdout?.trim() ||
      "No error output from engine-core",
  );
}

let analyzeJson;
try {
  analyzeJson = JSON.parse(analyze.stdout || "{}");
} catch (error) {
  fail(
    "engine-core analyze stdout is not valid JSON",
    error instanceof Error ? error.message : String(error),
  );
}

const outDir = path.join(request.cwd, "dist-go");
fs.mkdirSync(outDir, { recursive: true });
fs.writeFileSync(path.join(outDir, "main.go"), GO_MAIN_SCAFFOLD, "utf8");

const diagnostics = [
  "engine=rust-engine-core",
  ...(Array.isArray(analyzeJson?.diagnostics)
    ? analyzeJson.diagnostics
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
