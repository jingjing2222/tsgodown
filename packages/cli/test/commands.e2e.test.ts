import assert from "node:assert/strict";
import { spawnSync } from "node:child_process";
import crypto from "node:crypto";
import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import { after, test } from "node:test";

const tempDirs: string[] = [];
const fixturesDir = path.join(import.meta.dirname, "fixtures");
const cliEntry = path.resolve(import.meta.dirname, "..", "dist", "index.js");

after(() => {
  for (const dir of tempDirs) {
    fs.rmSync(dir, { recursive: true, force: true });
  }
});

function setupProject(
  sourceLines: string[],
  config = 'export default { entry: "src/index.ts", outDir: "dist-go" };\n',
) {
  const dir = fs.mkdtempSync(path.join(os.tmpdir(), "tsgodown-cli-e2e-"));
  tempDirs.push(dir);

  fs.mkdirSync(path.join(dir, "src"), { recursive: true });
  fs.writeFileSync(
    path.join(dir, "src", "index.ts"),
    `${sourceLines.join("\n")}\n`,
  );
  fs.writeFileSync(path.join(dir, "tsgodown.config.ts"), config);

  fs.mkdirSync(path.join(dir, "dist"), { recursive: true });
  fs.writeFileSync(path.join(dir, "dist", "index.js"), "export {};\n");

  return dir;
}

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

