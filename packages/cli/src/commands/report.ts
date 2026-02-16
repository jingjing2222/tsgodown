import { report } from "@tsgodown/core";
import { humanPrintSummary } from "../output/human.js";
import { printJson } from "../output/json.js";

export async function runReport(cwd: string, json: boolean) {
  const result = await report(cwd);
  if (json) {
    printJson(result);
    return;
  }
  humanPrintSummary("report", result);
}
