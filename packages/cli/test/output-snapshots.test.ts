import assert from "node:assert/strict";
import { spawnSync } from "node:child_process";
import crypto from "node:crypto";
import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import { after, test } from "node:test";

const tempDirs: string[] = [];
const fixturesDir = path.join(import.meta.dirname, "fixtures", "snapshots");
const cliEntry = path.resolve(import.meta.dirname, "..", "dist", "index.js");

after(() => {
  for (const dir of tempDirs) {
    fs.rmSync(dir, { recursive: true, force: true });
  }
});

function setupProject() {
  const dir = fs.mkdtempSync(path.join(os.tmpdir(), "tsgodown-cli-snapshots-"));
  tempDirs.push(dir);

  fs.mkdirSync(path.join(dir, "src"), { recursive: true });
  fs.writeFileSync(
    path.join(dir, "src", "index.ts"),
    [
      "const app = { get: (_path: string, _handler: () => unknown) => undefined };",
      "app.get('/health', () => ({ ok: true }));",
    ].join("\n"),
  );
  fs.writeFileSync(
    path.join(dir, "tsgodown.config.ts"),
    'export default { entry: "src/index.ts", outDir: "dist-go" };\n',
  );

  fs.mkdirSync(path.join(dir, "dist"), { recursive: true });
  fs.writeFileSync(path.join(dir, "dist", "index.js"), "export {};\n");

  return dir;
}

function createRustEngineLauncher(
  cwd: string,
  responseScript: string[],
): string {
  const stubPath = path.join(cwd, `rust-stub-${crypto.randomUUID()}.mjs`);
  fs.writeFileSync(stubPath, responseScript.join("\n"));

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

function runBuild(cwd: string, json: boolean, rustLauncher: string) {
  return spawnSync(
    process.execPath,
    [cliEntry, "build", ...(json ? ["--json"] : [])],
    {
      cwd,
      encoding: "utf8",
      env: {
        ...process.env,
        TSGODOWN_RUST_ENGINE_BIN: rustLauncher,
      },
    },
  );
}

function normalizeOutput(cwd: string, output: string): string {
  const roots = [cwd];
  try {
    roots.push(fs.realpathSync(cwd));
  } catch {
    // noop
  }

  roots.sort((a, b) => b.length - a.length);

  let normalized = output;
  for (const root of roots) {
    normalized = normalized.split(root).join("<CWD>");
  }
  return normalized;
}

function readFixture(name: string): string {
  return fs.readFileSync(path.join(fixturesDir, name), "utf8");
}

test("snapshot: build success output (human + json)", () => {
  const cwd = setupProject();
  const rustLauncher = createRustEngineLauncher(cwd, [
    "for await (const _ of process.stdin) { /* drain */ }",
    "process.stdout.write(JSON.stringify({",
    "  ok: true,",
    "  diagnostics: ['engine=rust-binary-stub'],",
    "  manifest: {",
    "    buildId: '1122334455667788',",
    "    entries: ['src/index.ts'],",
    "    bundles: [{ file: 'dist/index.mjs', map: 'dist/index.mjs.map', format: 'esm', exports: [] }],",
    "    types: ['dist/index.d.ts'],",
    "    tsconfigPath: 'tsconfig.json'",
    "  }",
    "}));",
  ]);

  const human = runBuild(cwd, false, rustLauncher);
  assert.equal(human.status, 0, human.stderr);
  assert.equal(human.stderr, "");

  const humanOut = normalizeOutput(cwd, human.stdout);
  assert.equal(humanOut, readFixture("build-success.human.stdout.txt"));

  const json = runBuild(cwd, true, rustLauncher);
  assert.equal(json.status, 0, json.stderr);
  assert.equal(json.stderr, "");

  const jsonOut = normalizeOutput(cwd, json.stdout);
  assert.equal(jsonOut, readFixture("build-success.json.stdout.txt"));
});

test("snapshot: build failure output (human + json)", () => {
  const cwd = setupProject();
  const rustLauncher = createRustEngineLauncher(cwd, [
    "for await (const _ of process.stdin) { /* drain */ }",
    "process.stdout.write(JSON.stringify({",
    "  ok: false,",
    "  error: {",
    "    source: 'rust-engine-adapter',",
    "    cause: 'invalid build graph',",
    "    guidance: 'Check Rust engine JSON contract and retry.'",
    "  }",
    "}));",
  ]);

  const human = runBuild(cwd, false, rustLauncher);
  assert.notEqual(human.status, 0, "build should fail");

  const humanOut = normalizeOutput(cwd, human.stdout);
  const humanErr = normalizeOutput(cwd, human.stderr);
  assert.equal(humanOut, readFixture("build-failure.human.stdout.txt"));
  assert.equal(humanErr, readFixture("build-failure.human.stderr.txt"));

  const json = runBuild(cwd, true, rustLauncher);
  assert.notEqual(json.status, 0, "build --json should fail");
  assert.equal(json.stderr, "");

  const jsonOut = normalizeOutput(cwd, json.stdout);
  assert.equal(jsonOut, readFixture("build-failure.json.stdout.txt"));
});
