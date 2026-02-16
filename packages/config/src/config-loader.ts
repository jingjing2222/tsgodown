import path from "node:path";
import { pathToFileURL } from "node:url";
import type { UserConfigExport } from "./types.js";

const DEFAULT_CONFIG_FILE = "tsgodown.config.ts";

export function resolveConfigModuleUrl(cwd: string): string {
  const configPath = path.resolve(cwd, DEFAULT_CONFIG_FILE);
  return pathToFileURL(configPath).href;
}

export async function importConfigExport(
  cwd: string,
): Promise<UserConfigExport> {
  const mod = await import(resolveConfigModuleUrl(cwd));
  return (mod.default ?? mod.config) as UserConfigExport;
}
