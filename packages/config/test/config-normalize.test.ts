import assert from "node:assert/strict";
import test from "node:test";

import { normalizeUserConfigExport } from "../src/config-normalize.ts";

test("normalizeUserConfigExport wraps plain object", async () => {
  const output = await normalizeUserConfigExport({ outDir: "dist-go" });
  assert.deepEqual(output, [{ outDir: "dist-go" }]);
});

test("normalizeUserConfigExport preserves arrays", async () => {
  const output = await normalizeUserConfigExport([
    { outDir: "dist-a" },
    { outDir: "dist-b" },
  ]);

  assert.deepEqual(output, [{ outDir: "dist-a" }, { outDir: "dist-b" }]);
});

test("normalizeUserConfigExport resolves function with NODE_ENV fallback", async () => {
  const previous = process.env.NODE_ENV;
  Reflect.deleteProperty(process.env, "NODE_ENV");

  try {
    const output = await normalizeUserConfigExport((env) => ({
      define: { MODE: JSON.stringify(env.mode) },
    }));

    assert.deepEqual(output, [
      {
        define: {
          MODE: '"development"',
        },
      },
    ]);
  } finally {
    if (previous === undefined) {
      Reflect.deleteProperty(process.env, "NODE_ENV");
    } else {
      process.env.NODE_ENV = previous;
    }
  }
});
