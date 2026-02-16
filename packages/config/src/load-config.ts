import { importConfigExport } from "./config-loader.js";
import { normalizeUserConfigExport } from "./config-normalize.js";
import type { UserConfig } from "./types.js";

export async function loadUserConfig(cwd: string): Promise<UserConfig[]> {
  const exported = await importConfigExport(cwd);
  return normalizeUserConfigExport(exported);
}
