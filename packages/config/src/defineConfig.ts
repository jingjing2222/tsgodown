import type { UserConfig, UserConfigExport, UserConfigFn } from "./types.js";

export function defineConfig(options: UserConfig): UserConfig;
export function defineConfig(options: UserConfig[]): UserConfig[];
export function defineConfig(options: UserConfigFn): UserConfigFn;
export function defineConfig(options: UserConfigExport): UserConfigExport;
export function defineConfig(options: UserConfigExport): UserConfigExport {
  return options;
}
