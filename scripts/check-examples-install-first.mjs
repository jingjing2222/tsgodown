import fs from "node:fs";
import path from "node:path";

const root = process.cwd();
const examplesRoot = path.join(root, "examples");

const requiredExamples = [
  "fastify-min",
  "fastify-complex",
  "fastify-scaffold-real",
];

let hasError = false;

function fail(msg) {
  hasError = true;
  console.error(`✖ ${msg}`);
}

function ok(msg) {
  console.log(`✔ ${msg}`);
}

for (const name of requiredExamples) {
  const exampleDir = path.join(examplesRoot, name);
  const pkgPath = path.join(exampleDir, "package.json");
  const readmePath = path.join(exampleDir, "README.md");

  if (!fs.existsSync(pkgPath)) {
    fail(`${name}: missing package.json`);
    continue;
  }

  const pkg = JSON.parse(fs.readFileSync(pkgPath, "utf8"));
  const buildGo = pkg?.scripts?.["build:go"];
  const tsgodownDep = pkg?.devDependencies?.tsgodown;

  if (buildGo !== "tsgodown build --json") {
    fail(
      `${name}: scripts.build:go must be \"tsgodown build --json\" (got ${JSON.stringify(buildGo)})`,
    );
  }

  if (tsgodownDep !== "workspace:*") {
    fail(
      `${name}: devDependencies.tsgodown must be \"workspace:*\" (got ${JSON.stringify(tsgodownDep)})`,
    );
  }

  if (!fs.existsSync(readmePath)) {
    fail(`${name}: missing README.md`);
    continue;
  }

  const readme = fs.readFileSync(readmePath, "utf8");
  if (!readme.includes("pnpm install")) {
    fail(`${name}: README must include 'pnpm install'`);
  }
  if (!readme.includes("pnpm run build:go")) {
    fail(`${name}: README must include 'pnpm run build:go'`);
  }

  ok(`${name}: install-first build flow contract is valid`);
}

if (hasError) {
  process.exit(1);
}

console.log("✔ examples install-first contract check passed");
