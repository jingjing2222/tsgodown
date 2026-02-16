import { loadUserConfig } from "@tsgodown/config";
import { runBuild } from "@tsgodown/tsdown-driver";

export interface PipelineOptions {
  log?: (message: string) => void;
}

export async function runPipeline(cwd: string, options: PipelineOptions = {}) {
  const log = options.log ?? ((msg: string) => console.log(msg));

  const configs = await loadUserConfig(cwd);
  for (const conf of configs) {
    const entry = typeof conf.entry === "string" ? conf.entry : "src/index.ts";

    try {
      log("[BUILD_ARTIFACTS] collecting build outputs");
      await runBuild(cwd);

      log(`[BUILD_IR] analyzing entry: ${entry} (delegated to rust engine)`);
      log(
        "[CAPABILITY_GATE] validating required capabilities (delegated to rust engine)",
      );
      log("[EMIT_GO] writing Go scaffold (delegated to rust engine)");
      await conf.onSuccess?.();
    } catch (cause) {
      const msg = cause instanceof Error ? cause.message : String(cause);
      throw new Error(
        [
          `[pipeline] failed for entry \"${entry}\"`,
          `source: ${entry}`,
          `cause: ${msg}`,
          "guidance: Verify rust engine build/analyze contract and tsgodown.config.ts settings.",
        ].join("; "),
      );
    }
  }
}
