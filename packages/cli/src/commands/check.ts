import { check } from "@tsgodown/core";
import { humanPrintSummary } from "../output/human.js";
import { printJson } from "../output/json.js";

export async function runCheck(cwd: string, json: boolean) {
  const result = await check(cwd);
  if (json) {
    printJson(result);
    return;
  }
  humanPrintSummary("check completed (no emit)", result);
}
