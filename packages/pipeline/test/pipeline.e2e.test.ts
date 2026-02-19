import assert from "node:assert/strict";
import { spawnSync } from "node:child_process";
import crypto from "node:crypto";
import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import { after, test } from "node:test";

import { emitGoProject } from "@tsgodown/emitter-go";
import type { RunBuildResult } from "@tsgodown/tsdown-driver";

import { runPipeline } from "../src/index.ts";
import { buildProgramIrFromArtifacts } from "../src/internal/artifact-to-ir.ts";

const tempDirs: string[] = [];

after(() => {
  for (const dir of tempDirs) {
    fs.rmSync(dir, { recursive: true, force: true });
  }
});

function createRustLauncher(cwd: string) {
  const stubPath = path.join(cwd, `rust-stub-${crypto.randomUUID()}.mjs`);

  fs.writeFileSync(
    stubPath,
    [
      "for await (const _ of process.stdin) { /* drain */ }",
      "const response = {",
      "  ok: true,",
      "  diagnostics: ['engine=rust-binary-stub'],",
      "  manifest: {",
      "    buildId: '1122334455667788',",
      "    entries: ['src/index.ts'],",
      "    bundles: [{ file: 'dist/index.mjs', map: 'dist/index.mjs.map', format: 'esm', exports: [] }],",
      "    types: ['dist/index.d.ts'],",
      "    tsconfigPath: 'tsconfig.json'",
      "  }",
      "};",
      "process.stdout.write(JSON.stringify(response));",
    ].join("\n"),
  );

  const launcherPath = path.join(
    cwd,
    `rust-launcher-${crypto.randomUUID()}.sh`,
  );
  fs.writeFileSync(
    launcherPath,
    `#!/usr/bin/env bash\nexec ${JSON.stringify(process.execPath)} ${JSON.stringify(stubPath)}\n`,
  );
  fs.chmodSync(launcherPath, 0o755);
  return launcherPath;
}

function setupProject() {
  const dir = fs.mkdtempSync(path.join(os.tmpdir(), "tsgodown-pipeline-e2e-"));
  tempDirs.push(dir);

  fs.mkdirSync(path.join(dir, "src", "routes"), { recursive: true });
  fs.writeFileSync(
    path.join(dir, "src", "routes", "users.ts"),
    [
      "export const health = () => ({ ok: true });",
      "export const createUser = () => ({ id: 'u1' });",
      "",
    ].join("\n"),
  );

  fs.writeFileSync(
    path.join(dir, "src", "index.ts"),
    [
      "import { health, createUser } from './routes/users';",
      "const app = {",
      "  get: (_path: string, _handler: () => unknown) => undefined,",
      "  post: (_path: string, _handler: () => unknown) => undefined,",
      "};",
      "app.get('/health', health);",
      "app.post('/users', createUser);",
      "",
    ].join("\n"),
  );

  fs.writeFileSync(
    path.join(dir, "tsgodown.config.ts"),
    `export default { entry: "src/index.ts", outDir: "dist-go" };\n`,
  );

  return dir;
}

function assertGoMainScaffold(goSource: string) {
  assert.match(goSource, /^package main/m);
  assert.match(goSource, /func main\(\)/);
  assert.match(goSource, /router\.handle\("GET", "\/health", route0\)/);
  assert.match(goSource, /"mode": "unknown"/);
  assert.doesNotMatch(goSource, /TODO implement handler/);
}

function assertGoBuildSuccessIfToolchainAvailable(goDir: string) {
  const hasGoToolchain =
    spawnSync("go", ["version"], { encoding: "utf8" }).status === 0;

  if (!hasGoToolchain) {
    return;
  }

  const modInit = spawnSync(
    "go",
    ["mod", "init", "example.com/tsgodown-pipeline"],
    {
      cwd: goDir,
      encoding: "utf8",
    },
  );
  assert.equal(modInit.status, 0, modInit.stderr || modInit.stdout);

  const goBuild = spawnSync("go", ["build", "./..."], {
    cwd: goDir,
    encoding: "utf8",
  });
  assert.equal(goBuild.status, 0, goBuild.stderr || goBuild.stdout);
}

