import assert from "node:assert/strict";
import { execFileSync } from "node:child_process";
import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import test from "node:test";

import { renderExecutableIrGoProgram } from "./lib/executable-ir-go-codegen.mjs";

test("renders a standalone Go program from source-lowered executable IR", () => {
  const source = renderExecutableIrGoProgram({
    stmts: [
      {
        kind: "function-decl",
        name: "main",
        params: [],
        async: false,
        body: [
          {
            kind: "var-decl",
            name: "name",
            init: { kind: "value", value: { kind: "string", value: "kim" } },
          },
          {
            kind: "var-decl",
            name: "label",
            init: {
              kind: "template",
              quasis: ["hello ", ""],
              exprs: [{ kind: "ident", name: "name" }],
            },
          },
          {
            kind: "return",
            value: {
              kind: "object",
              props: [
                {
                  key: "package",
                  value: {
                    kind: "value",
                    value: { kind: "string", value: "ir-demo" },
                  },
                },
                {
                  key: "probes",
                  value: {
                    kind: "object",
                    props: [
                      { key: "label", value: { kind: "ident", name: "label" } },
                      {
                        key: "items",
                        value: {
                          kind: "array",
                          items: [
                            {
                              kind: "value",
                              value: { kind: "number", value: "1" },
                            },
                            {
                              kind: "value",
                              value: { kind: "number", value: "2" },
                            },
                          ],
                        },
                      },
                      {
                        key: "ok",
                        value: {
                          kind: "value",
                          value: { kind: "bool", value: true },
                        },
                      },
                    ],
                  },
                },
              ],
            },
          },
        ],
      },
    ],
  });

  assert.doesNotMatch(source, /os\/exec|exec\.Command|node --/);

  const tempDir = fs.mkdtempSync(path.join(os.tmpdir(), "tsgodown-ir-go-"));
  try {
    fs.writeFileSync(
      path.join(tempDir, "go.mod"),
      "module ir-demo\n\ngo 1.22\n",
    );
    fs.writeFileSync(path.join(tempDir, "main.go"), source);
    const stdout = execFileSync("go", ["run", "."], {
      cwd: tempDir,
      encoding: "utf8",
    });

    assert.deepEqual(JSON.parse(stdout), {
      package: "ir-demo",
      probes: {
        items: [1, 2],
        label: "hello kim",
        ok: true,
      },
    });
  } finally {
    fs.rmSync(tempDir, { recursive: true, force: true });
  }
});
