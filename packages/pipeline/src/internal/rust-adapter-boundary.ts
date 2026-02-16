import { type RunBuildResult, runBuild } from "@tsgodown/tsdown-driver";

export async function runBuildArtifactsViaRustAdapter(
  cwd: string,
): Promise<RunBuildResult> {
  return runBuild(cwd);
}