test("M1 regression: runPipeline fastify scaffold TS -> dist-go/main.go -> go build (if available)", async () => {
  const cwd = setupProject();
  const logs: string[] = [];
  const launcherPath = createRustLauncher(cwd);
  const prevRustBin = process.env.TSGODOWN_RUST_ENGINE_BIN;
  process.env.TSGODOWN_RUST_ENGINE_BIN = launcherPath;

  try {
    await runPipeline(cwd, {
      log(message) {
        logs.push(message);
      },
    });

    assert.equal(logs.length, 5);
    assert.match(logs[0], /\[BUILD_ARTIFACTS\]/);
    assert.match(logs[1], /\[BUILD_IR\].*analyzing entry: src\/index\.ts/i);
    assert.match(logs[2], /\[CAPABILITY_GATE\].*delegated to rust engine/i);
    assert.match(logs[3], /\[EMIT_GO\].*Go scaffold/i);
    assert.match(logs[4], /\[ON_SUCCESS\].*src\/index\.ts/i);

    const manifestPath = path.join(
      cwd,
      "artifacts",
      "manifests",
      "manifest.json",
    );
    assert.equal(fs.existsSync(manifestPath), true);

    const manifest = JSON.parse(fs.readFileSync(manifestPath, "utf8")) as {
      buildId: string;
      entries: string[];
    };
    assert.match(manifest.buildId, /^[a-f0-9]{16}$/);
    assert.deepEqual(manifest.entries, ["src/index.ts"]);

    const manifestIndexPath = path.join(
      cwd,
      "artifacts",
      "manifests",
      "index.json",
    );
    assert.equal(fs.existsSync(manifestIndexPath), true);

    const manifestIndex = JSON.parse(
      fs.readFileSync(manifestIndexPath, "utf8"),
    ) as {
      buildId: string;
      manifest: string;
      generatedAt: string;
    };
    assert.equal(manifestIndex.buildId, manifest.buildId);
    assert.equal(manifestIndex.manifest, "manifest.json");
    assert.equal(typeof manifestIndex.generatedAt, "string");

    // Go emission/build now happens in the delegated rust engine path;
    // this pipeline e2e covers stage progression + artifact contract hand-off.
  } finally {
    if (prevRustBin === undefined) {
      Reflect.deleteProperty(process.env, "TSGODOWN_RUST_ENGINE_BIN");
    } else {
      process.env.TSGODOWN_RUST_ENGINE_BIN = prevRustBin;
    }
  }
});

