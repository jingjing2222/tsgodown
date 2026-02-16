import assert from "node:assert/strict";
import { spawnSync } from "node:child_process";
import fs from "node:fs";
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

const sampleIr: ProgramIR = {
  modules: [],
  handlers: [
    {
      id: "health",
      params: [
        { name: "req", role: "request" },
        { name: "reply", role: "response" },
      ],
      async: false,
      semantics: { responseMode: "response-object" },
    },
    {
      id: "createUser",
      params: [{ name: "req", role: "request" }],
      async: true,
      semantics: { responseMode: "return" },
    },
  ],
  diagnostics: [],
  routes: [
    {
      method: "GET",
      path: "/health",
      handlerRef: "health",
      middlewareRefs: ["auth"],
    },
    { method: "POST", path: "/users/:id", handlerRef: "createUser" },
  ],
};

test("emitGoProject emits deterministic main.go scaffold with method-aware route registration and actionable TODO stubs", () => {
  const outDir = createOutDir();

  emitGoProject(sampleIr, outDir);

  const emitted = fs.readFileSync(path.join(outDir, "main.go"), "utf8");

  assert.match(emitted, /func registerRoutes\(mux \*http\.ServeMux\) \{/);
  assert.match(emitted, /mux\.HandleFunc\("GET \/health", route0\)/);
  assert.match(emitted, /mux\.HandleFunc\("POST \/users\/\{id\}", route1\)/);
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
    /mux\.HandleFunc\("GET \/users\/\{userId\}\/orders\/\{orderId\}", route0\)/,
  );
  assert.match(emitted, /userId := req\.PathValue\("userId"\)/);
  assert.match(emitted, /orderId := req\.PathValue\("orderId"\)/);
});

const hasGoToolchain =
  spawnSync("go", ["version"], { encoding: "utf8" }).status === 0;

test(
  "emitGoProject smoke: generated representative fixtures can go build",
  { skip: !hasGoToolchain },
  () => {
    const fixtures: ProgramIR[] = [
      sampleIr,
      {
        modules: [],
        handlers: [{ id: "nested", params: [], async: false }],
        diagnostics: [],
        routes: [
          {
            method: "PATCH",
            path: "/api/v2/users/:id/devices/{deviceId}",
            handlerRef: "nested",
          },
        ],
      },
    ];

    for (const fixture of fixtures) {
      const outDir = createOutDir();
      emitGoProject(fixture, outDir);
      const result = spawnSync("go", ["build", "./..."], {
        cwd: outDir,
        encoding: "utf8",
      });
      assert.equal(
        result.status,
        0,
        `go build failed for fixture in ${outDir}\nstdout:\n${result.stdout}\nstderr:\n${result.stderr}`,
      );
    }
  },
);
