import assert from "node:assert/strict";
import test from "node:test";

import { buildProgramIrFromArtifacts } from "../src/internal/artifact-to-ir.ts";

test("buildProgramIrFromArtifacts emits deterministic handler semantics envelope", () => {
  const buildResult = {
    mode: "rust-engine-adapter",
    manifestPath: "artifacts/manifests/manifest.json",
    manifestIndexPath: "artifacts/manifests/index.json",
    manifest: {
      buildId: "aabbccddeeff0011",
      entries: ["src/index.ts"],
      bundles: [
        {
          file: "dist/index.mjs",
          map: "dist/index.mjs.map",
          format: "esm",
          exports: [],
        },
      ],
      types: ["dist/index.d.ts"],
      tsconfigPath: "tsconfig.json",
    },
    diagnostics: [],
  } as const;

  const ir = buildProgramIrFromArtifacts(buildResult, "src/index.ts");

  assert.equal(ir.handlers.length, 1);
  assert.deepEqual(ir.handlers[0]?.semantics, {
    responseMode: "return",
    usesStatus: false,
    usesBody: false,
    usesHeaders: false,
    usesJson: false,
  });
});