test("M1 regression: real JS+d.ts+sourcemap artifact provenance (file:// sourceRoot) -> typed IR -> Go compile smoke", () => {
  const cwd = fs.mkdtempSync(
    path.join(os.tmpdir(), "tsgodown-pipeline-artifact-go-e2e-"),
  );
  tempDirs.push(cwd);

  fs.mkdirSync(path.join(cwd, "src", "routes"), { recursive: true });
  fs.mkdirSync(path.join(cwd, "dist"), { recursive: true });

  fs.writeFileSync(
    path.join(cwd, "src", "routes", "health.ts"),
    "export const health = () => ({ ok: true });\n",
  );
  fs.writeFileSync(
    path.join(cwd, "src", "routes", "users.ts"),
    "export const listUsers = () => [{ id: 'u1' }];\n",
  );

  fs.writeFileSync(
    path.join(cwd, "dist", "index.mjs"),
    [
      "const health = () => ({ ok: true });",
      "const listUsers = () => [{ id: 'u1' }];",
      "export { health, listUsers };",
      "//# sourceMappingURL=index.mjs.map",
      "",
    ].join("\n"),
  );

  fs.writeFileSync(
    path.join(cwd, "dist", "index.d.ts"),
    [
      "export declare const health: () => { ok: boolean };",
      "export declare const listUsers: () => Array<{ id: string }>;",
      "",
    ].join("\n"),
  );

  fs.writeFileSync(
    path.join(cwd, "dist", "index.mjs.map"),
    JSON.stringify({
      version: 3,
      file: "index.mjs",
      sourceRoot: new URL(`file://${path.join(cwd, "src")}/`).toString(),
      sources: ["routes/health.ts", "routes/users.ts"],
      names: [],
      mappings: "",
    }),
  );

  const buildResult: RunBuildResult = {
    mode: "rust-engine-adapter",
    manifestPath: "artifacts/manifests/manifest.json",
    manifestIndexPath: "artifacts/manifests/index.json",
    manifest: {
      buildId: "0011223344556677",
      entries: ["src/index.ts"],
      bundles: [
        {
          file: "dist/index.mjs",
          map: "dist/index.mjs.map",
          format: "esm",
          exports: ["health", "listUsers"],
        },
      ],
      types: ["dist/index.d.ts"],
    },
    diagnostics: [],
  };

  const ir = buildProgramIrFromArtifacts(buildResult, "src/index.ts", { cwd });

  assert.deepEqual(
    ir.modules.map((module) => module.sourcePath),
    ["src/routes/health.ts", "src/routes/users.ts"],
  );
  assert.deepEqual(ir.modules[0]?.exports, ["health", "listUsers"]);
  assert.deepEqual(ir.diagnostics, []);

  const goOutDir = path.join(cwd, "dist-go");
  emitGoProject(ir, goOutDir);

  const goMainPath = path.join(goOutDir, "main.go");
  assert.equal(fs.existsSync(goMainPath), true);
  const goSource = fs.readFileSync(goMainPath, "utf8");
  assert.match(goSource, /^package main/m);
  assert.match(goSource, /router\.handle\("GET", "\/health", route0\)/);

  assertGoBuildSuccessIfToolchainAvailable(goOutDir);
});

test("M1 regression: JS sourcemap path discovered from bundle sourceMappingURL still drives typed IR -> Go compile smoke when manifest omits map", () => {
  const cwd = fs.mkdtempSync(
    path.join(os.tmpdir(), "tsgodown-pipeline-e2e-js-sourcemap-discovery-"),
  );
  tempDirs.push(cwd);

  fs.mkdirSync(path.join(cwd, "dist", "maps"), { recursive: true });
  fs.mkdirSync(path.join(cwd, "src", "routes"), { recursive: true });

  fs.writeFileSync(
    path.join(cwd, "dist", "index.mjs"),
    [
      "const health = () => ({ ok: true });",
      "const users = () => [{ id: 'u1' }];",
      "export { health, users };",
      "//# sourceMappingURL=maps/index.mjs.map",
      "",
    ].join("\n"),
  );

  fs.writeFileSync(
    path.join(cwd, "dist", "index.d.ts"),
    [
      "export declare const health: () => { ok: boolean };",
      "export declare const users: () => Array<{ id: string }>;",
      "",
    ].join("\n"),
  );

  fs.writeFileSync(
    path.join(cwd, "dist", "maps", "index.mjs.map"),
    JSON.stringify({
      version: 3,
      file: "../index.mjs",
      sourceRoot: new URL(`file://${path.join(cwd, "src")}/`).toString(),
      sections: [
        {
          offset: { line: 0, column: 0 },
          map: {
            version: 3,
            sources: ["routes/health.ts", "routes/users.ts"],
            mappings: "",
          },
        },
      ],
    }),
  );

  const buildResult: RunBuildResult = {
    mode: "rust-engine-adapter",
    manifestPath: "artifacts/manifests/manifest.json",
    manifestIndexPath: "artifacts/manifests/index.json",
    manifest: {
      buildId: "1122334455667788",
      entries: ["src/index.ts"],
      bundles: [
        {
          file: "dist/index.mjs",
          format: "esm",
          exports: ["health", "users"],
        },
      ],
      types: ["dist/index.d.ts"],
    },
    diagnostics: [],
  };

  const ir = buildProgramIrFromArtifacts(buildResult, "src/index.ts", { cwd });

  assert.deepEqual(
    ir.modules.map((module) => module.sourcePath),
    ["src/routes/health.ts", "src/routes/users.ts"],
  );
  assert.deepEqual(ir.modules[0]?.exports, ["health", "users"]);
  assert.deepEqual(ir.diagnostics, []);

  const goOutDir = path.join(cwd, "dist-go");
  emitGoProject(ir, goOutDir);

  const goMain = fs.readFileSync(path.join(goOutDir, "main.go"), "utf8");
  assert.match(goMain, /^package main/m);
  assert.match(goMain, /router\.handle\("GET", "\/health", route0\)/);
  assertGoBuildSuccessIfToolchainAvailable(goOutDir);
});

