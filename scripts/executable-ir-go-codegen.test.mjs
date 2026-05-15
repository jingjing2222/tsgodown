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

test("renders imported function and namespace calls through Go helpers", () => {
  const source = renderExecutableIrGoProgram(
    {
      stmts: [
        {
          kind: "var-decl",
          name: "parsed",
          init: {
            kind: "call",
            callee: { kind: "ident", name: "parser" },
            args: [
              {
                kind: "array",
                items: [
                  { kind: "value", value: { kind: "string", value: "--name" } },
                  { kind: "value", value: { kind: "string", value: "kim" } },
                ],
              },
            ],
          },
        },
        {
          kind: "var-decl",
          name: "query",
          init: {
            kind: "call",
            callee: {
              kind: "member",
              object: { kind: "ident", name: "qs" },
              property: "parse",
            },
            args: [
              {
                kind: "value",
                value: { kind: "string", value: "tag=a&tag=b" },
              },
            ],
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
                  value: { kind: "string", value: "imports" },
                },
              },
              {
                key: "probes",
                value: {
                  kind: "object",
                  props: [
                    { key: "parsed", value: { kind: "ident", name: "parsed" } },
                    { key: "query", value: { kind: "ident", name: "query" } },
                  ],
                },
              },
            ],
          },
        },
      ],
    },
    {
      externalFunctions: ["parser"],
      externalNamespaces: { qs: ["parse"] },
      helperSource: [
        "func js_parser(argv any) any {",
        '\treturn map[string]any{"name": "kim"}',
        "}",
        "",
        "func js_qs_parse(raw any) any {",
        '\treturn map[string]any{"tag": []any{"a", "b"}}',
        "}",
      ].join("\n"),
    },
  );

  assert.doesNotMatch(source, /os\/exec|exec\.Command|node --/);

  const stdout = runGoSource(source);
  assert.deepEqual(JSON.parse(stdout), {
    package: "imports",
    probes: {
      parsed: { name: "kim" },
      query: { tag: ["a", "b"] },
    },
  });
});

test("renders constructors, member calls, array spread, nullish, and timer await", () => {
  const source = renderExecutableIrGoProgram(
    {
      stmts: [
        {
          kind: "var-decl",
          name: "box",
          init: {
            kind: "new",
            callee: { kind: "ident", name: "Box" },
            args: [{ kind: "value", value: { kind: "number", value: "2" } }],
          },
        },
        {
          kind: "expr",
          expr: {
            kind: "call",
            callee: {
              kind: "member",
              object: { kind: "ident", name: "box" },
              property: "push",
            },
            args: [{ kind: "value", value: { kind: "number", value: "3" } }],
          },
        },
        {
          kind: "expr",
          expr: {
            kind: "await",
            arg: {
              kind: "new",
              callee: { kind: "ident", name: "Promise" },
              args: [
                {
                  kind: "function",
                  params: ["resolve"],
                  async: false,
                  body: [
                    {
                      kind: "return",
                      value: {
                        kind: "call",
                        callee: { kind: "ident", name: "setTimeout" },
                        args: [
                          { kind: "ident", name: "resolve" },
                          {
                            kind: "value",
                            value: { kind: "number", value: "1" },
                          },
                        ],
                      },
                    },
                  ],
                },
              ],
            },
          },
        },
        {
          kind: "return",
          value: {
            kind: "object",
            props: [
              {
                key: "items",
                value: {
                  kind: "array-spread",
                  items: [
                    {
                      spread: true,
                      value: {
                        kind: "call",
                        callee: {
                          kind: "member",
                          object: { kind: "ident", name: "box" },
                          property: "items",
                        },
                        args: [],
                      },
                    },
                  ],
                },
              },
              {
                key: "missing",
                value: {
                  kind: "binary",
                  op: "??",
                  left: {
                    kind: "call",
                    callee: {
                      kind: "member",
                      object: { kind: "ident", name: "box" },
                      property: "missing",
                    },
                    args: [],
                  },
                  right: {
                    kind: "value",
                    value: { kind: "string", value: "fallback" },
                  },
                },
              },
            ],
          },
        },
      ],
    },
    {
      externalConstructors: ["Box"],
      extraImports: ["time"],
      helperSource: [
        "type jsBox struct {",
        "\titems []any",
        "}",
        "",
        "func js_new_Box(value any) any {",
        "\treturn &jsBox{items: []any{value}}",
        "}",
        "",
        "func (box *jsBox) jsCallMember(property string, args ...any) any {",
        "\tswitch property {",
        '\tcase "push":',
        "\t\tbox.items = append(box.items, args[0])",
        "\t\treturn box",
        '\tcase "items":',
        "\t\treturn box.items",
        '\tcase "missing":',
        "\t\treturn nil",
        "\tdefault:",
        '\t\tpanic("unsupported box member")',
        "\t}",
        "}",
        "",
        "func jsSleepPromise(delay any) any {",
        "\ttime.Sleep(time.Duration(delay.(int)) * time.Millisecond)",
        "\treturn nil",
        "}",
      ].join("\n"),
    },
  );

  assert.doesNotMatch(source, /os\/exec|exec\.Command|node --/);

  const stdout = runGoSource(source);
  assert.deepEqual(JSON.parse(stdout), {
    items: [2, 3],
    missing: "fallback",
  });
});

