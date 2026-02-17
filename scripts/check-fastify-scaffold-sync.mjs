import fs from "node:fs";
import path from "node:path";

const root = process.cwd();

const required = [
  "examples/fastify-scaffold-real/src/app.ts",
  "examples/fastify-scaffold-real/src/plugins/sensible.ts",
  "examples/fastify-scaffold-real/src/plugins/support.ts",
  "examples/fastify-scaffold-real/src/routes/root.ts",
  "examples/fastify-scaffold-real/src/routes/example/index.ts",
  "examples/fastify-scaffold-real/tsgodown.config.ts",
  "packages/cli/test/fixtures/projects/fastify-scaffold-real/src/app.ts",
  "packages/cli/test/fixtures/projects/fastify-scaffold-real/src/routes/health.ts",
  "packages/cli/test/fixtures/projects/fastify-scaffold-real/src/routes/users.ts",
  "packages/cli/test/fixtures/projects/fastify-scaffold-real/tsgodown.config.ts",
];

const presentCount = required.filter((p) =>
  fs.existsSync(path.join(root, p)),
).length;

if (presentCount === 0) {
  console.log(
    "[scaffold-sync] SKIP (scaffold baseline not introduced on this branch yet)",
  );
  process.exit(0);
}

if (presentCount !== required.length) {
  const missing = required.filter((p) => !fs.existsSync(path.join(root, p)));
  console.error(
    "[scaffold-sync] partial scaffold baseline detected; missing files:",
  );
  for (const p of missing) {
    console.error(`- ${p}`);
  }
  process.exit(1);
}

console.log(`[scaffold-sync] PASS (${required.length} files present)`);
