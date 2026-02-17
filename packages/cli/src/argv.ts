import type { Command } from "./types.js";

export function parseArgv(argv: string[]): {
  command: Command | string;
  json: boolean;
} {
  const command = (argv[0] || "compiler") as Command | string;
  const flags = new Set(argv.slice(1));
  const json = flags.has("--json") || flags.has("-j");
  return { command, json };
}

export function isCommand(value: string): value is Command {
  return (
    value === "build" ||
    value === "check" ||
    value === "report" ||
    value === "stages" ||
    value === "compiler"
  );
}
