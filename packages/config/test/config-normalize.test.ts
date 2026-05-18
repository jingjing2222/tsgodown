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

test("normalizeUserConfigExport resolves tsdown-style config function args", async () => {
  const previous = process.env.CI;
  process.env.CI = "true";

  try {
    const output = await normalizeUserConfigExport((inlineConfig, context) => [
      {
        define: {
          CI: JSON.stringify(context.ci),
          INLINE_KEYS: JSON.stringify(Object.keys(inlineConfig)),
        },
        target: "node20",
      },
    ]);

    assert.deepEqual(output, [
      {
        define: {
          CI: "true",
          INLINE_KEYS: "[]",
        },
        target: "node20",
      },
    ]);
  } finally {
    if (previous === undefined) {
      Reflect.deleteProperty(process.env, "CI");
    } else {
      process.env.CI = previous;
    }
  }
});

test("normalizeUserConfigExport awaits async tsdown-compatible compiler config", async () => {
  const output = await normalizeUserConfigExport(
    Promise.resolve({
      dts: true,
      format: "esm",
      sourcemap: true,
      target: "node20",
    }),
  );

  assert.deepEqual(output, [
    {
      dts: true,
      format: "esm",
      sourcemap: true,
      target: "node20",
    },
  ]);
});
