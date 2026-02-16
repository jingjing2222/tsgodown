import assert from "node:assert/strict";
import { spawnSync } from "node:child_process";
import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import test from "node:test";

const repoRoot = path.resolve(import.meta.dirname, "../../..");
const packagesDir = path.join(repoRoot, "packages");

const packageExpectations = [
  ["analyzer", ["main", "types", "exports"]],
  ["cli", ["main", "types", "exports", "bin"]],
  ["config", ["main", "types", "exports"]],
  ["core", ["main", "types", "exports"]],
  ["emitter-go", ["main", "types", "exports"]],
  ["ir-core", ["main", "types", "exports"]],
  ["node-compat", ["main", "types", "exports"]],
  ["pipeline", ["main", "types", "exports"]],
  ["tsdown-driver", ["main", "types", "exports"]],
];

test("workspace package runtime entries point to emitted dist files", () => {
  for (const [pkgName, fields] of packageExpectations) {
    const packageRoot = path.join(packagesDir, pkgName);
    const packageJson = JSON.parse(
      fs.readFileSync(path.join(packageRoot, "package.json"), "utf8"),
    );

    for (const field of fields) {
      if (field === "bin") {
        const bins = Object.values(packageJson.bin ?? {});
        assert.ok(
          bins.length > 0,
          `${pkgName} should declare at least one bin`,
        );
        for (const entry of bins) {
          const target = path.join(packageRoot, entry);
          assert.ok(
            fs.existsSync(target),
            `${pkgName} bin target missing: ${entry}`,
          );
        }
        continue;
      }

      if (field === "exports") {
        const exportsEntry = packageJson.exports?.["."];
        assert.ok(exportsEntry, `${pkgName} exports["."] is required`);
        assert.equal(
          typeof exportsEntry.import,
          "string",
          `${pkgName} exports import must be a string`,
        );
        assert.equal(
          typeof exportsEntry.types,
          "string",
          `${pkgName} exports types must be a string`,
        );

        for (const [kind, entry] of Object.entries({
          import: exportsEntry.import,
          types: exportsEntry.types,
        })) {
          const target = path.join(packageRoot, entry);
          assert.ok(
            fs.existsSync(target),
            `${pkgName} exports ${kind} target missing: ${entry}`,
          );
        }
        continue;
      }

      const entry = packageJson[field];
      assert.equal(
        typeof entry,
        "string",
        `${pkgName} ${field} must be a string`,
      );
      const target = path.join(packageRoot, entry);
      assert.ok(
        fs.existsSync(target),
        `${pkgName} ${field} target missing: ${entry}`,
      );
    }
  }
});

test("legacy @tsgodown/ir package is marked inactive", () => {
  const packageRoot = path.join(packagesDir, "ir");
  const packageJson = JSON.parse(
    fs.readFileSync(path.join(packageRoot, "package.json"), "utf8"),
  );

  assert.equal(packageJson.private, true);
  assert.equal(packageJson.main, undefined);
  assert.equal(packageJson.types, undefined);
  assert.equal(packageJson.exports, undefined);
  assert.ok(
    typeof packageJson.description === "string" &&
      packageJson.description.includes("DEPRECATED"),
  );
});

test("cli dist entry runs build/check/report/stages commands", () => {
  const cwd = fs.mkdtempSync(path.join(os.tmpdir(), "tsgodown-cli-runtime-"));

  fs.mkdirSync(path.join(cwd, "src"), { recursive: true });
  fs.writeFileSync(
    path.join(cwd, "src", "index.ts"),
    [
      "const handler = () => ({ ok: true });",
      "fastify.get('/health', handler);",
      "",
    ].join("\n"),
    "utf8",
  );

  fs.writeFileSync(
    path.join(cwd, "tsgodown.config.ts"),
    [
      "export default {",
      "  entry: 'src/index.ts',",
      "  outDir: 'dist-go'",
      "};",
      "",
    ].join("\n"),
    "utf8",
  );

  const cliEntry = path.join(repoRoot, "packages", "cli", "dist", "index.js");
  for (const command of ["build", "check", "report", "stages"]) {
    const result = spawnSync(process.execPath, [cliEntry, command, "--json"], {
      cwd,
      encoding: "utf8",
    });

    assert.equal(result.status, 0, `${command} failed: ${result.stderr}`);
    const jsonStart = result.stdout.indexOf("{");
    assert.ok(jsonStart >= 0, `${command} should print JSON output`);
    const parsed = JSON.parse(result.stdout.slice(jsonStart));
    assert.ok(parsed.cwd, `${command} should print result`);
  }

  assert.ok(
    fs.existsSync(path.join(cwd, "artifacts", "manifests", "manifest.json")),
    "build command should emit artifact manifest via rust adapter",
  );
  assert.equal(
    fs.existsSync(path.join(cwd, "dist-go", "main.go")),
    false,
    "legacy TS core emitter should be inactive after rust cutover",
  );
});
