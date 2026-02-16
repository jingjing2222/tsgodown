import { stages } from "@tsgodown/core";
import { humanPrintStages } from "../output/human.js";
import { printJson } from "../output/json.js";

export async function runStages(cwd: string, json: boolean) {
  const result = await stages(cwd);
  if (json) {
    printJson(result);
    return;
  }
  humanPrintStages(result);
}
