import { build } from "@tsgodown/core";
import { humanPrintSummary } from "../output/human.js";
import { printJson } from "../output/json.js";

export async function runBuild(cwd: string, json: boolean) {
  const result = await build(cwd);
  if (json) {
    printJson(result);
    return;
  }
  humanPrintSummary("build completed", result);
}
