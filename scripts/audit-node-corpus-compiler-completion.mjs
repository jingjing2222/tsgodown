#!/usr/bin/env node

import fs from "node:fs";
import path from "node:path";

const repoRoot = path.resolve(import.meta.dirname, "..");

const checks = [
  {
    id: "corpus-manifest",
    description:
      "10 vendored corpus cases declare package source, entry, probe, and capabilities",
    inspect() {
      const manifest = readJson("test-corpus/node-real/manifest.json");
      const missing = [];
      for (const testCase of manifest.cases ?? []) {
        for (const key of [
          "id",
          "packagePath",
          "entry",
          "probe",
          "capabilities",
        ]) {
          if (
            !testCase[key] ||
            (Array.isArray(testCase[key]) && testCase[key].length === 0)
          ) {
            missing.push(`${testCase.id ?? "<unknown>"}.${key}`);
          }
        }
      }
      return {
        status:
          manifest.cases?.length === 10 && missing.length === 0
            ? "passed"
            : "failed",
        evidence: {
          cases: manifest.cases?.length ?? 0,
          missing,
        },
      };
    },
  },
  {
    id: "parity-gate",
    description:
      "node corpus parity gate exists and is wired as package script",
    inspect() {
      const pkg = readJson("package.json");
      const script = pkg.scripts?.["gate:node-corpus-parity"];
      return {
        status:
          script === "node scripts/node-corpus-parity.mjs" &&
          exists("scripts/node-corpus-parity.mjs")
            ? "passed"
            : "failed",
        evidence: { script },
      };
    },
  },
  {
    id: "no-node-runtime-fallback-guard",
    description:
      "parity gate rejects generated Go using Node/V8 fallback execution",
    inspect() {
      const source = read("scripts/node-corpus-parity.mjs");
      const needles = ['"os/exec"', "exec.Command", "syscall.Exec", "node --"];
      const missing = needles.filter((needle) => !source.includes(needle));
      return {
        status: missing.length === 0 ? "passed" : "failed",
        evidence: { guardedNeedles: needles, missing },
      };
    },
  },
  {
    id: "source-lowered-executable-ir",
    description:
      "generated Go is driven by source-lowered executable JS IR, not corpus capability renderers",
    inspect() {
      const generator = read("scripts/generate-node-corpus-go.mjs");
      const rendererMatches = [
        ...generator.matchAll(/function (render[A-Za-z0-9]+ProbeMain)\(/g),
      ].map((match) => match[1]);
      const capabilityBranches = (
        generator.match(/capabilities\?\.includes/g) ?? []
      ).length;
      const analyzerSources = [
        "packages/analyzer-rust/src/ir.rs",
        "packages/analyzer-rust/src/parser.rs",
        "packages/analyzer-rust/src/builder.rs",
      ]
        .map(read)
        .join("\n");
      const hasExecutableIr =
        /Executable(IR|Program|Stmt|Expr)|Js(Stmt|Expr|Value)/.test(
          analyzerSources,
        );

      return {
        status:
          rendererMatches.length === 0 &&
          capabilityBranches === 0 &&
          hasExecutableIr
            ? "passed"
            : "failed",
        evidence: {
          rendererFunctions: rendererMatches,
          capabilityBranches,
          hasExecutableIr,
        },
      };
    },
  },
  {
    id: "engine-uses-analyzer-project-graph",
    description: "engine-core analyzes cwd project graph through analyzer-rust",
    inspect() {
      const source = read("crates/engine-core/src/analyze.rs");
      const ok = source.includes("analyzer_rust::analyze_compiler_project");
      return {
        status: ok ? "passed" : "failed",
        evidence: { usesAnalyzeCompilerProject: ok },
      };
    },
  },
];

function exists(relativePath) {
  return fs.existsSync(path.join(repoRoot, relativePath));
}

function read(relativePath) {
  return fs.readFileSync(path.join(repoRoot, relativePath), "utf8");
}

function readJson(relativePath) {
  return JSON.parse(read(relativePath));
}

const results = checks.map((check) => ({
  id: check.id,
  description: check.description,
  ...check.inspect(),
}));
const failed = results.filter((result) => result.status !== "passed");
const report = {
  version: "node-corpus-compiler-completion-audit.v1",
  status: failed.length === 0 ? "passed" : "failed",
  summary: {
    total: results.length,
    passed: results.length - failed.length,
    failed: failed.length,
  },
  results,
};

process.stdout.write(`${JSON.stringify(report, null, 2)}\n`);
process.exit(failed.length === 0 ? 0 : 1);
