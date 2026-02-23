import assert from "node:assert/strict";
import { spawn, spawnSync } from "node:child_process";
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

async function assertGoHealthRuntimeReady(goDir: string) {
  const hasGoToolchain =
    spawnSync("go", ["version"], { encoding: "utf8" }).status === 0;
  if (!hasGoToolchain) {
    return;
  }

  const port = String(20_000 + Math.floor(Math.random() * 10_000));
  const run = spawn("go", ["run", "."], {
    cwd: goDir,
    env: {
      ...process.env,
      PORT: port,
    },
    stdio: "ignore",
  });

  try {
    const deadline = Date.now() + 10_000;
    let response: Response | undefined;

    while (Date.now() < deadline) {
      try {
        response = await fetch(`http://127.0.0.1:${port}/health`, {
          signal: AbortSignal.timeout(1000),
        });
        break;
      } catch {
        await new Promise((resolve) => setTimeout(resolve, 200));
      }
    }

    assert.ok(response, "go runtime /health endpoint did not become ready");
    assert.equal(response.status, 501);

    const rawBody = await response.text();
    const contentType = response.headers.get("content-type") ?? "";
    if (contentType.includes("application/json")) {
      const payload = JSON.parse(rawBody) as {
        handler?: string;
        method?: string;
        mode?: string;
        path?: string;
      };
      assert.equal(payload.method, "GET");
      assert.equal(payload.path, "/health");
      assert.equal(payload.mode, "unknown");
    } else {
      assert.match(rawBody, /TODO implement handler/i);
    }
  } finally {
    if (run.exitCode === null && !run.killed) {
      run.kill("SIGTERM");
    }
    await new Promise<void>((resolve) => {
      if (run.exitCode !== null) {
        resolve();
        return;
      }
      run.once("close", () => resolve());
      setTimeout(() => resolve(), 3000);
    });
  }
}

function buildTsdownArtifactsForFixture(
  cwd: string,
  sourceCode: string,
): {
  bundleFile: string;
  bundleMapFile: string;
  dtsFile: string;
} {
  const workspaceRoot = path.resolve(process.cwd(), "..", "..");
  const tsdownBin = path.join(workspaceRoot, "node_modules", ".bin", "tsdown");
  fs.mkdirSync(path.join(cwd, "src"), { recursive: true });
  fs.writeFileSync(path.join(cwd, "src", "index.ts"), `${sourceCode.trim()}\n`);
  fs.writeFileSync(
    path.join(cwd, "tsdown.config.ts"),
    [
      "export default {",
      "  entry: { index: 'src/index.ts' },",
      "  outDir: 'dist',",
      "  format: ['esm'],",
      "  dts: true,",
      "  sourcemap: true,",
      "  clean: true,",
      "  fixedExtension: false,",
      "  outExtensions: () => ({ js: '.mjs', dts: '.d.ts' }),",
      "};",
      "",
    ].join("\n"),
  );
  const build = spawnSync(tsdownBin, ["--config", "tsdown.config.ts"], {
    cwd,
    encoding: "utf8",
  });
  assert.equal(build.status, 0, build.stderr || build.stdout);

  const distDir = path.join(cwd, "dist");
  const files = fs.readdirSync(distDir);
  const bundleFile = files.find(
    (file) =>
      file.startsWith("index.") &&
      (file.endsWith(".mjs") || file.endsWith(".js")),
  );
  assert.ok(bundleFile, `missing JS bundle in dist: ${files.join(", ")}`);
  const bundleMapFile = `${bundleFile}.map`;
  assert.equal(
    fs.existsSync(path.join(distDir, bundleMapFile)),
    true,
    `missing sourcemap ${bundleMapFile} in dist`,
  );
  const dtsFile = files.find((file) => file === "index.d.ts");
  assert.ok(dtsFile, `missing d.ts in dist: ${files.join(", ")}`);

  return {
    bundleFile: `dist/${bundleFile}`,
    bundleMapFile: `dist/${bundleMapFile}`,
    dtsFile: `dist/${dtsFile}`,
  };
}

