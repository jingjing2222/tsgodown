import { loadUserConfig } from "@tsgodown/config";

import { orchestratePipelineStages } from "./internal/index.js";

export interface PipelineOptions {
  log?: (message: string) => void;
}

export async function runPipeline(cwd: string, options: PipelineOptions = {}) {
  const log = options.log ?? ((msg: string) => console.log(msg));
  const configs = await loadUserConfig(cwd);

  await orchestratePipelineStages({
    cwd,
    configs,
    log,
  });
}
