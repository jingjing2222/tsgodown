import { runBuild } from "@tsgodown/tsdown-driver";

export async function runBuildArtifactsViaRustAdapter(
  cwd: string,
): Promise<void> {
  await runBuild(cwd);
}
