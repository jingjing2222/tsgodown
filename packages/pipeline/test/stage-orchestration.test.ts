import assert from "node:assert/strict";
import test from "node:test";

import { assertBuildArtifactContract } from "../src/internal/stage-orchestration.ts";

test("assertBuildArtifactContract reports missing/invalid artifact fields clearly", () => {
  assert.throws(
    () =>
      assertBuildArtifactContract({
        mode: "rust-engine-adapter",
        manifestPath: "",
        manifestIndexPath: "",
        manifest: {
          buildId: "",
          entries: "not-an-array" as unknown as string[],
          bundles: [],
          types: [],
          tsconfigPath: "tsconfig.json",
        },
        diagnostics: [],
      }),
    /artifact contract violation: manifestPath must be a non-empty string; manifestIndexPath must be a non-empty string; manifest\.buildId must be a non-empty string; manifest\.entries must be an array/,
  );
});
