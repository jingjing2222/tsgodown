import assert from "node:assert/strict";
import test from "node:test";

import {
  assertBuildArtifactContract,
  assertCompileInputContract,
} from "../src/internal/stage-orchestration.ts";

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
          bundles: "not-an-array" as unknown as Array<{
            file: string;
            map?: string;
          }>,
          types: "not-an-array" as unknown as string[],
          tsconfigPath: "tsconfig.json",
        },
        diagnostics: [],
      }),
    /artifact contract violation: manifestPath must be a non-empty string; manifestIndexPath must be a non-empty string; manifest\.buildId must be a non-empty string; manifest\.entries must be an array; manifest\.bundles must be an array; manifest\.types must be an array/,
  );
});

test("assertCompileInputContract enforces bundle + sourcemap + d.ts compile envelope", () => {
  assert.throws(
    () =>
      assertCompileInputContract({
        mode: "rust-engine-adapter",
        manifestPath: "artifacts/manifests/manifest.json",
        manifestIndexPath: "artifacts/manifests/index.json",
        manifest: {
          buildId: "aabbccddeeff0011",
          entries: ["src/index.ts"],
          bundles: [{ file: "dist/index.mjs", format: "esm", exports: [] }],
          types: [],
          tsconfigPath: "tsconfig.json",
        },
        diagnostics: [],
      }),
    /compile-input contract violation: manifest\.bundles\[0\]\.map must be a non-empty string; manifest\.types\[0\] must be a non-empty string/,
  );
});
