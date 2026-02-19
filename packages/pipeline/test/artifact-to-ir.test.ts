import assert from "node:assert/strict";
import fs from "node:fs";
import os from "node:os";
import path from "node:path";
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

test("buildProgramIrFromArtifacts enriches module exports from d.ts and source location from sourcemap", () => {
  const cwd = fs.mkdtempSync(path.join(os.tmpdir(), "tsgodown-artifact-ir-"));
  const manifestDir = path.join(cwd, "artifacts", "manifests");
  const distDir = path.join(cwd, "dist");

  fs.mkdirSync(manifestDir, { recursive: true });
  fs.mkdirSync(distDir, { recursive: true });

  fs.writeFileSync(path.join(distDir, "index.d.ts"), "export declare function health(): string;\nexport declare const version: string;\n", "utf8");
  fs.writeFileSync(
    path.join(distDir, "index.mjs.map"),
    JSON.stringify({ version: 3, file: "index.mjs", sources: ["src/server.ts"], names: [], mappings: "" }),
    "utf8",
  );

  const ir = buildProgramIrFromArtifacts(
    {
      mode: "rust-engine-adapter",
      manifestPath: path.join(manifestDir, "manifest.json"),
      manifestIndexPath: path.join(manifestDir, "index.json"),
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
      },
      diagnostics: [],
    },
    "src/index.ts",
  );

  assert.deepEqual(ir.modules[0]?.exports, ["health", "version"]);
  assert.equal(ir.modules[0]?.sourcePath, "src/server.ts");
  assert.equal(ir.diagnostics.length, 0);
});

test("buildProgramIrFromArtifacts emits deterministic diagnostics for missing and invalid sourcemaps", () => {
  const cwd = fs.mkdtempSync(path.join(os.tmpdir(), "tsgodown-artifact-ir-"));
  const manifestDir = path.join(cwd, "artifacts", "manifests");
  const distDir = path.join(cwd, "dist");

  fs.mkdirSync(manifestDir, { recursive: true });
  fs.mkdirSync(distDir, { recursive: true });

  fs.writeFileSync(path.join(distDir, "index.d.ts"), "export declare function health(): string;\n", "utf8");
  fs.writeFileSync(path.join(distDir, "bad.mjs.map"), "{not-json", "utf8");

  const ir = buildProgramIrFromArtifacts(
    {
      mode: "rust-engine-adapter",
      manifestPath: path.join(manifestDir, "manifest.json"),
      manifestIndexPath: path.join(manifestDir, "index.json"),
      manifest: {
        buildId: "aabbccddeeff0011",
        entries: ["src/index.ts"],
        bundles: [
          {
            file: "dist/index.mjs",
            format: "esm",
            exports: [],
          },
          {
            file: "dist/bad.mjs",
            map: "dist/bad.mjs.map",
            format: "esm",
            exports: [],
          },
        ],
        types: ["dist/index.d.ts"],
      },
      diagnostics: [],
    },
    "src/index.ts",
  );

  assert.deepEqual(
    ir.diagnostics.map((diag) => diag.code),
    ["ARTIFACT_SOURCEMAP_INVALID", "ARTIFACT_SOURCEMAP_MISSING"],
  );
  assert.equal(ir.diagnostics[0]?.source?.file, "dist/bad.mjs.map");
  assert.equal(ir.diagnostics[0]?.source?.viaSourceMap, true);
});
