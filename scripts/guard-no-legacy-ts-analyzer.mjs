import fs from "node:fs";
import path from "node:path";

const repoRoot = path.resolve(import.meta.dirname, "..");
const runtimePackages = ["cli", "core", "pipeline"];
const bannedPackage = "@tsgodown/analyzer";

const failures = [];

const baseTsconfig = JSON.parse(
  fs.readFileSync(path.join(repoRoot, "tsconfig.base.json"), "utf8"),
);
if (baseTsconfig.compilerOptions?.paths?.[bannedPackage] !== undefined) {
  failures.push(`tsconfig.base.json -> paths must not include ${bannedPackage}`);
}

const rootTsconfig = JSON.parse(
  fs.readFileSync(path.join(repoRoot, "tsconfig.json"), "utf8"),
);
if ((rootTsconfig.references ?? []).some((ref) => ref.path === "packages/analyzer")) {
  failures.push("tsconfig.json -> references must not include packages/analyzer");
}

for (const pkg of runtimePackages) {
  const pkgRoot = path.join(repoRoot, "packages", pkg);
  const packageJsonPath = path.join(pkgRoot, "package.json");
  const packageJson = JSON.parse(fs.readFileSync(packageJsonPath, "utf8"));

  for (const depField of [
    "dependencies",
    "optionalDependencies",
    "peerDependencies",
  ]) {
    const deps = packageJson[depField] ?? {};
    if (Object.hasOwn(deps, bannedPackage)) {
      failures.push(
        `${pkg}/package.json -> ${depField} must not include ${bannedPackage}`,
      );
    }
  }

  const srcDir = path.join(pkgRoot, "src");
  if (!fs.existsSync(srcDir)) continue;

  const stack = [srcDir];
  while (stack.length > 0) {
    const current = stack.pop();
    if (!current) continue;

    for (const entry of fs.readdirSync(current, { withFileTypes: true })) {
      const fullPath = path.join(current, entry.name);
      if (entry.isDirectory()) {
        stack.push(fullPath);
        continue;
      }
      if (!entry.isFile()) continue;
      if (!/\.(ts|tsx|js|mjs|cjs)$/.test(entry.name)) continue;

      const text = fs.readFileSync(fullPath, "utf8");
      if (
        text.includes(`from \"${bannedPackage}\"`) ||
        text.includes(`from '${bannedPackage}'`)
      ) {
        failures.push(
          `${path.relative(repoRoot, fullPath)} -> import of ${bannedPackage} is forbidden in runtime path`,
        );
      }
    }
  }
}

if (failures.length > 0) {
  console.error("[guard:no-legacy-ts-analyzer] FAILED");
  for (const failure of failures) {
    console.error(` - ${failure}`);
  }
  process.exit(1);
}

console.log("[guard:no-legacy-ts-analyzer] OK");
