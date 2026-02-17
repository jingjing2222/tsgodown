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

function toGoPattern(routePath) {
  return routePath.replace(/:([A-Za-z_][\w]*)/g, "{$1}");
}

function extractPathParamNames(routePath) {
  return [...routePath.matchAll(/:([A-Za-z_][\w]*)/g)].map((m) => m[1]);
}

function toGoBindingName(name, index) {
  const base = name.replace(/[^A-Za-z0-9_]/g, "_");
  const normalized = /^[A-Za-z_]/.test(base) ? base : `param_${base}`;
  return index === 0 ? normalized : `${normalized}${index + 1}`;
}

function renderGoMainScaffold(routes) {
  return [
    "package main",
    "",
    "import (",
    '\t"fmt"',
    '\t"net/http"',
    '\t"os"',
    ")",
    "",
    "func resolveListenAddr() string {",
    '\tport := os.Getenv("PORT")',
    '\tif port == "" {',
    '\t\tport = "8080"',
    "\t}",
    '\treturn ":" + port',
    "}",
    "",
    "func main() {",
    '\tfmt.Println("tsgodown-fastify-runtime-ready")',
    "\tmux := http.NewServeMux()",
    ...routes.flatMap((route) => {
      const pathParams = extractPathParamNames(route.path);
      const pathParamBindings = pathParams.flatMap((name, index) => {
        const bindingName = toGoBindingName(name, index);
        return [
          `\t\t${bindingName} := req.PathValue(${JSON.stringify(name)})`,
          `\t\t_ = ${bindingName}`,
        ];
      });

      return [
        `\tmux.HandleFunc("${route.method} ${route.goPattern}", func(w http.ResponseWriter, req *http.Request) {`,
        ...pathParamBindings,
        "\t\tw.WriteHeader(http.StatusNotImplemented)",
        `\t\tfmt.Fprintln(w, "TODO implement handler ${route.handler} for ${route.method} ${route.path}")`,
        "\t})",
      ];
    }),
    "\t_ = http.ListenAndServe(resolveListenAddr(), mux)",
    "}",
    "",
  ].join("\n");
}

function parseRoutesFromSource(source) {
  const routeMatches = [
    ...source.matchAll(
      /\b(?:fastify|app)\.(get|post|put|patch|delete|options|head)\(\s*['"`]([^'"`]+)['"`]\s*,\s*([A-Za-z_$][\w$]*)/g,
    ),
  ];

  const routes = routeMatches.map((match) => ({
    method: match[1].toUpperCase(),
    path: match[2],
    goPattern: toGoPattern(match[2]),
    handler: match[3],
  }));

  if (routes.length > 0) {
    return routes;
  }

  return [
    {
      method: "GET",
      path: "/health",
      goPattern: "/health",
      handler: "health",
    },
  ];
}

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
const entryPath = path.resolve(request.cwd, request.entry ?? "src/index.ts");
const source = fs.existsSync(entryPath)
  ? fs.readFileSync(entryPath, "utf8")
  : "";
const routes = parseRoutesFromSource(source);
fs.writeFileSync(
  path.join(outDir, "main.go"),
  `${renderGoMainScaffold(routes)}\n`,
  "utf8",
);

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
