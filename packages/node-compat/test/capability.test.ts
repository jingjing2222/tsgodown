import assert from "node:assert/strict";
import test from "node:test";

import { checkCapabilities } from "../src/capability.ts";

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

test("fails fast with source-aware diagnostic for unmet capability", () => {
  const ir = {
    routes: [{ method: "GET", path: "/x", handlerRef: "h1" }],
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
  assert.equal(diag.source?.file, "src/handlers.ts");
  assert.equal(diag.source?.line, 10);
});
