import assert from "node:assert/strict";
import path from "node:path";
import test from "node:test";
import { fileURLToPath } from "node:url";

import { analyzeFastifyEntry } from "../dist/index.js";

const __filename = fileURLToPath(import.meta.url);
const __dirname = path.dirname(__filename);

function fixture(name) {
  return path.join(__dirname, "fixtures", name);
}

test("extracts method/path/handler from shorthand and route object", () => {
  const ir = analyzeFastifyEntry(fixture("basic-fastify.fixture.txt"));

  assert.deepEqual(ir.routes, [
    { method: "GET", path: "/users", handlerRef: "listUsers" },
    { method: "POST", path: "/users", handlerRef: "createUser" },
    { method: "PATCH", path: "/users/:id", handlerRef: "updateUser" },
  ]);
  assert.equal(ir.diagnostics.length, 0);
});

test("applies register prefix for inline and named plugin callbacks", () => {
  const ir = analyzeFastifyEntry(
    fixture("register-prefix-fastify.fixture.txt"),
  );

  assert.deepEqual(ir.routes, [
    { method: "GET", path: "/v1/users", handlerRef: "listV1Users" },
    { method: "GET", path: "/v1/users/:id", handlerRef: "showV1User" },
    { method: "GET", path: "/v1/admin/accounts", handlerRef: "listAccounts" },
  ]);
  assert.equal(ir.diagnostics.length, 0);
});

test("emits explicit diagnostics for unsupported patterns", () => {
  const ir = analyzeFastifyEntry(fixture("unsupported-fastify.fixture.txt"));

  assert.deepEqual(ir.routes, []);
  assert.deepEqual(ir.diagnostics.map((d) => d.code).sort(), [
    "ANALYZER_UNRESOLVED_PLUGIN",
    "ANALYZER_UNSUPPORTED_DYNAMIC_PATH",
    "ANALYZER_UNSUPPORTED_INLINE_HANDLER",
  ]);
});

test("handles nested register + prefix + route-object variants without policy decisions", () => {
  const ir = analyzeFastifyEntry(
    fixture("nested-register-route-object-fastify.fixture.txt"),
  );

  assert.deepEqual(ir.routes, [
    { method: "GET", path: "/api/users", handlerRef: "listApiUsers" },
    {
      method: "GET",
      path: "/api/v2/users/:id",
      handlerRef: "listApiUserById",
    },
    {
      method: "GET",
      path: "/private/devices",
      handlerRef: "listPrivateDevices",
    },
    {
      method: "POST",
      path: "/private/devices",
      handlerRef: "createPrivateDevice",
    },
    {
      method: "GET",
      path: "/internal/metrics",
      handlerRef: "listInternalMetrics",
    },
  ]);
  assert.equal(ir.diagnostics.length, 0);
});

test("keeps analyzer as semantic extractor/diagnostics only (SSoT boundary)", () => {
  const ir = analyzeFastifyEntry(fixture("ssot-boundary-fastify.fixture.txt"));

  assert.deepEqual(ir.routes, [
    { method: "GET", path: "/policy/allow", handlerRef: "allowAdminOnly" },
    { method: "POST", path: "/policy/deny", handlerRef: "denyGuest" },
  ]);

  const diagnosticCodes = ir.diagnostics.map((d) => d.code);
  assert.equal(diagnosticCodes.includes("CAPABILITY_UNMET"), false);
  assert.equal(
    diagnosticCodes.some((code) => code.startsWith("CAPABILITY_")),
    false,
  );
});
