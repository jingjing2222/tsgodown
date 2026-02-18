import { loadUserConfig } from "@tsgodown/config";

import {
  type PipelineStageEvent,
  orchestratePipelineStages,
} from "./internal/index.js";

export interface PipelineOptions {
  log?: (message: string) => void;
  onStage?: (event: PipelineStageEvent) => void;
}

export async function runPipeline(cwd: string, options: PipelineOptions = {}) {
  const log = options.log ?? ((msg: string) => console.log(msg));
  const configs = await loadUserConfig(cwd);

  await orchestratePipelineStages({
    cwd,
    configs,
    log,
    onStage: options.onStage,
  });
}
