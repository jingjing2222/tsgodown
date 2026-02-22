#!/usr/bin/env node
import fs from "node:fs";

const RULES = [
  {
    file: "scripts/rust-engine-launcher.mjs",
    forbidden: [
      /framework\s*:\s*["'`](fastify|hono|express|koa|nestjs?)["'`]/i,
    ],
    reason: "launcher request must not hardcode framework defaults",
  },
  {
    file: "scripts/differential-harness.mjs",
    required: [
      /getArg\("--scenario"\)\s*\?\?\s*"generic-simple-cli-get-health"/,
    ],
    reason: "differential harness default scenario must stay generic",
  },
  {
    file: "scripts/smoke-m1.sh",
    required: [/examples\/generic-simple-cli/],
    forbidden: [/EXAMPLE_DIR=.*fastify-scaffold-real/],
    reason: "smoke default fixture must stay generic",
  },
];

export function runFrameworkDefaultsGuard() {
  const violations = [];

  for (const rule of RULES) {
    if (!fs.existsSync(rule.file)) {
      violations.push({
        file: rule.file,
        reason: "required file is missing",
      });
      continue;
    }

    const body = fs.readFileSync(rule.file, "utf8");

    for (const pattern of rule.required ?? []) {
      if (!pattern.test(body)) {
        violations.push({
          file: rule.file,
          reason: `${rule.reason}; missing pattern ${pattern}`,
        });
      }
    }

    for (const pattern of rule.forbidden ?? []) {
      if (pattern.test(body)) {
        violations.push({
          file: rule.file,
          reason: `${rule.reason}; forbidden pattern ${pattern}`,
        });
      }
    }
  }

  if (violations.length > 0) {
    for (const violation of violations) {
      console.error(
        `[framework-defaults-guard] blocked: ${violation.reason} (${violation.file})`,
      );
    }
    console.error(
      "[framework-defaults-guard] fail: framework-centric defaults reintroduced",
    );
    return 1;
  }

  console.log(
    "[framework-defaults-guard] ok: generic defaults are enforced in launcher/harness/smoke scripts",
  );
  return 0;
}

if (import.meta.url === `file://${process.argv[1]}`) {
  process.exit(runFrameworkDefaultsGuard());
}
