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

test("buildProgramIrFromArtifacts resolves sourcemap sourceRoot deterministically for module locations", () => {
  const cwd = fs.mkdtempSync(
    path.join(os.tmpdir(), "tsgodown-artifact-ir-root-"),
  );
  fs.mkdirSync(path.join(cwd, "dist", "maps"), { recursive: true });

  fs.writeFileSync(
    path.join(cwd, "dist", "index.d.ts"),
    "export declare const ok: true;\n",
  );
  fs.writeFileSync(
    path.join(cwd, "dist", "maps", "index.mjs.map"),
    JSON.stringify({
      version: 3,
      file: "../index.mjs",
      sourceRoot: "../../src",
      sources: ["routes/a.ts", "routes/b.ts", "routes/a.ts"],
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
            map: "dist/maps/index.mjs.map",
            format: "esm",
            exports: ["ok"],
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
    ["src/routes/a.ts", "src/routes/b.ts"],
  );
});

test("buildProgramIrFromArtifacts resolves indexed sourcemap sections deterministically for module locations", () => {
  const cwd = fs.mkdtempSync(
    path.join(os.tmpdir(), "tsgodown-artifact-ir-indexed-"),
  );
  fs.mkdirSync(path.join(cwd, "dist", "maps"), { recursive: true });

  fs.writeFileSync(
    path.join(cwd, "dist", "index.d.ts"),
    "export declare const ok: true;\n",
  );
  fs.writeFileSync(
    path.join(cwd, "dist", "maps", "index.mjs.map"),
    JSON.stringify({
      version: 3,
      file: "../index.mjs",
      sections: [
        {
          offset: { line: 0, column: 0 },
          map: {
            version: 3,
            sourceRoot: "../../src",
            sources: ["routes/z.ts", "routes/a.ts"],
            mappings: "",
          },
        },
        {
          offset: { line: 10, column: 0 },
          map: {
            version: 3,
            sourceRoot: "../../src",
            sources: ["routes/a.ts", "routes/m.ts"],
            mappings: "",
          },
        },
      ],
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
            map: "dist/maps/index.mjs.map",
            format: "esm",
            exports: ["ok"],
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
    ["src/routes/a.ts", "src/routes/m.ts", "src/routes/z.ts"],
  );
  assert.deepEqual(ir.diagnostics, []);
});

test("buildProgramIrFromArtifacts emits deterministic diagnostics for indexed sourcemap sections with sparse offsets and missing section sources", () => {
  const cwd = fs.mkdtempSync(
    path.join(os.tmpdir(), "tsgodown-artifact-ir-indexed-sparse-"),
  );
  fs.mkdirSync(path.join(cwd, "dist", "maps"), { recursive: true });

  fs.writeFileSync(
    path.join(cwd, "dist", "index.d.ts"),
    "export declare const ok: true;\n",
  );
  fs.writeFileSync(
    path.join(cwd, "dist", "maps", "index.mjs.map"),
    JSON.stringify({
      version: 3,
      file: "../index.mjs",
      sections: [
        {
          offset: {},
          map: {
            version: 3,
            sourceRoot: "../../src",
            sources: ["routes/z.ts"],
            mappings: "",
          },
        },
        {
          offset: { line: 4 },
          map: {
            version: 3,
            sourceRoot: "../../src",
            sources: ["routes/a.ts"],
            mappings: "",
          },
        },
        {
          offset: { line: 8 },
          map: {
            version: 3,
            sourceRoot: "../../src",
            names: [],
            mappings: "",
          },
        },
      ],
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
            map: "dist/maps/index.mjs.map",
            format: "esm",
            exports: ["ok"],
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
    ["src/routes/a.ts", "src/routes/z.ts"],
  );
  assert.deepEqual(
    ir.diagnostics.filter(
      (diag) => diag.code === "PIPELINE_SOURCEMAP_POSITION_PARTIAL",
    ),
    [
      {
        level: "warn",
        code: "PIPELINE_SOURCEMAP_POSITION_PARTIAL",
        message:
          "indexed sourcemap section offset is partial; diagnostics remain file-scoped for deterministic mapping",
        source: {
          file: "dist/maps/index.mjs.map",
          viaSourceMap: true,
        },
      },
      {
        level: "warn",
        code: "PIPELINE_SOURCEMAP_POSITION_PARTIAL",
        message:
          "indexed sourcemap section offset is partial; diagnostics remain file-scoped for deterministic mapping",
        source: {
          file: "dist/maps/index.mjs.map",
          viaSourceMap: true,
          line: 5,
        },
      },
      {
        level: "warn",
        code: "PIPELINE_SOURCEMAP_POSITION_PARTIAL",
        message:
          "indexed sourcemap section offset is partial; diagnostics remain file-scoped for deterministic mapping",
        source: {
          file: "dist/maps/index.mjs.map",
          viaSourceMap: true,
          line: 9,
        },
      },
    ],
  );

  assert.deepEqual(
    ir.diagnostics.filter(
      (diag) => diag.code === "PIPELINE_INVALID_SOURCEMAP_MAPPING",
    ),
    [
      {
        level: "warn",
        code: "PIPELINE_INVALID_SOURCEMAP_MAPPING",
        message:
          "indexed sourcemap section map is missing sources[]; section ignored for deterministic mapping",
        source: {
          file: "dist/maps/index.mjs.map",
          viaSourceMap: true,
          line: 9,
        },
      },
    ],
  );
});

test("buildProgramIrFromArtifacts normalizes absolute sourceRoot with mixed relative segments into stable repo-relative module paths", () => {
  const cwd = fs.mkdtempSync(
    path.join(os.tmpdir(), "tsgodown-artifact-ir-abs-root-"),
  );
  fs.mkdirSync(path.join(cwd, "dist", "maps"), { recursive: true });

  fs.writeFileSync(
    path.join(cwd, "dist", "index.d.ts"),
    "export declare const ok: true;\n",
  );

  fs.writeFileSync(
    path.join(cwd, "dist", "maps", "index.mjs.map"),
    JSON.stringify({
      version: 3,
      file: "../index.mjs",
      sourceRoot: path.join(cwd, "src", "nested", ".."),
      sources: [
        "./routes/../health.ts",
        "./routes/users/../list.ts",
        "./routes/../health.ts",
      ],
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
            map: "dist/maps/index.mjs.map",
            format: "esm",
            exports: ["ok"],
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
    ["src/health.ts", "src/routes/list.ts"],
  );
  assert.deepEqual(ir.diagnostics, []);
});

test("buildProgramIrFromArtifacts emits deterministic file-only diagnostics for sparse indexed sourcemap sections", () => {
  const cwd = fs.mkdtempSync(
    path.join(os.tmpdir(), "tsgodown-artifact-ir-indexed-sparse-"),
  );
  fs.mkdirSync(path.join(cwd, "dist", "maps"), { recursive: true });

  fs.writeFileSync(
    path.join(cwd, "dist", "index.d.ts"),
    "export declare const ok: true;\n",
  );
  fs.writeFileSync(
    path.join(cwd, "dist", "maps", "index.mjs.map"),
    JSON.stringify({
      version: 3,
      file: "../index.mjs",
      sections: [
        {
          offset: { line: 0, column: 0 },
          map: {
            version: 3,
            sourceRoot: "../../src",
            sources: [null, "", "routes/health.ts"],
            mappings: ";;;",
          },
        },
      ],
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
            map: "dist/maps/index.mjs.map",
            format: "esm",
            exports: ["ok"],
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
    ["src/routes/health.ts"],
  );
  assert.deepEqual(
    ir.diagnostics
      .filter((diag) => diag.code === "PIPELINE_SOURCEMAP_SPARSE_MAPPING")
      .map((diag) => diag.source),
    [{ file: "dist/maps/index.mjs.map", viaSourceMap: true }],
  );
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
  assert.equal(ir.diagnostics[0]?.source?.line, 1);
  assert.equal(ir.diagnostics[0]?.source?.column, 1);
});

test("buildProgramIrFromArtifacts emits missing-map diagnostics with deterministic bundle source location", () => {
  const cwd = fs.mkdtempSync(
    path.join(os.tmpdir(), "tsgodown-artifact-ir-nomap-"),
  );
  fs.mkdirSync(path.join(cwd, "dist"), { recursive: true });

  const ir = buildProgramIrFromArtifacts(
    {
      mode: "rust-engine-adapter",
      manifestPath: "artifacts/manifests/manifest.json",
      manifestIndexPath: "artifacts/manifests/index.json",
      manifest: {
        buildId: "aabbccddeeff0011",
        entries: ["src/index.ts"],
        bundles: [{ file: "dist/index.mjs", format: "esm", exports: [] }],
      },
      diagnostics: [],
    },
    "src/index.ts",
    { cwd },
  );

  const missingMap = ir.diagnostics.find(
    (diag) => diag.code === "PIPELINE_MISSING_SOURCEMAP_MAPPING",
  );
  assert.ok(missingMap);
  assert.deepEqual(missingMap.source, {
    file: "dist/index.mjs",
    viaSourceMap: true,
    line: 1,
    column: 1,
  });
});

test("buildProgramIrFromArtifacts discovers encoded d.ts sourceMappingURL map paths when manifest omits declared map and keeps export type references stable", () => {
  const cwd = fs.mkdtempSync(
    path.join(os.tmpdir(), "tsgodown-artifact-ir-encoded-dts-map-"),
  );
  fs.mkdirSync(path.join(cwd, "dist", "maps"), { recursive: true });

  fs.writeFileSync(
    path.join(cwd, "dist", "index.mjs"),
    [
      "export const health = () => ({ ok: true });",
      "//# sourceMappingURL=maps/index.mjs.map",
      "",
    ].join("\n"),
  );

  fs.writeFileSync(
    path.join(cwd, "dist", "index.d.ts"),
    [
      "export declare const health: () => { ok: boolean };",
      "export type HealthResponse = { ok: boolean };",
      "//# sourceMappingURL=maps/index.d.ts.map%3Fcache%3Dv1%23types",
      "",
    ].join("\n"),
  );

  fs.writeFileSync(
    path.join(cwd, "dist", "maps", "index.mjs.map"),
    JSON.stringify({
      version: 3,
      file: "../index.mjs",
      sourceRoot: "../../src",
      sources: ["routes/health.ts"],
      names: [],
      mappings: "",
    }),
  );

  fs.writeFileSync(
    path.join(cwd, "dist", "maps", "index.d.ts.map"),
    JSON.stringify({
      version: 3,
      file: "../index.d.ts",
      sourceRoot: "../../src",
      sources: ["types/contracts.ts"],
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
            map: "dist/maps/index.mjs.map",
            format: "esm",
            exports: ["health"],
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
    ["src/routes/health.ts", "src/types/contracts.ts"],
  );
  assert.deepEqual(ir.modules[0]?.exports, ["health", "HealthResponse"]);
  assert.equal(
    ir.diagnostics.some(
      (diag) => diag.code === "PIPELINE_INVALID_SOURCEMAP_MAPPING",
    ),
    false,
  );
});

test("buildProgramIrFromArtifacts keeps deterministic module provenance ordering when JS + multiple d.ts sourcemaps contain duplicate logical paths in different relative forms", () => {
  const cwd = fs.mkdtempSync(
    path.join(os.tmpdir(), "tsgodown-artifact-ir-duplicate-relative-forms-"),
  );
  fs.mkdirSync(path.join(cwd, "dist", "maps"), { recursive: true });

  fs.writeFileSync(
    path.join(cwd, "dist", "index.mjs"),
    ["export const health = () => ({ ok: true });", ""].join("\n"),
  );
  fs.writeFileSync(
    path.join(cwd, "dist", "index.d.ts"),
    [
      "export declare const health: () => { ok: boolean };",
      "//# sourceMappingURL=maps/index.d.ts.map?cache=v1#types",
      "",
    ].join("\n"),
  );
  fs.writeFileSync(
    path.join(cwd, "dist", "users.d.ts"),
    [
      "export declare const listUsers: () => Array<{ id: string }>;",
      "//# sourceMappingURL=maps/users.d.ts.map#extra",
      "",
    ].join("\n"),
  );

  const sourceRoot = new URL(`file://${path.join(cwd, "src")}/`).toString();
  fs.writeFileSync(
    path.join(cwd, "dist", "maps", "index.mjs.map"),
    JSON.stringify({
      version: 3,
      file: "../index.mjs",
      sourceRoot,
      sources: ["routes/health.ts", "routes/../routes/health.ts"],
      names: [],
      mappings: "",
    }),
  );

  fs.writeFileSync(
    path.join(cwd, "dist", "maps", "index.d.ts.map"),
    JSON.stringify({
      version: 3,
      file: "../index.d.ts",
      sourceRoot,
      sources: ["./routes/health.ts", "./types/http.ts"],
      names: [],
      mappings: "",
    }),
  );

  fs.writeFileSync(
    path.join(cwd, "dist", "maps", "users.d.ts.map"),
    JSON.stringify({
      version: 3,
      file: "../users.d.ts",
      sourceRoot: `${sourceRoot}nested/..`,
      sources: [
        new URL(
          `file://${path.join(cwd, "src", "routes", "health.ts")}`,
        ).toString(),
        "types/./users.ts",
      ],
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
        buildId: "0102030405060708",
        entries: ["src/index.ts"],
        bundles: [
          {
            file: "dist/index.mjs",
            map: "dist/maps/index.mjs.map",
            format: "esm",
            exports: ["health"],
          },
        ],
        types: ["dist/index.d.ts", "dist/users.d.ts"],
      },
      diagnostics: [],
    },
    "src/index.ts",
    { cwd },
  );

  assert.deepEqual(
    ir.modules.map((module) => module.sourcePath),
    ["src/routes/health.ts", "src/types/http.ts", "src/types/users.ts"],
  );
});

test("buildProgramIrFromArtifacts deduplicates mixed JS + d.ts sourcemap provenance when logical source paths collide across relative/absolute + sourceRoot + query/hash forms", () => {
  const cwd = fs.mkdtempSync(
    path.join(os.tmpdir(), "tsgodown-artifact-ir-query-hash-collision-"),
  );
  fs.mkdirSync(path.join(cwd, "dist", "maps"), { recursive: true });

  fs.writeFileSync(
    path.join(cwd, "dist", "index.mjs"),
    [
      "export const health = () => ({ ok: true });",
      "//# sourceMappingURL=maps/index.mjs.map",
      "",
    ].join("\n"),
  );
  fs.writeFileSync(
    path.join(cwd, "dist", "index.d.ts"),
    [
      "export declare const health: () => { ok: boolean };",
      "export declare const status: () => number;",
      "//# sourceMappingURL=maps/index.d.ts.map?cache=v2#types",
      "",
    ].join("\n"),
  );

  const absHealthPath = path.join(cwd, "src", "routes", "health.ts");
  fs.writeFileSync(
    path.join(cwd, "dist", "maps", "index.mjs.map"),
    JSON.stringify({
      version: 3,
      file: "../index.mjs",
      sourceRoot: new URL(`file://${path.join(cwd, "src")}/`).toString(),
      sources: ["routes/health.ts?from=js#frag"],
      names: [],
      mappings: "",
    }),
  );

  fs.writeFileSync(
    path.join(cwd, "dist", "maps", "index.d.ts.map"),
    JSON.stringify({
      version: 3,
      file: "../index.d.ts",
      sourceRoot: path.join(cwd, "src", "nested", ".."),
      sources: [
        `${absHealthPath}?from=types#decl`,
        "./routes/../routes/health.ts#canonical",
      ],
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
        buildId: "1122334455667788",
        entries: ["src/index.ts"],
        bundles: [
          {
            file: "dist/index.mjs",
            map: "dist/maps/index.mjs.map",
            format: "esm",
            exports: ["health"],
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
    ["src/routes/health.ts"],
  );
  assert.deepEqual(ir.modules[0]?.exports, ["health", "status"]);
});

test("buildProgramIrFromArtifacts keeps deterministic dedupe/order when JS+d.ts sourcemaps overlap relative/absolute sources under file:// sourceRoot containing encoded # segments", () => {
  const cwd = fs.mkdtempSync(
    path.join(os.tmpdir(), "tsgodown-artifact-ir-overlap-encoded-hash-root-"),
  );

  const sourceRootDir = path.join(cwd, "src#v1");
  fs.mkdirSync(path.join(cwd, "dist", "maps"), { recursive: true });
  fs.mkdirSync(path.join(sourceRootDir, "routes"), { recursive: true });
  fs.mkdirSync(path.join(sourceRootDir, "types"), { recursive: true });

  fs.writeFileSync(
    path.join(cwd, "dist", "index.mjs"),
    [
      "export const health = () => ({ ok: true });",
      "//# sourceMappingURL=maps/index.mjs.map",
      "",
    ].join("\n"),
  );

  fs.writeFileSync(
    path.join(cwd, "dist", "index.d.ts"),
    [
      "export declare const health: () => { ok: boolean };",
      "export declare type HealthType = { ok: boolean };",
      "//# sourceMappingURL=maps/index.d.ts.map?rev=17#types",
      "",
    ].join("\n"),
  );

  const encodedSourceRoot = new URL(
    `file://${sourceRootDir.replace("#", "%23")}/`,
  ).toString();
  const absoluteHealthPath = path.join(sourceRootDir, "routes", "health.ts");

  fs.writeFileSync(
    path.join(cwd, "dist", "maps", "index.mjs.map"),
    JSON.stringify({
      version: 3,
      file: "../index.mjs",
      sourceRoot: encodedSourceRoot,
      sources: [
        "routes/health.ts",
        new URL(`file://${absoluteHealthPath.replace("#", "%23")}`).toString(),
      ],
      names: [],
      mappings: "",
    }),
  );

  fs.writeFileSync(
    path.join(cwd, "dist", "maps", "index.d.ts.map"),
    JSON.stringify({
      version: 3,
      file: "../index.d.ts",
      sourceRoot: encodedSourceRoot,
      sources: ["./routes/./health.ts", "types/http.ts"],
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
        buildId: "1717171717171717",
        entries: ["src/index.ts"],
        bundles: [
          {
            file: "dist/index.mjs",
            map: "dist/maps/index.mjs.map",
            format: "esm",
            exports: ["health"],
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
    ["src#v1/routes/health.ts", "src#v1/types/http.ts"],
  );
});

test("buildProgramIrFromArtifacts keeps indexed sourcemap source provenance deterministic when sections mix inherited sourceRoot and relative source paths", () => {
  const cwd = fs.mkdtempSync(
    path.join(os.tmpdir(), "tsgodown-artifact-ir-indexed-mixed-root-"),
  );
  fs.mkdirSync(path.join(cwd, "dist", "maps"), { recursive: true });

  fs.writeFileSync(
    path.join(cwd, "dist", "index.d.ts"),
    "export declare const ok: true;\n",
  );
  fs.writeFileSync(
    path.join(cwd, "dist", "maps", "index.mjs.map"),
    JSON.stringify({
      version: 3,
      file: "../index.mjs",
      sourceRoot: "../../src",
      sections: [
        {
          offset: { line: 0, column: 0 },
          map: {
            version: 3,
            sources: ["routes/health.ts", "./routes/users.ts"],
            mappings: "",
          },
        },
        {
          offset: { line: 8, column: 0 },
          map: {
            version: 3,
            sourceRoot: "../../src/nested/..",
            sources: ["routes/./users.ts", "routes/admin.ts"],
            mappings: "",
          },
        },
      ],
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
            map: "dist/maps/index.mjs.map",
            format: "esm",
            exports: ["ok"],
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
    ["src/routes/admin.ts", "src/routes/health.ts", "src/routes/users.ts"],
  );
  assert.deepEqual(ir.diagnostics, []);
});

test("buildProgramIrFromArtifacts preserves declaration linkage when JS sourcemap is inline and d.ts sourcemap uses sourceRoot with relative sources", () => {
  const cwd = fs.mkdtempSync(
    path.join(os.tmpdir(), "tsgodown-artifact-ir-inline-js-dts-linkage-"),
  );
  fs.mkdirSync(path.join(cwd, "dist", "maps"), { recursive: true });

  const inlineJsMapPayload = JSON.stringify({
    version: 3,
    file: "index.mjs",
    sourceRoot: "../src",
    sources: ["routes/health.ts"],
    names: [],
    mappings: "",
  });

  fs.writeFileSync(
    path.join(cwd, "dist", "index.mjs"),
    [
      "const health = () => ({ ok: true });",
      "export { health };",
      `//# sourceMappingURL=data:application/json;charset=utf-8;base64,${Buffer.from(inlineJsMapPayload, "utf8").toString("base64")}`,
      "",
    ].join("\n"),
  );

  fs.writeFileSync(
    path.join(cwd, "dist", "index.d.ts"),
    [
      "declare const health: () => { ok: boolean };",
      "declare interface HealthStatus { ok: boolean; }",
      "export { health as healthHandler };",
      "export type { HealthStatus as PublicHealthStatus };",
      "//# sourceMappingURL=maps/index.d.ts.map",
      "",
    ].join("\n"),
  );

  fs.writeFileSync(
    path.join(cwd, "dist", "maps", "index.d.ts.map"),
    JSON.stringify({
      version: 3,
      file: "../index.d.ts",
      sourceRoot: "../../src/nested/..",
      sources: ["contracts/health.ts"],
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
          { file: "dist/index.mjs", format: "esm", exports: ["health"] },
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
    ["src/contracts/health.ts", "src/routes/health.ts"],
  );
  assert.deepEqual(ir.modules[0]?.exports, [
    "healthHandler",
    "PublicHealthStatus",
  ]);
});
