import assert from "node:assert/strict";
import crypto from "node:crypto";
import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import { after, test } from "node:test";

import { runPipeline } from "../src/index.ts";

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

test("runPipeline delegates build/analysis/capability/emission to rust engine adapter", async () => {
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

    assert.equal(logs.length, 4);
    assert.match(logs[0], /\[BUILD_ARTIFACTS\]/);
    assert.match(logs[1], /\[BUILD_IR\].*delegated to rust engine/i);
    assert.match(logs[2], /\[CAPABILITY_GATE\].*delegated to rust engine/i);
    assert.match(logs[3], /\[EMIT_GO\].*delegated to rust engine/i);

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

    const goPath = path.join(cwd, "dist-go", "main.go");
    assert.equal(
      fs.existsSync(goPath),
      false,
      "legacy TS core emission should be disabled after rust cutover",
    );
  } finally {
    if (prevRustBin === undefined) {
      Reflect.deleteProperty(process.env, "TSGODOWN_RUST_ENGINE_BIN");
    } else {
      process.env.TSGODOWN_RUST_ENGINE_BIN = prevRustBin;
    }
  }
});
