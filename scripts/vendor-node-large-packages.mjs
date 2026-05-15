#!/usr/bin/env node

import fs from "node:fs";
import path from "node:path";

const repoRoot = path.resolve(import.meta.dirname, "..");
const corpusRoot = path.join(repoRoot, "test-corpus", "node-large");
const manifestPath = path.join(corpusRoot, "manifest.json");
const packagesRoot = path.join(corpusRoot, "packages");
const nodeModulesRoot = path.join(corpusRoot, "node_modules");

const manifest = JSON.parse(fs.readFileSync(manifestPath, "utf8"));

fs.rmSync(packagesRoot, { recursive: true, force: true });
fs.mkdirSync(packagesRoot, { recursive: true });

const entries = manifest.entries.map((entry) => {
  const sourceDir = path.join(nodeModulesRoot, entry.package);
  const packageJsonPath = path.join(sourceDir, "package.json");
  if (!fs.existsSync(packageJsonPath)) {
    throw new Error(
      `missing installed package for ${entry.id}: ${entry.package}`,
    );
  }

  const packageJson = JSON.parse(fs.readFileSync(packageJsonPath, "utf8"));
  const packageDirName = safePackageDir(entry.package);
  const packagePath = `packages/${packageDirName}`;
  const targetDir = path.join(corpusRoot, packagePath);
  copyTree(sourceDir, targetDir);

  const entryFile = resolveEntryFile(
    sourceDir,
    packageJson,
    entry.moduleFormat,
  );
  const declarationFile = resolveDeclarationFile(sourceDir, packageJson);
  const fileCount = countFiles(targetDir);

  return {
    ...entry,
    source:
      packageJson.repository?.url ??
      (typeof packageJson.repository === "string"
        ? packageJson.repository
        : `https://www.npmjs.com/package/${entry.package}/v/${entry.version}`),
    declarationSource: declarationFile
      ? `${packagePath}/${declarationFile}`
      : "not-bundled",
    packageManager: {
      name: "npm",
      lockfile: "package-lock.json",
      installedWith: "npm ci --ignore-scripts",
    },
    packagePath,
    packageMetadataPath: `${packagePath}/package.json`,
    entry: entryFile ? `${packagePath}/${entryFile}` : null,
    vendored: {
      source: "npm",
      package: entry.package,
      version: packageJson.version,
      files: fileCount,
    },
    vectors: {
      expected: entry.vectors?.expected ?? 100,
      status: "pending-vectors",
    },
  };
});

const nextManifest = {
  ...manifest,
  policy: {
    ...manifest.policy,
    status: "vendored",
  },
  entries,
};

fs.writeFileSync(
  manifestPath,
  `${JSON.stringify(nextManifest, null, 2)}\n`,
  "utf8",
);

console.log(
  JSON.stringify(
    {
      version: "node-large-vendor.v1",
      status: "passed",
      packages: entries.length,
      files: entries.reduce((total, entry) => total + entry.vendored.files, 0),
    },
    null,
    2,
  ),
);

function safePackageDir(packageName) {
  return packageName.replace(/^@/, "").replaceAll("/", "-");
}

function resolveEntryFile(sourceDir, packageJson, moduleFormat) {
  const cjsCandidates = [
    normalizePackagePath(packageJson.main),
    normalizeExportPath(packageJson.exports, "require"),
    "index.js",
    "dist/index.js",
  ];
  const esmCandidates = [
    normalizeExportPath(packageJson.exports, "import"),
    normalizePackagePath(packageJson.module),
    normalizePackagePath(packageJson.main),
    "index.mjs",
    "index.js",
    "dist/node/index.js",
    "dist/index.mjs",
    "dist/index.js",
  ];
  const candidates = (
    moduleFormat === "cjs"
      ? [...cjsCandidates, ...esmCandidates]
      : [...esmCandidates, ...cjsCandidates]
  ).filter(Boolean);

  for (const candidate of candidates) {
    if (fs.existsSync(path.join(sourceDir, candidate))) {
      return candidate;
    }
    if (fs.existsSync(path.join(sourceDir, `${candidate}.js`))) {
      return `${candidate}.js`;
    }
    if (fs.existsSync(path.join(sourceDir, `${candidate}.mjs`))) {
      return `${candidate}.mjs`;
    }
  }

  return null;
}

function resolveDeclarationFile(sourceDir, packageJson) {
  const candidates = [
    declarationForEntry(resolveEntryFile(sourceDir, packageJson)),
    normalizePackagePath(packageJson.types),
    normalizePackagePath(packageJson.typings),
    "index.d.ts",
    "dist/node/index.d.ts",
    "dist/index.d.ts",
  ].filter(Boolean);

  for (const candidate of candidates) {
    if (fs.existsSync(path.join(sourceDir, candidate))) {
      return candidate;
    }
  }

  return null;
}

function normalizePackagePath(value) {
  if (typeof value !== "string" || value.length === 0) {
    return null;
  }
  return value.replace(/^\.\//, "");
}

function normalizeExportPath(exportsField, condition = "import") {
  if (typeof exportsField === "string") {
    return normalizePackagePath(exportsField);
  }
  if (exportsField && typeof exportsField === "object") {
    const root = exportsField["."] ?? exportsField;
    if (typeof root === "string") {
      return normalizePackagePath(root);
    }
    if (root && typeof root === "object") {
      return normalizePackagePath(
        root[condition] ??
          root.default ??
          root.import ??
          root.require ??
          root.node,
      );
    }
  }
  return null;
}

function declarationForEntry(entryFile) {
  if (!entryFile || !/\.(mjs|cjs|js)$/.test(entryFile)) {
    return null;
  }
  return entryFile.replace(/\.(mjs|cjs|js)$/, ".d.ts");
}

function copyTree(source, target) {
  const stat = fs.statSync(source);
  if (stat.isDirectory()) {
    fs.mkdirSync(target, { recursive: true });
    for (const dirent of fs.readdirSync(source, { withFileTypes: true })) {
      if (shouldSkip(dirent.name)) {
        continue;
      }
      copyTree(path.join(source, dirent.name), path.join(target, dirent.name));
    }
    return;
  }
  if (stat.isFile()) {
    fs.copyFileSync(source, target);
  }
}

function shouldSkip(name) {
  return (
    name === "node_modules" ||
    name === ".git" ||
    name === ".cache" ||
    name === ".turbo" ||
    name === ".next" ||
    name === "coverage"
  );
}

function countFiles(dir) {
  let total = 0;
  for (const dirent of fs.readdirSync(dir, { withFileTypes: true })) {
    const child = path.join(dir, dirent.name);
    if (dirent.isDirectory()) {
      total += countFiles(child);
    } else if (dirent.isFile()) {
      total += 1;
    }
  }
  return total;
}
