#!/usr/bin/env node
import { isCommand, parseArgv } from "./argv.js";
import { executeCommand } from "./command-executor.js";
import {
  extractCommandErrorDetails,
  printHumanError,
  printJson,
} from "./output/index.js";

async function main() {
  const { command, json } = parseArgv(process.argv.slice(2));

  if (!isCommand(command)) {
    console.error(`[tsgodown] unsupported command: ${command}`);
    process.exit(1);
  }

  await executeCommand(command, process.cwd(), json);
}

try {
  await main();
} catch (error) {
  const details = extractCommandErrorDetails(error);
  const { json } = parseArgv(process.argv.slice(2));

  if (json) {
    printJson({
      ok: false,
      error: {
        message: details.message,
        source: details.source,
        stage: details.stage,
        cause: details.cause,
        guidance: details.guidance,
      },
    });
  } else {
    printHumanError(details);
  }
  process.exit(1);
}
