import assert from "node:assert/strict";
import { spawn, spawnSync } from "node:child_process";
import fs from "node:fs";
import net from "node:net";
import os from "node:os";
import path from "node:path";
import { after, test } from "node:test";
import type { ProgramIR } from "@tsgodown/ir-core";

import { emitGoProject } from "../src/index.ts";

const tempDirs: string[] = [];

after(() => {
  for (const dir of tempDirs) {
    fs.rmSync(dir, { recursive: true, force: true });
  }
});

function createOutDir() {
  const dir = fs.mkdtempSync(
    path.join(os.tmpdir(), "tsgodown-emitter-go-test-"),
  );
  tempDirs.push(dir);
  return dir;
}

async function allocateEphemeralPort(): Promise<string> {
  return await new Promise((resolve, reject) => {
    const server = net.createServer();
    server.unref();
    server.on("error", reject);
    server.listen(0, "127.0.0.1", () => {
      const address = server.address();
      if (!address || typeof address === "string") {
        server.close(() => reject(new Error("failed to allocate test port")));
        return;
      }
      const port = String(address.port);
      server.close((error) => {
        if (error) {
          reject(error);
          return;
        }
        resolve(port);
      });
    });
  });
}

const fixturesDir = path.join(import.meta.dirname, "fixtures");

function readFixture(name: string): ProgramIR {
  return JSON.parse(
    fs.readFileSync(path.join(fixturesDir, `${name}.fixture.json`), "utf8"),
  ) as ProgramIR;
}

function readGolden(name: string): string {
  return fs.readFileSync(path.join(fixturesDir, `${name}.golden.go`), "utf8");
}

const representativeFixtureNames = [
  "sample-ir",
  "nested-restish-route",
  "empty-method-fallback-and-diagnostics",
  "go-unsafe-path-params",
] as const;

const sampleIr = readFixture("sample-ir");