test("renders executable unary expressions with JS truthiness", () => {
  const source = renderExecutableIrGoProgram({
    stmts: [
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
                  key: "notEmpty",
                  value: {
                    kind: "unary",
                    op: "!",
                    arg: {
                      kind: "value",
                      value: { kind: "string", value: "value" },
                    },
                  },
                },
                {
                  key: "notEmptyString",
                  value: {
                    kind: "unary",
                    op: "!",
                    arg: {
                      kind: "value",
                      value: { kind: "string", value: "" },
                    },
                  },
                },
                {
                  key: "typeString",
                  value: {
                    kind: "unary",
                    op: "typeof",
                    arg: {
                      kind: "value",
                      value: { kind: "string", value: "value" },
                    },
                  },
                },
                {
                  key: "negated",
                  value: {
                    kind: "unary",
                    op: "-",
                    arg: {
                      kind: "value",
                      value: { kind: "number", value: "3" },
                    },
                  },
                },
              ],
            },
          },
        ],
      },
    ],
  });

  const stdout = runGoSource(source);
  assert.deepEqual(JSON.parse(stdout), {
    negated: -3,
    notEmpty: false,
    notEmptyString: true,
    typeString: "string",
  });
});

test("renders executable for-of loops over arrays", () => {
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
            name: "items",
            init: {
              kind: "array",
              items: [
                { kind: "value", value: { kind: "number", value: "1" } },
                { kind: "value", value: { kind: "number", value: "2" } },
                { kind: "value", value: { kind: "number", value: "3" } },
              ],
            },
          },
          {
            kind: "var-decl",
            name: "seen",
            init: { kind: "array", items: [] },
          },
          {
            kind: "for-of",
            left: "item",
            right: { kind: "ident", name: "items" },
            body: [
              {
                kind: "expr",
                expr: {
                  kind: "call",
                  callee: {
                    kind: "member",
                    object: { kind: "ident", name: "seen" },
                    property: "push",
                  },
                  args: [{ kind: "ident", name: "item" }],
                },
              },
            ],
          },
          {
            kind: "return",
            value: { kind: "ident", name: "seen" },
          },
        ],
      },
    ],
  });

  const stdout = runGoSource(source);
  assert.deepEqual(JSON.parse(stdout), [1, 2, 3]);
});

test("renders executable switch statements", () => {
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
            name: "kind",
            init: { kind: "value", value: { kind: "string", value: "beta" } },
          },
          {
            kind: "switch",
            discriminant: { kind: "ident", name: "kind" },
            cases: [
              {
                test: {
                  kind: "value",
                  value: { kind: "string", value: "alpha" },
                },
                consequent: [
                  {
                    kind: "return",
                    value: {
                      kind: "value",
                      value: { kind: "string", value: "a" },
                    },
                  },
                ],
              },
              {
                test: {
                  kind: "value",
                  value: { kind: "string", value: "beta" },
                },
                consequent: [
                  {
                    kind: "return",
                    value: {
                      kind: "value",
                      value: { kind: "string", value: "b" },
                    },
                  },
                ],
              },
              {
                test: null,
                consequent: [
                  {
                    kind: "return",
                    value: {
                      kind: "value",
                      value: { kind: "string", value: "fallback" },
                    },
                  },
                ],
              },
            ],
          },
        ],
      },
    ],
  });

  const stdout = runGoSource(source);
  assert.equal(JSON.parse(stdout), "b");
});

