import fs from "node:fs";
import path from "node:path";

const root = process.cwd();
const examplesRoot = path.join(root, "examples");

let hasError = false;

function fail(msg) {
  hasError = true;
  console.error(`✖ ${msg}`);
}

function ok(msg) {
  console.log(`✔ ${msg}`);
}

function listTrackedExamples() {
  const entries = fs.readdirSync(examplesRoot, { withFileTypes: true });
  return entries
    .filter((entry) => entry.isDirectory())
    .map((entry) => entry.name)
    .filter((name) =>
      fs.existsSync(path.join(examplesRoot, name, "tsgodown.config.ts")),
    )
    .sort();
}

function readEntryPathFromConfig(configText) {
  const match = configText.match(/\bentry\s*:\s*["']([^"']+)["']/);
  return match?.[1];
}

for (const name of listTrackedExamples()) {
  const exampleDir = path.join(examplesRoot, name);
  const pkgPath = path.join(exampleDir, "package.json");
  const readmePath = path.join(exampleDir, "README.md");
  const configPath = path.join(exampleDir, "tsgodown.config.ts");

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

  const configText = fs.readFileSync(configPath, "utf8");
  const entryPath = readEntryPathFromConfig(configText);
  if (!entryPath) {
    fail(`${name}: tsgodown.config.ts must declare string entry path`);
  } else if (!fs.existsSync(path.join(exampleDir, entryPath))) {
    fail(
      `${name}: tsgodown.config.ts entry path does not exist (${entryPath})`,
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

  ok(`${name}: install-first + compile-path contract is valid`);
}

if (hasError) {
  process.exit(1);
}

console.log("✔ examples install-first contract check passed");
