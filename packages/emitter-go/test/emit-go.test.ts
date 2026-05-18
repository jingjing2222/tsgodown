import assert from "node:assert/strict";
import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import { after, test } from "node:test";

import { emitGoProject } from "../src/index.ts";

const tempDirs: string[] = [];

after(() => {
  for (const dir of tempDirs) {
    fs.rmSync(dir, { recursive: true, force: true });
  }
});

test("emitGoProject delegates Go emission to engine-core and writes returned files", () => {
  const cwd = fs.mkdtempSync(path.join(os.tmpdir(), "tsgodown-emitter-go-"));
  tempDirs.push(cwd);
  fs.mkdirSync(path.join(cwd, "src"), { recursive: true });
  fs.writeFileSync(path.join(cwd, "src", "index.js"), "const value = 1;\n");

  const outDir = path.join(cwd, "dist-go");
  emitGoProject(
    {
      modules: [
        {
          id: "src/index.js",
          sourcePath: "src/index.js",
          exports: [],
          imports: [],
        },
      ],
      routes: [],
      handlers: [],
      diagnostics: [],
    },
    outDir,
  );

  const mainGo = fs.readFileSync(path.join(outDir, "main.go"), "utf8");
  assert.match(mainGo, /^package main/m);
  assert.match(mainGo, /tsgodownrt\.RunProgram/);
  assert.equal(
    fs.existsSync(path.join(outDir, "tsgodownrt", "runtime.go")),
    true,
  );
});
