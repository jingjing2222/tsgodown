import assert from "node:assert/strict";
import { spawnSync } from "node:child_process";
import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import test from "node:test";

const repoRoot = path.resolve(import.meta.dirname, "../../..");
const tscBin = path.join(repoRoot, "node_modules", ".bin", "tsc");

function typecheckFixture(source: string) {
  const cwd = fs.mkdtempSync(path.join(os.tmpdir(), "tsgodown-config-types-"));

  try {
    fs.writeFileSync(path.join(cwd, "fixture.ts"), source, "utf8");
    fs.writeFileSync(
      path.join(cwd, "tsconfig.json"),
      `${JSON.stringify(
        {
          compilerOptions: {
            target: "ES2022",
            module: "ESNext",
            moduleResolution: "Bundler",
            strict: true,
            skipLibCheck: true,
            noEmit: true,
            types: ["node"],
            typeRoots: [path.join(repoRoot, "node_modules", "@types")],
            baseUrl: repoRoot,
            paths: {
              "@tsgodown/config": ["packages/config/src/index.ts"],
            },
          },
          include: ["fixture.ts"],
        },
        null,
        2,
      )}\n`,
      "utf8",
    );

    return spawnSync(tscBin, ["-p", cwd], {
      cwd: repoRoot,
      encoding: "utf8",
    });
  } finally {
    fs.rmSync(cwd, { recursive: true, force: true });
  }
}

test("defineConfig accepts tsdown-compatible compiler config surface", () => {
  const result = typecheckFixture(`
    import { defineConfig } from "@tsgodown/config";

    export default defineConfig({
      entry: "src/index.ts",
      outDir: "dist-go",
      target: "node20",
      format: "esm",
      sourcemap: true,
      dts: true,
      define: { __TSGODOWN__: "true" },
      go: {
        package: "main",
        strictSemantics: true,
      },
    });

    export const functionConfig = defineConfig((inlineConfig, context) => [
      {
        target: context.ci ? "node20" : "node18",
        define: {
          INLINE_CONFIG_KEYS: JSON.stringify(Object.keys(inlineConfig)),
        },
      },
    ]);
  `);

  assert.equal(
    result.status,
    0,
    `expected tsdown-compatible config to typecheck\nstdout:\n${result.stdout}\nstderr:\n${result.stderr}`,
  );
});

test("defineConfig rejects framework-specific config keys", () => {
  const result = typecheckFixture(`
    import { defineConfig } from "@tsgodown/config";

    export default defineConfig({
      entry: "src/index.ts",
      fastify: {
        detectPlugins: true,
      },
    });
  `);

  assert.notEqual(result.status, 0, "framework config key must not typecheck");
  assert.match(
    `${result.stdout}\n${result.stderr}`,
    /fastify/,
    "type error should point at the framework-specific key",
  );
});
