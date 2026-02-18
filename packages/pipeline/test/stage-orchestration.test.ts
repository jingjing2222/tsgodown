import assert from "node:assert/strict";
import test from "node:test";

import {
  assertBuildArtifactContract,
  assertCompileInputContract,
  orchestratePipelineStages,
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
          bundles: [],
          types: [],
          tsconfigPath: "tsconfig.json",
        },
        diagnostics: [],
      }),
    /artifact contract violation: manifestPath must be a non-empty string; manifestIndexPath must be a non-empty string; manifest\.buildId must be a non-empty string; manifest\.entries must be an array/,
  );
});

test("assertCompileInputContract enforces minimal delegated compile envelope", () => {
  assert.throws(
    () =>
      assertCompileInputContract({
        mode: "rust-engine-adapter",
        manifestPath: "artifacts/manifests/manifest.json",
        manifestIndexPath: "artifacts/manifests/index.json",
        manifest: {
          buildId: "aabbccddeeff0011",
          entries: ["src/index.ts"],
          bundles: [],
          types: [],
          tsconfigPath: "tsconfig.json",
        },
        diagnostics: [],
      }),
    /compile-input contract violation: manifest\.bundles must include at least one JS bundle; manifest\.bundles\[0\]\.file must be a non-empty string/,
  );
});

test("orchestratePipelineStages emits deterministic stage events through delivery stream", async () => {
  const stages: string[] = [];

  await orchestratePipelineStages({
    cwd: process.cwd(),
    configs: [{ entry: "src/index.ts" }],
    log: () => {},
    onStage: (event) => stages.push(event.stage),
    runBuildArtifacts: async () => ({
      mode: "rust-engine-adapter",
      manifestPath: "artifacts/manifest.json",
      manifestIndexPath: "artifacts/manifest.index.json",
      manifest: {
        buildId: "build-1",
        entries: ["src/index.ts"],
        bundles: [{ file: "dist/index.js" }],
        types: [],
        tsconfigPath: "tsconfig.json",
      },
      diagnostics: [],
    }),
  });

  assert.deepEqual(stages, [
    "BUILD_ARTIFACTS",
    "BUILD_IR",
    "CAPABILITY_GATE",
    "EMIT_GO",
    "ON_SUCCESS",
  ]);
});