test("M1 regression: inline data URL sourcemap with charset metadata keeps typed IR provenance and Go diagnostic mapping deterministic", () => {
  const cwd = fs.mkdtempSync(
    path.join(os.tmpdir(), "tsgodown-pipeline-inline-map-charset-e2e-"),
  );
  tempDirs.push(cwd);

  fs.mkdirSync(path.join(cwd, "src", "routes"), { recursive: true });
  fs.mkdirSync(path.join(cwd, "dist"), { recursive: true });

  fs.writeFileSync(
    path.join(cwd, "dist", "index.d.ts"),
    [
      "export declare const health: () => { ok: boolean };",
      "export declare const listUsers: () => Array<{ id: string }>;",
      "",
    ].join("\n"),
  );

  const inlineMapPayload = JSON.stringify({
    version: 3,
    file: "index.mjs",
    sourceRoot: new URL(`file://${path.join(cwd, "src")}/`).toString(),
    sources: ["routes/health.ts", "routes/users.ts"],
    names: [],
    mappings: "",
  });

  fs.writeFileSync(
    path.join(cwd, "dist", "index.mjs"),
    [
      "const health = () => ({ ok: true });",
      "const listUsers = () => [{ id: 'u1' }];",
      "export { health, listUsers };",
      `//# sourceMappingURL=data:application/json;charset=utf-8;base64,${Buffer.from(inlineMapPayload, "utf8").toString("base64")}`,
      "",
    ].join("\n"),
  );

  const buildResult: RunBuildResult = {
    mode: "rust-engine-adapter",
    manifestPath: "artifacts/manifests/manifest.json",
    manifestIndexPath: "artifacts/manifests/index.json",
    manifest: {
      buildId: "2233445566778899",
      entries: ["src/index.ts"],
      bundles: [
        {
          file: "dist/index.mjs",
          format: "esm",
          exports: ["health", "listUsers"],
        },
      ],
      types: ["dist/index.d.ts"],
    },
    diagnostics: [],
  };

  const ir = buildProgramIrFromArtifacts(buildResult, "src/index.ts", { cwd });
  assert.deepEqual(
    ir.modules.map((module) => module.sourcePath),
    ["src/routes/health.ts", "src/routes/users.ts"],
  );

  const goOutDir = path.join(cwd, "dist-go");
  emitGoProject(
    {
      ...ir,
      diagnostics: [
        {
          level: "warn",
          code: "PIPELINE_SOURCEMAP_PROVENANCE",
          message: "typed IR provenance emitted from inline data URL sourcemap",
          source: {
            file: ir.modules[0]?.sourcePath ?? "src/index.ts",
            line: 1,
            column: 1,
          },
        },
      ],
    },
    goOutDir,
  );

  const goMain = fs.readFileSync(path.join(goOutDir, "main.go"), "utf8");
  assert.match(goMain, /\/\/\s+at src\/routes\/health\.ts:1:1/);
  assertGoBuildSuccessIfToolchainAvailable(goOutDir);
});