test("emitGoProject emits deterministic main.go scaffold with method-aware route registration and actionable TODO stubs", () => {
  const outDir = createOutDir();

  emitGoProject(sampleIr, outDir);

  const emitted = fs.readFileSync(path.join(outDir, "main.go"), "utf8");

  assert.match(emitted, /func registerRoutes\(router \*runtimeRouter\) \{/);
  assert.match(emitted, /router\.handle\("GET", "\/health", route0\)/);
  assert.match(emitted, /router\.handle\("POST", "\/users\/\{id\}", route1\)/);
  assert.match(emitted, /type runtimeRouter struct \{/);
  assert.match(
    emitted,
    /func \(r \*runtimeRouter\) ServeHTTP\(w http\.ResponseWriter, req \*http\.Request\) \{/,
  );
  assert.match(
    emitted,
    /w\.Header\(\)\.Set\("Allow", strings\.Join\(allow, ", "\)\)/,
  );
  assert.doesNotMatch(emitted, /if req\.Method != http\.MethodGet \{/);
  assert.doesNotMatch(emitted, /if req\.Method != http\.MethodPost \{/);
  assert.match(emitted, /\/\/ Route metadata:/);
  assert.match(emitted, /\/\/\s+Method:\s+GET/);
  assert.match(emitted, /\/\/\s+Path:\s+"\/health"/);
  assert.match(emitted, /\/\/\s+Handler:\s+"health"/);
  assert.match(emitted, /\/\/\s+Handler params:\s+request:req, response:reply/);
  assert.match(emitted, /\/\/\s+Handler async:\s+false/);
  assert.match(emitted, /\/\/\s+Handler response mode:\s+response-object/);
  assert.match(emitted, /\/\/\s+Middleware:\s+\["auth"\]/);
  assert.match(emitted, /\/\/\s+Method:\s+POST/);
  assert.match(emitted, /\/\/\s+Path:\s+"\/users\/:id"/);
  assert.match(emitted, /\/\/\s+Handler:\s+"createUser"/);
  assert.match(emitted, /\/\/\s+Handler params:\s+request:req/);
  assert.match(emitted, /\/\/\s+Handler async:\s+true/);
  assert.match(emitted, /\/\/\s+Handler response mode:\s+return/);
  assert.match(emitted, /id := req\.PathValue\("id"\)/);
  assert.match(
    emitted,
    /TODO\(tsgodown\): Implement handler "health" for GET \/health\./,
  );
  assert.match(
    emitted,
    /TODO\(tsgodown\): Implement handler "createUser" for POST \/users\/:id\./,
  );
  assert.match(emitted, /w\.WriteHeader\(http\.StatusNotImplemented\)/);
  assert.match(
    emitted,
    /fmt\.Fprintln\(w, "TODO implement handler health for GET \/health"\)/,
  );
  assert.match(
    emitted,
    /fmt\.Fprintln\(w, "TODO implement handler createUser for POST \/users\/:id"\)/,
  );
});

test("emitGoProject output is byte-for-byte stable across repeated runs", () => {
  const firstOutDir = createOutDir();
  const secondOutDir = createOutDir();

  emitGoProject(sampleIr, firstOutDir);
  emitGoProject(sampleIr, secondOutDir);

  const first = fs.readFileSync(path.join(firstOutDir, "main.go"), "utf8");
  const second = fs.readFileSync(path.join(secondOutDir, "main.go"), "utf8");

  assert.equal(first, second);
});

test("emitGoProject representative fixtures stay locked to checked-in golden outputs", () => {
  for (const fixtureName of representativeFixtureNames) {
    const outDir = createOutDir();
    emitGoProject(readFixture(fixtureName), outDir);

    const emitted = fs.readFileSync(path.join(outDir, "main.go"), "utf8");
    const golden = readGolden(fixtureName);

    assert.equal(
      emitted,
      golden,
      `fixture ${fixtureName} drifted from golden output; if intentional, refresh ${fixtureName}.golden.go`,
    );
  }
});

test("emitGoProject normalizes route methods and extracts both colon and braces path params", () => {
  const outDir = createOutDir();

  emitGoProject(
    {
      modules: [],
      handlers: [{ id: "showOrder", params: [], async: false }],
      diagnostics: [],
      routes: [
        {
          method: "get" as ProgramIR["routes"][number]["method"],
          path: "users/:userId/orders/{orderId}",
          handlerRef: "showOrder",
        },
      ],
    },
    outDir,
  );

  const emitted = fs.readFileSync(path.join(outDir, "main.go"), "utf8");

  assert.match(
    emitted,
    /router\.handle\("GET", "\/users\/\{userId\}\/orders\/\{orderId\}", route0\)/,
  );
  assert.match(emitted, /userId := req\.PathValue\("userId"\)/);
  assert.match(emitted, /orderId := req\.PathValue\("orderId"\)/);
});

test("emitGoProject falls back to GET for empty route methods to keep scaffold boot-stable", () => {
  const outDir = createOutDir();

  emitGoProject(
    {
      modules: [],
      handlers: [{ id: "fallbackMethod", params: [], async: false }],
      diagnostics: [],
      routes: [
        {
          method: "   " as ProgramIR["routes"][number]["method"],
          path: "/health",
          handlerRef: "fallbackMethod",
        },
      ],
    },
    outDir,
  );

  const emitted = fs.readFileSync(path.join(outDir, "main.go"), "utf8");

  assert.match(emitted, /router\.handle\("GET", "\/health", route0\)/);
  assert.match(
    emitted,
    /TODO\(tsgodown\): Implement handler "fallbackMethod" for GET \/health\./,
  );
});

test("emitGoProject avoids Go-unsafe path param bindings while preserving PathValue lookups", () => {
  const outDir = createOutDir();

  emitGoProject(
    {
      modules: [],
      handlers: [{ id: "keywordParams", params: [], async: false }],
      diagnostics: [],
      routes: [
        {
          method: "GET",
          path: "/things/:type/:req/:w/:pathParamType",
          handlerRef: "keywordParams",
        },
      ],
    },
    outDir,
  );

  const emitted = fs.readFileSync(path.join(outDir, "main.go"), "utf8");

  assert.match(emitted, /pathParamType := req\.PathValue\("type"\)/);
  assert.match(emitted, /pathParamReq := req\.PathValue\("req"\)/);
  assert.match(emitted, /pathParamW := req\.PathValue\("w"\)/);
  assert.match(emitted, /pathParamType2 := req\.PathValue\("pathParamType"\)/);
  assert.doesNotMatch(emitted, /\stype := req\.PathValue\("type"\)/);
  assert.doesNotMatch(emitted, /\sreq := req\.PathValue\("req"\)/);
  assert.doesNotMatch(emitted, /\sw := req\.PathValue\("w"\)/);
});

test("emitGoProject surfaces IR diagnostics as actionable comments without adding adapter policy", () => {
  const outDir = createOutDir();

  emitGoProject(
    {
      modules: [],
      handlers: [{ id: "h", params: [], async: false }],
      diagnostics: [
        {
          level: "warn",
          code: "UNSUPPORTED_DYNAMIC_PATH",
          message:
            "unsupported dynamic path in fastify.get(...). Use string literal path (e.g. '/users/:id') for IR extraction.",
          source: { file: "src/server.ts", line: 12, column: 3 },
        },
      ],
      routes: [{ method: "GET", path: "/health", handlerRef: "h" }],
    },
    outDir,
  );

  const emitted = fs.readFileSync(path.join(outDir, "main.go"), "utf8");

  assert.match(
    emitted,
    /\/\/ IR diagnostics carried from rust analyzer \(SSoT\):/,
  );
  assert.match(
    emitted,
    /\/\/\s+\[warn\] UNSUPPORTED_DYNAMIC_PATH: unsupported dynamic path in fastify\.get\(\.\.\.\)\. Use string literal path \(e\.g\. '\/users\/:id'\) for IR extraction\./,
  );
  assert.match(emitted, /\/\/\s+at src\/server\.ts:12:3/);
  assert.match(
    emitted,
    /\/\/\s+Action: fix diagnostics in source and regenerate\. Emitter does not own policy decisions\./,
  );
});

const hasGoToolchain =
  spawnSync("go", ["version"], { encoding: "utf8" }).status === 0;
const runGoSmoke = hasGoToolchain && process.env.CI !== "true";

function assertBuildsWithGoToolchain(fixture: ProgramIR, fixtureName: string) {
  const outDir = createOutDir();
  emitGoProject(fixture, outDir);

  const modulePath = `example.com/tsgodown-smoke/${fixtureName}`;
  const modInit = spawnSync("go", ["mod", "init", modulePath], {
    cwd: outDir,
    encoding: "utf8",
  });
  assert.equal(
    modInit.status,
    0,
    `go mod init failed for fixture ${fixtureName} in ${outDir}\nstdout:\n${modInit.stdout}\nstderr:\n${modInit.stderr}`,
  );

  const result = spawnSync("go", ["build", "./..."], {
    cwd: outDir,
    encoding: "utf8",
  });
  assert.equal(
    result.status,
    0,
    `go build failed for fixture ${fixtureName} in ${outDir}\nstdout:\n${result.stdout}\nstderr:\n${result.stderr}`,
  );
}

test(
  "emitGoProject smoke: generated representative fixtures can go build",
  { skip: !runGoSmoke },
  () => {
    for (const fixtureName of representativeFixtureNames) {
      assertBuildsWithGoToolchain(readFixture(fixtureName), fixtureName);
    }
  },
);

async function allocatePort(): Promise<number> {
  return await new Promise((resolve, reject) => {
    const server = net.createServer();
    server.listen(0, "127.0.0.1", () => {
      const address = server.address();
      if (!address || typeof address === "string") {
        server.close(() => reject(new Error("failed to allocate test port")));
        return;
      }
      server.close((error) => {
        if (error) {
          reject(error);
          return;
        }
        resolve(address.port);
      });
    });
    server.once("error", reject);
  });
}

function shutdownServer(server: ReturnType<typeof spawn>) {
  if (server.exitCode === null && !server.killed) {
    server.kill("SIGTERM");
  }

  return new Promise<void>((resolve) => {
    if (server.exitCode !== null) {
      resolve();
      return;
    }

    const forceKillTimer = setTimeout(() => {
      if (server.exitCode === null) {
        server.kill("SIGKILL");
      }
    }, 1500);

    const settleTimer = setTimeout(() => {
      clearTimeout(forceKillTimer);
      resolve();
    }, 5000);

    server.once("close", () => {
      clearTimeout(forceKillTimer);
      clearTimeout(settleTimer);
      resolve();
    });
  });
}

test(
  "emitGoProject smoke: generated server can boot and serve not-implemented route",
  { skip: !runGoSmoke },
  async () => {
    const outDir = createOutDir();
    const port = await allocateEphemeralPort();

    emitGoProject(sampleIr, outDir);

    const modInit = spawnSync(
      "go",
      ["mod", "init", "example.com/tsgodown-runtime"],
      {
        cwd: outDir,
        encoding: "utf8",
      },
    );
    assert.equal(
      modInit.status,
      0,
      `go mod init failed in ${outDir}\nstdout:\n${modInit.stdout}\nstderr:\n${modInit.stderr}`,
    );

    const server = spawn("go", ["run", "."], {
      cwd: outDir,
      env: {
        ...process.env,
        PORT: port,
      },
      stdio: "ignore",
    });

    const url = `http://127.0.0.1:${port}/health`;
    const deadline = Date.now() + 10_000;
    let lastError: unknown = undefined;
    let responseText = "";
    let responseStatus = 0;

    try {
      while (Date.now() < deadline) {
        try {
          const response = await fetch(url, {
            signal: AbortSignal.timeout(1000),
          });
          responseStatus = response.status;
          responseText = await response.text();
          break;
        } catch (error) {
          lastError = error;
          await new Promise((resolve) => setTimeout(resolve, 200));
        }
      }

      assert.equal(
        responseStatus,
        501,
        `server did not become ready at ${url}; lastError=${String(lastError)}`,
      );
      assert.match(
        responseText,
        /TODO implement handler health for GET \/health/,
      );
    } finally {
      await shutdownServer(server);
    }
  },
);

test(
  "emitGoProject smoke: runtime returns deterministic 404/405 for complex method/path matching",
  { skip: !runGoSmoke },
  async () => {
    const outDir = createOutDir();
    const port = await allocateEphemeralPort();

    emitGoProject(
      {
        modules: [],
        handlers: [
          { id: "showUser", params: [], async: false },
          { id: "deleteUser", params: [], async: false },
        ],
        diagnostics: [],
        routes: [
          { method: "GET", path: "/users/:id", handlerRef: "showUser" },
          { method: "DELETE", path: "/users/:id", handlerRef: "deleteUser" },
        ],
      },
      outDir,
    );

    const modInit = spawnSync(
      "go",
      ["mod", "init", "example.com/tsgodown-runtime-status"],
      {
        cwd: outDir,
        encoding: "utf8",
      },
    );
    assert.equal(
      modInit.status,
      0,
      `go mod init failed in ${outDir}\nstdout:\n${modInit.stdout}\nstderr:\n${modInit.stderr}`,
    );

    const server = spawn("go", ["run", "."], {
      cwd: outDir,
      env: { ...process.env, PORT: port },
      stdio: "ignore",
    });

    const base = `http://127.0.0.1:${port}`;
    const deadline = Date.now() + 10_000;

    try {
      while (Date.now() < deadline) {
        try {
          const warm = await fetch(`${base}/users/123`, {
            signal: AbortSignal.timeout(1000),
          });
          if (warm.status > 0) break;
        } catch {
          await new Promise((resolve) => setTimeout(resolve, 200));
        }
      }

      const methodNotAllowed = await fetch(`${base}/users/123`, {
        method: "POST",
        signal: AbortSignal.timeout(1000),
      });
      assert.equal(methodNotAllowed.status, 405);
      assert.equal(methodNotAllowed.headers.get("allow"), "DELETE, GET");

      const notFound = await fetch(`${base}/unknown/123`, {
        signal: AbortSignal.timeout(1000),
      });
      assert.equal(notFound.status, 404);
    } finally {
      await shutdownServer(server);
    }
  },
);

test(
  "emitGoProject smoke: nested prefixes keep method/path + allow semantics stable",
  { skip: !runGoSmoke },
  async () => {
    const outDir = createOutDir();
    const port = String(await allocatePort());

    emitGoProject(
      {
        modules: [],
        handlers: [
          { id: "listPosts", params: [], async: false },
          { id: "updatePost", params: [], async: false },
        ],
        diagnostics: [],
        routes: [
          {
            method: "GET",
            path: "/api/v1/posts/:postId",
            handlerRef: "listPosts",
          },
          {
            method: "PATCH",
            path: "/api/v1/posts/:postId",
            handlerRef: "updatePost",
          },
        ],
      },
      outDir,
    );

    const modInit = spawnSync(
      "go",
      ["mod", "init", "example.com/tsgodown-runtime-nested-prefix"],
      {
        cwd: outDir,
        encoding: "utf8",
      },
    );
    assert.equal(modInit.status, 0, modInit.stderr || modInit.stdout);

    const server = spawn("go", ["run", "."], {
      cwd: outDir,
      env: { ...process.env, PORT: port },
      stdio: "ignore",
    });

    const base = `http://127.0.0.1:${port}`;
    const deadline = Date.now() + 10_000;

    try {
      while (Date.now() < deadline) {
        try {
          const warm = await fetch(`${base}/api/v1/posts/42`, {
            signal: AbortSignal.timeout(1000),
          });
          if (warm.status > 0) break;
        } catch {
          await new Promise((resolve) => setTimeout(resolve, 200));
        }
      }

      const getRes = await fetch(`${base}/api/v1/posts/42`, {
        signal: AbortSignal.timeout(1000),
      });
      assert.equal(getRes.status, 501);

      const methodNotAllowed = await fetch(`${base}/api/v1/posts/42`, {
        method: "POST",
        signal: AbortSignal.timeout(1000),
      });
      assert.equal(methodNotAllowed.status, 405);
      assert.equal(methodNotAllowed.headers.get("allow"), "GET, PATCH");

      const notFound = await fetch(`${base}/api/v2/posts/42`, {
        signal: AbortSignal.timeout(1000),
      });
      assert.equal(notFound.status, 404);
    } finally {
      await shutdownServer(server);
    }
  },
);
