import assert from "node:assert/strict";
import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import { after, test } from "node:test";

import { runPipeline } from "../src/index.ts";

const tempDirs: string[] = [];

after(() => {
  for (const dir of tempDirs) {
    fs.rmSync(dir, { recursive: true, force: true });
  }
});

function setupProject() {
  const dir = fs.mkdtempSync(path.join(os.tmpdir(), "tsgodown-pipeline-e2e-"));
  tempDirs.push(dir);

  fs.mkdirSync(path.join(dir, "src"), { recursive: true });
  fs.writeFileSync(
    path.join(dir, "src", "index.ts"),
    [
      "const fastify = {} as any;",
      "const health = () => {};",
      "const createUser = () => {};",
      "fastify.get('/health', health);",
      "fastify.post('/users', createUser);",
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

test("runPipeline executes all stages and emits deterministic Go scaffold", async () => {
  const cwd = setupProject();
  const logs: string[] = [];

  await runPipeline(cwd, {
    log(message) {
      logs.push(message);
    },
  });

  assert.equal(logs.length, 4);
  assert.match(logs[0], /\[BUILD_ARTIFACTS\]/);
  assert.match(logs[1], /\[BUILD_IR\] analyzing entry: src\/index\.ts/);
  assert.match(logs[2], /\[CAPABILITY_GATE\]/);
  assert.match(logs[3], /\[EMIT_GO\] writing Go scaffold to dist-go/);

  const goPath = path.join(cwd, "dist-go", "main.go");
  assert.equal(fs.existsSync(goPath), true);

  const emitted = fs.readFileSync(goPath, "utf8");
  assert.ok(emitted.includes('mux.HandleFunc("GET /health", route0)'));
  assert.ok(emitted.includes('mux.HandleFunc("POST /users", route1)'));
  assert.ok(!emitted.includes("if req.Method != http.MethodGet {"));
  assert.ok(!emitted.includes("if req.Method != http.MethodPost {"));
  assert.ok(emitted.includes("// Route metadata:"));
  assert.ok(emitted.includes("//   Method: GET"));
  assert.ok(emitted.includes('//   Path: "/health"'));
  assert.ok(emitted.includes('//   Handler: "health"'));
  assert.ok(
    emitted.includes(
      'fmt.Fprintln(w, "TODO implement handler health for GET /health")',
    ),
  );
  assert.ok(
    emitted.includes(
      'fmt.Fprintln(w, "TODO implement handler createUser for POST /users")',
    ),
  );
});
