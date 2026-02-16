import assert from "node:assert/strict";
import path from "node:path";
import test from "node:test";

import { resolveTargetPlan } from "../src/internal/target-plan.ts";

test("resolveTargetPlan applies defaults when entry/outDir are not explicit strings", () => {
  const cwd = "/repo";
  const plan = resolveTargetPlan(cwd, { entry: { app: "src/app.ts" } }, 1);

  assert.deepEqual(plan, {
    configIndex: 1,
    entry: path.resolve(cwd, "src/index.ts"),
    outDir: path.resolve(cwd, "dist-go"),
    artifact: path.join(cwd, "artifacts", "manifests", "manifest.json"),
  });
});

test("resolveTargetPlan resolves explicit string entry and outDir", () => {
  const cwd = "/repo";
  const plan = resolveTargetPlan(
    cwd,
    { entry: "src/custom.ts", outDir: "custom-dist" },
    0,
  );

  assert.equal(plan.entry, path.resolve(cwd, "src/custom.ts"));
  assert.equal(plan.outDir, path.resolve(cwd, "custom-dist"));
});
