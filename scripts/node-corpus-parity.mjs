#!/usr/bin/env node

import { spawnSync } from "node:child_process";
import fs from "node:fs";
import path from "node:path";

const REPORT_VERSION = "node-corpus-parity.v1";
const repoRoot = path.resolve(import.meta.dirname, "..");
const corpusRoot = path.join(repoRoot, "test-corpus", "node-real");
const manifestPath = path.join(corpusRoot, "manifest.json");
const generatedRoot =
  process.env.TSGODOWN_NODE_CORPUS_GO_ROOT ??
  path.join(corpusRoot, "generated-go");
const buildRoot = path.join(repoRoot, ".tmp", "node-corpus-parity");

function hasFlag(name) {
  return process.argv.includes(name);
}

function stableStringify(value) {
  if (Array.isArray(value)) {
    return `[${value.map(stableStringify).join(",")}]`;
  }
  if (value && typeof value === "object") {
    return `{${Object.keys(value)
      .sort()
      .map((key) => `${JSON.stringify(key)}:${stableStringify(value[key])}`)
      .join(",")}}`;
  }
  return JSON.stringify(value);
}

function readJson(filePath) {
  return JSON.parse(fs.readFileSync(filePath, "utf8"));
}

function run(cmd, args, options = {}) {
  return spawnSync(cmd, args, {
    cwd: repoRoot,
    encoding: "utf8",
    stdio: ["ignore", "pipe", "pipe"],
    ...options,
  });
}

function ensureCorpusInstall() {
  if (fs.existsSync(path.join(corpusRoot, "node_modules"))) {
    return {
      command: null,
      status: "skipped",
      reason: "node_modules already exists",
    };
  }

  const install = run("npm", ["ci", "--omit=dev", "--ignore-scripts"], {
    cwd: corpusRoot,
  });
  return {
    command: "npm ci --omit=dev --ignore-scripts",
    status: install.status === 0 ? "passed" : "failed",
    exitCode: install.status,
    stdout: install.stdout,
    stderr: install.stderr,
  };
}

