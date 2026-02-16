#!/usr/bin/env node
import { isCommand, parseArgv } from "./argv.js";
import { executeCommand } from "./command-executor.js";

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
  const msg = error instanceof Error ? error.message : String(error);
  console.error(`[tsgodown] command failed: ${msg}`);
  process.exit(1);
}
