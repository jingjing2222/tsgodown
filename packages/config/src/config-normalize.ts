import type { UserConfig, UserConfigExport, UserConfigFn } from "./types.js";

function buildTsdownFunctionArgs(): Parameters<UserConfigFn> {
  return [{}, { ci: process.env.CI === "true" }];
}

function normalizeArrayableConfig(
  value: UserConfig | UserConfig[],
): UserConfig[] {
  return Array.isArray(value) ? value : [value];
}

export async function normalizeUserConfigExport(
  exported: UserConfigExport,
): Promise<UserConfig[]> {
  const resolvedExport = await exported;

  if (typeof resolvedExport === "function") {
    const [inlineConfig, context] = buildTsdownFunctionArgs();
    const resolved = await resolvedExport(inlineConfig, context);
    return normalizeArrayableConfig(resolved);
  }

  return normalizeArrayableConfig(resolvedExport);
}
