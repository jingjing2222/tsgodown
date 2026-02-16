import type { CommandResult, StagesResult } from "../types.js";

export function humanPrintSummary(kind: string, result: CommandResult) {
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

export function humanPrintStages(result: StagesResult) {
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