test("M1 regression: indexed sourcemap inherited file:// sourceRoot keeps typed IR provenance deterministic and survives Go diagnostic emission", () => {
  const cwd = fs.mkdtempSync(
    path.join(os.tmpdir(), "tsgodown-pipeline-e2e-indexed-inherit-"),
  );
  tempDirs.push(cwd);

  fs.mkdirSync(path.join(cwd, "dist", "maps"), { recursive: true });
  fs.mkdirSync(path.join(cwd, "src", "routes"), { recursive: true });

  fs.writeFileSync(
    path.join(cwd, "dist", "index.d.ts"),
    [
      "export declare const health: () => { ok: boolean };",
      "export declare const users: () => Array<{ id: string }>;",
      "",
    ].join("\n"),
  );

  fs.writeFileSync(
    path.join(cwd, "dist", "maps", "index.mjs.map"),
    JSON.stringify({
      version: 3,
      file: "../index.mjs",
      sourceRoot: new URL(`file://${path.join(cwd, "src")}/`).toString(),
      sections: [
        {
          offset: { line: 0, column: 0 },
          map: {
            version: 3,
            sources: ["routes/health.ts", "routes/users.ts"],
            mappings: "",
          },
        },
      ],
    }),
  );

  const buildResult: RunBuildResult = {
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
          exports: ["health", "users"],
        },
      ],
      types: ["dist/index.d.ts"],
    },
    diagnostics: [],
  };

  const ir = buildProgramIrFromArtifacts(buildResult, "src/index.ts", { cwd });
  assert.deepEqual(
    ir.modules.map((module) => module.sourcePath),
    ["src/routes/health.ts", "src/routes/users.ts"],
  );

  const goOutDir = path.join(cwd, "dist-go");
  emitGoProject(
    {
      ...ir,
      diagnostics: [
        {
          level: "warn",
          code: "PIPELINE_SOURCEMAP_PROVENANCE",
          message:
            "typed IR module provenance normalized from indexed sourcemap",
          source: {
            file: ir.modules[0]?.sourcePath ?? "src/index.ts",
            line: 1,
            column: 1,
          },
        },
      ],
    },
    goOutDir,
  );

  const goMain = fs.readFileSync(path.join(goOutDir, "main.go"), "utf8");
  assert.match(goMain, /\/\/\s+at src\/routes\/health\.ts:1:1/);
  assertGoBuildSuccessIfToolchainAvailable(goOutDir);
});

test("M1 regression: mixed indexed sourcemap JS+d.ts typed IR diagnostics keep source mapping and deterministic Go comment ordering", () => {
  const cwd = fs.mkdtempSync(
    path.join(os.tmpdir(), "tsgodown-pipeline-e2e-indexed-mixed-diag-"),
  );
  tempDirs.push(cwd);

  fs.mkdirSync(path.join(cwd, "dist", "maps"), { recursive: true });
  fs.mkdirSync(path.join(cwd, "src", "routes"), { recursive: true });

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
    ["export declare const health: () => { ok: boolean };", ""].join("\n"),
  );

  fs.writeFileSync(
    path.join(cwd, "dist", "maps", "index.mjs.map"),
    JSON.stringify({
      version: 3,
      file: "../index.mjs",
      sourceRoot: new URL(`file://${path.join(cwd, "src")}/`).toString(),
      sections: [
        {
          offset: {},
          map: {
            version: 3,
            sources: [null, "routes/health.ts"],
            mappings: ";;;",
          },
        },
        {
          offset: { line: 4 },
          map: {
            version: 3,
            names: [],
            mappings: "",
          },
        },
      ],
    }),
  );

  const buildResult: RunBuildResult = {
    mode: "rust-engine-adapter",
    manifestPath: "artifacts/manifests/manifest.json",
    manifestIndexPath: "artifacts/manifests/index.json",
    manifest: {
      buildId: "fedcba9876543210",
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
  };

  const ir = buildProgramIrFromArtifacts(buildResult, "src/index.ts", { cwd });
  assert.deepEqual(
    ir.modules.map((module) => module.sourcePath),
    ["src/routes/health.ts"],
  );

  const goOutDir = path.join(cwd, "dist-go");
  emitGoProject(ir, goOutDir);

  const goMain = fs.readFileSync(path.join(goOutDir, "main.go"), "utf8");
  const lineScoped = goMain.indexOf("at dist/maps/index.mjs.map:5");
  const fileOnly = goMain.indexOf("at dist/maps/index.mjs.map\n");
  assert.ok(lineScoped >= 0 && fileOnly >= 0);
  assert.ok(lineScoped < fileOnly);

  assertGoBuildSuccessIfToolchainAvailable(goOutDir);
});
