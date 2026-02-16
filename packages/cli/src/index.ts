#!/usr/bin/env node
import { build, check, report, stages } from "@tsgodown/core";

type Command = "build" | "check" | "report" | "stages";

const argv = process.argv.slice(2);
const command = (argv[0] || "build") as Command;
const flags = new Set(argv.slice(1));
const json = flags.has("--json") || flags.has("-j");

function humanPrintSummary(
  kind: string,
  result: {
    cwd: string;
    stages?: readonly string[];
    targets: Array<{
      configIndex: number;
      entry: string;
      outDir: string;
      artifact: string;
      emitted: boolean;
      diagnostics: { routes: number; warnings: string[] };
    }>;
  },
) {
  console.log(`[tsgodown] ${kind}`);
  console.log(`cwd: ${result.cwd}`);
  if (result.stages) {
    console.log(`stages: ${result.stages.join(" -> ")}`);
  }
  for (const target of result.targets) {
    console.log(`\n- config #${target.configIndex}`);
    console.log(`  entry: ${target.entry}`);
    console.log(`  outDir: ${target.outDir}`);
    console.log(
      `  artifact: ${target.artifact} ${target.emitted ? "(present)" : "(missing)"}`,
    );
    console.log(`  routes: ${target.diagnostics.routes}`);
    if (target.diagnostics.warnings.length > 0) {
      console.log("  warnings:");
      for (const warning of target.diagnostics.warnings) {
        console.log(`    - ${warning}`);
      }
    }
  }
}

async function main() {
  switch (command) {
    case "build": {
      const result = await build(process.cwd());
      if (json) console.log(JSON.stringify(result, null, 2));
      else {
        humanPrintSummary("build completed", result);
      }
      break;
    }
    case "check": {
      const result = await check(process.cwd());
      if (json) console.log(JSON.stringify(result, null, 2));
      else humanPrintSummary("check completed (no emit)", result);
      break;
    }
    case "report": {
      const result = await report(process.cwd());
      if (json) console.log(JSON.stringify(result, null, 2));
      else humanPrintSummary("report", result);
      break;
    }
    case "stages": {
      const result = await stages(process.cwd());
      if (json) console.log(JSON.stringify(result, null, 2));
      else {
        console.log("[tsgodown] active stages + paths");
        console.log(`cwd: ${result.cwd}`);
        console.log(`stages: ${result.stages.join(" -> ")}`);
        for (const target of result.targets) {
          console.log(`\n- config #${target.configIndex}`);
          console.log(`  entry: ${target.entry}`);
          console.log(`  outDir: ${target.outDir}`);
          console.log(
            `  artifact: ${target.artifact} ${target.emitted ? "(present)" : "(missing)"}`,
          );
        }
      }
      break;
    }
    default:
      console.error(`[tsgodown] unsupported command: ${command}`);
      process.exit(1);
  }
}

try {
  await main();
} catch (error) {
  const msg = error instanceof Error ? error.message : String(error);
  console.error(`[tsgodown] command failed: ${msg}`);
  process.exit(1);
}
