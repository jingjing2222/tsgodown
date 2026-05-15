#!/usr/bin/env node

import { spawnSync } from "node:child_process";
import crypto from "node:crypto";
import fs from "node:fs";
import path from "node:path";
import { differentialFuzzCases } from "./differential-fuzz-cases.mjs";

const REPORT_VERSION = "differential-fuzz-parity.v1";
const repoRoot = path.resolve(import.meta.dirname, "..");
const generatedRoot =
  process.env.TSGODOWN_DIFFERENTIAL_FUZZ_GO_ROOT ??
  path.join(repoRoot, "test-corpus", "differential-fuzz", "generated-go");

const buildRoot = path.join(repoRoot, ".tmp", "differential-fuzz-parity");
const cases = differentialFuzzCases();
let generated = false;

const reports = cases.map((testCase) => {
  const node = runNode(testCase);
  const go = runGo(testCase);
  const parity =
    node.status === "passed" &&
    go.status === "passed" &&
    stableStringify(node.observed) === stableStringify(go.observed);
  return {
    id: testCase.id,
    group: testCase.group,
    capability: testCase.capability,
    node: stripObserved(node),
    go: stripObserved(go),
    parity: parity ? { status: "passed" } : { status: "blocked" },
  };
});

const summary = {
  total: reports.length,
  nodePassed: reports.filter((report) => report.node.status === "passed")
    .length,
  goPassed: reports.filter((report) => report.go.status === "passed").length,
  parityPassed: reports.filter((report) => report.parity.status === "passed")
    .length,
  groups: Object.fromEntries(
    [...new Set(reports.map((report) => report.group))]
      .sort()
      .map((group) => [
        group,
        reports.filter((report) => report.group === group).length,
      ]),
  ),
};

const report = {
  version: REPORT_VERSION,
  status: summary.parityPassed === summary.total ? "passed" : "blocked",
  nodeLts: "24.15.0",
  policy: {
    deterministicSeed: "tsgodown-differential-fuzz-v1",
    noPrecomputedExpected: true,
    noNodeFallbackForGo: true,
  },
  summary,
  cases: reports,
};

console.log(JSON.stringify(report, null, 2));
if (report.status !== "passed") {
  process.exit(1);
}

function runNode(testCase) {
  const result = spawnSync(
    process.execPath,
    ["--input-type=module", "-e", testCase.source],
    {
      cwd: repoRoot,
      encoding: "utf8",
      stdio: ["ignore", "pipe", "pipe"],
      maxBuffer: 16 * 1024 * 1024,
    },
  );
  const base = {
    status: result.status === 0 ? "passed" : "failed",
    exitCode: result.status,
    stderr: result.stderr,
  };
  if (result.status !== 0) return { ...base, stdout: result.stdout };
  try {
    const observed = JSON.parse(result.stdout);
    return { ...base, digest: digest(observed), observed };
  } catch (error) {
    return {
      ...base,
      status: "failed",
      stdout: result.stdout,
      parseError: error instanceof Error ? error.message : String(error),
    };
  }
}

function runGo(testCase) {
  ensureGeneratedGo();
  const goDir = path.join(generatedRoot, testCase.id);
  if (!fs.existsSync(path.join(goDir, "go.mod"))) {
    return {
      status: "blocked",
      reason: "generated Go differential fuzz case missing",
      expectedPath: path.relative(repoRoot, goDir),
    };
  }

  const outDir = path.join(buildRoot, testCase.id);
  fs.mkdirSync(outDir, { recursive: true });
  const binaryPath = path.join(outDir, "probe");
  const build = spawnSync("go", ["build", "-o", binaryPath, "."], {
    cwd: goDir,
    encoding: "utf8",
    stdio: ["ignore", "pipe", "pipe"],
    maxBuffer: 16 * 1024 * 1024,
  });
  if (build.status !== 0) {
    return {
      status: "blocked",
      reason: "generated Go differential fuzz build failed",
      exitCode: build.status,
      stdout: build.stdout,
      stderr: build.stderr,
    };
  }

  const result = spawnSync(binaryPath, [], {
    cwd: goDir,
    encoding: "utf8",
    stdio: ["ignore", "pipe", "pipe"],
    maxBuffer: 16 * 1024 * 1024,
  });
  const base = {
    status: result.status === 0 ? "passed" : "blocked",
    exitCode: result.status,
    stderr: result.stderr,
  };
  if (result.status !== 0) return { ...base, stdout: result.stdout };
  try {
    const observed = JSON.parse(result.stdout);
    return { ...base, digest: digest(observed), observed };
  } catch (error) {
    return {
      ...base,
      status: "blocked",
      stdout: result.stdout,
      parseError: error instanceof Error ? error.message : String(error),
    };
  }
}

function ensureGeneratedGo() {
  if (
    generated ||
    cases.every((testCase) =>
      fs.existsSync(path.join(generatedRoot, testCase.id, "go.mod")),
    )
  ) {
    generated = true;
    return;
  }
  const result = spawnSync(
    "node",
    ["scripts/generate-differential-fuzz-go.mjs"],
    {
      cwd: repoRoot,
      encoding: "utf8",
      stdio: ["ignore", "pipe", "pipe"],
      maxBuffer: 64 * 1024 * 1024,
    },
  );
  generated = true;
  if (result.status !== 0) {
    process.stderr.write(result.stderr || result.stdout);
  }
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

function stripObserved(result) {
  if (!result || !("observed" in result)) return result;
  const { observed: _observed, ...rest } = result;
  return rest;
}
