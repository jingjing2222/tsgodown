import path from "node:path";
import { analyzeFastifyEntry } from "@tsgodown/analyzer";
import { loadUserConfig } from "@tsgodown/config";
import { emitGoProject } from "@tsgodown/emitter-go";
import { checkCapabilities } from "@tsgodown/node-compat";
import { runBuild } from "@tsgodown/tsdown-driver";
export async function runPipeline(cwd, options = {}) {
    const log = options.log ?? ((msg) => console.log(msg));
    const configs = await loadUserConfig(cwd);
    for (const conf of configs) {
        const entry = typeof conf.entry === "string" ? conf.entry : "src/index.ts";
        const outDir = conf.outDir ?? "dist-go";
        try {
            log("[BUILD_ARTIFACTS] collecting build outputs");
            await runBuild(cwd);
            log(`[BUILD_IR] analyzing entry: ${entry}`);
            const ir = analyzeFastifyEntry(path.resolve(cwd, entry));
            log("[CAPABILITY_GATE] validating required capabilities");
            const gate = checkCapabilities(ir, {
                allowWip: true,
                failFast: false,
            });
            if (!gate.ok) {
                const details = gate.diagnostics
                    .map((d) => `${d.capability}=${d.status}`)
                    .join(", ");
                throw new Error(`unsupported capabilities: ${details}. Check docs/specs/CAPABILITY_MATRIX.md or simplify source features.`);
            }
            log(`[EMIT_GO] writing Go scaffold to ${outDir}`);
            emitGoProject(ir, path.resolve(cwd, outDir));
            await conf.onSuccess?.();
        }
        catch (cause) {
            const msg = cause instanceof Error ? cause.message : String(cause);
            throw new Error(`[pipeline] failed for entry "${entry}" -> outDir "${outDir}": ${msg}. Verify tsgodown.config.ts entry/outDir and input source syntax.`);
        }
    }
}
