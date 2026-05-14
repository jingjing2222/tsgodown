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

test("renders executable control flow without Node fallback", () => {
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
            name: "total",
            init: { kind: "value", value: { kind: "number", value: "0" } },
          },
          {
            kind: "for",
            init: [
              {
                kind: "var-decl",
                name: "i",
                init: { kind: "value", value: { kind: "number", value: "0" } },
              },
            ],
            test: {
              kind: "binary",
              op: "<",
              left: { kind: "ident", name: "i" },
              right: { kind: "value", value: { kind: "number", value: "5" } },
            },
            update: {
              kind: "update",
              op: "++",
              arg: { kind: "ident", name: "i" },
              prefix: false,
            },
            body: [
              {
                kind: "if",
                test: {
                  kind: "binary",
                  op: "==",
                  left: { kind: "ident", name: "i" },
                  right: {
                    kind: "value",
                    value: { kind: "number", value: "2" },
                  },
                },
                consequent: [{ kind: "continue" }],
                alternate: [],
              },
              {
                kind: "expr",
                expr: {
                  kind: "assign",
                  op: "+=",
                  left: { kind: "ident", name: "total" },
                  right: { kind: "ident", name: "i" },
                },
              },
            ],
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
                    value: { kind: "string", value: "control-flow" },
                  },
                },
                {
                  key: "probes",
                  value: {
                    kind: "object",
                    props: [
                      { key: "total", value: { kind: "ident", name: "total" } },
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

  const stdout = runGoSource(source);
  assert.deepEqual(JSON.parse(stdout), {
    package: "control-flow",
    probes: {
      total: 8,
    },
  });
});

test("renders helper function declarations and calls", () => {
  const source = renderExecutableIrGoProgram({
    stmts: [
      {
        kind: "function-decl",
        name: "makeProbe",
        params: ["name"],
        async: false,
        body: [
          {
            kind: "return",
            value: {
              kind: "object",
              props: [
                {
                  key: "label",
                  value: {
                    kind: "template",
                    quasis: ["probe:", ""],
                    exprs: [{ kind: "ident", name: "name" }],
                  },
                },
              ],
            },
          },
        ],
      },
      {
        kind: "function-decl",
        name: "main",
        params: [],
        async: false,
        body: [
          {
            kind: "return",
            value: {
              kind: "object",
              props: [
                {
                  key: "package",
                  value: {
                    kind: "value",
                    value: { kind: "string", value: "functions" },
                  },
                },
                {
                  key: "probes",
                  value: {
                    kind: "call",
                    callee: { kind: "ident", name: "makeProbe" },
                    args: [
                      {
                        kind: "value",
                        value: { kind: "string", value: "alpha" },
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

  const stdout = runGoSource(source);
  assert.deepEqual(JSON.parse(stdout), {
    package: "functions",
    probes: {
      label: "probe:alpha",
    },
  });
});

test("renders probe-style top-level console JSON output", () => {
  const source = renderExecutableIrGoProgram({
    stmts: [
      {
        kind: "var-decl",
        name: "report",
        init: {
          kind: "object",
          props: [
            {
              key: "package",
              value: {
                kind: "value",
                value: { kind: "string", value: "probe-style" },
              },
            },
            {
              key: "probes",
              value: {
                kind: "object",
                props: [
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
      {
        kind: "expr",
        expr: {
          kind: "call",
          callee: {
            kind: "member",
            object: { kind: "ident", name: "console" },
            property: "log",
          },
          args: [
            {
              kind: "call",
              callee: {
                kind: "member",
                object: { kind: "ident", name: "JSON" },
                property: "stringify",
              },
              args: [
                { kind: "ident", name: "report" },
                { kind: "value", value: { kind: "null" } },
                { kind: "value", value: { kind: "number", value: "2" } },
              ],
            },
          ],
        },
      },
    ],
  });

  assert.doesNotMatch(source, /os\/exec|exec\.Command|node --/);

  const stdout = runGoSource(source);
  assert.deepEqual(JSON.parse(stdout), {
    package: "probe-style",
    probes: {
      ok: true,
    },
  });
});

function runGoSource(source) {
  const tempDir = fs.mkdtempSync(path.join(os.tmpdir(), "tsgodown-ir-go-"));
  try {
    fs.writeFileSync(
      path.join(tempDir, "go.mod"),
      "module ir-demo\n\ngo 1.22\n",
    );
    fs.writeFileSync(path.join(tempDir, "main.go"), source);
    return execFileSync("go", ["run", "."], {
      cwd: tempDir,
      encoding: "utf8",
    });
  } finally {
    fs.rmSync(tempDir, { recursive: true, force: true });
  }
}
