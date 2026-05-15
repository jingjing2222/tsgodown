#!/usr/bin/env node

import { spawnSync } from "node:child_process";
import crypto from "node:crypto";
import fs from "node:fs";
import path from "node:path";

const REPORT_VERSION = "node-corpus-vector-parity.v1";
const repoRoot = path.resolve(import.meta.dirname, "..");
const corpusRoot = path.join(repoRoot, "test-corpus", "node-real");
const manifest = JSON.parse(
  fs.readFileSync(path.join(corpusRoot, "manifest.json"), "utf8"),
);
const generatedRoot =
  process.env.TSGODOWN_NODE_CORPUS_GO_ROOT ??
  path.join(corpusRoot, "generated-go");
const buildRoot = path.join(repoRoot, ".tmp", "node-corpus-vector-parity");

function hasFlag(flag) {
  return process.argv.includes(flag);
}

function run(cmd, args, options = {}) {
  return spawnSync(cmd, args, {
    cwd: repoRoot,
    encoding: "utf8",
    stdio: ["ignore", "pipe", "pipe"],
    maxBuffer: 128 * 1024 * 1024,
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

function runNodeVectorSuite(testCase) {
  const vectorPath = path.join(
    corpusRoot,
    "cases",
    testCase.id,
    "vectors.json",
  );
  const result = run(
    "node",
    ["tests/vector-suite-entry.mjs", testCase.id, vectorPath],
    { cwd: corpusRoot },
  );
  const report = {
    command: `node tests/vector-suite-entry.mjs ${testCase.id} cases/${testCase.id}/vectors.json`,
    status: result.status === 0 ? "passed" : "failed",
    exitCode: result.status,
    stderr: result.stderr,
  };
  if (result.status !== 0) return { ...report, stdout: result.stdout };
  try {
    const json = JSON.parse(result.stdout);
    return {
      ...report,
      json,
      digest: digest(json),
      total: json.total,
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

function runGoVectorSuite(testCase) {
  const goDir = path.join(generatedRoot, testCase.id);
  const mainPath = path.join(goDir, "vector_suite.go");
  if (!fs.existsSync(mainPath)) {
    return {
      build: {
        status: "failed",
        reason: "missing-generated-vector-suite",
        expectedPath: path.relative(repoRoot, mainPath),
      },
      run: { status: "skipped", reason: "go vector suite missing" },
    };
  }

  const outDir = path.join(buildRoot, testCase.id);
  fs.mkdirSync(outDir, { recursive: true });
  const binaryPath = path.join(outDir, "vector-suite");
  const build = run("go", ["build", "-o", binaryPath, "."], { cwd: goDir });
  const buildReport = {
    command: `go build -o ${path.relative(repoRoot, binaryPath)} .`,
    cwd: path.relative(repoRoot, goDir),
    status: build.status === 0 ? "passed" : "failed",
    exitCode: build.status,
    stdout: build.stdout,
    stderr: build.stderr,
  };
  if (build.status !== 0) {
    return { build: buildReport, run: { status: "skipped" } };
  }

  const vectorPath = path.join(
    corpusRoot,
    "cases",
    testCase.id,
    "vectors.json",
  );
  const result = run(binaryPath, [testCase.id, vectorPath], { cwd: goDir });
  const runReport = {
    command: `${path.relative(repoRoot, binaryPath)} ${testCase.id} ${path.relative(
      repoRoot,
      vectorPath,
    )}`,
    status: result.status === 0 ? "passed" : "failed",
    exitCode: result.status,
    stderr: result.stderr,
  };
  if (result.status !== 0) return { build: buildReport, run: runReport };
  try {
    const json = JSON.parse(result.stdout);
    return {
      build: buildReport,
      run: { ...runReport, json, digest: digest(json), total: json.total },
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

const cases = [];

for (const testCase of manifest.cases) {
  const node = runNodeVectorSuite(testCase);
  const go = hasFlag("--node-only") ? null : runGoVectorSuite(testCase);
  const parity =
    node.status === "passed" &&
    (hasFlag("--node-only") ||
      (go.build.status === "passed" &&
        go.run.status === "passed" &&
        stableStringify(node.json) === stableStringify(go.run.json)));

  cases.push({
    id: testCase.id,
    node: stripJson(node),
    go: stripGoJson(go),
    parity: parity ? { status: "passed" } : { status: "failed" },
  });
}

const summary = {
  total: cases.length,
  nodePassed: cases.filter((testCase) => testCase.node.status === "passed")
    .length,
  goBuildPassed: cases.filter(
    (testCase) => testCase.go?.build.status === "passed",
  ).length,
  goRunPassed: cases.filter((testCase) => testCase.go?.run.status === "passed")
    .length,
  parityPassed: cases.filter((testCase) => testCase.parity.status === "passed")
    .length,
};

const report = {
  version: REPORT_VERSION,
  status: summary.parityPassed === summary.total ? "passed" : "failed",
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

function stripGoJson(report) {
  if (!report) return report;
  return {
    build: report.build,
    run: stripJson(report.run),
  };
}
