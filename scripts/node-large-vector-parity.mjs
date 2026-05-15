#!/usr/bin/env node

import { spawnSync } from "node:child_process";
import crypto from "node:crypto";
import fs from "node:fs";
import path from "node:path";

const REPORT_VERSION = "node-large-vector-parity.v1";
const repoRoot = path.resolve(import.meta.dirname, "..");
const corpusRoot = path.join(repoRoot, "test-corpus", "node-large");
const manifest = JSON.parse(
  fs.readFileSync(path.join(corpusRoot, "manifest.json"), "utf8"),
);
const generatedRoot =
  process.env.TSGODOWN_NODE_LARGE_GO_ROOT ??
  path.join(corpusRoot, "generated-go");
const buildRoot = path.join(repoRoot, ".tmp", "node-large-vector-parity");
let generated = false;

function run(cmd, args, options = {}) {
  return spawnSync(cmd, args, {
    cwd: repoRoot,
    encoding: "utf8",
    stdio: ["ignore", "pipe", "pipe"],
    maxBuffer: 256 * 1024 * 1024,
    ...options,
  });
}

function stableStringify(value) {
  if (Array.isArray(value)) return `[${value.map(stableStringify).join(",")}]`;
  if (value && typeof value === "object") {
    return `{${Object.keys(value)
      .sort()
      .map((key) => `${JSON.stringify(key)}:${stableStringify(value[key])}`)
      .join(",")}}`;
  }
  return JSON.stringify(value);
}

function digest(value) {
  return crypto
    .createHash("sha256")
    .update(stableStringify(value))
    .digest("hex");
}

function runNodeVectorSuite(entry) {
  const vectorPath = path.join(corpusRoot, "cases", entry.id, "vectors.json");
  if (!fs.existsSync(vectorPath)) {
    return {
      status: "blocked",
      vectors: 0,
      requiredVectors: entry.vectors.expected,
      reason: "100 Vitest vectors not implemented yet",
    };
  }
  const result = run(
    "node",
    ["tests/vector-suite-entry.mjs", entry.id, vectorPath],
    { cwd: corpusRoot },
  );
  const report = {
    command: `node tests/vector-suite-entry.mjs ${entry.id} cases/${entry.id}/vectors.json`,
    status: result.status === 0 ? "passed" : "failed",
    exitCode: result.status,
    stderr: result.stderr,
  };
  if (result.status !== 0) return { ...report, stdout: result.stdout };
  try {
    const json = JSON.parse(result.stdout);
    return {
      ...report,
      total: json.total,
      digest: digest(json),
      json,
    };
  } catch (error) {
    return {
      ...report,
      status: "failed",
      stdout: result.stdout,
      parseError: error instanceof Error ? error.message : String(error),
    };
  }
}

function runGoVectorSuite(entry) {
  ensureGeneratedGo();
  const goDir = path.join(generatedRoot, entry.id);
  const vectorSuite = path.join(goDir, "vector_suite.go");
  if (!fs.existsSync(vectorSuite)) {
    return {
      build: { status: "blocked", reason: "generated Go vector suite missing" },
      run: { status: "blocked", reason: "generated Go vector suite missing" },
    };
  }

  const outDir = path.join(buildRoot, entry.id);
  fs.mkdirSync(outDir, { recursive: true });
  const binaryPath = path.join(outDir, "vector-suite");
  const build = run(
    "go",
    ["build", "-tags", "tsgodown_vector", "-o", binaryPath, "vector_suite.go"],
    { cwd: goDir },
  );
  const buildReport = {
    command: `go build -tags tsgodown_vector -o ${path.relative(repoRoot, binaryPath)} vector_suite.go`,
    cwd: path.relative(repoRoot, goDir),
    status: build.status === 0 ? "passed" : "failed",
    exitCode: build.status,
    stdout: build.stdout,
    stderr: build.stderr,
  };
  if (build.status !== 0) {
    return { build: buildReport, run: { status: "skipped" } };
  }

  const vectorPath = path.join(corpusRoot, "cases", entry.id, "vectors.json");
  const result = run(binaryPath, [entry.id, vectorPath], { cwd: goDir });
  const runReport = {
    command: `${path.relative(repoRoot, binaryPath)} ${entry.id} ${path.relative(
      repoRoot,
      vectorPath,
    )}`,
    status: result.status === 0 ? "passed" : "failed",
    exitCode: result.status,
    stderr: result.stderr,
  };
  if (result.status !== 0) {
    return {
      build: buildReport,
      run: { ...runReport, stdout: result.stdout },
    };
  }
  try {
    const json = JSON.parse(result.stdout);
    return {
      build: buildReport,
      run: {
        ...runReport,
        total: json.total,
        digest: digest(json),
        json,
      },
    };
  } catch (error) {
    return {
      build: buildReport,
      run: {
        ...runReport,
        status: "failed",
        stdout: result.stdout,
        parseError: error instanceof Error ? error.message : String(error),
      },
    };
  }
}

function ensureGeneratedGo() {
  if (
    generated ||
    manifest.entries.every((entry) =>
      fs.existsSync(path.join(generatedRoot, entry.id, "vector_suite.go")),
    )
  ) {
    generated = true;
    return;
  }
  const result = run("node", ["scripts/generate-node-large-go.mjs"]);
  if (result.status !== 0) {
    console.error(result.stderr || result.stdout);
  }
  generated = true;
}

function stripGoJson(report) {
  if (!report) return report;
  return {
    build: report.build,
    run: stripJson(report.run),
  };
}

const cases = manifest.entries.map((entry) => {
  const node = runNodeVectorSuite(entry);
  const go = runGoVectorSuite(entry);
  const parity =
    node.status === "passed" &&
    go.build.status === "passed" &&
    go.run.status === "passed" &&
    stableStringify(node.json) === stableStringify(go.run.json);
  return {
    id: entry.id,
    package: entry.package,
    node: stripJson(node),
    go: stripGoJson(go),
    parity: parity ? { status: "passed" } : { status: "blocked" },
  };
});

const summary = {
  total: cases.length,
  vectorsRequired: cases.reduce(
    (sum, entry) =>
      sum +
      (entry.node.requiredVectors ??
        entry.node.total ??
        manifest.policy.vectorsPerEntry),
    0,
  ),
  vectorsPresent: cases.reduce(
    (sum, entry) => sum + (entry.node.total ?? 0),
    0,
  ),
  nodePassed: cases.filter((entry) => entry.node.status === "passed").length,
  goBuildPassed: cases.filter((entry) => entry.go.build.status === "passed")
    .length,
  goRunPassed: cases.filter((entry) => entry.go.run.status === "passed").length,
  parityPassed: cases.filter((entry) => entry.parity.status === "passed")
    .length,
};

const report = {
  version: REPORT_VERSION,
  status: summary.parityPassed === summary.total ? "passed" : "blocked",
  nodeLts: manifest.nodeLts,
  summary,
  cases,
};

console.log(JSON.stringify(report, null, 2));
if (report.status !== "passed") {
  process.exit(1);
}

function stripJson(report) {
  if (!report || !("json" in report)) return report;
  const { json: _json, ...rest } = report;
  return rest;
}
