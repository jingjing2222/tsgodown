import assert from "node:assert/strict";
import { spawn, spawnSync } from "node:child_process";
import crypto from "node:crypto";
import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import { after, test } from "node:test";

const tempDirs: string[] = [];
const fixturesDir = path.join(import.meta.dirname, "fixtures");
const cliEntry = path.resolve(import.meta.dirname, "..", "dist", "index.js");
const repoRoot = path.resolve(import.meta.dirname, "..", "..", "..");

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

function copyRecursive(from: string, to: string) {
  const stat = fs.statSync(from);
  if (stat.isDirectory()) {
    fs.mkdirSync(to, { recursive: true });
    for (const entry of fs.readdirSync(from)) {
      copyRecursive(path.join(from, entry), path.join(to, entry));
    }
    return;
  }
  fs.mkdirSync(path.dirname(to), { recursive: true });
  fs.copyFileSync(from, to);
}

function setupProjectFromFixture(name: string) {
  const sourceRoot = path.join(fixturesDir, "projects", name);
  const dir = fs.mkdtempSync(path.join(os.tmpdir(), "tsgodown-cli-e2e-"));
  tempDirs.push(dir);

  copyRecursive(sourceRoot, dir);
  fs.mkdirSync(path.join(dir, "dist"), { recursive: true });
  fs.writeFileSync(path.join(dir, "dist", "index.js"), "export {};\n");
  return dir;
}

