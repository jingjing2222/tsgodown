import assert from "node:assert/strict";
import test from "node:test";

import {
  formatPipelineFailure,
  resolveEntry,
} from "../src/internal/result-normalization.ts";

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

test("formatPipelineFailure falls back to stable defaults when details contain blank strings", () => {
  const error = formatPipelineFailure("src/blank.ts", {
    source: "   ",
    stage: "",
    cause: "\t",
    guidance: "\n",
  });

  assert.match(error.message, /source: pipeline-entry\(src\/blank\.ts\)/);
  assert.match(error.message, /stage: UNKNOWN/);
  assert.match(error.message, /cause: unknown pipeline failure/);
  assert.match(
    error.message,
    /guidance: Verify rust engine build\/analyze contract and tsgodown\.config\.ts settings\./,
  );
});

test("formatPipelineFailure treats nullish causes as unknown pipeline failure", () => {
  const withUndefined = formatPipelineFailure("src/undefined.ts", undefined);
  const withNull = formatPipelineFailure("src/null.ts", null);

  assert.match(withUndefined.message, /cause: unknown pipeline failure/);
  assert.match(withNull.message, /cause: unknown pipeline failure/);
});

test("resolveEntry uses string entry and falls back for non-string values", () => {
  assert.equal(resolveEntry({ entry: "src/custom.ts" }), "src/custom.ts");
  assert.equal(resolveEntry({ entry: { app: "src/app.ts" } }), "src/index.ts");
});
