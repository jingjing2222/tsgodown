import assert from "node:assert/strict";
import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import test from "node:test";

import { runFrameworkDefaultsGuard } from "./guard-framework-defaults.mjs";

function withTempRepo(run) {
  const prev = process.cwd();
  const tmp = fs.mkdtempSync(
    path.join(os.tmpdir(), "tsgodown-defaults-guard-"),
  );

  try {
    fs.mkdirSync(path.join(tmp, "scripts"), { recursive: true });
    process.chdir(tmp);
    run(tmp);
  } finally {
    process.chdir(prev);
    fs.rmSync(tmp, { recursive: true, force: true });
  }
}

test("framework defaults guard passes for generic defaults", () => {
  withTempRepo((tmp) => {
    fs.writeFileSync(
      path.join(tmp, "scripts", "rust-engine-launcher.mjs"),
      "const request = { action: 'build' };",
    );
    fs.writeFileSync(
      path.join(tmp, "scripts", "differential-harness.mjs"),
      'const scenarioName = getArg("--scenario") ?? "generic-simple-cli-get-health";',
    );
    fs.writeFileSync(
      path.join(tmp, "scripts", "smoke-m1.sh"),
      'EXAMPLE_DIR="${ROOT}/examples/generic-simple-cli"\n',
    );

    assert.equal(runFrameworkDefaultsGuard(), 0);
  });
});

test("framework defaults guard fails when framework default is reintroduced", () => {
  withTempRepo((tmp) => {
    fs.writeFileSync(
      path.join(tmp, "scripts", "rust-engine-launcher.mjs"),
      'const request = { framework: "fastify" };',
    );
    fs.writeFileSync(
      path.join(tmp, "scripts", "differential-harness.mjs"),
      'const scenarioName = getArg("--scenario") ?? "fastify-scaffold-real-get-health";',
    );
    fs.writeFileSync(
      path.join(tmp, "scripts", "smoke-m1.sh"),
      'EXAMPLE_DIR="${ROOT}/examples/fastify-scaffold-real"\n',
    );

    assert.equal(runFrameworkDefaultsGuard(), 1);
  });
});
