import assert from "node:assert/strict";
import test from "node:test";

import type { ProgramIR } from "../src/index.js";

const PROGRAM_IR_V1_SAMPLE = {
  modules: [
    {
      id: "module.health",
      sourcePath: "src/health.ts",
      exports: ["healthHandler"],
      imports: [{ spec: "fastify", kind: "esm" }],
    },
  ],
  routes: [
    {
      method: "GET",
      path: "/health",
      handlerRef: "healthHandler",
      middlewareRefs: ["auth"],
    },
  ],
  handlers: [
    {
      id: "healthHandler",
      params: [
        { name: "request", role: "request" },
        { name: "reply", role: "response" },
      ],
      async: true,
      semantics: {
        responseMode: "response-object",
        requestParam: "request",
        responseParam: "reply",
        usesStatus: true,
        usesBody: true,
        usesHeaders: true,
        usesJson: false,
      },
    },
  ],
  diagnostics: [
    {
      level: "warn",
      code: "ANALYZER_UNSUPPORTED_DYNAMIC_PATH",
      message: "path must be string literal",
      source: {
        file: "src/health.ts",
        line: 12,
        column: 4,
        viaSourceMap: true,
      },
    },
  ],
} satisfies ProgramIR;

test("ProgramIR v1 schema contract: canonical sample remains type-valid", () => {
  assert.equal(PROGRAM_IR_V1_SAMPLE.routes[0]?.method, "GET");
  assert.equal(
    PROGRAM_IR_V1_SAMPLE.handlers[0]?.semantics?.responseMode,
    "response-object",
  );
  assert.equal(PROGRAM_IR_V1_SAMPLE.diagnostics[0]?.source?.viaSourceMap, true);
});
