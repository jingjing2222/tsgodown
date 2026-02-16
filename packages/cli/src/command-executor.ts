import { runBuild, runCheck, runReport, runStages } from "./commands/index.js";
import type { Command } from "./types.js";

export async function executeCommand(
  command: Command,
  cwd: string,
  json: boolean,
) {
  switch (command) {
    case "build":
      await runBuild(cwd, json);
      return;
    case "check":
      await runCheck(cwd, json);
      return;
    case "report":
      await runReport(cwd, json);
      return;
    case "stages":
      await runStages(cwd, json);
      return;
    default:
      throw new Error(`unsupported command: ${String(command)}`);
  }
}