function setupProjectFromExample(name: string) {
  const sourceRoot = path.join(repoRoot, "examples", name);
  const dir = fs.mkdtempSync(path.join(os.tmpdir(), "tsgodown-cli-e2e-"));
  tempDirs.push(dir);

  copyRecursive(sourceRoot, dir);
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

let cachedEngineCoreBin: string | undefined;

function resolveEngineCoreBin(): string {
  if (cachedEngineCoreBin && fs.existsSync(cachedEngineCoreBin)) {
    return cachedEngineCoreBin;
  }

  const candidate = path.join(repoRoot, "target", "debug", "engine-core");
  if (!fs.existsSync(candidate)) {
    const build = spawnSync("cargo", ["build", "-p", "engine-core"], {
      cwd: repoRoot,
      encoding: "utf8",
    });
    assert.equal(build.status, 0, build.stderr || build.stdout);
  }

  assert.equal(
    fs.existsSync(candidate),
    true,
    "engine-core binary should exist",
  );
  cachedEngineCoreBin = candidate;
  return candidate;
}

function resolveRustEngineLauncherScript(): string {
  const launcher = path.join(repoRoot, "scripts", "rust-engine-launcher.sh");
  assert.equal(
    fs.existsSync(launcher),
    true,
    "rust launcher script should exist",
  );
  fs.chmodSync(launcher, 0o755);
  return launcher;
}

function assertGoMainScaffold(goSource: string) {
  assert.match(goSource, /^package main/m);
  assert.match(goSource, /func main\(\)/);
  assert.match(goSource, /HandleFunc\("GET \/health"/);
}

function assertGoBuildSuccessIfToolchainAvailable(goDir: string) {
  const hasGoToolchain =
    spawnSync("go", ["version"], { encoding: "utf8" }).status === 0;

  if (!hasGoToolchain) {
    return;
  }

  const modInit = spawnSync(
    "go",
    ["mod", "init", "example.com/tsgodown-cli-fastify-min"],
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

const hasGoToolchain =
  spawnSync("go", ["version"], { encoding: "utf8" }).status === 0;
const initializedGoRuntimeDirs = new Set<string>();

function ensureGoRuntimeModule(goDir: string) {
  if (!hasGoToolchain || initializedGoRuntimeDirs.has(goDir)) {
    return;
  }

  const modInit = spawnSync(
    "go",
    ["mod", "init", `example.com/tsgodown-cli-runtime-${crypto.randomUUID()}`],
    {
      cwd: goDir,
      encoding: "utf8",
    },
  );
  assert.equal(modInit.status, 0, modInit.stderr || modInit.stdout);
  initializedGoRuntimeDirs.add(goDir);
}

async function assertGoRunRequest(
  goDir: string,
  request: {
    method?: string;
    routePath: string;
    expectedStatus: number;
    expectedBodyFragment?: string;
  },
) {
  if (!hasGoToolchain) {
    return;
  }

  ensureGoRuntimeModule(goDir);

  const port = String(20000 + Math.floor(Math.random() * 30000));
  const child = spawn("go", ["run", "."], {
    cwd: goDir,
    env: {
      ...process.env,
      PORT: port,
    },
    stdio: "ignore",
  });

  const deadline = Date.now() + 10_000;
  let status = 0;
  let body = "";
  let lastError: unknown;

  try {
    while (Date.now() < deadline) {
      try {
        const response = await fetch(
          `http://127.0.0.1:${port}${request.routePath}`,
          {
            method: request.method ?? "GET",
            signal: AbortSignal.timeout(1000),
          },
        );
        status = response.status;
        body = await response.text();
        break;
      } catch (error) {
        lastError = error;
        await new Promise((resolve) => setTimeout(resolve, 200));
      }
    }

    assert.equal(
      status,
      request.expectedStatus,
      `server did not become ready for ${request.method ?? "GET"} ${request.routePath}; lastError=${String(lastError)}`,
    );
    if (request.expectedBodyFragment) {
      assert.match(body, new RegExp(request.expectedBodyFragment));
    }
  } finally {
    if (child.exitCode === null && !child.killed) {
      child.kill("SIGTERM");
    }

    await new Promise<void>((resolve) => {
      if (child.exitCode !== null) {
        resolve();
        return;
      }

      const forceKillTimer = setTimeout(() => {
        if (child.exitCode === null) {
          child.kill("SIGKILL");
        }
      }, 1500);

      const settleTimer = setTimeout(() => {
        clearTimeout(forceKillTimer);
        resolve();
      }, 5000);

      child.once("close", () => {
        clearTimeout(forceKillTimer);
        clearTimeout(settleTimer);
        resolve();
      });
    });
  }
}

async function assertGoRunRoute(
  goDir: string,
  routePath: string,
  expectedBodyFragment: string,
) {
  await assertGoRunRequest(goDir, {
    routePath,
    expectedStatus: 501,
    expectedBodyFragment,
  });
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

  const parsed = parseJsonStdout(result.stdout) as {
    error: Record<string, string>;
  };
  const haystack = JSON.stringify(parsed.error);
  for (const token of fixture.stderrIncludes) {
    assert.match(
      haystack,
      new RegExp(token.replace(/[.*+?^${}()|[\]\\]/g, "\\$&")),
    );
  }
});

test("CLI --json failure output keeps source/stage/cause/guidance consistent", () => {
  const cwd = setupProject(
    ["const fastify = {} as any;", "fastify.get('/health', () => {});"],
    'export default { entry: "src/missing.ts", outDir: "dist-go" };\n',
  );

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
  const parsed = parseJsonStdout(result.stdout) as {
    ok: boolean;
    error: {
      source?: string;
      stage?: string;
      cause?: string;
      guidance?: string;
      message: string;
    };
  };

  assert.equal(parsed.ok, false);
  assert.equal(parsed.error.source, "pipeline-entry(src/missing.ts)");
  assert.equal(parsed.error.stage, "BUILD_ARTIFACTS");
  assert.match(
    parsed.error.cause ?? "",
    /\[tsdown-driver\] rust engine failed/,
  );
  assert.equal(
    parsed.error.guidance,
    "Verify rust engine build/analyze contract and tsgodown.config.ts settings.",
  );
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
  const parsed = parseJsonStdout(result.stdout) as {
    error: { message: string };
  };
  assert.match(parsed.error.message, /source=rust-engine-bin-env/);
  assert.match(
    parsed.error.message,
    /cause=TSGODOWN_RUST_ENGINE_BIN is not set/,
  );
  assert.match(
    parsed.error.message,
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
  const parsed = parseJsonStdout(result.stdout) as {
    error: { message: string };
  };
  assert.match(parsed.error.message, /source=rust-engine-binary-spawn/);
  assert.match(parsed.error.message, /cause=Error: spawn .* ENOENT/);
  assert.match(
    parsed.error.message,
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
  const parsed = parseJsonStdout(result.stdout) as {
    error: { message: string };
  };
  assert.match(parsed.error.message, /source=rust-engine-binary/);
  assert.match(
    parsed.error.message,
    /cause=exit=17 stderr=fatal: fixture forced non-zero exit/,
  );
  assert.match(
    parsed.error.message,
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
  const parsed = parseJsonStdout(result.stdout) as {
    error: { message: string };
  };
  assert.match(parsed.error.message, /source=rust-engine-adapter/);
  assert.match(parsed.error.message, /cause=invalid build graph/);
  assert.match(
    parsed.error.message,
    /guidance=Check Rust engine JSON contract and retry\./,
  );
});

test("rust-only fixture matrix keeps build/check/report/stages deterministic", () => {
  const fixtures = [
    "multi-file",
    "nested-register-prefix",
    "route-object-variants",
  ] as const;

  for (const fixture of fixtures) {
    const cwd = setupProjectFromFixture(fixture);
    const capturePath = path.join(cwd, `request-${crypto.randomUUID()}.json`);
    const rustLauncher = createRustEngineLauncher(cwd, [
      "import fs from 'node:fs';",
      "const chunks = [];",
      "for await (const chunk of process.stdin) chunks.push(chunk);",
      "const request = JSON.parse(Buffer.concat(chunks).toString('utf8'));",
      `fs.appendFileSync(${JSON.stringify(capturePath)}, JSON.stringify(request) + '\\n');`,
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

    const buildResult = runCli(cwd, "build", {
      ...process.env,
      TSGODOWN_RUST_ENGINE_BIN: rustLauncher,
    });
    assert.equal(buildResult.status, 0, `build failed (${fixture})`);
    const buildJson = parseJsonStdout(buildResult.stdout);
    assert.equal(buildJson.command, "build");
    assert.equal(
      (buildJson.targets as Array<Record<string, unknown>>)[0]?.emitted,
      true,
      `build should emit manifest (${fixture})`,
    );

    fs.rmSync(path.join(cwd, "artifacts"), { recursive: true, force: true });
    const checkResult = runCli(cwd, "check", {
      ...process.env,
      TSGODOWN_RUST_ENGINE_BIN: rustLauncher,
    });
    assert.equal(checkResult.status, 0, `check failed (${fixture})`);
    const checkJson = parseJsonStdout(checkResult.stdout);
    assert.equal(checkJson.command, "check");

    fs.rmSync(path.join(cwd, "artifacts"), { recursive: true, force: true });
    const reportResult = runCli(cwd, "report", {
      ...process.env,
      TSGODOWN_RUST_ENGINE_BIN: rustLauncher,
    });
    assert.equal(reportResult.status, 0, `report failed (${fixture})`);
    const reportJson = parseJsonStdout(reportResult.stdout);
    assert.equal(reportJson.command, "report");

    fs.rmSync(path.join(cwd, "artifacts"), { recursive: true, force: true });
    const stagesResult = runCli(cwd, "stages", {
      ...process.env,
      TSGODOWN_RUST_ENGINE_BIN: rustLauncher,
    });
    assert.equal(stagesResult.status, 0, `stages failed (${fixture})`);
    const stagesJson = parseJsonStdout(stagesResult.stdout);
    assert.deepEqual(stagesJson.stages, [
      "load-config",
      "analyze",
      "emit",
      "onSuccess",
    ]);
    assert.equal(
      (stagesJson.targets as Array<Record<string, unknown>>)[0]?.emitted,
      false,
      `stages should not emit manifest (${fixture})`,
    );

    const requests = fs
      .readFileSync(capturePath, "utf8")
      .trim()
      .split("\n")
      .filter(Boolean)
      .map((line) => JSON.parse(line) as { action: string });
    assert.equal(
      requests.length,
      3,
      `expected build/check/report invokes (${fixture})`,
    );
    for (const req of requests) {
      assert.equal(req.action, "build");
    }
  }
});

test("M1 release gate: CLI build fastify-min fixture -> dist-go/main.go -> go build (if available)", () => {
  const cwd = setupProjectFromFixture("fastify-min");
  const rustLauncher = resolveRustEngineLauncherScript();
  const engineCoreBin = resolveEngineCoreBin();

  const result = runCli(cwd, "build", {
    ...process.env,
    TSGODOWN_RUST_ENGINE_BIN: rustLauncher,
    TSGODOWN_ENGINE_CORE_BIN: engineCoreBin,
  });

  assert.equal(result.status, 0, `build failed: ${result.stderr}`);

  const parsed = parseJsonStdout(result.stdout);
  assert.equal(parsed.command, "build");

  const goPath = path.join(cwd, "dist-go", "main.go");
  assert.equal(fs.existsSync(goPath), true);

  const goMain = fs.readFileSync(goPath, "utf8");
  assert.match(goMain, /mux\.HandleFunc\("GET \/health"/);
  assert.match(goMain, /mux\.HandleFunc\("GET \/users"/);
  assert.match(goMain, /TODO implement handler health for GET \/health/);
  assert.match(goMain, /TODO implement handler users for GET \/users/);
  assertGoMainScaffold(goMain);
  assertGoBuildSuccessIfToolchainAvailable(path.dirname(goPath));
});

test("M2 acceptance: TS fixture routes are reachable in generated Go runtime", async () => {
  const cwd = setupProjectFromFixture("fastify-min");
  const rustLauncher = resolveRustEngineLauncherScript();
  const engineCoreBin = resolveEngineCoreBin();

  const result = runCli(cwd, "build", {
    ...process.env,
    TSGODOWN_RUST_ENGINE_BIN: rustLauncher,
    TSGODOWN_ENGINE_CORE_BIN: engineCoreBin,
  });

  assert.equal(result.status, 0, `build failed: ${result.stderr}`);

  const goPath = path.join(cwd, "dist-go", "main.go");
  assert.equal(fs.existsSync(goPath), true);

  const goDir = path.dirname(goPath);
  await assertGoRunRoute(
    goDir,
    "/health",
    "TODO implement handler health for GET /health",
  );
  await assertGoRunRoute(
    goDir,
    "/users",
    "TODO implement handler users for GET /users",
  );
});

test("M2 acceptance: fastify-complex fixture preserves method contracts and path params", async () => {
  const cwd = setupProjectFromExample("fastify-complex");
  const rustLauncher = resolveRustEngineLauncherScript();
  const engineCoreBin = resolveEngineCoreBin();

  const result = runCli(cwd, "build", {
    ...process.env,
    TSGODOWN_RUST_ENGINE_BIN: rustLauncher,
    TSGODOWN_ENGINE_CORE_BIN: engineCoreBin,
  });

  assert.equal(result.status, 0, `build failed: ${result.stderr}`);

  const goPath = path.join(cwd, "dist-go", "main.go");
  assert.equal(fs.existsSync(goPath), true);

  const goMain = fs.readFileSync(goPath, "utf8");
  assert.match(goMain, /HandleFunc\("GET \/health"/);
  assert.match(goMain, /HandleFunc\("POST \/users"/);
  assert.match(goMain, /HandleFunc\("PATCH \/users\/{id}"/);
  assert.match(goMain, /HandleFunc\("DELETE \/users\/{id}"/);
  assert.match(goMain, /id := req\.PathValue\("id"\)/);

  const goDir = path.dirname(goPath);
  await assertGoRunRequest(goDir, {
    method: "GET",
    routePath: "/health",
    expectedStatus: 501,
    expectedBodyFragment: "TODO implement handler health for GET /health",
  });
  await assertGoRunRequest(goDir, {
    method: "POST",
    routePath: "/users",
    expectedStatus: 501,
    expectedBodyFragment: "TODO implement handler createUser for POST /users",
  });
  await assertGoRunRequest(goDir, {
    method: "PATCH",
    routePath: "/users/abc-123",
    expectedStatus: 501,
    expectedBodyFragment:
      "TODO implement handler updateUser for PATCH /users/:id",
  });
  await assertGoRunRequest(goDir, {
    method: "DELETE",
    routePath: "/users/abc-123",
    expectedStatus: 501,
    expectedBodyFragment:
      "TODO implement handler removeUser for DELETE /users/:id",
  });
  await assertGoRunRequest(goDir, {
    method: "GET",
    routePath: "/users/abc-123",
    expectedStatus: 405,
    expectedBodyFragment: "Method Not Allowed",
  });
});

test("rust-only fixture matrix surfaces deterministic contract error path", () => {
  const cwd = setupProjectFromFixture("error-missing-entry");
  const rustLauncher = createRustEngineLauncher(cwd, [
    "for await (const _ of process.stdin) { /* drain */ }",
    "process.stdout.write(JSON.stringify({",
    "  ok: false,",
    "  error: {",
    "    source: 'rust-engine-adapter',",
    "    cause: 'ENOENT: missing entry src/missing.ts',",
    "    guidance: 'Verify tsgodown.config.ts entry path and file existence.'",
    "  }",
    "}));",
  ]);

  for (const command of ["build", "check", "report"] as const) {
    const result = runCli(cwd, command, {
      ...process.env,
      TSGODOWN_RUST_ENGINE_BIN: rustLauncher,
    });
    assert.notEqual(result.status, 0, `${command} should fail`);
    const parsed = parseJsonStdout(result.stdout) as {
      error: { message: string };
    };
    assert.match(parsed.error.message, /source=rust-engine-adapter/);
    assert.match(
      parsed.error.message,
      /cause=ENOENT: missing entry src\/missing\.ts/,
    );
    assert.match(
      parsed.error.message,
      /guidance=Verify tsgodown\.config\.ts entry path and file existence\./,
    );
  }
});
