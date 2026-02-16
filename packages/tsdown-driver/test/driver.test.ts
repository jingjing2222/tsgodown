import assert from "node:assert/strict";
import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import test from "node:test";

import {
  type RustEngineRequest,
  buildManifestFromBundles,
  runBuild,
} from "../src/index.ts";

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

test("runBuild invokes rust adapter with JSON request contract", async () => {
  let seenRequest: RustEngineRequest | undefined;
  const cwd = fs.mkdtempSync(path.join(os.tmpdir(), "tsgodown-driver-test-"));

  try {
    const result = await runBuild(cwd, "tsdown.config.ts", {
      executeRustEngine: async (request) => {
        seenRequest = request;
        return {
          ok: true,
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
          diagnostics: ["adapter=stub"],
        };
      },
    });

    assert.deepEqual(seenRequest, {
      action: "build",
      cwd,
      configPath: "tsdown.config.ts",
    });
    assert.equal(result.mode, "rust-engine-adapter");
    assert.equal(
      result.manifestPath,
      path.join(cwd, "artifacts", "manifests", "manifest.json"),
    );
    assert.equal(result.manifest.buildId, "aabbccddeeff0011");
    assert.deepEqual(result.diagnostics, ["adapter=stub"]);
  } finally {
    fs.rmSync(cwd, { recursive: true, force: true });
  }
});

test("runBuild reports rust engine error in source/cause/guidance format", async () => {
  await assert.rejects(
    runBuild("/repo", "tsdown.config.ts", {
      executeRustEngine: async () => ({
        ok: false,
        error: {
          source: "rust-engine-adapter",
          cause: "ENOENT: missing entry src/index.ts",
          guidance: "Verify tsgodown.config.ts entry path and file existence.",
        },
      }),
    }),
    (error: unknown) => {
      assert.ok(error instanceof Error);
      assert.match(error.message, /source=rust-engine-adapter/);
      assert.match(error.message, /cause=ENOENT: missing entry src\/index\.ts/);
      assert.match(
        error.message,
        /guidance=Verify tsgodown\.config\.ts entry path and file existence\./,
      );
      return true;
    },
  );
});
