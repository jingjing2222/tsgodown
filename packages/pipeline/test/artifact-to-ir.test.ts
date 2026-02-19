import assert from "node:assert/strict";
import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import test, { after } from "node:test";

import { buildProgramIrFromArtifacts } from "../src/internal/artifact-to-ir.ts";

const tempDirs: string[] = [];

after(() => {
  for (const dir of tempDirs) {
    fs.rmSync(dir, { recursive: true, force: true });
  }
});

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

test("buildProgramIrFromArtifacts consumes js + d.ts + sourcemap metadata into typed IR", () => {
  const cwd = fs.mkdtempSync(path.join(os.tmpdir(), "tsgodown-artifact-ir-"));
  tempDirs.push(cwd);

  fs.mkdirSync(path.join(cwd, "artifacts", "manifests"), { recursive: true });
  fs.mkdirSync(path.join(cwd, "dist"), { recursive: true });

  fs.writeFileSync(
    path.join(cwd, "dist", "index.mjs"),
    [
      "import { helper } from './helper.mjs';",
      "export function health(req) {",
      "  helper(req);",
      "  return { ok: true, source: 'bundle' };",
      "}",
      "",
    ].join("\n"),
  );

  fs.writeFileSync(
    path.join(cwd, "dist", "index.mjs.map"),
    JSON.stringify({
      version: 3,
      file: "index.mjs",
      sources: ["../src/index.ts"],
      names: [],
      mappings: "",
    }),
  );

  fs.writeFileSync(
    path.join(cwd, "dist", "index.d.ts"),
    "export declare function health(req: Request): { ok: boolean; source: string };\n",
  );

  const ir = buildProgramIrFromArtifacts(
    {
      mode: "rust-engine-adapter",
      manifestPath: path.join(cwd, "artifacts", "manifests", "manifest.json"),
      manifestIndexPath: path.join(cwd, "artifacts", "manifests", "index.json"),
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

  assert.equal(ir.modules[0]?.sourcePath, "src/index.ts");
  assert.deepEqual(ir.modules[0]?.exports, ["health"]);
  assert.deepEqual(ir.modules[0]?.imports, [
    { spec: "./helper.mjs", kind: "esm", resolved: "dist/helper.mjs" },
  ]);

  assert.equal(ir.handlers[0]?.id, "health");
  assert.equal(ir.handlers[0]?.async, false);
  assert.equal(ir.handlers[0]?.bodyRef, '{"ok":true,"source":"bundle"}');
  assert.deepEqual(ir.handlers[0]?.params, [{ name: "req", role: "request" }]);
  assert.equal(ir.handlers[0]?.semantics?.responseMode, "return");
  assert.equal(ir.handlers[0]?.semantics?.usesJson, true);

  assert.match(
    ir.diagnostics[0]?.message ?? "",
    /consumed artifacts: bundle=dist\/index\.mjs, map=dist\/index\.mjs\.map, types=dist\/index\.d\.ts/,
  );
});
