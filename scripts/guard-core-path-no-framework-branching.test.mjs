import assert from "node:assert/strict";
import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import test from "node:test";

import { runCorePathFrameworkGuard } from "./guard-core-path-no-framework-branching.mjs";

function withTempRepo(run) {
  const prev = process.cwd();
  const tmp = fs.mkdtempSync(
    path.join(os.tmpdir(), "tsgodown-core-path-guard-"),
  );

  try {
    fs.mkdirSync(path.join(tmp, "packages/core/src"), { recursive: true });
    fs.mkdirSync(path.join(tmp, "packages/pipeline/src/internal"), {
      recursive: true,
    });
    fs.mkdirSync(path.join(tmp, "packages/cli/src/commands"), {
      recursive: true,
    });
    process.chdir(tmp);
    run(tmp);
  } finally {
    process.chdir(prev);
    fs.rmSync(tmp, { recursive: true, force: true });
  }
}

test("core path guard passes for framework-agnostic sources", () => {
  withTempRepo((tmp) => {
    fs.writeFileSync(
      path.join(tmp, "packages/core/src/index.ts"),
      'export const MODE = "compiler";\n',
    );
    fs.writeFileSync(
      path.join(tmp, "packages/pipeline/src/internal/stage.ts"),
      "export function run() { return true; }\n",
    );

    assert.equal(runCorePathFrameworkGuard(), 0);
  });
});

test("core path guard fails on framework literals and branching identifiers", () => {
  withTempRepo((tmp) => {
    fs.writeFileSync(
      path.join(tmp, "packages/pipeline/src/internal/stage.ts"),
      'if (frameworkName === "fastify") return null;\n',
    );

    assert.equal(runCorePathFrameworkGuard(), 1);
  });
});
