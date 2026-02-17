import { spawnSync } from "node:child_process";
import fs from "node:fs";
import os from "node:os";
import path from "node:path";

import {
  PERF_SCENARIOS,
  type PerfScenario,
  evaluateRegression,
  summarizeMs,
} from "../packages/cli/src/perf-baseline.js";

interface BaselineEntry {
  medianMs: number | null;
}

interface BaselineFile {
  schemaVersion: 1;
  generatedAt: string;
  scenarios: Record<string, BaselineEntry>;
}

const repoRoot = path.resolve(import.meta.dirname, "..");
const cliEntry = path.join(repoRoot, "packages", "cli", "dist", "index.js");
const fixturesDir = path.join(
  repoRoot,
  "packages",
  "cli",
  "test",
  "fixtures",
  "projects",
);
const baselinePath = path.join(repoRoot, "docs", "perf", "baseline.json");
const reportPath = path.join(repoRoot, "artifacts", "perf", "report.json");

const updateBaseline = process.argv.includes("--update-baseline");

main();

function main() {
  ensureCliBuildExists();

  const launcherPath = createRustEngineLauncher();
  const baseline = readBaseline();

  const report: {
    generatedAt: string;
    host: string;
    platform: string;
    scenarios: Array<Record<string, unknown>>;
  } = {
    generatedAt: new Date().toISOString(),
    host: os.hostname(),
    platform: `${process.platform}-${process.arch}`,
    scenarios: [],
  };

  let hasFailure = false;

  for (const scenario of PERF_SCENARIOS) {
    const samples = runScenario(scenario, launcherPath);
    const stats = summarizeMs(samples);
    const baselineMedian = baseline?.scenarios[scenario.id]?.medianMs ?? null;

    const thresholdOk = stats.p95Ms <= scenario.thresholdMs;
    const regression =
      baselineMedian === null
        ? { ok: true, deltaMs: 0, deltaPct: 0 }
        : evaluateRegression(
            baselineMedian,
            stats.medianMs,
            scenario.regressionTolerancePct,
          );

    const scenarioOk = thresholdOk && regression.ok;
    if (!scenarioOk) {
      hasFailure = true;
    }

    console.log(
      [
        `[perf] ${scenario.id}`,
        `median=${stats.medianMs.toFixed(2)}ms`,
        `p95=${stats.p95Ms.toFixed(2)}ms`,
        `threshold<=${scenario.thresholdMs}ms ${thresholdOk ? "OK" : "FAIL"}`,
        baselineMedian === null
          ? "baseline=n/a"
          : `delta=${regression.deltaPct.toFixed(2)}% (<=${scenario.regressionTolerancePct}% ${regression.ok ? "OK" : "FAIL"})`,
      ].join(" | "),
    );

    report.scenarios.push({
      id: scenario.id,
      fixture: scenario.fixture,
      command: scenario.command,
      samplesMs: samples,
      stats,
      thresholdMs: scenario.thresholdMs,
      thresholdOk,
      baselineMedianMs: baselineMedian,
      regressionTolerancePct: scenario.regressionTolerancePct,
      regression,
      ok: scenarioOk,
    });
  }

  fs.mkdirSync(path.dirname(reportPath), { recursive: true });
  fs.writeFileSync(reportPath, `${JSON.stringify(report, null, 2)}\n`);

  if (updateBaseline) {
    writeBaseline(report);
    console.log(
      `[perf] baseline updated: ${path.relative(repoRoot, baselinePath)}`,
    );
  }

  console.log(`[perf] report written: ${path.relative(repoRoot, reportPath)}`);

  if (hasFailure) {
    process.exitCode = 1;
  }
}

function runScenario(scenario: PerfScenario, launcherPath: string): number[] {
  const cwd = path.join(fixturesDir, scenario.fixture);
  const samples: number[] = [];

  for (let i = 0; i < scenario.warmupRuns + scenario.sampleRuns; i++) {
    const start = process.hrtime.bigint();
    const result = spawnSync(
      process.execPath,
      [cliEntry, scenario.command, "--json"],
      {
        cwd,
        encoding: "utf8",
        env: {
          ...process.env,
          TSGODOWN_RUST_ENGINE_BIN: launcherPath,
        },
      },
    );

    if (result.status !== 0) {
      throw new Error(
        `[perf] scenario=${scenario.id} command failed status=${result.status}\n${result.stderr || result.stdout}`,
      );
    }

    if (i >= scenario.warmupRuns) {
      const elapsedMs = Number(process.hrtime.bigint() - start) / 1_000_000;
      samples.push(elapsedMs);
    }
  }

  return samples;
}

function ensureCliBuildExists() {
  if (!fs.existsSync(cliEntry)) {
    throw new Error(
      "packages/cli/dist/index.js not found. Run `pnpm run build` first.",
    );
  }
}

function readBaseline(): BaselineFile | null {
  if (!fs.existsSync(baselinePath)) {
    return null;
  }

  return JSON.parse(fs.readFileSync(baselinePath, "utf8")) as BaselineFile;
}

function writeBaseline(report: {
  generatedAt: string;
  scenarios: Array<Record<string, unknown>>;
}) {
  const scenarios: Record<string, BaselineEntry> = {};

  for (const row of report.scenarios) {
    scenarios[String(row.id)] = {
      medianMs: Number((row.stats as { medianMs: number }).medianMs.toFixed(2)),
    };
  }

  const baseline: BaselineFile = {
    schemaVersion: 1,
    generatedAt: report.generatedAt,
    scenarios,
  };

  fs.mkdirSync(path.dirname(baselinePath), { recursive: true });
  fs.writeFileSync(baselinePath, `${JSON.stringify(baseline, null, 2)}\n`);
}

function createRustEngineLauncher(): string {
  const tempDir = fs.mkdtempSync(path.join(os.tmpdir(), "tsgodown-perf-"));
  const stubPath = path.join(tempDir, "rust-stub.mjs");

  fs.writeFileSync(
    stubPath,
    [
      "for await (const _ of process.stdin) { /* drain */ }",
      "const response = {",
      "  ok: true,",
      "  diagnostics: ['engine=rust-binary-stub'],",
      "  manifest: {",
      "    buildId: '1122334455667788',",
      "    entries: ['src/index.ts'],",
      "    bundles: [{ file: 'dist/index.mjs', map: 'dist/index.mjs.map', format: 'esm', exports: [] }],",
      "    types: ['dist/index.d.ts'],",
      "    tsconfigPath: 'tsconfig.json'",
      "  }",
      "};",
      "process.stdout.write(JSON.stringify(response));",
    ].join("\n"),
  );

  const launcherPath = path.join(tempDir, "rust-launcher.sh");
  fs.writeFileSync(
    launcherPath,
    `#!/usr/bin/env bash\nexec ${JSON.stringify(process.execPath)} ${JSON.stringify(stubPath)}\n`,
  );
  fs.chmodSync(launcherPath, 0o755);

  return launcherPath;
}
