import assert from "node:assert/strict";
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
  handlers: [],
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
  assert.match(emitted, /\/\/\s+Middleware:\s+\["auth"\]/);
  assert.match(emitted, /\/\/\s+Method:\s+POST/);
  assert.match(emitted, /\/\/\s+Path:\s+"\/users\/:id"/);
  assert.match(emitted, /\/\/\s+Handler:\s+"createUser"/);
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
