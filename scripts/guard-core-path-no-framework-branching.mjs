#!/usr/bin/env node
import fs from "node:fs";
import path from "node:path";

const CORE_SCAN_ROOTS = [
  "packages/core/src",
  "packages/pipeline/src",
  "packages/cli/src/commands",
];

const FRAMEWORK_LITERAL = /\b(fastify|express|koa|hono|nestjs?)\b/i;
const BRANCHING_IDENTIFIER =
  /\b(framework|frameworkName|targetFramework|adapterRegistry|frameworkAdapter)\b/;

function walk(targetPath) {
  if (!fs.existsSync(targetPath)) return [];
  const stat = fs.statSync(targetPath);

  if (stat.isFile()) {
    return targetPath.endsWith(".ts") || targetPath.endsWith(".js")
      ? [targetPath]
      : [];
  }

  if (!stat.isDirectory()) return [];

  const files = [];
  for (const entry of fs.readdirSync(targetPath)) {
    if (entry === "dist" || entry === "node_modules" || entry === ".git")
      continue;
    files.push(...walk(path.join(targetPath, entry)));
  }
  return files;
}

function scanFiles(files) {
  const violations = [];

  for (const file of files) {
    const body = fs.readFileSync(file, "utf8");
    const lines = body.split(/\r?\n/);

    for (const [index, line] of lines.entries()) {
      if (FRAMEWORK_LITERAL.test(line)) {
        violations.push({
          file,
          line: index + 1,
          reason: "framework literal found in core execution path",
          snippet: line.trim(),
        });
      }

      if (BRANCHING_IDENTIFIER.test(line)) {
        violations.push({
          file,
          line: index + 1,
          reason:
            "framework/adapter branching identifier found in core execution path",
          snippet: line.trim(),
        });
      }
    }
  }

  return violations;
}

export function runCorePathFrameworkGuard() {
  const files = CORE_SCAN_ROOTS.flatMap((root) => walk(root));
  const violations = scanFiles(files);

  if (violations.length > 0) {
    for (const violation of violations) {
      console.error(
        `[core-path-guard] blocked: ${violation.reason} (${violation.file}:${violation.line})`,
      );
      console.error(`  -> ${violation.snippet}`);
    }

    console.error(
      "[core-path-guard] fail: core pipeline must stay framework-agnostic and single-path",
    );
    return 1;
  }

  console.log(
    "[core-path-guard] ok: no framework-name branching/adapters found in core execution path",
  );
  return 0;
}

if (import.meta.url === `file://${process.argv[1]}`) {
  process.exit(runCorePathFrameworkGuard());
}