function runNodeProbe(testCase) {
  const result = run("npm", ["run", "--silent", `probe:${testCase.id}`], {
    cwd: corpusRoot,
  });
  const report = {
    command: `npm run --silent probe:${testCase.id}`,
    status: result.status === 0 ? "passed" : "failed",
    exitCode: result.status,
    stderr: result.stderr,
  };

  if (result.status !== 0) {
    return {
      ...report,
      stdout: result.stdout,
    };
  }

  try {
    return {
      ...report,
      json: JSON.parse(result.stdout),
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

function runGoProbe(testCase) {
  const goDir = path.join(generatedRoot, testCase.id);
  const goModPath = path.join(goDir, "go.mod");
  if (!fs.existsSync(goModPath)) {
    return {
      build: {
        status: "failed",
        reason: "missing-generated-go-project",
        path: path.relative(repoRoot, goDir),
      },
      run: {
        status: "skipped",
        reason: "go build did not run",
      },
    };
  }

  const outDir = path.join(buildRoot, testCase.id);
  fs.mkdirSync(outDir, { recursive: true });
  const binaryPath = path.join(outDir, "probe");
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
    return {
      build: buildReport,
      run: {
        status: "skipped",
        reason: "go build failed",
      },
    };
  }

  const runResult = run(binaryPath, [], { cwd: goDir });
  const runReport = {
    command: path.relative(repoRoot, binaryPath),
    cwd: path.relative(repoRoot, goDir),
    status: runResult.status === 0 ? "passed" : "failed",
    exitCode: runResult.status,
    stderr: runResult.stderr,
  };

  if (runResult.status !== 0) {
    return {
      build: buildReport,
      run: {
        ...runReport,
        stdout: runResult.stdout,
      },
    };
  }

  try {
    return {
      build: buildReport,
      run: {
        ...runReport,
        json: JSON.parse(runResult.stdout),
      },
    };
  } catch (error) {
    return {
      build: buildReport,
      run: {
        ...runReport,
        status: "failed",
        stdout: runResult.stdout,
        parseError: error instanceof Error ? error.message : String(error),
      },
    };
  }
}

function compareReports(nodeReport, goReport) {
  if (nodeReport.status !== "passed") {
    return {
      status: "skipped",
      match: false,
      diffs: ["node-probe-failed"],
    };
  }
  if (goReport.run.status !== "passed") {
    return {
      status: "failed",
      match: false,
      diffs: [goReport.build.reason ?? goReport.run.reason ?? "go-run-failed"],
    };
  }

  const nodeStable = stableStringify(nodeReport.json);
  const goStable = stableStringify(goReport.run.json);
  if (nodeStable === goStable) {
    return {
      status: "passed",
      match: true,
      diffs: [],
    };
  }

  return {
    status: "failed",
    match: false,
    diffs: ["json-mismatch"],
    node: nodeReport.json,
    go: goReport.run.json,
  };
}

function summarize(caseReports, { nodeOnly }) {
  const total = caseReports.length;
  const nodePassed = caseReports.filter(
    (entry) => entry.node.status === "passed",
  ).length;
  const goBuildPassed = caseReports.filter(
    (entry) => entry.go?.build?.status === "passed",
  ).length;
  const goRunPassed = caseReports.filter(
    (entry) => entry.go?.run?.status === "passed",
  ).length;
  const parityPassed = caseReports.filter(
    (entry) => entry.parity?.status === "passed",
  ).length;
  const pass = nodeOnly
    ? nodePassed === total
    : nodePassed === total &&
      goBuildPassed === total &&
      goRunPassed === total &&
      parityPassed === total;

  return {
    total,
    nodePassed,
    goBuildPassed: nodeOnly ? null : goBuildPassed,
    goRunPassed: nodeOnly ? null : goRunPassed,
    parityPassed: nodeOnly ? null : parityPassed,
    pass,
  };
}

function main() {
  const nodeOnly = hasFlag("--node-only");
  const skipInstall = hasFlag("--skip-install");
  const manifest = readJson(manifestPath);
  const install = skipInstall
    ? { status: "skipped", reason: "--skip-install" }
    : ensureCorpusInstall();

  if (install.status === "failed") {
    const report = {
      version: REPORT_VERSION,
      mode: nodeOnly ? "node-only" : "full",
      manifestVersion: manifest.version,
      install,
      summary: {
        total: manifest.cases.length,
        nodePassed: 0,
        goBuildPassed: nodeOnly ? null : 0,
        goRunPassed: nodeOnly ? null : 0,
        parityPassed: nodeOnly ? null : 0,
        pass: false,
      },
      cases: [],
    };
    process.stdout.write(`${JSON.stringify(report, null, 2)}\n`);
    process.exit(1);
  }

  const cases = manifest.cases.map((testCase) => {
    const node = runNodeProbe(testCase);
    if (nodeOnly) {
      return {
        id: testCase.id,
        package: testCase.package,
        version: testCase.version,
        capabilities: testCase.capabilities,
        node,
      };
    }

    const go = runGoProbe(testCase);
    return {
      id: testCase.id,
      package: testCase.package,
      version: testCase.version,
      capabilities: testCase.capabilities,
      node,
      go,
      parity: compareReports(node, go),
    };
  });

  const summary = summarize(cases, { nodeOnly });
  const report = {
    version: REPORT_VERSION,
    mode: nodeOnly ? "node-only" : "full",
    manifestVersion: manifest.version,
    allowWip: manifest.allowWip,
    generatedRoot: path.relative(repoRoot, generatedRoot),
    install,
    summary,
    cases,
  };

  process.stdout.write(`${JSON.stringify(report, null, 2)}\n`);
  process.exit(summary.pass ? 0 : 1);
}

main();
