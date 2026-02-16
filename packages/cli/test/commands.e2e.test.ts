import assert from "node:assert/strict";
import { spawnSync } from "node:child_process";
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

function runCli(cwd: string, command: "build" | "check" | "report" | "stages") {
  const result = spawnSync(process.execPath, [cliEntry, command, "--json"], {
    cwd,
    encoding: "utf8",
  });
  return result;
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

  for (const command of ["build", "check", "report", "stages"] as const) {
    const result = runCli(cwd, command);
    assert.equal(result.status, 0, `${command} failed: ${result.stderr}`);

    const parsed = parseJsonStdout(result.stdout);
    const normalized = normalizeResult(cwd, parsed);
    assertSubset(normalized, fixture[command], command);
  }

  const goPath = path.join(cwd, "dist-go", "main.go");
  assert.equal(fs.existsSync(goPath), true);
  const go = fs.readFileSync(goPath, "utf8");
  assert.ok(go.includes('mux.HandleFunc("/users", route0)'));
  assert.ok(go.includes('mux.HandleFunc("/users/:id", route1)'));
  assert.ok(go.includes("if req.Method != http.MethodGet {"));
  assert.ok(go.includes("// Route metadata:"));
  assert.ok(go.includes("//   Method: GET"));
  assert.ok(go.includes('//   Path: "/users"'));
  assert.ok(go.includes('//   Handler: "listUsers"'));
  assert.ok(
    go.includes(
      'fmt.Fprintln(w, "TODO implement handler listUsers for GET /users")',
    ),
  );
  assert.ok(
    go.includes(
      'fmt.Fprintln(w, "TODO implement handler getUser for GET /users/:id")',
    ),
  );
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

  for (const command of ["check", "report"] as const) {
    const result = runCli(cwd, command);
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

  const result = runCli(cwd, "build");
  assert.notEqual(result.status, 0, "build should fail for missing entry");

  for (const token of fixture.stderrIncludes) {
    assert.match(
      result.stderr,
      new RegExp(token.replace(/[.*+?^${}()|[\]\\]/g, "\\$&")),
    );
  }
});
