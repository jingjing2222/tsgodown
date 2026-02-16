import assert from "node:assert/strict";
import test from "node:test";

import { formatPipelineFailure } from "../src/internal/result-normalization.ts";

test("formatPipelineFailure keeps source/stage/cause/guidance contract", () => {
  const error = formatPipelineFailure("src/missing.ts", {
    source: "pipeline-entry(src/missing.ts)",
    stage: "BUILD_ARTIFACTS",
    cause:
      "source=rust-engine-adapter; cause=ENOENT: missing entry src/missing.ts; guidance=Verify tsgodown.config.ts entry path and file existence.",
    guidance:
      "Verify rust engine build/analyze contract and tsgodown.config.ts settings.",
  });

  assert.match(
    error.message,
    /\[pipeline\] failed for entry "src\/missing\.ts"/,
  );
  assert.match(error.message, /source: pipeline-entry\(src\/missing\.ts\)/);
  assert.match(error.message, /stage: BUILD_ARTIFACTS/);
  assert.match(error.message, /cause: source=rust-engine-adapter/);
  assert.match(
    error.message,
    /guidance: Verify rust engine build\/analyze contract and tsgodown\.config\.ts settings\./,
  );
});
