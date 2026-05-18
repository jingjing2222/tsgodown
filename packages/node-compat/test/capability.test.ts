import assert from "node:assert/strict";
import test from "node:test";

import {
  CapabilityStatus,
  checkCapabilities,
  collectRequiredCapabilities,
} from "../src/capability.ts";

test("passes when only WIP-supported capabilities are required", () => {
  const ir = {
    routes: [{ method: "GET", path: "/health", handlerRef: "h1" }],
    modules: [
      {
        id: "m1",
        sourcePath: "src/app.ts",
        exports: [],
        imports: [{ spec: "x", kind: "esm" }],
      },
    ],
    handlers: [{ id: "h1", params: [], async: false }],
    diagnostics: [],
  };

  const result = checkCapabilities(ir);
  assert.equal(result.ok, true);
  assert.equal(result.diagnostics.length, 0);
});

test("target backend selects backend-specific capability status", () => {
  const ir = {
    routes: [{ method: "GET", path: "/health", handlerRef: "h1" }],
    modules: [],
    handlers: [{ id: "h1", params: [], async: false }],
    diagnostics: [],
  };

  const result = checkCapabilities(ir, {
    allowWip: true,
    targetBackend: "rust",
  });

  assert.equal(result.ok, false);
  assert.equal(result.diagnostics[0]?.backend, "rust");
  assert.equal(result.diagnostics[0]?.capability, "route.basic");
  assert.equal(result.diagnostics[0]?.status, CapabilityStatus.TODO);
});

test("collectRequiredCapabilities expands node/runtime feature detection from diagnostics", () => {
  const ir = {
    routes: [],
    modules: [],
    handlers: [],
    diagnostics: [
      {
        level: "warn",
        code: "NODE_FS_BASIC_REQUIRED",
        message: "uses fs.readFileSync",
        source: { file: "src/fs.ts", line: 2, column: 4 },
      },
      {
        level: "warn",
        code: "NODE_PATH_BASIC_REQUIRED",
        message: "uses path.join",
        source: { file: "src/path.ts", line: 3, column: 2 },
      },
      {
        level: "warn",
        code: "NODE_URL_BASIC_REQUIRED",
        message: "uses URL",
        source: { file: "src/url.ts", line: 5, column: 1 },
      },
      {
        level: "warn",
        code: "NODE_PROCESS_ENV_REQUIRED",
        message: "uses process.env",
        source: { file: "src/env.ts", line: 8, column: 7 },
      },
      {
        level: "warn",
        code: "NODE_BUFFER_BASIC_REQUIRED",
        message: "uses Buffer.from",
        source: { file: "src/buf.ts", line: 13, column: 9 },
      },
      {
        level: "warn",
        code: "RUNTIME_EVENT_LOOP_REQUIRED",
        message: "uses setTimeout",
        source: { file: "src/timers.ts", line: 21, column: 3 },
      },
    ],
  };

  const required = collectRequiredCapabilities(ir);
  const capabilities = required.map((r) => r.capability);

  assert.deepEqual(capabilities, [
    "node.fs.basic",
    "node.path.basic",
    "node.url.basic",
    "node.process.env",
    "node.buffer.basic",
    "runtime.event_loop",
  ]);
  assert.equal(required[0]?.source?.file, "src/fs.ts");
  assert.equal(required[5]?.source?.line, 21);
});

test("fails fast with source-aware diagnostic including cause and guidance", () => {
  const ir = {
    routes: [],
    modules: [
      {
        id: "m1",
        sourcePath: "src/app.ts",
        exports: [],
        imports: [{ spec: "legacy-lib", kind: "cjs" }],
        source: { file: "src/app.ts", line: 1, column: 1, viaSourceMap: true },
      },
    ],
    handlers: [
      {
        id: "h1",
        params: [],
        async: true,
        source: {
          file: "src/handlers.ts",
          line: 10,
          column: 3,
          viaSourceMap: true,
        },
      },
    ],
    diagnostics: [],
  };

  const result = checkCapabilities(ir, { failFast: true });
  assert.equal(result.ok, false);
  assert.equal(result.diagnostics.length, 1);

  const [diag] = result.diagnostics;
  assert.equal(diag.code, "CAPABILITY_UNMET");
  assert.equal(diag.capability, "handler.async");
  assert.equal(diag.status, CapabilityStatus.TODO);
  assert.equal(diag.backend, "go");
  assert.equal(diag.source?.file, "src/handlers.ts");
  assert.equal(diag.source?.line, 10);
  assert.match(diag.message, /src\/handlers.ts:10:3/);
  assert.match(diag.message, /Cause:/);
  assert.match(diag.message, /Guidance:/);
  assert.equal(typeof diag.cause, "string");
  assert.equal(typeof diag.guidance, "string");
});

test("treats path/url diagnostics as supported in node-compat v1", () => {
  const ir = {
    routes: [],
    modules: [],
    handlers: [],
    diagnostics: [
      {
        level: "warn",
        code: "NODE_PATH_BASIC_REQUIRED",
        message: "uses path.join",
        source: { file: "src/path.ts", line: 3, column: 2 },
      },
      {
        level: "warn",
        code: "NODE_URL_BASIC_REQUIRED",
        message: "uses new URL",
        source: { file: "src/url.ts", line: 5, column: 1 },
      },
    ],
  };

  const result = checkCapabilities(ir, { failFast: false });
  assert.equal(result.ok, true);
  assert.equal(result.diagnostics.length, 0);
  assert.deepEqual(
    result.required.map((req) => req.capability),
    ["node.path.basic", "node.url.basic"],
  );
});

test("reports all unmet capabilities when failFast is disabled", () => {
  const ir = {
    routes: [],
    modules: [
      {
        id: "m1",
        sourcePath: "src/app.ts",
        exports: [],
        imports: [{ spec: "legacy-lib", kind: "cjs" }],
      },
    ],
    handlers: [{ id: "h1", params: [], async: true }],
    diagnostics: [
      {
        level: "warn",
        code: "NODE_FS_BASIC_REQUIRED",
        message: "uses fs.readFileSync",
        source: { file: "src/fs.ts", line: 2, column: 4 },
      },
      {
        level: "warn",
        code: "NODE_PATH_BASIC_REQUIRED",
        message: "uses path.join",
        source: { file: "src/path.ts", line: 3, column: 2 },
      },
      {
        level: "warn",
        code: "NODE_URL_BASIC_REQUIRED",
        message: "uses new URL",
        source: { file: "src/url.ts", line: 5, column: 1 },
      },
    ],
  };

  const result = checkCapabilities(ir, { failFast: false });
  assert.equal(result.ok, false);
  assert.equal(result.diagnostics.length, 3);
  assert.deepEqual(
    result.diagnostics.map((d) => d.capability),
    ["handler.async", "module.cjs", "node.fs.basic"],
  );
  assert.deepEqual(
    result.diagnostics.map((d) => d.backend),
    ["go", "go", "go"],
  );
});
