import assert from "node:assert/strict";
import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import { after, test } from "node:test";

import { build, check, report, stages } from "../../core/src/index.ts";

const tempDirs: string[] = [];

after(() => {
  for (const dir of tempDirs) {
    fs.rmSync(dir, { recursive: true, force: true });
  }
});

function setupProject() {
  const dir = fs.mkdtempSync(path.join(os.tmpdir(), "tsgodown-cli-e2e-"));
  tempDirs.push(dir);

  fs.mkdirSync(path.join(dir, "src"), { recursive: true });
  fs.writeFileSync(
    path.join(dir, "src", "index.ts"),
    [
      "const fastify = {} as any;",
      "const listUsers = () => {};",
      "const getUser = () => {};",
      "fastify.get('/users', listUsers);",
      "fastify.get('/users/:id', getUser);",
      "",
    ].join("\n"),
  );

  fs.writeFileSync(
    path.join(dir, "tsgodown.config.ts"),
    `export default { entry: "src/index.ts", outDir: "dist-go" };\n`,
  );

  fs.mkdirSync(path.join(dir, "dist"), { recursive: true });
  fs.writeFileSync(path.join(dir, "dist", "index.js"), "export {};\n");

  return dir;
}

test("build/check/report/stages produce stable command payloads and deterministic Go output", async () => {
  const cwd = setupProject();

  const buildResult = await build(cwd);
  assert.equal(buildResult.command, "build");
  assert.deepEqual(buildResult.stages, [
    "load-config",
    "analyze",
    "emit",
    "onSuccess",
  ]);
  assert.equal(buildResult.targets.length, 1);
  assert.equal(buildResult.targets[0].diagnostics.routes, 2);
  assert.equal(buildResult.targets[0].emitted, true);

  const goPath = path.join(cwd, "dist-go", "main.go");
  assert.equal(fs.existsSync(goPath), true);
  const go = fs.readFileSync(goPath, "utf8");
  assert.ok(go.includes('mux.HandleFunc("/users", route0)'));
  assert.ok(go.includes('mux.HandleFunc("/users/:id", route1)'));
  assert.ok(go.includes("if req.Method != http.MethodGet {"));
  assert.ok(
    go.includes(
      'fmt.Fprintln(w, "TODO implement handler listUsers for GET /users")',
    ),
  );
  assert.ok(
    go.includes(
      'fmt.Fprintln(w, "TODO implement handler getUser for GET /users/:id")',
    ),
  );

  const checkResult = await check(cwd);
  assert.equal(checkResult.command, "check");
  assert.equal(checkResult.targets[0].diagnostics.routes, 2);
  assert.equal(checkResult.targets[0].emitted, true);

  const reportResult = await report(cwd);
  assert.equal(reportResult.command, "report");
  assert.equal(reportResult.targets[0].diagnostics.routes, 2);

  const stagesResult = await stages(cwd);
  assert.deepEqual(stagesResult.stages, [
    "load-config",
    "analyze",
    "emit",
    "onSuccess",
  ]);
  assert.equal(stagesResult.targets[0].emitted, true);
});
