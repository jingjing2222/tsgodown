import assert from "node:assert/strict";
import test from "node:test";

import { buildTargetResult } from "../src/internal/target-result.ts";

test("buildTargetResult keeps plan fields and appends default diagnostics", () => {
  const result = buildTargetResult(
    {
      configIndex: 0,
      entry: "/repo/src/index.ts",
      outDir: "/repo/dist-go",
      artifact: "/repo/artifacts/manifests/manifest.json",
    },
    true,
  );

  assert.equal(result.configIndex, 0);
  assert.equal(result.entry, "/repo/src/index.ts");
  assert.equal(result.outDir, "/repo/dist-go");
  assert.equal(result.artifact, "/repo/artifacts/manifests/manifest.json");
  assert.equal(result.emitted, true);
  assert.deepEqual(result.diagnostics, {
    routes: 0,
    warnings: [
      "DEPRECATED: TS core analyzer diagnostics are disabled after Rust cutover; use IR diagnostics from the Rust engine.",
    ],
  });
});