function runCli(
  cwd: string,
  command: "build" | "check" | "report" | "stages",
  env?: NodeJS.ProcessEnv,
) {
  const result = spawnSync(process.execPath, [cliEntry, command, "--json"], {
    cwd,
    encoding: "utf8",
    env: {
      ...process.env,
      ...env,
    },
  });
  return result;
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

function parseJsonStdout(stdout: string) {
  const jsonStart = stdout.indexOf("{");
  assert.ok(jsonStart >= 0, "expected JSON output");
  return JSON.parse(stdout.slice(jsonStart)) as Record<string, unknown>;
}

function normalizeResult(cwd: string, value: unknown): unknown {
  const roots = [cwd];
  try {
    roots.push(fs.realpathSync(cwd));
  } catch {
    // noop
  }

  function walk(current: unknown): unknown {
    if (Array.isArray(current)) {
      return current.map((entry) => walk(entry));
    }
    if (current && typeof current === "object") {
      const rec = current as Record<string, unknown>;
      const out: Record<string, unknown> = {};
      for (const [key, raw] of Object.entries(rec)) {
        if (typeof raw === "string") {
          const root = roots.find(
            (candidate) =>
              raw === candidate || raw.startsWith(`${candidate}${path.sep}`),
          );
          if (root) {
            out[key] = raw === root ? "." : raw.slice(root.length + 1);
            continue;
          }
        }
        out[key] = walk(raw);
      }
      return out;
    }
    return current;
  }

  return walk(value);
}

function assertSubset(actual: unknown, expected: unknown, scope = "root") {
  if (Array.isArray(expected)) {
    assert.ok(Array.isArray(actual), `${scope} should be an array`);
    assert.equal(actual.length, expected.length, `${scope} length mismatch`);
    for (let i = 0; i < expected.length; i++) {
      assertSubset(actual[i], expected[i], `${scope}[${i}]`);
    }
    return;
  }

  if (expected && typeof expected === "object") {
    assert.ok(
      actual && typeof actual === "object",
      `${scope} should be an object`,
    );
    const actualRec = actual as Record<string, unknown>;
    const expectedRec = expected as Record<string, unknown>;
    for (const [key, value] of Object.entries(expectedRec)) {
      assert.ok(key in actualRec, `${scope}.${key} should exist`);
      assertSubset(actualRec[key], value, `${scope}.${key}`);
    }
    return;
  }

  assert.equal(actual, expected, `${scope} mismatch`);
}

test("CLI JSON contract fixtures: success path", () => {
  const cwd = setupProject([
    "const fastify = {} as any;",
    "const listUsers = () => {};",
    "const getUser = () => {};",
    "fastify.get('/users', listUsers);",
    "fastify.get('/users/:id', getUser);",
  ]);

  const fixture = JSON.parse(
    fs.readFileSync(path.join(fixturesDir, "contract-success.json"), "utf8"),
  ) as Record<string, unknown>;

  const rustLauncher = createRustEngineLauncher(cwd, [
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
  ]);

  for (const command of ["build", "check", "report", "stages"] as const) {
    const result = runCli(cwd, command, {
      ...process.env,
      TSGODOWN_RUST_ENGINE_BIN: rustLauncher,
    });
    assert.equal(result.status, 0, `${command} failed: ${result.stderr}`);

    const parsed = parseJsonStdout(result.stdout);
    const normalized = normalizeResult(cwd, parsed);
    assertSubset(normalized, fixture[command], command);
  }

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
  assert.equal(fs.existsSync(goPath), false);
});

test("CLI JSON contract fixtures: warn path", () => {
  const cwd = setupProject([
    "const fastify = {} as any;",
    "const health = () => {};",
    "fastify.get('/health', health);",
    "await import('node:fs');",
  ]);

  const fixture = JSON.parse(
    fs.readFileSync(path.join(fixturesDir, "contract-warn.json"), "utf8"),
  ) as Record<string, unknown>;

  const rustLauncher = createRustEngineLauncher(cwd, [
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
  ]);

  for (const command of ["check", "report"] as const) {
    const result = runCli(cwd, command, {
      ...process.env,
      TSGODOWN_RUST_ENGINE_BIN: rustLauncher,
    });
    assert.equal(result.status, 0, `${command} failed: ${result.stderr}`);
    const parsed = parseJsonStdout(result.stdout);
    const normalized = normalizeResult(cwd, parsed);
    assertSubset(normalized, fixture[command], command);
  }
});

test("CLI fail diagnostics include source/cause/guidance contract", () => {
  const cwd = setupProject(
    ["const fastify = {} as any;", "fastify.get('/health', () => {});"],
    'export default { entry: "src/missing.ts", outDir: "dist-go" };\n',
  );

  const fixture = JSON.parse(
    fs.readFileSync(path.join(fixturesDir, "contract-fail.json"), "utf8"),
  ) as { stderrIncludes: string[] };

  const rustLauncher = createRustEngineLauncher(cwd, [
    "for await (const _ of process.stdin) { /* drain */ }",
    "const response = {",
    "  ok: false,",
    "  error: {",
    "    source: 'rust-engine-adapter',",
    "    cause: 'ENOENT: missing entry src/missing.ts',",
    "    guidance: 'Verify tsgodown.config.ts entry path and file existence.'",
    "  }",
    "};",
    "process.stdout.write(JSON.stringify(response));",
  ]);

  const result = runCli(cwd, "build", {
    ...process.env,
    TSGODOWN_RUST_ENGINE_BIN: rustLauncher,
  });
  assert.notEqual(result.status, 0, "build should fail for missing entry");

  for (const token of fixture.stderrIncludes) {
    assert.match(
      result.stderr,
      new RegExp(token.replace(/[.*+?^${}()|[\]\\]/g, "\\$&")),
    );
  }
});

test("CLI fails with explicit diagnostic when rust engine bin env is missing", () => {
  const cwd = setupProject([
    "const fastify = {} as any;",
    "const health = () => {};",
    "fastify.get('/health', health);",
  ]);

  const env = { ...process.env };
  Reflect.deleteProperty(env, "TSGODOWN_RUST_ENGINE_BIN");

  const result = runCli(cwd, "build", env);
  assert.notEqual(result.status, 0);
  assert.match(result.stderr, /source=rust-engine-bin-env/);
  assert.match(result.stderr, /cause=TSGODOWN_RUST_ENGINE_BIN is not set/);
  assert.match(
    result.stderr,
    /guidance=Set TSGODOWN_RUST_ENGINE_BIN to the Rust engine executable path\./,
  );
});

test("CLI fails with explicit diagnostic when rust engine binary cannot spawn", () => {
  const cwd = setupProject([
    "const app = { get: (_path: string, _handler: () => unknown) => undefined };",
    "app.get('/health', () => ({ ok: true }));",
  ]);

  const missingBinary = path.join(
    cwd,
    `missing-rust-bin-${crypto.randomUUID()}`,
  );
  const result = runCli(cwd, "build", {
    ...process.env,
    TSGODOWN_RUST_ENGINE_BIN: missingBinary,
  });

  assert.notEqual(result.status, 0);
  assert.match(result.stderr, /source=rust-engine-binary-spawn/);
  assert.match(result.stderr, /cause=Error: spawn .* ENOENT/);
  assert.match(
    result.stderr,
    /guidance=Check TSGODOWN_RUST_ENGINE_BIN points to an executable binary\./,
  );
});

test("CLI fails with explicit diagnostic when rust engine exits non-zero", () => {
  const cwd = setupProject([
    "const app = { get: (_path: string, _handler: () => unknown) => undefined };",
    "app.get('/health', () => ({ ok: true }));",
  ]);

  const rustLauncher = createRustEngineLauncher(cwd, [
    "for await (const _ of process.stdin) { /* drain */ }",
    "process.stderr.write('fatal: fixture forced non-zero exit\\n');",
    "process.exit(17);",
  ]);

  const result = runCli(cwd, "build", {
    ...process.env,
    TSGODOWN_RUST_ENGINE_BIN: rustLauncher,
  });

  assert.notEqual(result.status, 0);
  assert.match(result.stderr, /source=rust-engine-binary/);
  assert.match(
    result.stderr,
    /cause=exit=17 stderr=fatal: fixture forced non-zero exit/,
  );
  assert.match(
    result.stderr,
    /guidance=Inspect rust engine logs and JSON response contract\./,
  );
});

test("CLI build routes through rust adapter binary with JSON contract", () => {
  const cwd = setupProject([
    "const fastify = {} as any;",
    "const health = () => {};",
    "fastify.get('/health', health);",
  ]);

  const capturePath = path.join(cwd, `request-${crypto.randomUUID()}.json`);
  const stubPath = path.join(cwd, `rust-stub-${crypto.randomUUID()}.mjs`);

  fs.writeFileSync(
    stubPath,
    [
      "import fs from 'node:fs';",
      "const chunks = [];",
      "for await (const chunk of process.stdin) chunks.push(chunk);",
      "const request = JSON.parse(Buffer.concat(chunks).toString('utf8'));",
      `fs.writeFileSync(${JSON.stringify(capturePath)}, JSON.stringify(request, null, 2));`,
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

  const result = runCli(cwd, "build", {
    ...process.env,
    TSGODOWN_RUST_ENGINE_BIN: launcherPath,
  });

  assert.equal(result.status, 0, `build failed: ${result.stderr}`);
  const captured = JSON.parse(fs.readFileSync(capturePath, "utf8")) as {
    action: string;
    cwd: string;
  };
  assert.equal(captured.action, "build");
  const realCwd = fs.realpathSync(cwd);
  assert.ok(captured.cwd === cwd || captured.cwd === realCwd);
});

test("CLI surfaces rust adapter error propagation format", () => {
  const cwd = setupProject([
    "const fastify = {} as any;",
    "const health = () => {};",
    "fastify.get('/health', health);",
  ]);

  const stubPath = path.join(cwd, `rust-stub-error-${crypto.randomUUID()}.mjs`);

  fs.writeFileSync(
    stubPath,
    [
      "for await (const _ of process.stdin) { /* drain */ }",
      "const response = {",
      "  ok: false,",
      "  error: {",
      "    source: 'rust-engine-adapter',",
      "    cause: 'invalid build graph',",
      "    guidance: 'Check Rust engine JSON contract and retry.'",
      "  }",
      "};",
      "process.stdout.write(JSON.stringify(response));",
    ].join("\n"),
  );

  const launcherPath = path.join(
    cwd,
    `rust-launcher-error-${crypto.randomUUID()}.sh`,
  );
  fs.writeFileSync(
    launcherPath,
    `#!/usr/bin/env bash\nexec ${JSON.stringify(process.execPath)} ${JSON.stringify(stubPath)}\n`,
  );
  fs.chmodSync(launcherPath, 0o755);

  const result = runCli(cwd, "build", {
    ...process.env,
    TSGODOWN_RUST_ENGINE_BIN: launcherPath,
  });

  assert.notEqual(result.status, 0);
  assert.match(result.stderr, /source=rust-engine-adapter/);
  assert.match(result.stderr, /cause=invalid build graph/);
  assert.match(
    result.stderr,
    /guidance=Check Rust engine JSON contract and retry\./,
  );
});
