import assert from "node:assert/strict";
import path from "node:path";
import test from "node:test";

import { resolveConfigModuleUrl } from "../src/config-loader.ts";

test("resolveConfigModuleUrl points to tsgodown.config.ts in cwd", () => {
  const cwd = "/repo/example";
  const url = resolveConfigModuleUrl(cwd);

  assert.equal(url.startsWith("file://"), true);
  assert.equal(
    decodeURIComponent(new URL(url).pathname),
    path.resolve(cwd, "tsgodown.config.ts"),
  );
});
