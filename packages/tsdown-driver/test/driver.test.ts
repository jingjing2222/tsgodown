import assert from "node:assert/strict";
import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import test from "node:test";

import {
  type RustEngineRequest,
  type RustEngineResponse,
  assertManifestIndexContract,
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

test("assertManifestIndexContract rejects malformed artifact index payloads with actionable details", () => {
  const manifest = {
    buildId: "aabbccddeeff0011",
    entries: ["src/index.ts"],
    bundles: [
      {
        file: "dist/index.mjs",
        map: "dist/index.mjs.map",
        format: "esm" as const,
        exports: [],
      },
    ],
    types: ["dist/index.d.ts"],
    tsconfigPath: "tsconfig.json",
  };

  assert.throws(
    () =>
      assertManifestIndexContract(manifest, {
        buildId: "ffeeddccbbaa9988",
        manifest: "manifest-v2.json",
        generatedAt: "not-a-date",
      }),
    /artifact contract violation: manifest index buildId mismatch .*manifest index manifest must equal "manifest\.json" .*generatedAt must be ISO-8601 parseable/s,
  );
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
    assert.equal(
      result.manifestIndexPath,
      path.join(cwd, "artifacts", "manifests", "index.json"),
    );
    assert.equal(result.manifest.buildId, "aabbccddeeff0011");
    assert.deepEqual(result.diagnostics, ["adapter=stub"]);

    const indexPayload = JSON.parse(
      fs.readFileSync(result.manifestIndexPath, "utf8"),
    ) as { buildId: string; manifest: string; generatedAt: string };
    assert.equal(indexPayload.buildId, "aabbccddeeff0011");
    assert.equal(indexPayload.manifest, "manifest.json");
    assert.equal(typeof indexPayload.generatedAt, "string");
  } finally {
    fs.rmSync(cwd, { recursive: true, force: true });
  }
});

test("runBuild fails with explicit diagnostic when rust engine bin is missing", async () => {
  const prev = process.env.TSGODOWN_RUST_ENGINE_BIN;
  Reflect.deleteProperty(process.env, "TSGODOWN_RUST_ENGINE_BIN");

  try {
    await assert.rejects(
      runBuild("/repo", "tsdown.config.ts"),
      (error: unknown) => {
        assert.ok(error instanceof Error);
        assert.match(error.message, /source=rust-engine-bin-env/);
        assert.match(
          error.message,
          /cause=TSGODOWN_RUST_ENGINE_BIN is not set/,
        );
        assert.match(
          error.message,
          /guidance=Set TSGODOWN_RUST_ENGINE_BIN to the Rust engine executable path\./,
        );
        return true;
      },
    );
  } finally {
    if (prev === undefined) {
      Reflect.deleteProperty(process.env, "TSGODOWN_RUST_ENGINE_BIN");
    } else {
      process.env.TSGODOWN_RUST_ENGINE_BIN = prev;
    }
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

test("runBuild accepts status-envelope success variants and normalizes diagnostics", async () => {
  const fixtures: Array<{
    name: string;
    status: "ok" | "success";
    buildId: string;
  }> = [
    { name: "status=ok", status: "ok", buildId: "0011223344556677" },
    {
      name: "status=success",
      status: "success",
      buildId: "8899aabbccddeeff",
    },
  ];

  for (const fixture of fixtures) {
    const cwd = fs.mkdtempSync(path.join(os.tmpdir(), "tsgodown-driver-test-"));

    try {
      const result = await runBuild(cwd, "tsdown.config.ts", {
        executeRustEngine: async () => ({
          status: fixture.status,
          manifest: {
            buildId: fixture.buildId,
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
          diagnostics: ["  keep-me  ", "", 42],
        }),
      });

      assert.equal(
        result.manifest.buildId,
        fixture.buildId,
        `manifest.buildId drifted for fixture: ${fixture.name}`,
      );
      assert.deepEqual(
        result.diagnostics,
        ["keep-me"],
        `diagnostics normalization drifted for fixture: ${fixture.name}`,
      );
    } finally {
      fs.rmSync(cwd, { recursive: true, force: true });
    }
  }
});

test("runBuild maps malformed responses to deterministic contract errors", async () => {
  const fixtures: Array<{
    name: string;
    response: unknown;
    expectedCause: RegExp;
  }> = [
    {
      name: "invalid status value",
      response: { status: "maybe" },
      expectedCause: /cause=missing or invalid status envelope/,
    },
    {
      name: "non-object payload",
      response: "not-json-object",
      expectedCause: /cause=missing or invalid status envelope/,
    },
    {
      name: "ok envelope missing manifest",
      response: { status: "ok" },
      expectedCause: /cause=ok envelope missing valid manifest payload/,
    },
  ];

  for (const fixture of fixtures) {
    await assert.rejects(
      runBuild("/repo", "tsdown.config.ts", {
        executeRustEngine: async () => fixture.response as RustEngineResponse,
      }),
      (error: unknown) => {
        assert.ok(error instanceof Error);
        assert.match(
          error.message,
          /source=rust-engine-binary-contract/,
          `source drifted for fixture: ${fixture.name}`,
        );
        assert.match(
          error.message,
          fixture.expectedCause,
          `cause drifted for fixture: ${fixture.name}`,
        );
        assert.match(
          error.message,
          /guidance=Ensure rust engine returns deterministic status or ok envelope\./,
          `guidance drifted for fixture: ${fixture.name}`,
        );
        return true;
      },
      `fixture should fail: ${fixture.name}`,
    );
  }
});

test("runBuild maps failed status-envelope variants to deterministic source/cause/guidance", async () => {
  const fixtures: Array<{
    name: string;
    response: RustEngineResponse;
    source: RegExp;
    cause: RegExp;
    guidance: RegExp;
  }> = [
    {
      name: "status=failed top-level fallbacks",
      response: {
        status: "failed",
        source: "  ",
        cause: "",
      },
      source: /source=rust-engine-binary/,
      cause: /cause=rust engine returned error without cause/,
      guidance:
        /guidance=Inspect rust engine logs and JSON response contract\./,
    },
    {
      name: "status=error nested error object",
      response: {
        status: "error",
        error: {
          source: "rust-engine-adapter",
          cause: "invalid graph",
          guidance: "Retry with fixed config.",
        },
      },
      source: /source=rust-engine-adapter/,
      cause: /cause=invalid graph/,
      guidance: /guidance=Retry with fixed config\./,
    },
  ];

  for (const fixture of fixtures) {
    await assert.rejects(
      runBuild("/repo", "tsdown.config.ts", {
        executeRustEngine: async () => fixture.response,
      }),
      (error: unknown) => {
        assert.ok(error instanceof Error);
        assert.match(
          error.message,
          fixture.source,
          `source drifted for fixture: ${fixture.name}`,
        );
        assert.match(
          error.message,
          fixture.cause,
          `cause drifted for fixture: ${fixture.name}`,
        );
        assert.match(
          error.message,
          fixture.guidance,
          `guidance drifted for fixture: ${fixture.name}`,
        );
        return true;
      },
      `fixture should fail: ${fixture.name}`,
    );
  }
});
