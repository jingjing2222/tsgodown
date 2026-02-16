import fs from "node:fs";
import path from "node:path";
import { analyzeFastifyEntry } from "@tsgodown/analyzer";
import { loadUserConfig } from "@tsgodown/config";
import { runPipeline } from "@tsgodown/pipeline";
export const ACTIVE_STAGES = [
    "load-config",
    "analyze",
    "emit",
    "onSuccess",
];
function resolvePlan(cwd, conf, configIndex) {
    const entry = typeof conf.entry === "string" ? conf.entry : "src/index.ts";
    const outDir = conf.outDir ?? "dist-go";
    const resolvedOut = path.resolve(cwd, outDir);
    return {
        configIndex,
        entry: path.resolve(cwd, entry),
        outDir: resolvedOut,
        artifact: path.join(resolvedOut, "main.go"),
    };
}
async function run(cwd, command) {
    const configs = await loadUserConfig(cwd);
    const targets = [];
    if (command === "build") {
        await runPipeline(cwd);
    }
    for (const [idx, conf] of configs.entries()) {
        const plan = resolvePlan(cwd, conf, idx);
        const ir = analyzeFastifyEntry(plan.entry);
        if (command === "build") {
            // Emission already handled by pipeline orchestration.
        }
        const artifactExists = fs.existsSync(plan.artifact);
        targets.push({
            ...plan,
            emitted: command === "build" ? true : artifactExists,
            diagnostics: {
                routes: ir.routes.length,
                warnings: ir.diagnostics
                    .filter((d) => d.level === "warn")
                    .map((d) => d.message),
            },
        });
    }
    return {
        ok: true,
        cwd,
        command,
        stages: ACTIVE_STAGES,
        targets,
    };
}
export async function build(cwd) {
    return run(cwd, "build");
}
export async function check(cwd) {
    return run(cwd, "check");
}
export async function report(cwd) {
    return run(cwd, "report");
}
export async function stages(cwd) {
    const configs = await loadUserConfig(cwd);
    return {
        cwd,
        stages: ACTIVE_STAGES,
        targets: configs.map((conf, idx) => {
            const plan = resolvePlan(cwd, conf, idx);
            return {
                ...plan,
                diagnostics: { routes: 0, warnings: [] },
                emitted: fs.existsSync(plan.artifact),
            };
        }),
    };
}
