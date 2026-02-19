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

test("emitGoProject emits deterministic main.go scaffold with IR-aware semantic handler behavior", () => {
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
    /w\.Header\(\)\.Set\("Content-Type", "application\/json; charset=utf-8"\)/,
  );
  assert.match(
    emitted,
    /w\.Header\(\)\.Set\("X-TSGoDown-Handler", "response-object"\)/,
  );
  assert.match(emitted, /w\.Header\(\)\.Set\("X-TSGoDown-Handler", "return"\)/);
  assert.match(emitted, /w\.WriteHeader\(http\.StatusOK\)/);
  assert.match(emitted, /json\.NewEncoder\(w\)\.Encode\(map\[string\]any\{/);
  assert.doesNotMatch(
    emitted,
    /TODO\(tsgodown\): Implement handler "health" for GET \/health\./,
  );
  assert.doesNotMatch(
    emitted,
    /TODO\(tsgodown\): Implement handler "createUser" for POST \/users\/:id\./,
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

test("emitGoProject keeps deterministic output for equivalent IR with reordered handlers/diagnostics", () => {
  const firstOutDir = createOutDir();
  const secondOutDir = createOutDir();

  const firstIr: ProgramIR = {
    modules: [],
    handlers: [
      {
        id: "createUser",
        params: [{ role: "request", name: "req" }],
        async: true,
        semantics: {
          responseMode: "return",
          usesStatus: false,
          usesBody: false,
          usesHeaders: false,
          usesJson: false,
        },
      },
      {
        id: "health",
        params: [
          { role: "request", name: "req" },
          { role: "response", name: "reply" },
        ],
        async: false,
        semantics: {
          responseMode: "response-object",
          usesStatus: false,
          usesBody: false,
          usesHeaders: false,
          usesJson: false,
        },
      },
    ],
    diagnostics: [
      {
        level: "warn",
        code: "LATE_WARNING",
        message: "later warning",
        source: { file: "src/server.ts", line: 20, column: 5 },
      },
      {
        level: "warn",
        code: "EARLY_WARNING",
        message: "earlier warning",
        source: { file: "src/server.ts", line: 2, column: 1 },
      },
    ],
    routes: [
      { method: "GET", path: "/health", handlerRef: "health" },
      { method: "POST", path: "/users/:id", handlerRef: "createUser" },
    ],
  };

  const secondIr: ProgramIR = {
    ...firstIr,
    handlers: [...firstIr.handlers].reverse(),
    diagnostics: [...firstIr.diagnostics].reverse(),
  };

  emitGoProject(firstIr, firstOutDir);
  emitGoProject(secondIr, secondOutDir);

  const first = fs.readFileSync(path.join(firstOutDir, "main.go"), "utf8");
  const second = fs.readFileSync(path.join(secondOutDir, "main.go"), "utf8");

  assert.equal(first, second);
  assert.match(second, /\[warn\] EARLY_WARNING: earlier warning/);
  assert.match(second, /\[warn\] LATE_WARNING: later warning/);
  assert.ok(
    second.indexOf("[warn] EARLY_WARNING") <
      second.indexOf("[warn] LATE_WARNING"),
  );
});

test("emitGoProject emits literal object payload for supported return-mode bodyRef subset", () => {
  const outDir = createOutDir();

  emitGoProject(
    {
      modules: [],
      handlers: [
        {
          id: "health",
          params: [],
          async: false,
          bodyRef: '{"ok":true,"message":"healthy"}',
          semantics: {
            responseMode: "return",
            usesStatus: false,
            usesBody: false,
            usesHeaders: false,
            usesJson: false,
          },
        },
      ],
      diagnostics: [],
      routes: [{ method: "GET", path: "/health", handlerRef: "health" }],
    },
    outDir,
  );

  const emitted = fs.readFileSync(path.join(outDir, "main.go"), "utf8");

  assert.match(emitted, /"message": "healthy"/);
  assert.match(emitted, /"ok": true/);
  assert.doesNotMatch(emitted, /"handler": "health"/);
  assert.doesNotMatch(emitted, /"mode": "return"/);
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
    /w\.Header\(\)\.Set\("Content-Type", "application\/json; charset=utf-8"\)/,
  );
  assert.match(
    emitted,
    /w\.Header\(\)\.Set\("X-TSGoDown-Handler", "unknown"\)/,
  );
  assert.match(emitted, /"mode": "unknown"/);
  assert.doesNotMatch(emitted, /TODO\(tsgodown\): Implement handler/);
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

test("emitGoProject keeps middleware metadata comments deterministic for equivalent reordered middleware refs", () => {
  const firstOutDir = createOutDir();
  const secondOutDir = createOutDir();

  const firstIr: ProgramIR = {
    modules: [],
    handlers: [{ id: "h", params: [], async: false }],
    diagnostics: [],
    routes: [
      {
        method: "GET",
        path: "/users/:id",
        handlerRef: "h",
        middlewareRefs: ["z-auth", "a-trace", "m-cache"],
      },
    ],
  };

  const secondIr: ProgramIR = {
    ...firstIr,
    routes: [
      {
        ...firstIr.routes[0],
        middlewareRefs: ["m-cache", "z-auth", "a-trace"],
      },
    ],
  };

  emitGoProject(firstIr, firstOutDir);
  emitGoProject(secondIr, secondOutDir);

  const first = fs.readFileSync(path.join(firstOutDir, "main.go"), "utf8");
  const second = fs.readFileSync(path.join(secondOutDir, "main.go"), "utf8");

  assert.equal(first, second);
  assert.match(second, /\/\/\s+Middleware:\s+\["a-trace","m-cache","z-auth"\]/);
});

test("emitGoProject unknown handler semantics use concrete JSON fallback without TODO markers", () => {
  const sampleIr = readFixture("sample-ir");
  const outDir = createOutDir();

  emitGoProject(
    {
      ...sampleIr,
      routes: [
        {
          method: "GET",
          path: "/health",
          handlerRef: "handler_health",
        },
      ],
      handlers: [
        {
          id: "handler_health",
          params: [],
          async: false,
          semantics: {
            responseMode: "unknown",
            usesStatus: false,
            usesBody: false,
            usesHeaders: false,
            usesJson: false,
          },
        },
      ],
    },
    outDir,
  );

  const goSource = fs.readFileSync(path.join(outDir, "main.go"), "utf8");
  assert.match(
    goSource,
    /w\.Header\(\)\.Set\("Content-Type", "application\/json; charset=utf-8"\)/,
  );
  assert.match(
    goSource,
    /w\.Header\(\)\.Set\("X-TSGoDown-Handler", "unknown"\)/,
  );
  assert.match(goSource, /"handler": "handler_health"/);
  assert.match(goSource, /"mode": "unknown"/);
  assert.doesNotMatch(goSource, /TODO\(tsgodown\): Implement handler/);
  assert.doesNotMatch(goSource, /TODO implement handler/);
});

test("emitGoProject renders sparse indexed sourcemap diagnostics as file-only comments even when partial coordinates leak into source metadata", () => {
  const outDir = createOutDir();

  emitGoProject(
    {
      modules: [],
      handlers: [{ id: "h", params: [], async: false }],
      diagnostics: [
        {
          level: "warn",
          code: "PIPELINE_SOURCEMAP_SPARSE_MAPPING",
          message:
            "sourcemap sections include sparse source entries; positional metadata omitted deterministically",
          source: {
            file: "dist/maps/index.mjs.map",
            viaSourceMap: true,
            line: 4,
            column: 8,
          },
        },
      ],
      routes: [{ method: "GET", path: "/health", handlerRef: "h" }],
    },
    outDir,
  );

  const emitted = fs.readFileSync(path.join(outDir, "main.go"), "utf8");
  assert.match(
    emitted,
    /\/\/\s+\[warn\] PIPELINE_SOURCEMAP_SPARSE_MAPPING: sourcemap sections include sparse source entries; positional metadata omitted deterministically/,
  );
  assert.match(emitted, /\/\/\s+at dist\/maps\/index\.mjs\.map/);
  assert.doesNotMatch(emitted, /dist\/maps\/index\.mjs\.map:4:8/);

  if (hasGoToolchain) {
    const modulePath =
      "example.com/tsgodown-smoke/sparse-file-only-diagnostics";
    const modInit = spawnSync("go", ["mod", "init", modulePath], {
      cwd: outDir,
      encoding: "utf8",
    });
    assert.equal(modInit.status, 0, modInit.stderr || modInit.stdout);

    const build = spawnSync("go", ["build", "./..."], {
      cwd: outDir,
      encoding: "utf8",
    });
    assert.equal(build.status, 0, build.stderr || build.stdout);
  }
});

test("emitGoProject renders diagnostic source without placeholder coordinates when sourcemap omits line/column", () => {
  const outDir = createOutDir();

  emitGoProject(
    {
      modules: [],
      handlers: [{ id: "h", params: [], async: false }],
      diagnostics: [
        {
          level: "warn",
          code: "SOURCEMAP_POSITION_PARTIAL",
          message:
            "sourcemap provided original file without stable generated coordinates",
          source: { file: "src/generated/entry.js" },
        },
      ],
      routes: [{ method: "GET", path: "/health", handlerRef: "h" }],
    },
    outDir,
  );

  const emitted = fs.readFileSync(path.join(outDir, "main.go"), "utf8");

  assert.match(
    emitted,
    /\/\/\s+\[warn\] SOURCEMAP_POSITION_PARTIAL: sourcemap provided original file without stable generated coordinates/,
  );
  assert.match(emitted, /\/\/\s+at src\/generated\/entry\.js/);
  assert.doesNotMatch(emitted, /src\/generated\/entry\.js:\?:\?/);
});

test("emitGoProject renders column-only sourcemap diagnostic source without placeholder coordinates and keeps go build smoke green", () => {
  const outDir = createOutDir();

  emitGoProject(
    {
      modules: [],
      handlers: [{ id: "h", params: [], async: false }],
      diagnostics: [
        {
          level: "warn",
          code: "SOURCEMAP_POSITION_COLUMN_ONLY",
          message:
            "sourcemap provided a stable source file but omitted generated line",
          source: { file: "src/generated/chunk.js", column: 7 },
        },
      ],
      routes: [{ method: "GET", path: "/health", handlerRef: "h" }],
    },
    outDir,
  );

  const emitted = fs.readFileSync(path.join(outDir, "main.go"), "utf8");

  assert.match(
    emitted,
    /\/\/\s+\[warn\] SOURCEMAP_POSITION_COLUMN_ONLY: sourcemap provided a stable source file but omitted generated line/,
  );
  assert.match(emitted, /\/\/\s+at src\/generated\/chunk\.js/);
  assert.doesNotMatch(emitted, /src\/generated\/chunk\.js:\?:7/);

  if (hasGoToolchain) {
    const modulePath =
      "example.com/tsgodown-smoke/column-only-sourcemap-diagnostic";
    const modInit = spawnSync("go", ["mod", "init", modulePath], {
      cwd: outDir,
      encoding: "utf8",
    });
    assert.equal(modInit.status, 0, modInit.stderr || modInit.stdout);

    const build = spawnSync("go", ["build", "./..."], {
      cwd: outDir,
      encoding: "utf8",
    });
    assert.equal(build.status, 0, build.stderr || build.stdout);
  }
});

test("emitGoProject keeps deterministic comment rendering for indexed sourcemap diagnostics with missing section sources", () => {
  const outDir = createOutDir();

  emitGoProject(
    {
      modules: [],
      handlers: [{ id: "h", params: [], async: false }],
      diagnostics: [
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
      routes: [{ method: "GET", path: "/health", handlerRef: "h" }],
    },
    outDir,
  );

  const emitted = fs.readFileSync(path.join(outDir, "main.go"), "utf8");
  assert.match(
    emitted,
    /\/\/\s+\[warn\] PIPELINE_INVALID_SOURCEMAP_MAPPING: indexed sourcemap section map is missing sources\[\]; section ignored for deterministic mapping/,
  );
  assert.match(
    emitted,
    /\/\/\s+\[warn\] PIPELINE_SOURCEMAP_POSITION_PARTIAL: indexed sourcemap section offset is partial; diagnostics remain file-scoped for deterministic mapping/,
  );
  assert.match(emitted, /\/\/\s+at dist\/maps\/index\.mjs\.map:9/);
});

test("emitGoProject sorts same-file indexed missing-sources diagnostics by numeric line for stable Go comments", () => {
  const outDir = createOutDir();

  emitGoProject(
    {
      modules: [],
      handlers: [{ id: "h", params: [], async: false }],
      diagnostics: [
        {
          level: "warn",
          code: "PIPELINE_INVALID_SOURCEMAP_MAPPING",
          message:
            "indexed sourcemap section map is missing sources[]; section ignored for deterministic mapping",
          source: {
            file: "dist/maps/index.mjs.map",
            viaSourceMap: true,
            line: 21,
          },
        },
        {
          level: "warn",
          code: "PIPELINE_INVALID_SOURCEMAP_MAPPING",
          message:
            "indexed sourcemap section map is missing sources[]; section ignored for deterministic mapping",
          source: {
            file: "dist/maps/index.mjs.map",
            viaSourceMap: true,
            line: 4,
          },
        },
      ],
      routes: [{ method: "GET", path: "/health", handlerRef: "h" }],
    },
    outDir,
  );

  const emitted = fs.readFileSync(path.join(outDir, "main.go"), "utf8");
  const line4 = emitted.indexOf("at dist/maps/index.mjs.map:4");
  const line21 = emitted.indexOf("at dist/maps/index.mjs.map:21");
  assert.ok(line4 >= 0 && line21 >= 0);
  assert.ok(line4 < line21);
});

test("emitGoProject keeps deterministic ordering for mixed typed-IR sourcemap diagnostics across path styles", () => {
  const firstOutDir = createOutDir();
  const secondOutDir = createOutDir();

  const baseDiagnostics: ProgramIR["diagnostics"] = [
    {
      level: "warn",
      code: "PIPELINE_MISSING_SOURCEMAP_MAPPING",
      message:
        "missing sourcemap path required for deterministic source mapping",
      source: {
        file: "dist/index.mjs",
        viaSourceMap: true,
        line: 1,
        column: 1,
      },
    },
    {
      level: "warn",
      code: "PIPELINE_SOURCEMAP_POSITION_PARTIAL",
      message:
        "indexed sourcemap section offset is partial; diagnostics remain file-scoped for deterministic mapping",
      source: { file: "dist/maps/index.mjs.map", viaSourceMap: true, line: 5 },
    },
    {
      level: "warn",
      code: "PIPELINE_INVALID_SOURCEMAP_MAPPING",
      message:
        "sourcemap metadata missing sources[] for artifact-to-ir mapping",
      source: {
        file: "dist/maps/index.mjs.map",
        viaSourceMap: true,
        line: 1,
        column: 1,
      },
    },
  ];

  const firstIr: ProgramIR = {
    modules: [],
    handlers: [{ id: "h", params: [], async: false }],
    diagnostics: [...baseDiagnostics].reverse(),
    routes: [{ method: "GET", path: "/health", handlerRef: "h" }],
  };

  const secondIr: ProgramIR = {
    ...firstIr,
    diagnostics: [
      {
        ...baseDiagnostics[2],
        source: {
          ...baseDiagnostics[2].source,
          file: "dist\\maps\\index.mjs.map",
        },
      },
      {
        ...baseDiagnostics[0],
        source: { ...baseDiagnostics[0].source, file: "dist\\index.mjs" },
      },
      {
        ...baseDiagnostics[1],
        source: {
          ...baseDiagnostics[1].source,
          file: "dist\\maps\\index.mjs.map",
        },
      },
    ],
  };

  emitGoProject(firstIr, firstOutDir);
  emitGoProject(secondIr, secondOutDir);

  const first = fs.readFileSync(path.join(firstOutDir, "main.go"), "utf8");
  const second = fs.readFileSync(path.join(secondOutDir, "main.go"), "utf8");

  assert.equal(first, second);
  assert.ok(
    second.indexOf("PIPELINE_MISSING_SOURCEMAP_MAPPING") <
      second.indexOf("PIPELINE_INVALID_SOURCEMAP_MAPPING"),
  );
  assert.ok(
    second.indexOf("PIPELINE_INVALID_SOURCEMAP_MAPPING") <
      second.indexOf("PIPELINE_SOURCEMAP_POSITION_PARTIAL"),
  );
  assert.match(second, /\/\/\s+at dist\/maps\/index\.mjs\.map:1:1/);
  assert.match(second, /\/\/\s+at dist\/index\.mjs:1:1/);
  assert.match(second, /\/\/\s+at dist\/maps\/index\.mjs\.map:5/);
  assert.doesNotMatch(second, /dist\\maps\\index\.mjs\.map/);
});

test("emitGoProject keeps normalized repo-relative sourcemap diagnostic paths in comments and go-build flow", () => {
  const outDir = createOutDir();

  emitGoProject(
    {
      modules: [
        {
          id: "module_0",
          sourcePath: "src/health.ts",
          exports: ["health"],
          imports: [],
        },
        {
          id: "module_1",
          sourcePath: "src/routes/list.ts",
          exports: ["health"],
          imports: [],
        },
      ],
      handlers: [{ id: "h", params: [], async: false }],
      diagnostics: [
        {
          level: "warn",
          code: "SOURCEMAP_NORMALIZED_PATH",
          message: "normalized sourcemap source path for stable module mapping",
          source: { file: "src/routes/list.ts", line: 1, column: 1 },
        },
      ],
      routes: [{ method: "GET", path: "/health", handlerRef: "h" }],
    },
    outDir,
  );

  const emitted = fs.readFileSync(path.join(outDir, "main.go"), "utf8");
  assert.match(emitted, /\/\/\s+at src\/routes\/list\.ts:1:1/);

  if (hasGoToolchain) {
    const modulePath = "example.com/tsgodown-smoke/normalized-sourcemap-paths";
    const modInit = spawnSync("go", ["mod", "init", modulePath], {
      cwd: outDir,
      encoding: "utf8",
    });
    assert.equal(modInit.status, 0, modInit.stderr || modInit.stdout);

    const build = spawnSync("go", ["build", "./..."], {
      cwd: outDir,
      encoding: "utf8",
    });
    assert.equal(build.status, 0, build.stderr || build.stdout);
  }
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
  "emitGoProject smoke: file-only sparse sourcemap diagnostics keep runtime semantics stable",
  { skip: !runGoSmoke },
  async () => {
    const outDir = createOutDir();
    const port = await allocateEphemeralPort();

    emitGoProject(
      {
        modules: [],
        handlers: [{ id: "h", params: [], async: false }],
        diagnostics: [
          {
            level: "warn",
            code: "PIPELINE_SOURCEMAP_SPARSE_MAPPING",
            message:
              "indexed sourcemap section had sparse mappings; positional metadata omitted deterministically",
            source: { file: "dist/maps/index.mjs.map", viaSourceMap: true },
          },
        ],
        routes: [{ method: "GET", path: "/health", handlerRef: "h" }],
      },
      outDir,
    );

    const emitted = fs.readFileSync(path.join(outDir, "main.go"), "utf8");
    assert.match(emitted, /\/\/\s+at dist\/maps\/index\.mjs\.map/);
    assert.doesNotMatch(emitted, /dist\/maps\/index\.mjs\.map:\?:\?/);

    const modInit = spawnSync(
      "go",
      ["mod", "init", "example.com/tsgodown-runtime-sparse-sourcemap"],
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

    const url = `http://127.0.0.1:${port}/health`;
    const deadline = Date.now() + 10_000;
    let responseStatus = 0;
    let responseText = "";

    try {
      while (Date.now() < deadline) {
        try {
          const response = await fetch(url, {
            signal: AbortSignal.timeout(1000),
          });
          responseStatus = response.status;
          responseText = await response.text();
          break;
        } catch {
          await new Promise((resolve) => setTimeout(resolve, 200));
        }
      }

      assert.equal(responseStatus, 501);
      assert.match(responseText, /"handler":"h"/);
      assert.match(responseText, /"mode":"unknown"/);
    } finally {
      await shutdownServer(server);
    }
  },
);

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
  "emitGoProject smoke: generated server can boot and serve semantic JSON route",
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
        200,
        `server did not become ready at ${url}; lastError=${String(lastError)}`,
      );
      assert.match(responseText, /"handler":"health"/);
      assert.match(responseText, /"mode":"response-object"/);
    } finally {
      await shutdownServer(server);
    }
  },
);

test(
  "emitGoProject smoke: return-mode literal object payload bodyRef serves payload JSON",
  { skip: !runGoSmoke },
  async () => {
    const outDir = createOutDir();
    const port = await allocateEphemeralPort();

    emitGoProject(
      {
        modules: [],
        handlers: [
          {
            id: "health",
            params: [],
            async: false,
            bodyRef: '{"ok":true,"message":"healthy"}',
            semantics: {
              responseMode: "return",
              usesStatus: false,
              usesBody: false,
              usesHeaders: false,
              usesJson: false,
            },
          },
        ],
        diagnostics: [],
        routes: [{ method: "GET", path: "/health", handlerRef: "health" }],
      },
      outDir,
    );

    const modInit = spawnSync(
      "go",
      ["mod", "init", "example.com/tsgodown-runtime-literal-payload"],
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

    const url = `http://127.0.0.1:${port}/health`;
    const deadline = Date.now() + 10_000;
    let responseStatus = 0;
    let body = "";

    try {
      while (Date.now() < deadline) {
        try {
          const response = await fetch(url, {
            signal: AbortSignal.timeout(1000),
          });
          responseStatus = response.status;
          body = await response.text();
          break;
        } catch {
          await new Promise((resolve) => setTimeout(resolve, 200));
        }
      }

      assert.equal(responseStatus, 200);
      assert.match(body, /"ok":true/);
      assert.match(body, /"message":"healthy"/);
      assert.doesNotMatch(body, /"handler":"health"/);
      assert.doesNotMatch(body, /"mode":"return"/);
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
      assert.equal(methodNotAllowed.headers.get("allow"), "DELETE, GET, HEAD");

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
  "emitGoProject smoke: method mismatch Allow includes implicit HEAD when GET route exists",
  { skip: !runGoSmoke },
  async () => {
    const outDir = createOutDir();
    const port = String(await allocatePort());

    emitGoProject(
      {
        modules: [],
        handlers: [{ id: "showUser", params: [], async: false }],
        diagnostics: [],
        routes: [{ method: "GET", path: "/users/:id", handlerRef: "showUser" }],
      },
      outDir,
    );

    const modInit = spawnSync(
      "go",
      ["mod", "init", "example.com/tsgodown-runtime-head-allow"],
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
          const warm = await fetch(`${base}/users/42`, {
            signal: AbortSignal.timeout(1000),
          });
          if (warm.status > 0) break;
        } catch {
          await new Promise((resolve) => setTimeout(resolve, 200));
        }
      }

      const methodNotAllowed = await fetch(`${base}/users/42`, {
        method: "POST",
        signal: AbortSignal.timeout(1000),
      });
      assert.equal(methodNotAllowed.status, 405);
      assert.equal(methodNotAllowed.headers.get("allow"), "GET, HEAD");
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
      assert.equal(methodNotAllowed.headers.get("allow"), "GET, HEAD, PATCH");

      const notFound = await fetch(`${base}/api/v2/posts/42`, {
        signal: AbortSignal.timeout(1000),
      });
      assert.equal(notFound.status, 404);
    } finally {
      await shutdownServer(server);
    }
  },
);
