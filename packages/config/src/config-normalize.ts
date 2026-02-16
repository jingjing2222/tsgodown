import type { UserConfig, UserConfigExport } from "./types.js";

export async function normalizeUserConfigExport(
  exported: UserConfigExport,
): Promise<UserConfig[]> {
  if (typeof exported === "function") {
    const resolved = await exported({
      mode: process.env.NODE_ENV || "development",
    });
    return [resolved];
  }

  return Array.isArray(exported) ? exported : [exported];
}
