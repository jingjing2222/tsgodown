import type { UserConfig, UserConfigExport } from "./types.js";

function buildConfigEnv(): { mode: string } {
  return {
    mode: process.env.NODE_ENV || "development",
  };
}

export async function normalizeUserConfigExport(
  exported: UserConfigExport,
): Promise<UserConfig[]> {
  if (typeof exported === "function") {
    const resolved = await exported(buildConfigEnv());
    return [resolved];
  }

  return Array.isArray(exported) ? exported : [exported];
}