test("renders Node path, os, and fs sync runtime helpers", () => {
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
            name: "root",
            init: {
              kind: "call",
              callee: { kind: "ident", name: "mkdtempSync" },
              args: [
                {
                  kind: "call",
                  callee: { kind: "ident", name: "join" },
                  args: [
                    {
                      kind: "call",
                      callee: { kind: "ident", name: "tmpdir" },
                      args: [],
                    },
                    {
                      kind: "value",
                      value: { kind: "string", value: "tsgodown-runtime-" },
                    },
                  ],
                },
              ],
            },
          },
          {
            kind: "var-decl",
            name: "file",
            init: {
              kind: "call",
              callee: { kind: "ident", name: "join" },
              args: [
                { kind: "ident", name: "root" },
                { kind: "value", value: { kind: "string", value: "data.txt" } },
              ],
            },
          },
          {
            kind: "expr",
            expr: {
              kind: "call",
              callee: { kind: "ident", name: "writeFileSync" },
              args: [
                { kind: "ident", name: "file" },
                { kind: "value", value: { kind: "string", value: "ok" } },
              ],
            },
          },
          {
            kind: "var-decl",
            name: "parent",
            init: {
              kind: "call",
              callee: { kind: "ident", name: "dirname" },
              args: [{ kind: "ident", name: "file" }],
            },
          },
          {
            kind: "expr",
            expr: {
              kind: "call",
              callee: { kind: "ident", name: "rmSync" },
              args: [
                { kind: "ident", name: "root" },
                {
                  kind: "object",
                  props: [
                    {
                      key: "recursive",
                      value: {
                        kind: "value",
                        value: { kind: "bool", value: true },
                      },
                    },
                  ],
                },
              ],
            },
          },
          {
            kind: "return",
            value: {
              kind: "object",
              props: [
                {
                  key: "parentMatches",
                  value: {
                    kind: "binary",
                    op: "===",
                    left: { kind: "ident", name: "parent" },
                    right: { kind: "ident", name: "root" },
                  },
                },
              ],
            },
          },
        ],
      },
    ],
  });

  const stdout = runGoSource(source);
  assert.deepEqual(JSON.parse(stdout), { parentMatches: true });
});

test("promotes top-level function-valued bindings into callable functions", () => {
  const source = renderExecutableIrGoProgram({
    stmts: [
      {
        kind: "var-decl",
        name: "double",
        init: {
          kind: "function",
          params: ["value"],
          async: false,
          body: [
            {
              kind: "return",
              value: {
                kind: "binary",
                op: "*",
                left: { kind: "ident", name: "value" },
                right: { kind: "value", value: { kind: "number", value: "2" } },
              },
            },
          ],
        },
      },
      {
        kind: "return",
        value: {
          kind: "call",
          callee: { kind: "ident", name: "double" },
          args: [{ kind: "value", value: { kind: "number", value: "4" } }],
        },
      },
    ],
  });

  const stdout = runGoSource(source);
  assert.equal(JSON.parse(stdout), 8);
});

test("renders RegExp values with test member calls", () => {
  const source = renderExecutableIrGoProgram({
    stmts: [
      {
        kind: "var-decl",
        name: "matcher",
        init: {
          kind: "value",
          value: { kind: "regexp", pattern: "^a+$", flags: "" },
        },
      },
      {
        kind: "return",
        value: {
          kind: "array",
          items: [
            {
              kind: "call",
              callee: {
                kind: "member",
                object: { kind: "ident", name: "matcher" },
                property: "test",
              },
              args: [
                { kind: "value", value: { kind: "string", value: "aaa" } },
              ],
            },
            {
              kind: "call",
              callee: {
                kind: "member",
                object: { kind: "ident", name: "matcher" },
                property: "test",
              },
              args: [
                { kind: "value", value: { kind: "string", value: "bbb" } },
              ],
            },
          ],
        },
      },
    ],
  });

  const stdout = runGoSource(source);
  assert.deepEqual(JSON.parse(stdout), [true, false]);
});

test("renders Symbol constructor calls as deterministic runtime values", () => {
  const source = renderExecutableIrGoProgram({
    stmts: [
      {
        kind: "return",
        value: {
          kind: "call",
          callee: { kind: "ident", name: "Symbol" },
          args: [{ kind: "value", value: { kind: "string", value: "probe" } }],
        },
      },
    ],
  });

  const stdout = runGoSource(source);
  assert.equal(JSON.parse(stdout), "Symbol(probe)");
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
