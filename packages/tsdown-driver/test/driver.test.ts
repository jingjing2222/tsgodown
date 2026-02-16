import assert from "node:assert/strict";
import test from "node:test";

import { buildManifestFromBundles, runBuild } from "../src/index.ts";

test("buildManifestFromBundles maps real bundle, sourcemap and d.ts outputs", () => {
  const cwd = "/repo";
  const manifest = buildManifestFromBundles(cwd, [
    {
      chunks: [
        { fileName: "dist/index.mjs" },
        { fileName: "dist/index.mjs.map" },
        { fileName: "dist/index.d.ts" },
        { fileName: "dist/index.cjs" },
      ],
      config: {
        entry: { index: "/repo/src/index.ts" },
        tsconfig: "/repo/tsconfig.build.json",
      },
    },
  ]);

  assert.deepEqual(manifest.entries, ["src/index.ts"]);
  assert.deepEqual(manifest.bundles, [
    {
      file: "dist/index.cjs",
      map: undefined,
      format: "cjs",
      exports: [],
    },
    {
      file: "dist/index.mjs",
      map: "dist/index.mjs.map",
      format: "esm",
      exports: [],
    },
  ]);
  assert.deepEqual(manifest.types, ["dist/index.d.ts"]);
  assert.equal(manifest.tsconfigPath, "tsconfig.build.json");
  assert.match(manifest.buildId, /^[a-f0-9]{16}$/);
});

test("runBuild reports source and exact cause chain on tsdown failure", async () => {
  const rootCause = new Error("ENOENT: missing entry src/index.ts");
  const tsdownError = new Error("config resolution failed", {
    cause: rootCause,
  });

  await assert.rejects(
    runBuild("/repo", "tsdown.config.ts", {
      executeBuild: async () => {
        throw tsdownError;
      },
    }),
    (error: unknown) => {
      assert.ok(error instanceof Error);
      assert.match(error.message, /source=tsdown\.build/);
      assert.match(error.message, /config=tsdown\.config\.ts/);
      assert.match(error.message, /Error: config resolution failed/);
      assert.match(
        error.message,
        /cause: Error: ENOENT: missing entry src\/index\.ts/,
      );
      return true;
    },
  );
});
