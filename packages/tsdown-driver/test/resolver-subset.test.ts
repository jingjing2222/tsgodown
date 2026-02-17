import assert from "node:assert/strict";
import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import test from "node:test";

import { resolveSubsetFromEntries } from "../src/artifact-indexer/resolver.ts";

test("resolver subset resolves in-scope modules/symbols and keeps unresolved diagnostics deterministic", () => {
  const cwd = fs.mkdtempSync(path.join(os.tmpdir(), "tsgodown-resolver-"));

  try {
    fs.mkdirSync(path.join(cwd, "src"), { recursive: true });
    fs.writeFileSync(
      path.join(cwd, "src", "entry.ts"),
      [
        "import mainHandler, { helper as useHelper, type User } from './handlers';",
        "import { ping } from './missing';",
        "import * as ns from './handlers';",
        "export * from './handlers';",
        "const lazy = import('./lazy');",
        "const fsmod = require('node:fs');",
        "export { helper } from './handlers';",
        "",
      ].join("\n"),
      "utf8",
    );
    fs.writeFileSync(
      path.join(cwd, "src", "handlers.ts"),
      "export const helper = () => 'ok'; export default helper; export type User = { id: string };\n",
      "utf8",
    );

    const result = resolveSubsetFromEntries(cwd, ["src/entry.ts"]);

    assert.deepEqual(result.modules, [
      { from: "src/entry.ts", spec: "./handlers", resolved: "src/handlers.ts" },
    ]);

    assert.deepEqual(result.symbols, [
      {
        from: "src/entry.ts",
        spec: "./handlers",
        imported: "default",
        local: "mainHandler",
        kind: "value",
      },
      {
        from: "src/entry.ts",
        spec: "./handlers",
        imported: "helper",
        local: "useHelper",
        kind: "value",
      },
      {
        from: "src/entry.ts",
        spec: "./handlers",
        imported: "User",
        local: "User",
        kind: "type",
      },
    ]);

    assert.deepEqual(result.unresolved, [
      {
        code: "UNRESOLVED_MODULE",
        file: "src/entry.ts",
        line: 2,
        message: 'cannot resolve module specifier "./missing"',
      },
      {
        code: "UNSUPPORTED_NAMESPACE_IMPORT",
        file: "src/entry.ts",
        line: 3,
        message: "namespace import is unsupported in resolver subset",
      },
      {
        code: "UNSUPPORTED_EXPORT_ALL",
        file: "src/entry.ts",
        line: 4,
        message: "export * from is unsupported in resolver subset",
      },
      {
        code: "UNSUPPORTED_DYNAMIC_IMPORT",
        file: "src/entry.ts",
        line: 5,
        message: "dynamic import(...) is unsupported in resolver subset",
      },
      {
        code: "UNSUPPORTED_REQUIRE_CALL",
        file: "src/entry.ts",
        line: 6,
        message: "require(...) is unsupported in resolver subset",
      },
    ]);
  } finally {
    fs.rmSync(cwd, { recursive: true, force: true });
  }
});

test("resolver subset stays fail-closed for unsupported import clauses", () => {
  const cwd = fs.mkdtempSync(path.join(os.tmpdir(), "tsgodown-resolver-"));

  try {
    fs.mkdirSync(path.join(cwd, "src"), { recursive: true });
    fs.writeFileSync(path.join(cwd, "src", "entry.ts"), "import {\n  bad as ok\n} from './ok';\n", "utf8");
    fs.writeFileSync(path.join(cwd, "src", "ok.ts"), "export const bad = 1;\n", "utf8");

    const result = resolveSubsetFromEntries(cwd, ["src/entry.ts"]);

    assert.deepEqual(result.modules, []);
    assert.deepEqual(result.symbols, []);
    assert.deepEqual(result.unresolved, [
      {
        code: "UNSUPPORTED_IMPORT_CLAUSE",
        file: "src/entry.ts",
        line: 1,
        message: "import clause is unsupported in resolver subset",
      },
    ]);
  } finally {
    fs.rmSync(cwd, { recursive: true, force: true });
  }
});