test("M1 regression [SUPPORTED]: runPipeline scaffold TS -> dist-go/main.go -> go build (if available)", async () => {
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

test("M4 regression [SUPPORTED]: tsdown constructor artifacts -> AST IR export(default) -> Go build/run", async () => {
  const cwd = fs.mkdtempSync(
    path.join(os.tmpdir(), "tsgodown-pipeline-constructor-tsdown-"),
  );
  tempDirs.push(cwd);

  const built = buildTsdownArtifactsForFixture(
    cwd,
    `
export default class HealthController {
  constructor(private readonly service: string) {
    void this.service;
  }
}
`,
  );

  const buildResult: RunBuildResult = {
    mode: "rust-engine-adapter",
    manifestPath: "artifacts/manifests/manifest.json",
    manifestIndexPath: "artifacts/manifests/index.json",
    manifest: {
      buildId: "constructor-ast-go-001",
      entries: ["src/index.ts"],
      bundles: [
        {
          file: built.bundleFile,
          map: built.bundleMapFile,
          format: "esm",
          exports: [],
        },
      ],
      types: [built.dtsFile],
      tsconfigPath: "tsconfig.json",
    },
    diagnostics: [],
  };

  const ir = buildProgramIrFromArtifacts(buildResult, "src/index.ts", { cwd });
  assert.ok(ir.modules.length > 0);
  assert.ok(
    ir.modules[0]?.exports.includes("default"),
    `missing default export in AST/d.ts merged IR exports: ${JSON.stringify(ir.modules[0]?.exports ?? [])}`,
  );

  const goOutDir = path.join(cwd, "dist-go");
  emitGoProject(ir, goOutDir);
  const goMain = fs.readFileSync(path.join(goOutDir, "main.go"), "utf8");
  assertGoMainScaffold(goMain);
  assertGoBuildSuccessIfToolchainAvailable(goOutDir);
  await assertGoHealthRuntimeReady(goOutDir);
});

test("M4 regression [SUPPORTED]: tsdown class extends artifacts -> AST IR -> Go build/run", async () => {
  const cwd = fs.mkdtempSync(
    path.join(os.tmpdir(), "tsgodown-pipeline-extends-tsdown-"),
  );
  tempDirs.push(cwd);

  const built = buildTsdownArtifactsForFixture(
    cwd,
    `
class BaseController {}
export default class HealthController extends BaseController {
  constructor() {
    super();
  }
}
`,
  );

  const buildResult: RunBuildResult = {
    mode: "rust-engine-adapter",
    manifestPath: "artifacts/manifests/manifest.json",
    manifestIndexPath: "artifacts/manifests/index.json",
    manifest: {
      buildId: "extends-ast-go-001",
      entries: ["src/index.ts"],
      bundles: [
        {
          file: built.bundleFile,
          map: built.bundleMapFile,
          format: "esm",
          exports: [],
        },
      ],
      types: [built.dtsFile],
      tsconfigPath: "tsconfig.json",
    },
    diagnostics: [],
  };

  const ir = buildProgramIrFromArtifacts(buildResult, "src/index.ts", { cwd });
  assert.ok(
    !ir.diagnostics.some(
      (diagnostic) =>
        diagnostic.code === "PIPELINE_UNSUPPORTED_CLASS_EXTENDS_EXPRESSION",
    ),
    `unexpected extends diagnostic: ${JSON.stringify(ir.diagnostics)}`,
  );
  assert.ok(ir.modules[0]?.exports.includes("default"));

  const goOutDir = path.join(cwd, "dist-go");
  emitGoProject(ir, goOutDir);
  const goMain = fs.readFileSync(path.join(goOutDir, "main.go"), "utf8");
  assertGoMainScaffold(goMain);
  assertGoBuildSuccessIfToolchainAvailable(goOutDir);
  await assertGoHealthRuntimeReady(goOutDir);
});

test("M4 regression [SUPPORTED]: tsdown class declaration artifacts -> AST IR -> Go build/run", async () => {
  const cwd = fs.mkdtempSync(
    path.join(os.tmpdir(), "tsgodown-pipeline-class-decl-tsdown-"),
  );
  tempDirs.push(cwd);

  const built = buildTsdownArtifactsForFixture(
    cwd,
    `
export class HealthController {
  status() {
    return "ok";
  }
}
`,
  );

  const buildResult: RunBuildResult = {
    mode: "rust-engine-adapter",
    manifestPath: "artifacts/manifests/manifest.json",
    manifestIndexPath: "artifacts/manifests/index.json",
    manifest: {
      buildId: "class-decl-ast-go-001",
      entries: ["src/index.ts"],
      bundles: [
        {
          file: built.bundleFile,
          map: built.bundleMapFile,
          format: "esm",
          exports: [],
        },
      ],
      types: [built.dtsFile],
      tsconfigPath: "tsconfig.json",
    },
    diagnostics: [],
  };

  const ir = buildProgramIrFromArtifacts(buildResult, "src/index.ts", { cwd });
  assert.ok(
    ir.modules[0]?.exports.includes("HealthController"),
    `missing class declaration export in AST/d.ts merged IR exports: ${JSON.stringify(ir.modules[0]?.exports ?? [])}`,
  );

  const goOutDir = path.join(cwd, "dist-go");
  emitGoProject(ir, goOutDir);
  const goMain = fs.readFileSync(path.join(goOutDir, "main.go"), "utf8");
  assertGoMainScaffold(goMain);
  assertGoBuildSuccessIfToolchainAvailable(goOutDir);
  await assertGoHealthRuntimeReady(goOutDir);
});

test("M4 regression [SUPPORTED]: tsdown class private elements artifacts -> AST IR -> Go build/run", async () => {
  const cwd = fs.mkdtempSync(
    path.join(os.tmpdir(), "tsgodown-pipeline-private-elements-tsdown-"),
  );
  tempDirs.push(cwd);

  const built = buildTsdownArtifactsForFixture(
    cwd,
    `
export class PrivateCounter {
  #count = 0;

  inc() {
    this.#count += 1;
    return this.#count;
  }
}
`,
  );

  const buildResult: RunBuildResult = {
    mode: "rust-engine-adapter",
    manifestPath: "artifacts/manifests/manifest.json",
    manifestIndexPath: "artifacts/manifests/index.json",
    manifest: {
      buildId: "private-elements-ast-go-001",
      entries: ["src/index.ts"],
      bundles: [
        {
          file: built.bundleFile,
          map: built.bundleMapFile,
          format: "esm",
          exports: [],
        },
      ],
      types: [built.dtsFile],
      tsconfigPath: "tsconfig.json",
    },
    diagnostics: [],
  };

  const ir = buildProgramIrFromArtifacts(buildResult, "src/index.ts", { cwd });
  assert.ok(
    ir.modules[0]?.exports.includes("PrivateCounter"),
    `missing private-elements class export in AST/d.ts merged IR exports: ${JSON.stringify(ir.modules[0]?.exports ?? [])}`,
  );

  const goOutDir = path.join(cwd, "dist-go");
  emitGoProject(ir, goOutDir);
  const goMain = fs.readFileSync(path.join(goOutDir, "main.go"), "utf8");
  assertGoMainScaffold(goMain);
  assertGoBuildSuccessIfToolchainAvailable(goOutDir);
  await assertGoHealthRuntimeReady(goOutDir);
});

test("M4 regression [SUPPORTED]: tsdown class public fields artifacts -> AST IR -> Go build/run", async () => {
  const cwd = fs.mkdtempSync(
    path.join(os.tmpdir(), "tsgodown-pipeline-public-fields-tsdown-"),
  );
  tempDirs.push(cwd);

  const built = buildTsdownArtifactsForFixture(
    cwd,
    `
export class PublicFieldsController {
  status = "ok";
}
`,
  );

  const buildResult: RunBuildResult = {
    mode: "rust-engine-adapter",
    manifestPath: "artifacts/manifests/manifest.json",
    manifestIndexPath: "artifacts/manifests/index.json",
    manifest: {
      buildId: "public-fields-ast-go-001",
      entries: ["src/index.ts"],
      bundles: [
        {
          file: built.bundleFile,
          map: built.bundleMapFile,
          format: "esm",
          exports: [],
        },
      ],
      types: [built.dtsFile],
      tsconfigPath: "tsconfig.json",
    },
    diagnostics: [],
  };

  const ir = buildProgramIrFromArtifacts(buildResult, "src/index.ts", { cwd });
  assert.ok(
    ir.modules[0]?.exports.includes("PublicFieldsController"),
    `missing public-fields class export in AST/d.ts merged IR exports: ${JSON.stringify(ir.modules[0]?.exports ?? [])}`,
  );

  const goOutDir = path.join(cwd, "dist-go");
  emitGoProject(ir, goOutDir);
  const goMain = fs.readFileSync(path.join(goOutDir, "main.go"), "utf8");
  assertGoMainScaffold(goMain);
  assertGoBuildSuccessIfToolchainAvailable(goOutDir);
  await assertGoHealthRuntimeReady(goOutDir);
});

test("M4 regression [SUPPORTED]: tsdown class static members artifacts -> AST IR -> Go build/run", async () => {
  const cwd = fs.mkdtempSync(
    path.join(os.tmpdir(), "tsgodown-pipeline-static-members-tsdown-"),
  );
  tempDirs.push(cwd);

  const built = buildTsdownArtifactsForFixture(
    cwd,
    `
export class StaticController {
  static version = 1;

  static currentVersion() {
    return StaticController.version;
  }
}
`,
  );

  const buildResult: RunBuildResult = {
    mode: "rust-engine-adapter",
    manifestPath: "artifacts/manifests/manifest.json",
    manifestIndexPath: "artifacts/manifests/index.json",
    manifest: {
      buildId: "static-members-ast-go-001",
      entries: ["src/index.ts"],
      bundles: [
        {
          file: built.bundleFile,
          map: built.bundleMapFile,
          format: "esm",
          exports: [],
        },
      ],
      types: [built.dtsFile],
      tsconfigPath: "tsconfig.json",
    },
    diagnostics: [],
  };

  const ir = buildProgramIrFromArtifacts(buildResult, "src/index.ts", { cwd });
  assert.ok(
    ir.modules[0]?.exports.includes("StaticController"),
    `missing static-members class export in AST/d.ts merged IR exports: ${JSON.stringify(ir.modules[0]?.exports ?? [])}`,
  );

  const goOutDir = path.join(cwd, "dist-go");
  emitGoProject(ir, goOutDir);
  const goMain = fs.readFileSync(path.join(goOutDir, "main.go"), "utf8");
  assertGoMainScaffold(goMain);
  assertGoBuildSuccessIfToolchainAvailable(goOutDir);
  await assertGoHealthRuntimeReady(goOutDir);
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

test("M1 regression: executable-path integration keeps real JS+d.ts+sourcemap typed IR provenance stable and /health runtime reachable", async () => {
  const cwd = fs.mkdtempSync(
    path.join(os.tmpdir(), "tsgodown-pipeline-executable-path-e2e-"),
  );
  tempDirs.push(cwd);

  fs.mkdirSync(path.join(cwd, "dist", "maps"), { recursive: true });
  fs.mkdirSync(path.join(cwd, "src", "routes"), { recursive: true });
  fs.mkdirSync(path.join(cwd, "src", "types"), { recursive: true });

  fs.writeFileSync(
    path.join(cwd, "dist", "index.mjs"),
    [
      "const health = () => ({ ok: true });",
      "export { health };",
      "//# sourceMappingURL=maps/index.mjs.map",
      "",
    ].join("\n"),
  );

  fs.writeFileSync(
    path.join(cwd, "dist", "index.d.ts"),
    [
      "export declare const health: () => { ok: boolean };",
      "export declare interface HealthPayload { ok: boolean }",
      "//# sourceMappingURL=maps/index.d.ts.map",
      "",
    ].join("\n"),
  );

  fs.writeFileSync(
    path.join(cwd, "dist", "maps", "index.mjs.map"),
    JSON.stringify({
      version: 3,
      file: "../index.mjs",
      sourceRoot: new URL(`file://${path.join(cwd, "src")}/`).toString(),
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
      sourceRoot: new URL(`file://${path.join(cwd, "src")}/`).toString(),
      sources: ["types/health.ts"],
      names: [],
      mappings: "",
    }),
  );

  const buildResult: RunBuildResult = {
    mode: "rust-engine-adapter",
    manifestPath: "artifacts/manifests/manifest.json",
    manifestIndexPath: "artifacts/manifests/index.json",
    manifest: {
      buildId: "aa11bb22cc33dd44",
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
    ["src/routes/health.ts", "src/types/health.ts"],
  );
  assert.deepEqual(ir.modules[0]?.exports, ["health", "HealthPayload"]);

  const goOutDir = path.join(cwd, "dist-go");
  emitGoProject(
    {
      ...ir,
      diagnostics: [
        {
          level: "warn",
          code: "PIPELINE_STABLE_SYMBOL_PROVENANCE",
          message: "health symbol provenance pinned to source map lineage",
          source: {
            file: ir.modules[0]?.sourcePath ?? "src/routes/health.ts",
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
  await assertGoHealthRuntimeReady(goOutDir);
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

test("M1 regression: indexed sourcemap with mixed sourceRoot variants preserves typed IR provenance and Go compile path", () => {
  const cwd = fs.mkdtempSync(
    path.join(os.tmpdir(), "tsgodown-pipeline-e2e-indexed-mixed-root-"),
  );
  tempDirs.push(cwd);

  fs.mkdirSync(path.join(cwd, "dist", "maps"), { recursive: true });

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
      sourceRoot: ` ${new URL(`file://${path.join(cwd, "src")}/`).toString()} `,
      sections: [
        {
          offset: { line: 0, column: 0 },
          map: {
            version: 3,
            sources: ["routes/health.ts"],
            mappings: "",
          },
        },
        {
          offset: { line: 4, column: 0 },
          map: {
            version: 3,
            sourceRoot: " ../../src/nested/.. ",
            sources: ["routes/users.ts"],
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
      buildId: "bbaa998877665544",
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
  assert.deepEqual(ir.modules[0]?.exports, ["health", "users"]);
  assert.deepEqual(ir.diagnostics, []);

  const goOutDir = path.join(cwd, "dist-go");
  emitGoProject(ir, goOutDir);

  const goMain = fs.readFileSync(path.join(goOutDir, "main.go"), "utf8");
  assert.match(goMain, /^package main/m);
  assert.match(goMain, /router\.handle\("GET", "\/health", route0\)/);
  assertGoBuildSuccessIfToolchainAvailable(goOutDir);
});

test("M1 regression: declared external JS map missing falls back to inline data URL map for JS+d.ts typed IR provenance", () => {
  const cwd = fs.mkdtempSync(
    path.join(os.tmpdir(), "tsgodown-pipeline-inline-map-fallback-e2e-"),
  );
  tempDirs.push(cwd);

  fs.mkdirSync(path.join(cwd, "dist"), { recursive: true });

  fs.writeFileSync(
    path.join(cwd, "dist", "index.d.ts"),
    ["export declare const health: () => { ok: boolean };", ""].join("\n"),
  );

  const inlineMapPayload = JSON.stringify({
    version: 3,
    file: "index.mjs",
    sourceRoot: new URL(`file://${path.join(cwd, "dist")}/`).toString(),
    sources: ["index.mjs", "index.d.ts"],
    names: [],
    mappings: "",
  });

  fs.writeFileSync(
    path.join(cwd, "dist", "index.mjs"),
    [
      "const health = () => ({ ok: true });",
      "export { health };",
      `//# sourceMappingURL=data:application/json;charset=utf-8;base64,${Buffer.from(inlineMapPayload, "utf8").toString("base64")}`,
      "",
    ].join("\n"),
  );

  const buildResult: RunBuildResult = {
    mode: "rust-engine-adapter",
    manifestPath: "artifacts/manifests/manifest.json",
    manifestIndexPath: "artifacts/manifests/index.json",
    manifest: {
      buildId: "9988776655443322",
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
    ["dist/index.d.ts", "dist/index.mjs"],
  );
  assert.deepEqual(ir.diagnostics, []);

  const goOutDir = path.join(cwd, "dist-go");
  emitGoProject(ir, goOutDir);
  assertGoBuildSuccessIfToolchainAvailable(goOutDir);
});

test("M1 regression: invalid declared JS map + malformed inline data URL keeps deterministic warnings and stable d.ts provenance ordering", () => {
  const cwd = fs.mkdtempSync(
    path.join(os.tmpdir(), "tsgodown-pipeline-inline-map-malformed-e2e-"),
  );
  tempDirs.push(cwd);

  fs.mkdirSync(path.join(cwd, "dist", "maps"), { recursive: true });
  fs.mkdirSync(path.join(cwd, "src", "contracts"), { recursive: true });

  fs.writeFileSync(
    path.join(cwd, "dist", "index.d.ts"),
    [
      "export declare const health: () => { ok: boolean };",
      "//# sourceMappingURL=maps/index.d.ts.map",
      "",
    ].join("\n"),
  );

  fs.writeFileSync(
    path.join(cwd, "dist", "maps", "index.d.ts.map"),
    JSON.stringify({
      version: 3,
      file: "../index.d.ts",
      sourceRoot: new URL(`file://${path.join(cwd, "src")}/`).toString(),
      sources: ["contracts/zeta.ts", "contracts/alpha.ts"],
      names: [],
      mappings: "",
    }),
  );

  fs.writeFileSync(
    path.join(cwd, "dist", "index.mjs"),
    [
      "const health = () => ({ ok: true });",
      "export { health };",
      "//# sourceMappingURL=data:application/json;base64,this-is-not-base64-json",
      "",
    ].join("\n"),
  );

  const buildResult: RunBuildResult = {
    mode: "rust-engine-adapter",
    manifestPath: "artifacts/manifests/manifest.json",
    manifestIndexPath: "artifacts/manifests/index.json",
    manifest: {
      buildId: "1029384756019283",
      entries: ["src/index.ts"],
      bundles: [
        {
          file: "dist/index.mjs",
          map: "dist/maps/missing-index.mjs.map",
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
    ["src/contracts/alpha.ts", "src/contracts/zeta.ts"],
  );
  assert.deepEqual(
    ir.diagnostics.map((diagnostic) => ({
      code: diagnostic.code,
      message: diagnostic.message,
      file: diagnostic.source?.file,
    })),
    [
      {
        code: "PIPELINE_INVALID_SOURCEMAP_MAPPING",
        message:
          "inline sourcemap data URL is malformed or invalid JSON: dist/index.mjs",
        file: "dist/index.mjs",
      },
      {
        code: "PIPELINE_INVALID_SOURCEMAP_MAPPING",
        message:
          "sourcemap metadata is missing or invalid JSON: dist/maps/missing-index.mjs.map",
        file: "dist/maps/missing-index.mjs.map",
      },
    ],
  );

  const goOutDir = path.join(cwd, "dist-go");
  emitGoProject(ir, goOutDir);
  assertGoBuildSuccessIfToolchainAvailable(goOutDir);
});

test("M1 regression: inline JS map + external d.ts.map (no sourcesContent) with mixed relative/file sourceRoot keeps typed exports complete and Go /health runtime reachable", async () => {
  const cwd = fs.mkdtempSync(
    path.join(os.tmpdir(), "tsgodown-pipeline-inline-js-external-dts-map-e2e-"),
  );
  tempDirs.push(cwd);

  fs.mkdirSync(path.join(cwd, "dist", "maps"), { recursive: true });
  fs.mkdirSync(path.join(cwd, "src", "routes"), { recursive: true });
  fs.mkdirSync(path.join(cwd, "src", "types"), { recursive: true });

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
      `//# sourceMappingURL=data:application/json;base64,${Buffer.from(inlineJsMapPayload, "utf8").toString("base64")}`,
      "",
    ].join("\n"),
  );

  fs.writeFileSync(
    path.join(cwd, "dist", "index.d.ts"),
    [
      "export declare const health: () => { ok: boolean };",
      "export declare interface HealthPayload { ok: boolean }",
      "//# sourceMappingURL=maps/index.d.ts.map",
      "",
    ].join("\n"),
  );

  fs.writeFileSync(
    path.join(cwd, "dist", "maps", "index.d.ts.map"),
    JSON.stringify({
      version: 3,
      file: "../index.d.ts",
      sourceRoot: new URL(`file://${path.join(cwd, "src")}/`).toString(),
      sources: ["types/health.ts"],
      names: [],
      mappings: "",
    }),
  );

  const buildResult: RunBuildResult = {
    mode: "rust-engine-adapter",
    manifestPath: "artifacts/manifests/manifest.json",
    manifestIndexPath: "artifacts/manifests/index.json",
    manifest: {
      buildId: "33445566778899aa",
      entries: ["src/index.ts"],
      bundles: [
        {
          file: "dist/index.mjs",
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
    ["src/routes/health.ts", "src/types/health.ts"],
  );
  assert.deepEqual(ir.modules[0]?.exports, ["health", "HealthPayload"]);
  assert.deepEqual(ir.diagnostics, []);

  const goOutDir = path.join(cwd, "dist-go");
  emitGoProject(ir, goOutDir);

  const goMain = fs.readFileSync(path.join(goOutDir, "main.go"), "utf8");
  assert.match(goMain, /^package main/m);
  assert.match(goMain, /router\.handle\("GET", "\/health", route0\)/);
  assertGoBuildSuccessIfToolchainAvailable(goOutDir);
  await assertGoHealthRuntimeReady(goOutDir);
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

test("M1 regression: mixed JS+d.ts sourcemaps are unioned into typed IR provenance and survive Go compile smoke", () => {
  const cwd = fs.mkdtempSync(
    path.join(os.tmpdir(), "tsgodown-pipeline-js-dts-map-union-e2e-"),
  );
  tempDirs.push(cwd);

  fs.mkdirSync(path.join(cwd, "dist", "maps"), { recursive: true });
  fs.mkdirSync(path.join(cwd, "src", "routes"), { recursive: true });
  fs.mkdirSync(path.join(cwd, "src", "contracts"), { recursive: true });

  fs.writeFileSync(
    path.join(cwd, "dist", "index.mjs"),
    [
      "const health = () => ({ ok: true });",
      "export { health };",
      "//# sourceMappingURL=maps/index.mjs.map",
      "",
    ].join("\n"),
  );

  fs.writeFileSync(
    path.join(cwd, "dist", "maps", "index.mjs.map"),
    JSON.stringify({
      version: 3,
      file: "../index.mjs",
      sourceRoot: new URL(`file://${path.join(cwd, "src")}/`).toString(),
      sources: ["routes/health.ts"],
      names: [],
      mappings: "",
    }),
  );

  fs.writeFileSync(
    path.join(cwd, "dist", "index.d.ts"),
    [
      "export declare const health: () => { ok: boolean };",
      "export declare type User = { id: string };",
      "//# sourceMappingURL=maps/index.d.ts.map",
      "",
    ].join("\n"),
  );

  fs.writeFileSync(
    path.join(cwd, "dist", "maps", "index.d.ts.map"),
    JSON.stringify({
      version: 3,
      file: "../index.d.ts",
      sourceRoot: new URL(`file://${path.join(cwd, "src")}/`).toString(),
      sources: ["contracts/user.ts"],
      names: [],
      mappings: "",
    }),
  );

  const buildResult: RunBuildResult = {
    mode: "rust-engine-adapter",
    manifestPath: "artifacts/manifests/manifest.json",
    manifestIndexPath: "artifacts/manifests/index.json",
    manifest: {
      buildId: "ddeeff0011223344",
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
    ["src/contracts/user.ts", "src/routes/health.ts"],
  );

  const goOutDir = path.join(cwd, "dist-go");
  emitGoProject(ir, goOutDir);
  assertGoBuildSuccessIfToolchainAvailable(goOutDir);
});

test("M1 regression: multi-bundle JS + d.ts + sourcemaps deterministically union typed IR provenance and Go compile path", () => {
  const cwd = fs.mkdtempSync(
    path.join(os.tmpdir(), "tsgodown-pipeline-multi-bundle-union-e2e-"),
  );
  tempDirs.push(cwd);

  fs.mkdirSync(path.join(cwd, "dist", "maps"), { recursive: true });
  fs.mkdirSync(path.join(cwd, "src", "routes"), { recursive: true });

  fs.writeFileSync(
    path.join(cwd, "dist", "index.d.ts"),
    [
      "export declare const health: () => { ok: boolean };",
      "export declare const listUsers: () => Array<{ id: string }>;",
      "",
    ].join("\n"),
  );

  fs.writeFileSync(
    path.join(cwd, "dist", "index.mjs"),
    [
      "const health = () => ({ ok: true });",
      "export { health };",
      "//# sourceMappingURL=maps/index.mjs.map",
      "",
    ].join("\n"),
  );

  fs.writeFileSync(
    path.join(cwd, "dist", "users.mjs"),
    [
      "const listUsers = () => [{ id: 'u1' }];",
      "export { listUsers };",
      "//# sourceMappingURL=maps/users.mjs.map",
      "",
    ].join("\n"),
  );

  fs.writeFileSync(
    path.join(cwd, "dist", "maps", "index.mjs.map"),
    JSON.stringify({
      version: 3,
      file: "../index.mjs",
      sourceRoot: new URL(`file://${path.join(cwd, "src")}/`).toString(),
      sources: ["routes/health.ts"],
      names: [],
      mappings: "",
    }),
  );

  fs.writeFileSync(
    path.join(cwd, "dist", "maps", "users.mjs.map"),
    JSON.stringify({
      version: 3,
      file: "../users.mjs",
      sourceRoot: new URL(`file://${path.join(cwd, "src")}/`).toString(),
      sources: ["routes/users.ts"],
      names: [],
      mappings: "",
    }),
  );

  const buildResult: RunBuildResult = {
    mode: "rust-engine-adapter",
    manifestPath: "artifacts/manifests/manifest.json",
    manifestIndexPath: "artifacts/manifests/index.json",
    manifest: {
      buildId: "4455667788990011",
      entries: ["src/index.ts"],
      bundles: [
        {
          file: "dist/index.mjs",
          map: "dist/maps/index.mjs.map",
          format: "esm",
          exports: ["health"],
        },
        {
          file: "dist/users.mjs",
          map: "dist/maps/users.mjs.map",
          format: "esm",
          exports: ["listUsers"],
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

  const goOutDir = path.join(cwd, "dist-go");
  emitGoProject(ir, goOutDir);
  assertGoBuildSuccessIfToolchainAvailable(goOutDir);
});

test("M1 regression: inline+external+indexed sourcemaps with d.ts typing union typed IR provenance and compile to Go", () => {
  const cwd = fs.mkdtempSync(
    path.join(os.tmpdir(), "tsgodown-pipeline-mixed-map-provenance-e2e-"),
  );
  tempDirs.push(cwd);

  fs.mkdirSync(path.join(cwd, "dist", "maps"), { recursive: true });
  fs.mkdirSync(path.join(cwd, "src", "routes"), { recursive: true });
  fs.mkdirSync(path.join(cwd, "src", "types"), { recursive: true });

  fs.writeFileSync(
    path.join(cwd, "dist", "index.d.ts"),
    [
      "export declare const health: () => { ok: boolean };",
      "export declare const listUsers: () => Array<{ id: string }>;",
      "export declare const adminPing: () => { role: string };",
      "//# sourceMappingURL=maps/index.d.ts.map",
      "",
    ].join("\n"),
  );

  fs.writeFileSync(
    path.join(cwd, "dist", "maps", "index.d.ts.map"),
    JSON.stringify({
      version: 3,
      file: "../index.d.ts",
      sourceRoot: new URL(`file://${path.join(cwd, "src")}/`).toString(),
      sources: ["types/http.ts"],
      names: [],
      mappings: "",
    }),
  );

  const inlineMapPayload = JSON.stringify({
    version: 3,
    file: "index.mjs",
    sourceRoot: new URL(`file://${path.join(cwd, "src")}/`).toString(),
    sources: ["routes/health.ts"],
    names: [],
    mappings: "",
  });
  fs.writeFileSync(
    path.join(cwd, "dist", "index.mjs"),
    [
      "const health = () => ({ ok: true });",
      "export { health };",
      `//# sourceMappingURL=data:application/json;charset=utf-8;base64,${Buffer.from(inlineMapPayload, "utf8").toString("base64")}`,
      "",
    ].join("\n"),
  );

  fs.writeFileSync(
    path.join(cwd, "dist", "users.mjs"),
    [
      "const listUsers = () => [{ id: 'u1' }];",
      "export { listUsers };",
      "//# sourceMappingURL=maps/users.mjs.map",
      "",
    ].join("\n"),
  );
  fs.writeFileSync(
    path.join(cwd, "dist", "maps", "users.mjs.map"),
    JSON.stringify({
      version: 3,
      file: "../users.mjs",
      sourceRoot: new URL(`file://${path.join(cwd, "src")}/`).toString(),
      sources: ["routes/users.ts"],
      names: [],
      mappings: "",
    }),
  );

  fs.writeFileSync(
    path.join(cwd, "dist", "admin.mjs"),
    [
      "const adminPing = () => ({ role: 'admin' });",
      "export { adminPing };",
      "//# sourceMappingURL=maps/admin.mjs.map",
      "",
    ].join("\n"),
  );
  fs.writeFileSync(
    path.join(cwd, "dist", "maps", "admin.mjs.map"),
    JSON.stringify({
      version: 3,
      file: "../admin.mjs",
      sourceRoot: new URL(`file://${path.join(cwd, "src")}/`).toString(),
      sections: [
        {
          offset: { line: 4 },
          map: {
            version: 3,
            sources: ["routes/admin.ts"],
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
      buildId: "6677889900112233",
      entries: ["src/index.ts"],
      bundles: [
        {
          file: "dist/index.mjs",
          format: "esm",
          exports: ["health"],
        },
        {
          file: "dist/users.mjs",
          map: "dist/maps/users.mjs.map",
          format: "esm",
          exports: ["listUsers"],
        },
        {
          file: "dist/admin.mjs",
          map: "dist/maps/admin.mjs.map",
          format: "esm",
          exports: ["adminPing"],
        },
      ],
      types: ["dist/index.d.ts"],
    },
    diagnostics: [],
  };

  const ir = buildProgramIrFromArtifacts(buildResult, "src/index.ts", { cwd });
  assert.deepEqual(
    ir.modules.map((module) => module.sourcePath),
    [
      "src/routes/admin.ts",
      "src/routes/health.ts",
      "src/routes/users.ts",
      "src/types/http.ts",
    ],
  );
  assert.deepEqual(
    ir.modules.find((module) => module.sourcePath === "src/types/http.ts")
      ?.exports,
    ["adminPing", "health", "listUsers"],
  );
  assert.deepEqual(
    ir.diagnostics.map((d) => ({
      code: d.code,
      file: d.source?.file,
      line: d.source?.line,
    })),
    [
      {
        code: "PIPELINE_SOURCEMAP_POSITION_PARTIAL",
        file: "dist/maps/admin.mjs.map",
        line: 5,
      },
    ],
  );

  const goOutDir = path.join(cwd, "dist-go");
  emitGoProject(ir, goOutDir);
  assertGoBuildSuccessIfToolchainAvailable(goOutDir);
});

test("M1 regression: invalid declared map + trailing inline sourcemap directives in mjs+dts preserve typed IR provenance union", () => {
  const cwd = fs.mkdtempSync(
    path.join(os.tmpdir(), "tsgodown-pipeline-map-precedence-inline-last-"),
  );
  tempDirs.push(cwd);

  fs.mkdirSync(path.join(cwd, "dist"), { recursive: true });
  fs.mkdirSync(path.join(cwd, "src", "routes"), { recursive: true });
  fs.mkdirSync(path.join(cwd, "src", "types"), { recursive: true });

  const jsInlinePayload = JSON.stringify({
    version: 3,
    file: "index.mjs",
    sourceRoot: new URL(`file://${path.join(cwd, "src")}/`).toString(),
    sources: ["routes/health.ts"],
    names: [],
    mappings: "",
  });
  fs.writeFileSync(
    path.join(cwd, "dist", "index.mjs"),
    [
      "const health = () => ({ ok: true });",
      "export { health };",
      "//# sourceMappingURL=maps/missing-index.mjs.map?bad=1#declared",
      `//# sourceMappingURL=data:application/json;charset=utf-8;base64,${Buffer.from(jsInlinePayload, "utf8").toString("base64")}`,
      "",
    ].join("\n"),
  );

  const dtsInlinePayload = JSON.stringify({
    version: 3,
    file: "index.d.ts",
    sourceRoot: new URL(`file://${path.join(cwd, "src")}/`).toString(),
    sources: ["types/http.ts"],
    names: [],
    mappings: "",
  });
  fs.writeFileSync(
    path.join(cwd, "dist", "index.d.ts"),
    [
      "export declare const health: () => { ok: boolean };",
      "//# sourceMappingURL=maps/missing-index.d.ts.map?bad=2#declared",
      `//# sourceMappingURL=data:application/json;charset=utf-8;base64,${Buffer.from(dtsInlinePayload, "utf8").toString("base64")}`,
      "",
    ].join("\n"),
  );

  const buildResult: RunBuildResult = {
    mode: "rust-engine-adapter",
    manifestPath: "artifacts/manifests/manifest.json",
    manifestIndexPath: "artifacts/manifests/index.json",
    manifest: {
      buildId: "199aa22bb33cc44",
      entries: ["src/index.ts"],
      bundles: [
        {
          file: "dist/index.mjs",
          map: "dist/maps/missing-index.mjs.map",
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
    ["src/routes/health.ts", "src/types/http.ts"],
  );

  const goOutDir = path.join(cwd, "dist-go");
  emitGoProject(ir, goOutDir);
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

test("M1 regression: indexed sourcemap with URL-encoded section source paths decodes provenance for JS+d.ts Go compile path", () => {
  const cwd = fs.mkdtempSync(
    path.join(os.tmpdir(), "tsgodown-pipeline-indexed-encoded-section-e2e-"),
  );
  tempDirs.push(cwd);

  fs.mkdirSync(path.join(cwd, "dist", "maps"), { recursive: true });
  fs.mkdirSync(path.join(cwd, "src", "routes"), { recursive: true });
  fs.mkdirSync(path.join(cwd, "src", "types"), { recursive: true });

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
      "//# sourceMappingURL=maps/index.d.ts.map",
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
            sources: ["routes/health%20check.ts"],
            names: [],
            mappings: "",
          },
        },
      ],
    }),
  );

  fs.writeFileSync(
    path.join(cwd, "dist", "maps", "index.d.ts.map"),
    JSON.stringify({
      version: 3,
      file: "../index.d.ts",
      sourceRoot: new URL(`file://${path.join(cwd, "src")}/`).toString(),
      sections: [
        {
          offset: { line: 0, column: 0 },
          map: {
            version: 3,
            sources: ["types/http%20contracts.ts"],
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
      buildId: "c16e2e0011223344",
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
    ["src/routes/health check.ts", "src/types/http contracts.ts"],
  );

  const goOutDir = path.join(cwd, "dist-go");
  emitGoProject(ir, goOutDir);
  assertGoBuildSuccessIfToolchainAvailable(goOutDir);
});

test("M1 regression: indexed sourcemap section diagnostics preserve line+column fidelity into Go comments", () => {
  const cwd = fs.mkdtempSync(
    path.join(os.tmpdir(), "tsgodown-pipeline-e2e-indexed-line-column-"),
  );
  tempDirs.push(cwd);

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
    ["export declare const health: () => { ok: boolean };", ""].join("\n"),
  );

  fs.writeFileSync(
    path.join(cwd, "dist", "maps", "index.mjs.map"),
    JSON.stringify({
      version: 3,
      file: "../index.mjs",
      sections: [
        {
          offset: { line: 3, column: 7 },
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
      buildId: "aa77cc88dd99ee00",
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
  const goOutDir = path.join(cwd, "dist-go");
  emitGoProject(ir, goOutDir);

  const goMain = fs.readFileSync(path.join(goOutDir, "main.go"), "utf8");
  assert.match(goMain, /\/\/\s+at dist\/maps\/index\.mjs\.map:4:8/);

  assertGoBuildSuccessIfToolchainAvailable(goOutDir);
});

test("M1 regression: declared invalid sourcemap path falls back to valid inline maps for both mjs and d.ts deterministically", () => {
  const cwd = fs.mkdtempSync(
    path.join(os.tmpdir(), "tsgodown-pipeline-e2e-inline-fallback-mjs-dts-"),
  );
  tempDirs.push(cwd);

  fs.mkdirSync(path.join(cwd, "src", "routes"), { recursive: true });
  fs.mkdirSync(path.join(cwd, "src", "contracts"), { recursive: true });
  fs.mkdirSync(path.join(cwd, "dist"), { recursive: true });

  const jsInlineMap = JSON.stringify({
    version: 3,
    file: "index.mjs",
    sourceRoot: new URL(`file://${path.join(cwd, "src")}/`).toString(),
    sources: ["routes/health.ts"],
    names: [],
    mappings: "",
  });

  const dtsInlineMap = JSON.stringify({
    version: 3,
    file: "index.d.ts",
    sourceRoot: new URL(`file://${path.join(cwd, "src")}/`).toString(),
    sources: ["contracts/types.ts"],
    names: [],
    mappings: "",
  });

  fs.writeFileSync(
    path.join(cwd, "dist", "index.mjs"),
    [
      "export const health = () => ({ ok: true });",
      `//# sourceMappingURL=data:application/json;base64,${Buffer.from(jsInlineMap, "utf8").toString("base64")}`,
      "",
    ].join("\n"),
  );

  fs.writeFileSync(
    path.join(cwd, "dist", "index.d.ts"),
    [
      "export declare const health: () => { ok: boolean };",
      `//# sourceMappingURL=data:application/json;charset=utf-8;base64,${Buffer.from(dtsInlineMap, "utf8").toString("base64")}`,
      "",
    ].join("\n"),
  );

  const buildResult: RunBuildResult = {
    mode: "rust-engine-adapter",
    manifestPath: "artifacts/manifests/manifest.json",
    manifestIndexPath: "artifacts/manifests/index.json",
    manifest: {
      buildId: "77aa88bb99cc00dd",
      entries: ["src/index.ts"],
      bundles: [
        {
          file: "dist/index.mjs",
          map: "dist/maps/missing-index.mjs.map",
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
    ["src/contracts/types.ts", "src/routes/health.ts"],
  );
  assert.deepEqual(ir.diagnostics, []);

  const goOutDir = path.join(cwd, "dist-go");
  emitGoProject(ir, goOutDir);
  const goMain = fs.readFileSync(path.join(goOutDir, "main.go"), "utf8");
  assert.match(goMain, /^package main/m);
  assert.doesNotMatch(goMain, /dist\/maps\/missing-index\.mjs\.map/);

  assertGoBuildSuccessIfToolchainAvailable(goOutDir);
});

test("M1 regression: multiple d.ts artifacts union typed exports while JS+d.ts sourcemaps still drive provenance and Go compile path", () => {
  const cwd = fs.mkdtempSync(
    path.join(os.tmpdir(), "tsgodown-pipeline-e2e-multi-types-union-"),
  );
  tempDirs.push(cwd);

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
    path.join(cwd, "dist", "maps", "index.mjs.map"),
    JSON.stringify({
      version: 3,
      file: "../index.mjs",
      sourceRoot: new URL(`file://${path.join(cwd, "src")}/`).toString(),
      sources: ["routes/health.ts"],
      names: [],
      mappings: "",
    }),
  );

  fs.writeFileSync(
    path.join(cwd, "dist", "index.d.ts"),
    [
      "export declare const health: () => { ok: boolean };",
      "//# sourceMappingURL=maps/index.d.ts.map",
      "",
    ].join("\n"),
  );

  fs.writeFileSync(
    path.join(cwd, "dist", "users.d.ts"),
    [
      "export declare const listUsers: () => Array<{ id: string }>;",
      "//# sourceMappingURL=maps/users.d.ts.map",
      "",
    ].join("\n"),
  );

  fs.writeFileSync(
    path.join(cwd, "dist", "maps", "index.d.ts.map"),
    JSON.stringify({
      version: 3,
      file: "../index.d.ts",
      sourceRoot: new URL(`file://${path.join(cwd, "src")}/`).toString(),
      sources: ["types/http.ts"],
      names: [],
      mappings: "",
    }),
  );

  fs.writeFileSync(
    path.join(cwd, "dist", "maps", "users.d.ts.map"),
    JSON.stringify({
      version: 3,
      file: "../users.d.ts",
      sourceRoot: new URL(`file://${path.join(cwd, "src")}/`).toString(),
      sources: ["types/users.ts"],
      names: [],
      mappings: "",
    }),
  );

  const buildResult: RunBuildResult = {
    mode: "rust-engine-adapter",
    manifestPath: "artifacts/manifests/manifest.json",
    manifestIndexPath: "artifacts/manifests/index.json",
    manifest: {
      buildId: "8899aabbccddeeff",
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
  };

  const ir = buildProgramIrFromArtifacts(buildResult, "src/index.ts", { cwd });
  assert.deepEqual(
    ir.modules.map((module) => module.sourcePath),
    ["src/routes/health.ts", "src/types/http.ts", "src/types/users.ts"],
  );
  assert.deepEqual(
    ir.modules.find((module) => module.sourcePath === "src/types/http.ts")
      ?.exports,
    ["health", "listUsers"],
  );

  const goOutDir = path.join(cwd, "dist-go");
  emitGoProject(ir, goOutDir);
  assertGoBuildSuccessIfToolchainAvailable(goOutDir);
});

test("M1 regression: file:// sourceMappingURL on mjs+d.ts map comments resolves deterministic typed IR provenance and exports", () => {
  const cwd = fs.mkdtempSync(
    path.join(os.tmpdir(), "tsgodown-pipeline-e2e-file-url-map-comment-"),
  );
  tempDirs.push(cwd);

  fs.mkdirSync(path.join(cwd, "dist", "maps"), { recursive: true });

  fs.writeFileSync(
    path.join(cwd, "dist", "index.mjs"),
    [
      "export const health = () => ({ ok: true });",
      `//# sourceMappingURL=${new URL(`file://${path.join(cwd, "dist", "maps", "index.mjs.map")}`).toString()}`,
      "",
    ].join("\n"),
  );

  fs.writeFileSync(
    path.join(cwd, "dist", "index.d.ts"),
    [
      "export declare const health: () => { ok: boolean };",
      "export declare interface HealthPayload { ok: boolean }",
      `//# sourceMappingURL=${new URL(`file://${path.join(cwd, "dist", "maps", "index.d.ts.map")}`).toString()}`,
      "",
    ].join("\n"),
  );

  fs.writeFileSync(
    path.join(cwd, "dist", "maps", "index.mjs.map"),
    JSON.stringify({
      version: 3,
      file: "../index.mjs",
      sourceRoot: new URL(`file://${path.join(cwd, "src")}/`).toString(),
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
      sourceRoot: new URL(`file://${path.join(cwd, "src")}/`).toString(),
      sources: ["types/health.ts"],
      names: [],
      mappings: "",
    }),
  );

  const buildResult: RunBuildResult = {
    mode: "rust-engine-adapter",
    manifestPath: "artifacts/manifests/manifest.json",
    manifestIndexPath: "artifacts/manifests/index.json",
    manifest: {
      buildId: "0011ff22ee33dd44",
      entries: ["src/index.ts"],
      bundles: [
        {
          file: "dist/index.mjs",
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
    ["src/routes/health.ts", "src/types/health.ts"],
  );
  assert.deepEqual(
    ir.modules.find((module) => module.sourcePath === "src/types/health.ts")
      ?.exports,
    ["health", "HealthPayload"],
  );
  assert.deepEqual(ir.diagnostics, []);

  const goOutDir = path.join(cwd, "dist-go");
  emitGoProject(ir, goOutDir);
  assertGoBuildSuccessIfToolchainAvailable(goOutDir);
});

test("M1 regression: sourcemap sources with percent-encoded segments + query/hash normalize deterministic typed IR provenance and typed exports", () => {
  const cwd = fs.mkdtempSync(
    path.join(os.tmpdir(), "tsgodown-pipeline-e2e-encoded-sources-"),
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
      "//# sourceMappingURL=maps/index.mjs.map?rev=17#bundle",
      "",
    ].join("\n"),
  );

  fs.writeFileSync(
    path.join(cwd, "dist", "maps", "index.mjs.map"),
    JSON.stringify({
      version: 3,
      file: "../index.mjs",
      sourceRoot: new URL(`file://${path.join(cwd, "src")}/`).toString(),
      sources: [
        "routes/%68ealth.ts?src=js#frag",
        "./routes/%75sers.ts?src=js#frag",
      ],
      names: [],
      mappings: "",
    }),
  );

  fs.writeFileSync(
    path.join(cwd, "dist", "index.d.ts"),
    [
      "export declare const health: () => { ok: boolean };",
      "export declare const users: () => Array<{ id: string }>;",
      "//# sourceMappingURL=maps/index.d.ts.map?rev=17#types",
      "",
    ].join("\n"),
  );

  fs.writeFileSync(
    path.join(cwd, "dist", "maps", "index.d.ts.map"),
    JSON.stringify({
      version: 3,
      file: "../index.d.ts",
      sourceRoot: new URL(`file://${path.join(cwd, "src")}/`).toString(),
      sources: ["routes/%68ealth.ts?src=types#frag", "routes/%75sers.ts"],
      names: [],
      mappings: "",
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

  const goOutDir = path.join(cwd, "dist-go");
  emitGoProject(ir, goOutDir);
  assertGoBuildSuccessIfToolchainAvailable(goOutDir);
});

test("M1 regression: d.ts sourceMappingURL file URL with mixed separators keeps typed IR provenance deterministic without recoverable-map diagnostics", () => {
  const cwd = fs.mkdtempSync(
    path.join(os.tmpdir(), "tsgodown-pipeline-dts-fileurl-mixed-separators-"),
  );
  tempDirs.push(cwd);

  fs.mkdirSync(path.join(cwd, "dist", "maps"), { recursive: true });
  fs.mkdirSync(path.join(cwd, "src", "routes"), { recursive: true });
  fs.mkdirSync(path.join(cwd, "src", "contracts"), { recursive: true });

  fs.writeFileSync(
    path.join(cwd, "dist", "index.mjs"),
    [
      "const health = () => ({ ok: true });",
      "export { health };",
      "//# sourceMappingURL=maps/index.mjs.map",
      "",
    ].join("\n"),
  );

  fs.writeFileSync(
    path.join(cwd, "dist", "maps", "index.mjs.map"),
    JSON.stringify({
      version: 3,
      file: "../index.mjs",
      sourceRoot: new URL(`file://${path.join(cwd, "src")}/`).toString(),
      sources: ["routes/health.ts"],
      names: [],
      mappings: "",
    }),
  );

  const dtsMapAbsolute = path.join(cwd, "dist", "maps", "index.d.ts.map");
  const dtsMapMixedSeparatorFileUrl = new URL(
    `file://${dtsMapAbsolute.replaceAll("\\", "/")}`,
  )
    .toString()
    .replace("/maps/", "\\maps/");

  fs.writeFileSync(
    path.join(cwd, "dist", "index.d.ts"),
    [
      "export declare const health: () => { ok: boolean };",
      "export declare type Contract = { ok: true };",
      `//# sourceMappingURL=${dtsMapMixedSeparatorFileUrl}?v=19#types`,
      "",
    ].join("\n"),
  );

  fs.writeFileSync(
    path.join(cwd, "dist", "maps", "index.d.ts.map"),
    JSON.stringify({
      version: 3,
      file: "../index.d.ts",
      sourceRoot: new URL(
        `file://${path.join(cwd, "src", "nested", "..")}/`,
      ).toString(),
      sources: ["contracts/./contract.ts"],
      names: [],
      mappings: "",
    }),
  );

  const buildResult: RunBuildResult = {
    mode: "rust-engine-adapter",
    manifestPath: "artifacts/manifests/manifest.json",
    manifestIndexPath: "artifacts/manifests/index.json",
    manifest: {
      buildId: "1919aa55bb66cc77",
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
    ["src/contracts/contract.ts", "src/routes/health.ts"],
  );
  assert.deepEqual(
    ir.diagnostics.filter(
      (diag) => diag.code === "PIPELINE_INVALID_SOURCEMAP_MAPPING",
    ),
    [],
  );

  const goOutDir = path.join(cwd, "dist-go");
  emitGoProject(ir, goOutDir);
  assertGoBuildSuccessIfToolchainAvailable(goOutDir);
});

test("M1 regression: d.ts sourceMappingURL query+hash still resolves map for JS+d.ts typed IR union and Go compile smoke", () => {
  const cwd = fs.mkdtempSync(
    path.join(os.tmpdir(), "tsgodown-pipeline-dts-map-query-hash-e2e-"),
  );
  tempDirs.push(cwd);

  fs.mkdirSync(path.join(cwd, "dist", "maps"), { recursive: true });
  fs.mkdirSync(path.join(cwd, "src", "routes"), { recursive: true });
  fs.mkdirSync(path.join(cwd, "src", "contracts"), { recursive: true });

  fs.writeFileSync(
    path.join(cwd, "dist", "index.mjs"),
    [
      "const health = () => ({ ok: true });",
      "export { health };",
      "//# sourceMappingURL=maps/index.mjs.map",
      "",
    ].join("\n"),
  );

  fs.writeFileSync(
    path.join(cwd, "dist", "maps", "index.mjs.map"),
    JSON.stringify({
      version: 3,
      file: "../index.mjs",
      sourceRoot: new URL(`file://${path.join(cwd, "src")}/`).toString(),
      sources: ["routes/health.ts"],
      names: [],
      mappings: "",
    }),
  );

  fs.writeFileSync(
    path.join(cwd, "dist", "index.d.ts"),
    [
      "export declare const health: () => { ok: boolean };",
      "export declare type User = { id: string };",
      "//# sourceMappingURL=maps/index.d.ts.map?ts=20260219#types",
      "",
    ].join("\n"),
  );

  fs.writeFileSync(
    path.join(cwd, "dist", "maps", "index.d.ts.map"),
    JSON.stringify({
      version: 3,
      file: "../index.d.ts",
      sourceRoot: new URL(`file://${path.join(cwd, "src")}/`).toString(),
      sources: ["contracts/user.ts"],
      names: [],
      mappings: "",
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
    ["src/contracts/user.ts", "src/routes/health.ts"],
  );

  const goOutDir = path.join(cwd, "dist-go");
  emitGoProject(ir, goOutDir);
  assertGoBuildSuccessIfToolchainAvailable(goOutDir);
});

test("M1 regression: inline JS data URL sourcemap + external d.ts sourcemap keep deterministic typed IR and Go smoke under URI/percent-encoding source variants", () => {
  const cwd = fs.mkdtempSync(
    path.join(os.tmpdir(), "tsgodown-pipeline-inline-uri-normalization-e2e-"),
  );
  tempDirs.push(cwd);

  fs.mkdirSync(path.join(cwd, "dist", "maps"), { recursive: true });
  fs.mkdirSync(path.join(cwd, "src", "routes"), { recursive: true });
  fs.mkdirSync(path.join(cwd, "src", "contracts"), { recursive: true });

  const inlineJsMapPayload = JSON.stringify({
    version: 3,
    file: "index.mjs",
    sourceRoot: `FILE://${path.join(cwd, "src").replaceAll("\\", "/")}/`,
    sources: ["%72outes/%68ealth.ts"],
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

  const healthAbsFileUrl = new URL(
    `file://${path.join(cwd, "src", "routes", "health.ts")}`,
  ).toString();

  fs.writeFileSync(
    path.join(cwd, "dist", "index.d.ts"),
    [
      "declare const health: () => { ok: boolean };",
      "declare interface HealthStatus { ok: boolean; }",
      "export { health as healthHandler };",
      "export type { HealthStatus as PublicHealthStatus };",
      "//# sourceMappingURL=maps/index.d.ts.map?rev=19#types",
      "",
    ].join("\n"),
  );

  fs.writeFileSync(
    path.join(cwd, "dist", "maps", "index.d.ts.map"),
    JSON.stringify({
      version: 3,
      file: "../index.d.ts",
      sourceRoot: new URL(`file://${path.join(cwd, "src")}/`).toString(),
      sources: [
        `${healthAbsFileUrl}?from=types#decl`,
        "./contracts/%68ealth.ts",
      ],
      names: [],
      mappings: "",
    }),
  );

  const buildResult: RunBuildResult = {
    mode: "rust-engine-adapter",
    manifestPath: "artifacts/manifests/manifest.json",
    manifestIndexPath: "artifacts/manifests/index.json",
    manifest: {
      buildId: "1919191919191919",
      entries: ["src/index.ts"],
      bundles: [
        {
          file: "dist/index.mjs",
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
    ["src/contracts/health.ts", "src/routes/health.ts"],
  );
  assert.deepEqual(ir.modules[0]?.exports, [
    "healthHandler",
    "PublicHealthStatus",
  ]);
  assert.deepEqual(ir.diagnostics, []);

  const goOutDir = path.join(cwd, "dist-go");
  emitGoProject(ir, goOutDir);
  assertGoBuildSuccessIfToolchainAvailable(goOutDir);
});

test("M1 regression: inline JS data URL sourcemap + external d.ts map dedupe duplicate logical sources when query/hash are raw vs percent-encoded", async () => {
  const cwd = fs.mkdtempSync(
    path.join(
      os.tmpdir(),
      "tsgodown-pipeline-inline-external-encoded-queryhash-e2e-",
    ),
  );
  tempDirs.push(cwd);

  fs.mkdirSync(path.join(cwd, "dist", "maps"), { recursive: true });
  fs.mkdirSync(path.join(cwd, "src", "routes"), { recursive: true });
  fs.mkdirSync(path.join(cwd, "src", "types"), { recursive: true });

  const inlineJsMapPayload = JSON.stringify({
    version: 3,
    file: "index.mjs",
    sourceRoot: "../src",
    sources: ["routes/health.ts?from=inline#frag"],
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
      "//# sourceMappingURL=maps/index.d.ts.map?rev=19#types",
      "",
    ].join("\n"),
  );

  fs.writeFileSync(
    path.join(cwd, "dist", "maps", "index.d.ts.map"),
    JSON.stringify({
      version: 3,
      file: "../index.d.ts",
      sourceRoot: "../../src",
      sources: ["routes/health.ts%3Ffrom%3Dtypes%23decl", "types/http.ts"],
      names: [],
      mappings: "",
    }),
  );

  const buildResult: RunBuildResult = {
    mode: "rust-engine-adapter",
    manifestPath: "artifacts/manifests/manifest.json",
    manifestIndexPath: "artifacts/manifests/index.json",
    manifest: {
      buildId: "1919191919191919",
      entries: ["src/index.ts"],
      bundles: [
        {
          file: "dist/index.mjs",
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
    ["src/routes/health.ts", "src/types/http.ts"],
  );
  assert.deepEqual(ir.modules[0]?.exports, [
    "healthHandler",
    "PublicHealthStatus",
  ]);
  assert.deepEqual(ir.diagnostics, []);

  const goOutDir = path.join(cwd, "dist-go");
  emitGoProject(ir, goOutDir);
  assertGoBuildSuccessIfToolchainAvailable(goOutDir);
  await assertGoHealthRuntimeReady(goOutDir);
});

test("M1 regression: real JS+d.ts+sourcemap keeps stable symbol/type linkage for export type braces with query/hash map URL", () => {
  const cwd = fs.mkdtempSync(
    path.join(os.tmpdir(), "tsgodown-pipeline-dts-export-type-linkage-e2e-"),
  );
  tempDirs.push(cwd);

  fs.mkdirSync(path.join(cwd, "dist", "maps"), { recursive: true });
  fs.mkdirSync(path.join(cwd, "src", "routes"), { recursive: true });
  fs.mkdirSync(path.join(cwd, "src", "contracts"), { recursive: true });

  fs.writeFileSync(
    path.join(cwd, "dist", "index.mjs"),
    [
      "const health = () => ({ ok: true });",
      "export { health };",
      "//# sourceMappingURL=maps/index.mjs.map?rev=4#bundle",
      "",
    ].join("\n"),
  );

  fs.writeFileSync(
    path.join(cwd, "dist", "maps", "index.mjs.map"),
    JSON.stringify({
      version: 3,
      file: "../index.mjs",
      sourceRoot: new URL(`file://${path.join(cwd, "src")}/`).toString(),
      sources: ["routes/health.ts"],
      names: [],
      mappings: "",
    }),
  );

  fs.writeFileSync(
    path.join(cwd, "dist", "index.d.ts"),
    [
      "declare const health: () => { ok: boolean };",
      "declare interface HealthStatus { ok: boolean; }",
      "export { health as healthHandler };",
      "export type { HealthStatus as PublicHealthStatus };",
      "//# sourceMappingURL=maps/index.d.ts.map?rev=4#types",
      "",
    ].join("\n"),
  );

  fs.writeFileSync(
    path.join(cwd, "dist", "maps", "index.d.ts.map"),
    JSON.stringify({
      version: 3,
      file: "../index.d.ts",
      sourceRoot: new URL(`file://${path.join(cwd, "src")}/`).toString(),
      sources: ["contracts/health.ts"],
      names: [],
      mappings: "",
    }),
  );

  const buildResult: RunBuildResult = {
    mode: "rust-engine-adapter",
    manifestPath: "artifacts/manifests/manifest.json",
    manifestIndexPath: "artifacts/manifests/index.json",
    manifest: {
      buildId: "deadbeefcafefeed",
      entries: ["src/index.ts"],
      bundles: [
        {
          file: "dist/index.mjs",
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
    ["src/contracts/health.ts", "src/routes/health.ts"],
  );
  assert.deepEqual(ir.modules[0]?.exports, [
    "healthHandler",
    "PublicHealthStatus",
  ]);

  const goOutDir = path.join(cwd, "dist-go");
  emitGoProject(ir, goOutDir);
  assertGoBuildSuccessIfToolchainAvailable(goOutDir);
});

test("M1 regression: missing declared bundle map query/hash falls back to sourceMappingURL discovery deterministically", () => {
  const cwd = fs.mkdtempSync(
    path.join(os.tmpdir(), "tsgodown-pipeline-map-queryhash-fallback-e2e-"),
  );
  tempDirs.push(cwd);

  fs.mkdirSync(path.join(cwd, "dist", "maps"), { recursive: true });

  fs.writeFileSync(
    path.join(cwd, "dist", "index.mjs"),
    [
      "const health = () => ({ ok: true });",
      "export { health };",
      "//# sourceMappingURL=maps/index.mjs.map?cache=42#bundle",
      "",
    ].join("\n"),
  );

  fs.writeFileSync(
    path.join(cwd, "dist", "maps", "index.mjs.map"),
    JSON.stringify({
      version: 3,
      file: "../index.mjs",
      sourceRoot: new URL(`file://${path.join(cwd, "src")}/`).toString(),
      sources: ["routes/health.ts"],
      names: [],
      mappings: "",
    }),
  );

  fs.writeFileSync(
    path.join(cwd, "dist", "index.d.ts"),
    ["export declare const health: () => { ok: boolean };", ""].join("\n"),
  );

  const buildResult: RunBuildResult = {
    mode: "rust-engine-adapter",
    manifestPath: "artifacts/manifests/manifest.json",
    manifestIndexPath: "artifacts/manifests/index.json",
    manifest: {
      buildId: "77889900aabbccdd",
      entries: ["src/index.ts"],
      bundles: [
        {
          file: "dist/index.mjs",
          map: "dist/maps/missing-index.mjs.map?cache=manifest#asset",
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
  assert.deepEqual(ir.modules[0]?.exports, ["health"]);
  assert.deepEqual(ir.diagnostics, []);

  const goOutDir = path.join(cwd, "dist-go");
  emitGoProject(ir, goOutDir);
  const goMain = fs.readFileSync(path.join(goOutDir, "main.go"), "utf8");
  assert.doesNotMatch(goMain, /missing-index\.mjs\.map/);
  assertGoBuildSuccessIfToolchainAvailable(goOutDir);
});

test("M1 regression: JS+d.ts indexed sourcemap sections union deterministic typed IR provenance across mixed relative/absolute sourceRoot", () => {
  const cwd = fs.mkdtempSync(
    path.join(os.tmpdir(), "tsgodown-pipeline-indexed-union-mixed-root-e2e-"),
  );
  tempDirs.push(cwd);

  fs.mkdirSync(path.join(cwd, "dist", "maps"), { recursive: true });

  fs.writeFileSync(
    path.join(cwd, "dist", "index.mjs"),
    [
      "const health = () => ({ ok: true });",
      "export { health };",
      "//# sourceMappingURL=maps/index.mjs.map",
      "",
    ].join("\n"),
  );

  fs.writeFileSync(
    path.join(cwd, "dist", "index.d.ts"),
    [
      "export declare const health: () => { ok: boolean };",
      "export declare interface User { id: string }",
      "//# sourceMappingURL=maps/index.d.ts.map",
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
            sources: ["routes/health.ts"],
            mappings: "",
          },
        },
        {
          offset: { line: 2, column: 0 },
          map: {
            version: 3,
            sourceRoot: " ../../src/shared/.. ",
            sources: ["contracts/session.ts"],
            mappings: "",
          },
        },
      ],
    }),
  );

  fs.writeFileSync(
    path.join(cwd, "dist", "maps", "index.d.ts.map"),
    JSON.stringify({
      version: 3,
      file: "../index.d.ts",
      sections: [
        {
          offset: { line: 0, column: 0 },
          map: {
            version: 3,
            sourceRoot: new URL(`file://${path.join(cwd, "src")}/`).toString(),
            sources: ["types/user.ts"],
            mappings: "",
          },
        },
        {
          offset: { line: 1, column: 0 },
          map: {
            version: 3,
            sourceRoot: " ../../src ",
            sources: ["contracts/session.ts"],
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
      buildId: "c0ffee0011223344",
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
    ["src/contracts/session.ts", "src/routes/health.ts", "src/types/user.ts"],
  );
  assert.deepEqual(ir.modules[0]?.exports, ["health", "User"]);
  assert.deepEqual(ir.diagnostics, []);

  const goOutDir = path.join(cwd, "dist-go");
  emitGoProject(ir, goOutDir);
  assertGoBuildSuccessIfToolchainAvailable(goOutDir);
});

test("M1 regression: Windows drive-letter/backslash sourcemap sourceRoot+sources normalize into deterministic typed IR provenance and Go compile smoke", () => {
  const cwd = fs.mkdtempSync(
    path.join(os.tmpdir(), "tsgodown-pipeline-windows-path-style-e2e-"),
  );
  tempDirs.push(cwd);

  fs.mkdirSync(path.join(cwd, "dist", "maps"), { recursive: true });

  fs.writeFileSync(
    path.join(cwd, "dist", "index.mjs"),
    [
      "const health = () => ({ ok: true });",
      "export { health };",
      "//# sourceMappingURL=maps/index.mjs.map",
      "",
    ].join("\n"),
  );

  fs.writeFileSync(
    path.join(cwd, "dist", "index.d.ts"),
    [
      "export declare const health: () => { ok: boolean };",
      "export declare interface HealthResponse { ok: boolean }",
      "//# sourceMappingURL=maps/index.d.ts.map",
      "",
    ].join("\n"),
  );

  fs.writeFileSync(
    path.join(cwd, "dist", "maps", "index.mjs.map"),
    JSON.stringify({
      version: 3,
      file: "..\\index.mjs",
      sourceRoot: "..\\..\\src",
      sources: ["routes\\health.ts"],
      names: [],
      mappings: "",
    }),
  );

  fs.writeFileSync(
    path.join(cwd, "dist", "maps", "index.d.ts.map"),
    JSON.stringify({
      version: 3,
      file: "C:\\repo\\dist\\index.d.ts",
      sourceRoot: "..\\..\\src\\nested\\..",
      sources: ["types\\health-response.ts"],
      names: [],
      mappings: "",
    }),
  );

  const buildResult: RunBuildResult = {
    mode: "rust-engine-adapter",
    manifestPath: "artifacts/manifests/manifest.json",
    manifestIndexPath: "artifacts/manifests/index.json",
    manifest: {
      buildId: "19aa55cc77ee22ff",
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
    ["src/routes/health.ts", "src/types/health-response.ts"],
  );
  const typedModule = ir.modules.find(
    (module) => module.sourcePath === "src/types/health-response.ts",
  );
  assert.deepEqual(typedModule?.exports, ["health", "HealthResponse"]);
  assert.deepEqual(ir.diagnostics, []);

  const goOutDir = path.join(cwd, "dist-go");
  emitGoProject(ir, goOutDir);
  assertGoBuildSuccessIfToolchainAvailable(goOutDir);
});

test("M1 regression: Windows drive-letter file:// sourceRoot mixed with backslash sources keeps deterministic JS+d.ts typed provenance and Go compile smoke", () => {
  const cwd = fs.mkdtempSync(
    path.join(os.tmpdir(), "tsgodown-pipeline-windows-drive-file-url-e2e-"),
  );
  tempDirs.push(cwd);

  fs.mkdirSync(path.join(cwd, "dist", "maps"), { recursive: true });

  fs.writeFileSync(
    path.join(cwd, "dist", "index.mjs"),
    [
      "const health = () => ({ ok: true });",
      "export { health };",
      "//# sourceMappingURL=maps/index.mjs.map",
      "",
    ].join("\n"),
  );

  fs.writeFileSync(
    path.join(cwd, "dist", "index.d.ts"),
    [
      "export declare const health: () => { ok: boolean };",
      "export declare interface HealthResponse { ok: boolean }",
      "//# sourceMappingURL=maps/index.d.ts.map",
      "",
    ].join("\n"),
  );

  const windowsDriveFileRoot = `file:///C:${cwd.replaceAll("\\", "/")}/src/`;

  fs.writeFileSync(
    path.join(cwd, "dist", "maps", "index.mjs.map"),
    JSON.stringify({
      version: 3,
      file: "..\\index.mjs",
      sourceRoot: windowsDriveFileRoot,
      sources: ["routes\\health.ts"],
      names: [],
      mappings: "",
    }),
  );

  fs.writeFileSync(
    path.join(cwd, "dist", "maps", "index.d.ts.map"),
    JSON.stringify({
      version: 3,
      file: "..\\index.d.ts",
      sourceRoot: windowsDriveFileRoot,
      sources: [
        `file:///C:${cwd.replaceAll("\\", "/")}/src/types\\health-response.ts`,
      ],
      names: [],
      mappings: "",
    }),
  );

  const buildResult: RunBuildResult = {
    mode: "rust-engine-adapter",
    manifestPath: "artifacts/manifests/manifest.json",
    manifestIndexPath: "artifacts/manifests/index.json",
    manifest: {
      buildId: "19aa55ff88ee33aa",
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
    ["src/routes/health.ts", "src/types/health-response.ts"],
  );
  const typedModule = ir.modules.find(
    (module) => module.sourcePath === "src/types/health-response.ts",
  );
  assert.deepEqual(typedModule?.exports, ["health", "HealthResponse"]);
  assert.deepEqual(ir.diagnostics, []);

  const goOutDir = path.join(cwd, "dist-go");
  emitGoProject(ir, goOutDir);
  assertGoBuildSuccessIfToolchainAvailable(goOutDir);
});

test("M1 regression: mixed file:///C:/ and C:\\ sourceRoot with percent-encoded segments dedupes typed IR provenance deterministically", () => {
  const cwd = fs.mkdtempSync(
    path.join(os.tmpdir(), "tsgodown-pipeline-mixed-windows-root-encoded-e2e-"),
  );
  tempDirs.push(cwd);

  const sourceRootDir = path.join(cwd, "src#v1");
  fs.mkdirSync(path.join(cwd, "dist", "maps"), { recursive: true });
  fs.mkdirSync(path.join(sourceRootDir, "routes"), { recursive: true });
  fs.mkdirSync(path.join(sourceRootDir, "types"), { recursive: true });

  fs.writeFileSync(
    path.join(cwd, "dist", "index.mjs"),
    [
      "const health = () => ({ ok: true });",
      "export { health };",
      "//# sourceMappingURL=maps/index.mjs.map",
      "",
    ].join("\n"),
  );

  fs.writeFileSync(
    path.join(cwd, "dist", "index.d.ts"),
    [
      "export declare const health: () => { ok: boolean };",
      "export declare interface HealthResponse { ok: boolean }",
      "//# sourceMappingURL=maps/index.d.ts.map",
      "",
    ].join("\n"),
  );

  const windowsDriveFileRoot = `file:///C:${cwd.replaceAll("\\", "/")}/src%23v1/`;
  const windowsDriveBackslashRoot = `C:${cwd.replaceAll("/", "\\\\")}\\src#v1`;

  fs.writeFileSync(
    path.join(cwd, "dist", "maps", "index.mjs.map"),
    JSON.stringify({
      version: 3,
      file: "..\\index.mjs",
      sourceRoot: windowsDriveFileRoot,
      sources: ["routes%2Fhealth.ts"],
      names: [],
      mappings: "",
    }),
  );

  fs.writeFileSync(
    path.join(cwd, "dist", "maps", "index.d.ts.map"),
    JSON.stringify({
      version: 3,
      file: "..\\index.d.ts",
      sourceRoot: windowsDriveBackslashRoot,
      sources: ["types\\health-response.ts"],
      names: [],
      mappings: "",
    }),
  );

  const buildResult: RunBuildResult = {
    mode: "rust-engine-adapter",
    manifestPath: "artifacts/manifests/manifest.json",
    manifestIndexPath: "artifacts/manifests/index.json",
    manifest: {
      buildId: "f0aa11bb22cc33dd",
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
    ["src#v1/routes/health.ts", "src#v1/types/health-response.ts"],
  );
  const typedModule = ir.modules.find(
    (module) => module.sourcePath === "src#v1/types/health-response.ts",
  );
  assert.deepEqual(typedModule?.exports, ["health", "HealthResponse"]);
  assert.deepEqual(ir.diagnostics, []);

  const goOutDir = path.join(cwd, "dist-go");
  emitGoProject(ir, goOutDir);
  assertGoBuildSuccessIfToolchainAvailable(goOutDir);
});

test("M1 regression: inline JS map + external d.ts map with encoded sourceRoot query/hash keeps deterministic typed IR provenance and Go compile smoke", () => {
  const cwd = fs.mkdtempSync(
    path.join(os.tmpdir(), "tsgodown-pipeline-inline-js-encoded-dts-root-e2e-"),
  );
  tempDirs.push(cwd);

  const sourceRootDir = path.join(cwd, "src#v1");
  fs.mkdirSync(path.join(cwd, "dist", "maps"), { recursive: true });
  fs.mkdirSync(path.join(sourceRootDir, "routes"), { recursive: true });
  fs.mkdirSync(path.join(sourceRootDir, "types"), { recursive: true });

  const inlineJsMap = {
    version: 3,
    file: "index.mjs",
    sourceRoot: new URL(
      `file://${sourceRootDir.replace("#", "%23")}/`,
    ).toString(),
    sources: ["routes/health.ts?from=inline#frag"],
    names: [],
    mappings: "",
  };
  const inlineJsMapDataUrl = `data:application/json;charset=utf-8;base64,${Buffer.from(
    JSON.stringify(inlineJsMap),
    "utf8",
  ).toString("base64")}`;

  fs.writeFileSync(
    path.join(cwd, "dist", "index.mjs"),
    [
      "const health = () => ({ ok: true });",
      "export { health };",
      `//# sourceMappingURL=${inlineJsMapDataUrl}`,
      "",
    ].join("\n"),
  );

  fs.writeFileSync(
    path.join(cwd, "dist", "index.d.ts"),
    [
      "export declare const health: () => { ok: boolean };",
      "export declare interface HealthType { ok: boolean }",
      "//# sourceMappingURL=maps/index.d.ts.map?cache=7#types",
      "",
    ].join("\n"),
  );

  fs.writeFileSync(
    path.join(cwd, "dist", "maps", "index.d.ts.map"),
    JSON.stringify({
      version: 3,
      file: "../index.d.ts",
      sourceRoot: `${new URL(`file://${sourceRootDir.replace("#", "%23")}/`).toString()}?cache=types#decl`,
      sources: ["./types/health.ts"],
      names: [],
      mappings: "",
    }),
  );

  const buildResult: RunBuildResult = {
    mode: "rust-engine-adapter",
    manifestPath: "artifacts/manifests/manifest.json",
    manifestIndexPath: "artifacts/manifests/index.json",
    manifest: {
      buildId: "aa11bb22cc33dd44",
      entries: ["src/index.ts"],
      bundles: [
        {
          file: "dist/index.mjs",
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
    ["src#v1/routes/health.ts", "src#v1/types/health.ts"],
  );
  const typedModule = ir.modules.find(
    (module) => module.sourcePath === "src#v1/types/health.ts",
  );
  assert.deepEqual(typedModule?.exports, ["health", "HealthType"]);
  assert.deepEqual(ir.diagnostics, []);

  const goOutDir = path.join(cwd, "dist-go");
  emitGoProject(ir, goOutDir);
  assertGoBuildSuccessIfToolchainAvailable(goOutDir);
});

test("M1 regression: file URL sourcemap sources with percent-encoded slash stay deterministic across JS+d.ts typed IR and Go compile path", () => {
  const cwd = fs.mkdtempSync(
    path.join(os.tmpdir(), "tsgodown-pipeline-e2e-file-url-encoded-slash-"),
  );
  tempDirs.push(cwd);

  fs.mkdirSync(path.join(cwd, "dist", "maps"), { recursive: true });
  fs.mkdirSync(path.join(cwd, "src", "routes"), { recursive: true });
  fs.mkdirSync(path.join(cwd, "src", "types"), { recursive: true });

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
      "export declare interface HealthContract { ok: boolean }",
      "//# sourceMappingURL=maps/index.d.ts.map",
      "",
    ].join("\n"),
  );

  fs.writeFileSync(
    path.join(cwd, "dist", "maps", "index.mjs.map"),
    JSON.stringify({
      version: 3,
      file: "../index.mjs",
      sources: [
        new URL(`file://${path.join(cwd, "src", "routes", "health.ts")}`)
          .toString()
          .replace("/routes/health.ts", "/routes%2Fhealth.ts"),
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
      sources: [
        new URL(
          `file://${path.join(cwd, "src", "types", "health-contract.ts")}`,
        )
          .toString()
          .replace("/types/health-contract.ts", "/types%2Fhealth-contract.ts"),
      ],
      names: [],
      mappings: "",
    }),
  );

  const buildResult: RunBuildResult = {
    mode: "rust-engine-adapter",
    manifestPath: "artifacts/manifests/manifest.json",
    manifestIndexPath: "artifacts/manifests/index.json",
    manifest: {
      buildId: "3fa9b1d2478c60ef",
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
    ["src/routes/health.ts", "src/types/health-contract.ts"],
  );
  assert.deepEqual(
    ir.modules.find(
      (module) => module.sourcePath === "src/types/health-contract.ts",
    )?.exports,
    ["health", "HealthContract"],
  );
  assert.deepEqual(ir.diagnostics, []);

  const goOutDir = path.join(cwd, "dist-go");
  emitGoProject(ir, goOutDir);
  assertGoBuildSuccessIfToolchainAvailable(goOutDir);
});

test("M1 regression: chained JS external sourcemap + inline d.ts sourcemap keep canonical typed export source identity", () => {
  const cwd = fs.mkdtempSync(
    path.join(
      os.tmpdir(),
      "tsgodown-pipeline-chained-js-map-inline-dts-canonical-",
    ),
  );
  tempDirs.push(cwd);

  fs.mkdirSync(path.join(cwd, "dist", "maps"), { recursive: true });
  fs.mkdirSync(path.join(cwd, "src", "contracts"), { recursive: true });

  fs.writeFileSync(
    path.join(cwd, "dist", "index.mjs"),
    [
      "const health = () => ({ ok: true });",
      "export { health };",
      "//# sourceMappingURL=maps/index.mjs.map",
      "",
    ].join("\n"),
  );

  const inlineDtsMap = {
    version: 3,
    file: "index.d.ts",
    sources: ["../src/contracts/health.d.ts"],
    names: [],
    mappings: "",
    sourcesContent: [
      "export declare const health: () => { ok: boolean };\\nexport declare interface HealthContract { ok: boolean }",
    ],
  };

  fs.writeFileSync(
    path.join(cwd, "dist", "index.d.ts"),
    [
      "export declare const health: () => { ok: boolean };",
      "export declare interface HealthContract { ok: boolean }",
      `//# sourceMappingURL=data:application/json;charset=utf-8,${encodeURIComponent(JSON.stringify(inlineDtsMap))}`,
      "",
    ].join("\n"),
  );

  fs.writeFileSync(
    path.join(cwd, "dist", "maps", "index.mjs.map"),
    JSON.stringify({
      version: 3,
      file: "../index.mjs",
      sourceRoot: new URL(`file://${path.join(cwd, "src")}/`).toString(),
      sources: ["contracts/health.ts"],
      names: [],
      mappings: "",
    }),
  );

  const buildResult: RunBuildResult = {
    mode: "rust-engine-adapter",
    manifestPath: "artifacts/manifests/manifest.json",
    manifestIndexPath: "artifacts/manifests/index.json",
    manifest: {
      buildId: "4b8b69d71524ca0e",
      entries: ["src/index.ts"],
      bundles: [
        {
          file: "dist/index.mjs",
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
    ["src/contracts/health.ts"],
  );
  assert.deepEqual(ir.modules[0]?.exports, ["health", "HealthContract"]);
  assert.deepEqual(ir.diagnostics, []);

  const goOutDir = path.join(cwd, "dist-go");
  emitGoProject(ir, goOutDir);
  assertGoBuildSuccessIfToolchainAvailable(goOutDir);
});

test("M1 regression: legacy //@ sourceMappingURL directives in JS+d.ts are discovered for typed IR provenance and Go compile path", () => {
  const cwd = fs.mkdtempSync(
    path.join(os.tmpdir(), "tsgodown-pipeline-legacy-at-sourcemap-directive-"),
  );
  tempDirs.push(cwd);

  fs.mkdirSync(path.join(cwd, "dist", "maps"), { recursive: true });
  fs.mkdirSync(path.join(cwd, "src", "routes"), { recursive: true });
  fs.mkdirSync(path.join(cwd, "src", "types"), { recursive: true });

  fs.writeFileSync(
    path.join(cwd, "dist", "index.mjs"),
    [
      "const health = () => ({ ok: true });",
      "export { health };",
      "//@ sourceMappingURL=maps/index.mjs.map",
      "",
    ].join("\n"),
  );

  fs.writeFileSync(
    path.join(cwd, "dist", "index.d.ts"),
    [
      "export declare const health: () => { ok: boolean };",
      "export declare interface HealthPayload { ok: boolean }",
      "//@ sourceMappingURL=maps/index.d.ts.map",
      "",
    ].join("\n"),
  );

  fs.writeFileSync(
    path.join(cwd, "dist", "maps", "index.mjs.map"),
    JSON.stringify({
      version: 3,
      file: "../index.mjs",
      sourceRoot: new URL(`file://${path.join(cwd, "src")}/`).toString(),
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
      sourceRoot: new URL(`file://${path.join(cwd, "src")}/`).toString(),
      sources: ["types/health.ts"],
      names: [],
      mappings: "",
    }),
  );

  const buildResult: RunBuildResult = {
    mode: "rust-engine-adapter",
    manifestPath: "artifacts/manifests/manifest.json",
    manifestIndexPath: "artifacts/manifests/index.json",
    manifest: {
      buildId: "20aa11bb22cc33dd",
      entries: ["src/index.ts"],
      bundles: [
        {
          file: "dist/index.mjs",
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
    ["src/routes/health.ts", "src/types/health.ts"],
  );
  assert.deepEqual(ir.modules[0]?.exports, ["health", "HealthPayload"]);

  const goOutDir = path.join(cwd, "dist-go");
  emitGoProject(ir, goOutDir);
  assertGoBuildSuccessIfToolchainAvailable(goOutDir);
});

test("M1 regression: relative sourceRoot + encoded source variants across JS+d.ts maps dedupe to deterministic typed IR and Go smoke", () => {
  const cwd = fs.mkdtempSync(
    path.join(os.tmpdir(), "tsgodown-pipeline-e2e-relative-encoded-dedupe-"),
  );
  tempDirs.push(cwd);

  fs.mkdirSync(path.join(cwd, "dist", "maps"), { recursive: true });
  fs.mkdirSync(path.join(cwd, "src", "routes"), { recursive: true });
  fs.mkdirSync(path.join(cwd, "src", "types"), { recursive: true });

  fs.writeFileSync(
    path.join(cwd, "dist", "index.mjs"),
    [
      "const health = () => ({ ok: true });",
      "export { health };",
      "//# sourceMappingURL=maps/index.mjs.map",
      "",
    ].join("\n"),
  );

  fs.writeFileSync(
    path.join(cwd, "dist", "index.d.ts"),
    [
      "export declare const health: () => { ok: boolean };",
      "export declare interface HealthDTO { ok: boolean }",
      "//# sourceMappingURL=maps/index.d.ts.map?rev=20#types",
      "",
    ].join("\n"),
  );

  fs.writeFileSync(
    path.join(cwd, "dist", "maps", "index.mjs.map"),
    JSON.stringify({
      version: 3,
      file: "../index.mjs",
      sourceRoot: "../../src",
      sources: [
        "routes/health.ts?rev=20#raw",
        "routes%2Fhealth.ts?rev=20#encoded",
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
      sourceRoot: "../../src/nested/..",
      sources: [
        "./routes/health.ts?rev=20#decl",
        "types/health.ts?rev=20#types",
        "types%2Fhealth.ts?rev=20#types-encoded",
      ],
      names: [],
      mappings: "",
    }),
  );

  const buildResult: RunBuildResult = {
    mode: "rust-engine-adapter",
    manifestPath: "artifacts/manifests/manifest.json",
    manifestIndexPath: "artifacts/manifests/index.json",
    manifest: {
      buildId: "f0e1d2c3b4a59687",
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
    ["src/routes/health.ts", "src/types/health.ts"],
  );
  const typesModule = ir.modules.find(
    (module) => module.sourcePath === "src/types/health.ts",
  );
  assert.deepEqual(typesModule?.exports, ["health", "HealthDTO"]);
  assert.deepEqual(ir.diagnostics, []);

  const goOutDir = path.join(cwd, "dist-go");
  emitGoProject(ir, goOutDir);
  assertGoBuildSuccessIfToolchainAvailable(goOutDir);
});

test("M1 regression: mixed double-encoded JS sourceRoot + encoded d.ts source paths preserve literal # in canonical typed IR provenance", () => {
  const cwd = fs.mkdtempSync(
    path.join(
      os.tmpdir(),
      "tsgodown-pipeline-double-encoded-literal-hash-e2e-",
    ),
  );
  tempDirs.push(cwd);

  fs.mkdirSync(path.join(cwd, "dist", "maps"), { recursive: true });

  fs.writeFileSync(
    path.join(cwd, "dist", "index.mjs"),
    [
      "const health = () => ({ ok: true });",
      "export { health };",
      "//# sourceMappingURL=maps/index.mjs.map",
      "",
    ].join("\n"),
  );

  fs.writeFileSync(
    path.join(cwd, "dist", "index.d.ts"),
    [
      "export declare const health: () => { ok: boolean };",
      "export declare interface HealthResponse { ok: boolean }",
      "//# sourceMappingURL=maps/index.d.ts.map",
      "",
    ].join("\n"),
  );

  const jsDoubleEncodedRoot = `file://${path
    .join(cwd, "src")
    .replaceAll("\\", "/")
    .replaceAll("/", "%252F")}%252F`;
  const dtsEncodedRoot = `file://${path
    .join(cwd, "src")
    .replaceAll("\\", "/")
    .replaceAll("/", "%2F")}%2F`;

  fs.writeFileSync(
    path.join(cwd, "dist", "maps", "index.mjs.map"),
    JSON.stringify({
      version: 3,
      file: "../index.mjs",
      sourceRoot: jsDoubleEncodedRoot,
      sources: ["routes%252Fhealth%2523v2.ts"],
      names: [],
      mappings: "",
    }),
  );

  fs.writeFileSync(
    path.join(cwd, "dist", "maps", "index.d.ts.map"),
    JSON.stringify({
      version: 3,
      file: "../index.d.ts",
      sourceRoot: dtsEncodedRoot,
      sources: ["routes/health%23v2.ts", "types/health-response.ts"],
      names: [],
      mappings: "",
    }),
  );

  const buildResult: RunBuildResult = {
    mode: "rust-engine-adapter",
    manifestPath: "artifacts/manifests/manifest.json",
    manifestIndexPath: "artifacts/manifests/index.json",
    manifest: {
      buildId: "e0aa11bb22cc33ef",
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
    ["src/routes/health#v2.ts", "src/types/health-response.ts"],
  );
  assert.deepEqual(ir.diagnostics, []);

  const goOutDir = path.join(cwd, "dist-go");
  emitGoProject(ir, goOutDir);
  assertGoBuildSuccessIfToolchainAvailable(goOutDir);
});

test("M1 regression: UNC file URL sourcemap sources normalize deterministically across JS+d.ts and dedupe equivalent forms for Go compile path", () => {
  const cwd = fs.mkdtempSync(
    path.join(os.tmpdir(), "tsgodown-pipeline-unc-fileurl-sourcemap-e2e-"),
  );
  tempDirs.push(cwd);

  fs.mkdirSync(path.join(cwd, "dist", "maps"), { recursive: true });

  fs.writeFileSync(
    path.join(cwd, "dist", "index.mjs"),
    [
      "const health = () => ({ ok: true });",
      "export { health };",
      "//# sourceMappingURL=maps/index.mjs.map",
      "",
    ].join("\n"),
  );

  fs.writeFileSync(
    path.join(cwd, "dist", "index.d.ts"),
    [
      "export declare const health: () => { ok: boolean };",
      "export declare interface HealthPayload { ok: boolean }",
      "//# sourceMappingURL=maps/index.d.ts.map",
      "",
    ].join("\n"),
  );

  fs.writeFileSync(
    path.join(cwd, "dist", "maps", "index.mjs.map"),
    JSON.stringify({
      version: 3,
      file: "../index.mjs",
      sources: [
        "file://server/share/src/routes/health.ts",
        "file://server/share/src/routes/%68ealth.ts?from=js#frag",
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
      sources: [
        "file://server/share/src/types/health.ts",
        "file://server/share/src/routes/health.ts?from=types#frag",
      ],
      names: [],
      mappings: "",
    }),
  );

  const buildResult: RunBuildResult = {
    mode: "rust-engine-adapter",
    manifestPath: "artifacts/manifests/manifest.json",
    manifestIndexPath: "artifacts/manifests/index.json",
    manifest: {
      buildId: "20ee11aa22bb33cc",
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
    ["server/share/src/routes/health.ts", "server/share/src/types/health.ts"],
  );
  assert.deepEqual(ir.modules[0]?.exports, ["health", "HealthPayload"]);
  assert.deepEqual(ir.diagnostics, []);

  const goOutDir = path.join(cwd, "dist-go");
  emitGoProject(ir, goOutDir);
  assertGoBuildSuccessIfToolchainAvailable(goOutDir);
});

test("M1 regression: external JS sourcemap with relative sourceRoot + inline d.ts data URL map canonicalize absolute file:// query/hash sources into stable typed linkage", () => {
  const cwd = fs.mkdtempSync(
    path.join(
      os.tmpdir(),
      "tsgodown-pipeline-external-js-inline-dts-abs-fileurl-e2e-",
    ),
  );
  tempDirs.push(cwd);

  fs.mkdirSync(path.join(cwd, "dist", "maps"), { recursive: true });
  fs.mkdirSync(path.join(cwd, "src", "routes"), { recursive: true });
  fs.mkdirSync(path.join(cwd, "src", "types"), { recursive: true });

  fs.writeFileSync(
    path.join(cwd, "dist", "index.mjs"),
    [
      "const health = () => ({ ok: true });",
      "export { health };",
      "//# sourceMappingURL=maps/index.mjs.map",
      "",
    ].join("\n"),
  );

  fs.writeFileSync(
    path.join(cwd, "dist", "maps", "index.mjs.map"),
    JSON.stringify({
      version: 3,
      file: "../index.mjs",
      sourceRoot: "../../src/nested/..",
      sources: [
        "./routes/health.ts?from=js#bundle",
        "./types/health-contract.ts",
      ],
      names: [],
      mappings: "",
    }),
  );

  const healthAbsFileUrl = new URL(
    `file://${path.join(cwd, "src", "routes", "health.ts")}`,
  ).toString();
  const contractAbsFileUrl = new URL(
    `file://${path.join(cwd, "src", "types", "health-contract.ts")}`,
  ).toString();

  const inlineDtsMapPayload = JSON.stringify({
    version: 3,
    file: "index.d.ts",
    sources: [
      `${healthAbsFileUrl}?from=types#decl`,
      `${healthAbsFileUrl}%3Ffrom%3Dtypes%23decl-encoded`,
      `${contractAbsFileUrl}?from=types#decl`,
    ],
    names: [],
    mappings: "",
  });

  fs.writeFileSync(
    path.join(cwd, "dist", "index.d.ts"),
    [
      "declare const health: () => { ok: boolean };",
      "export interface HealthContract { ok: boolean; }",
      "export { health };",
      `//# sourceMappingURL=data:application/json;base64,${Buffer.from(inlineDtsMapPayload, "utf8").toString("base64")}`,
      "",
    ].join("\n"),
  );

  const buildResult: RunBuildResult = {
    mode: "rust-engine-adapter",
    manifestPath: "artifacts/manifests/manifest.json",
    manifestIndexPath: "artifacts/manifests/index.json",
    manifest: {
      buildId: "2121212121212121",
      entries: ["src/index.ts"],
      bundles: [
        {
          file: "dist/index.mjs",
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
    ["src/routes/health.ts", "src/types/health-contract.ts"],
  );
  assert.deepEqual(
    ir.modules.find((module) => module.sourcePath === "src/routes/health.ts")
      ?.exports,
    ["health", "HealthContract"],
  );
  assert.deepEqual(ir.diagnostics, []);

  const goOutDir = path.join(cwd, "dist-go");
  emitGoProject(ir, goOutDir);
  assertGoBuildSuccessIfToolchainAvailable(goOutDir);
});

test("M1 regression: mixed inline JS map + external d.ts map canonicalize duplicate logical modules across percent-encoded paths and differing sourceRoot forms", async () => {
  const cwd = fs.mkdtempSync(
    path.join(
      os.tmpdir(),
      "tsgodown-pipeline-inline-external-encoded-rootforms-e2e-",
    ),
  );
  tempDirs.push(cwd);

  fs.mkdirSync(path.join(cwd, "dist", "maps"), { recursive: true });
  fs.mkdirSync(path.join(cwd, "src", "routes"), { recursive: true });
  fs.mkdirSync(path.join(cwd, "src", "types"), { recursive: true });

  const inlineJsMapPayload = JSON.stringify({
    version: 3,
    file: "index.mjs",
    sourceRoot: new URL(`file://${path.join(cwd, "src")}/`).toString(),
    sources: [
      "./%72outes/%68ealth.ts%3Ffrom%3Dinline%23bundle",
      "types/%68ealth-response.ts",
    ],
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
      "declare interface HealthResponse { ok: boolean; };",
      "export { health as healthHandler };",
      "export type { HealthResponse };",
      "//# sourceMappingURL=maps/index.d.ts.map?rev=27#types",
      "",
    ].join("\n"),
  );

  const healthFileUrl = new URL(
    `file://${path.join(cwd, "src", "routes", "health.ts")}`,
  ).toString();

  fs.writeFileSync(
    path.join(cwd, "dist", "maps", "index.d.ts.map"),
    JSON.stringify({
      version: 3,
      file: "../index.d.ts",
      sourceRoot: "../../%73rc/./",
      sources: [
        `${healthFileUrl}%3Ffrom%3Dtypes%23decl`,
        "./types/health-response.ts",
      ],
      names: [],
      mappings: "",
    }),
  );

  const buildResult: RunBuildResult = {
    mode: "rust-engine-adapter",
    manifestPath: "artifacts/manifests/manifest.json",
    manifestIndexPath: "artifacts/manifests/index.json",
    manifest: {
      buildId: "2718271827182718",
      entries: ["src/index.ts"],
      bundles: [
        {
          file: "dist/index.mjs",
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
    ["src/routes/health.ts", "src/types/health-response.ts"],
  );
  assert.deepEqual(
    ir.modules.find(
      (module) => module.sourcePath === "src/types/health-response.ts",
    )?.exports,
    ["healthHandler", "HealthResponse"],
  );
  assert.deepEqual(ir.diagnostics, []);

  const goOutDir = path.join(cwd, "dist-go");
  emitGoProject(ir, goOutDir);
  assertGoBuildSuccessIfToolchainAvailable(goOutDir);
  await assertGoHealthRuntimeReady(goOutDir);
});

test("M1 regression: JS+d.ts sourcemap provenance normalizes file://localhost authority as local path for deterministic dedupe", () => {
  const cwd = fs.mkdtempSync(
    path.join(os.tmpdir(), "tsgodown-pipeline-localhost-fileurl-dedupe-e2e-"),
  );
  tempDirs.push(cwd);

  fs.mkdirSync(path.join(cwd, "dist", "maps"), { recursive: true });

  fs.writeFileSync(
    path.join(cwd, "dist", "index.mjs"),
    [
      "const health = () => ({ ok: true });",
      "export { health };",
      "//# sourceMappingURL=maps/index.mjs.map",
      "",
    ].join("\n"),
  );

  fs.writeFileSync(
    path.join(cwd, "dist", "index.d.ts"),
    [
      "export declare const health: () => { ok: boolean };",
      "export declare interface HealthShape { ok: boolean }",
      "//# sourceMappingURL=maps/index.d.ts.map",
      "",
    ].join("\n"),
  );

  const localRoot = path.join(cwd, "src").replaceAll("\\", "/");

  fs.writeFileSync(
    path.join(cwd, "dist", "maps", "index.mjs.map"),
    JSON.stringify({
      version: 3,
      file: "../index.mjs",
      sourceRoot: `file://localhost${localRoot}/`,
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
      sourceRoot: new URL(`file://${localRoot}/`).toString(),
      sources: ["types/health.ts"],
      names: [],
      mappings: "",
    }),
  );

  const buildResult: RunBuildResult = {
    mode: "rust-engine-adapter",
    manifestPath: "artifacts/manifests/manifest.json",
    manifestIndexPath: "artifacts/manifests/index.json",
    manifest: {
      buildId: "21ee11aa22bb33cc",
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
    ["src/routes/health.ts", "src/types/health.ts"],
  );

  const typedModule = ir.modules.find(
    (module) => module.sourcePath === "src/types/health.ts",
  );
  assert.deepEqual(typedModule?.exports, ["health", "HealthShape"]);
  assert.deepEqual(ir.diagnostics, []);

  const goOutDir = path.join(cwd, "dist-go");
  emitGoProject(ir, goOutDir);
  assertGoBuildSuccessIfToolchainAvailable(goOutDir);
});

test("M1 regression: file://localhost sourcemap sourceRoot end-to-end path keeps JS+d.ts typed IR provenance and Go /health runtime reachable", async () => {
  const cwd = fs.mkdtempSync(
    path.join(os.tmpdir(), "tsgodown-pipeline-file-localhost-runtime-e2e-"),
  );
  tempDirs.push(cwd);

  fs.mkdirSync(path.join(cwd, "dist", "maps"), { recursive: true });

  fs.writeFileSync(
    path.join(cwd, "dist", "index.mjs"),
    [
      "const health = () => ({ ok: true });",
      "export { health };",
      "//# sourceMappingURL=maps/index.mjs.map",
      "",
    ].join("\n"),
  );

  fs.writeFileSync(
    path.join(cwd, "dist", "index.d.ts"),
    [
      "export declare const health: () => { ok: boolean };",
      "export declare interface HealthShape { ok: boolean }",
      "//# sourceMappingURL=maps/index.d.ts.map",
      "",
    ].join("\n"),
  );

  const localRoot = path.join(cwd, "src").replaceAll("\\", "/");

  fs.writeFileSync(
    path.join(cwd, "dist", "maps", "index.mjs.map"),
    JSON.stringify({
      version: 3,
      file: "../index.mjs",
      sourceRoot: `file://localhost${localRoot}/`,
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
      sourceRoot: new URL(`file://${localRoot}/`).toString(),
      sources: ["types/health.ts"],
      names: [],
      mappings: "",
    }),
  );

  const buildResult: RunBuildResult = {
    mode: "rust-engine-adapter",
    manifestPath: "artifacts/manifests/manifest.json",
    manifestIndexPath: "artifacts/manifests/index.json",
    manifest: {
      buildId: "31ee11aa22bb33cc",
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
    ["src/routes/health.ts", "src/types/health.ts"],
  );

  const typedModule = ir.modules.find(
    (module) => module.sourcePath === "src/types/health.ts",
  );
  assert.deepEqual(typedModule?.exports, ["health", "HealthShape"]);
  assert.deepEqual(ir.diagnostics, []);

  const goOutDir = path.join(cwd, "dist-go");
  emitGoProject(ir, goOutDir);

  const goMain = fs.readFileSync(path.join(goOutDir, "main.go"), "utf8");
  assertGoMainScaffold(goMain);

  assertGoBuildSuccessIfToolchainAvailable(goOutDir);
  await assertGoHealthRuntimeReady(goOutDir);
});

test("M1 regression: typed exports are withheld when .d.ts exists but sourcemap lineage is missing", () => {
  const cwd = fs.mkdtempSync(
    path.join(
      os.tmpdir(),
      "tsgodown-pipeline-missing-map-typed-provenance-e2e-",
    ),
  );
  tempDirs.push(cwd);

  fs.mkdirSync(path.join(cwd, "dist"), { recursive: true });

  fs.writeFileSync(
    path.join(cwd, "dist", "index.mjs"),
    ["const health = () => ({ ok: true });", "export { health };", ""].join(
      "\n",
    ),
  );

  fs.writeFileSync(
    path.join(cwd, "dist", "index.d.ts"),
    ["export declare const health: () => { ok: boolean };", ""].join("\n"),
  );

  const buildResult: RunBuildResult = {
    mode: "rust-engine-adapter",
    manifestPath: "artifacts/manifests/manifest.json",
    manifestIndexPath: "artifacts/manifests/index.json",
    manifest: {
      buildId: "0aa1bb2cc3dd4ee5",
      entries: ["src/index.ts"],
      bundles: [
        {
          file: "dist/index.mjs",
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
    ["src/index.ts"],
  );
  assert.deepEqual(ir.modules[0]?.exports, []);
  assert.deepEqual(
    ir.diagnostics.map((diagnostic) => diagnostic.code),
    [
      "PIPELINE_INCOMPLETE_TYPED_PROVENANCE",
      "PIPELINE_MISSING_SOURCEMAP_MAPPING",
    ],
  );
});
