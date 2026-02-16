import path from "node:path";
import { pathToFileURL } from "node:url";
export async function loadUserConfig(cwd) {
    const configPath = path.resolve(cwd, "tsgodown.config.ts");
    const mod = await import(pathToFileURL(configPath).href);
    const exported = mod.default ?? mod.config;
    if (typeof exported === "function") {
        const resolved = await exported({
            mode: process.env.NODE_ENV || "development",
        });
        return [resolved];
    }
    return Array.isArray(exported) ? exported : [exported];
}
