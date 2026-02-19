import fs from "node:fs";
import os from "node:os";
import path from "node:path";

import assert from "node:assert/strict";
import test from "node:test";

import { buildProgramIrFromArtifacts } from "../src/internal/artifact-to-ir.ts";

test("buildProgramIrFromArtifacts falls back to resolved entry when manifest entries are empty", () => {
  const ir = buildProgramIrFromArtifacts(
    {
      mode: "rust-engine-adapter",
      manifestPath: "artifacts/manifests/manifest.json",
      manifestIndexPath: "artifacts/manifests/index.json",
      manifest: {
        buildId: "aabbccddeeff0011",
        entries: [],
        bundles: [{ file: "dist/index.mjs", format: "esm", exports: [] }],
      },
      diagnostics: [],
    },
    "src/index.ts",
  );

  assert.equal(ir.modules.length, 1);
  assert.equal(ir.modules[0]?.sourcePath, "src/index.ts");
});

test("buildProgramIrFromArtifacts ingests d.ts and sourcemap into deterministic typed module metadata", () => {
  const cwd = fs.mkdtempSync(path.join(os.tmpdir(), "tsgodown-artifact-ir-"));
  fs.mkdirSync(path.join(cwd, "dist"), { recursive: true });

  fs.writeFileSync(
    path.join(cwd, "dist", "index.d.ts"),
    [
      "export declare function zed(req: unknown): Promise<void>;",
      "export declare const alpha: () => { ok: boolean };",
      "",
    ].join("\n"),
  );

  fs.writeFileSync(
    path.join(cwd, "dist", "index.mjs.map"),
    JSON.stringify({
      version: 3,
      file: "index.mjs",
      sources: ["../src/z-route.ts", "../src/a-route.ts", "../src/z-route.ts"],
      names: [],
      mappings: "",
    }),
  );

  const ir = buildProgramIrFromArtifacts(
    {
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
            exports: ["zed", "alpha"],
          },
        ],
        types: ["dist/index.d.ts"],
      },
      diagnostics: [],
    },
    "src/index.ts",
    { cwd },
  );

  assert.deepEqual(
    ir.modules.map((module) => module.sourcePath),
    ["src/a-route.ts", "src/z-route.ts"],
  );
  assert.deepEqual(ir.modules[0]?.exports, ["alpha", "zed"]);
  assert.deepEqual(ir.diagnostics, []);
});

test("buildProgramIrFromArtifacts emits deterministic diagnostics for missing/invalid typed mapping metadata", () => {
  const cwd = fs.mkdtempSync(
    path.join(os.tmpdir(), "tsgodown-artifact-ir-diag-"),
  );
  fs.mkdirSync(path.join(cwd, "dist"), { recursive: true });

  fs.writeFileSync(path.join(cwd, "dist", "broken.map"), "{oops");

  const ir = buildProgramIrFromArtifacts(
    {
      mode: "rust-engine-adapter",
      manifestPath: "artifacts/manifests/manifest.json",
      manifestIndexPath: "artifacts/manifests/index.json",
      manifest: {
        buildId: "aabbccddeeff0011",
        entries: ["src/index.ts"],
        bundles: [
          {
            file: "dist/index.mjs",
            map: "dist/broken.map",
            format: "esm",
            exports: [],
          },
        ],
      },
      diagnostics: [],
    },
    "src/index.ts",
    { cwd },
  );

  assert.deepEqual(
    ir.diagnostics.map((diag) => diag.code),
    ["PIPELINE_INVALID_SOURCEMAP_MAPPING", "PIPELINE_MISSING_TYPES_METADATA"],
  );
  assert.equal(ir.diagnostics[0]?.source?.viaSourceMap, true);
});
