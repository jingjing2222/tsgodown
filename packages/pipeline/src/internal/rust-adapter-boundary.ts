import type { UserConfig } from "@tsgodown/config";
import { type RunBuildResult, runBuild } from "@tsgodown/tsdown-driver";

export async function runBuildArtifactsViaRustAdapter(
  cwd: string,
  config?: UserConfig,
): Promise<RunBuildResult> {
  return runBuild(cwd, undefined, { tsdownConfig: config });
}
