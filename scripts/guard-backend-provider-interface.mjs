#!/usr/bin/env node

import fs from "node:fs";
import path from "node:path";

const repoRoot = path.resolve(import.meta.dirname, "..");

const files = {
  backend: "crates/engine-core/src/backend.rs",
  emitGo: "crates/engine-core/src/emit_go.rs",
  lib: "crates/engine-core/src/lib.rs",
  packageJson: "package.json",
};

const checks = [
  {
    file: "backend",
    code: "BACKEND_PROVIDER_TRAIT_MISSING",
    pattern: /pub trait BackendProvider\b/,
    reason:
      "Rust compiler core must expose backend providers through one trait",
  },
  {
    file: "backend",
    code: "BACKEND_PROVIDER_REGISTRY_MISSING",
    pattern: /pub fn backend_provider\b/,
    reason: "Backend lookup must go through registry, not direct emitter calls",
  },
  {
    file: "backend",
    code: "BACKEND_PROVIDER_LIST_MISSING",
    pattern: /pub fn registered_backend_names\b/,
    reason: "Provider registry must expose deterministic backend inventory",
  },
  {
    file: "backend",
    code: "UNSUPPORTED_BACKEND_DIAGNOSTIC_MISSING",
    pattern: /BACKEND_PROVIDER_UNSUPPORTED/,
    reason:
      "Unsupported backend names must fail closed with deterministic diagnostic",
  },
  {
    file: "emitGo",
    code: "GO_BACKEND_PROVIDER_MISSING",
    pattern: /pub static GO_BACKEND_PROVIDER\b/,
    reason:
      "Go backend must be registered as provider, not ad hoc target branch",
  },
  {
    file: "emitGo",
    code: "GO_BACKEND_ADAPTER_MISSING",
    pattern: /impl BackendProvider for GoBackendProvider\b/,
    reason: "Go backend must adapt through BackendProvider",
  },
  {
    file: "emitGo",
    code: "EMIT_BACKEND_WRAPPER_MISSING",
    pattern: /pub fn emit_backend\b/,
    reason:
      "Backend emission entrypoint must accept target backend via registry",
  },
  {
    file: "lib",
    code: "BACKEND_PROVIDER_EXPORT_MISSING",
    pattern: /BackendProvider/,
    reason: "Provider interface must be part of engine-core contract surface",
  },
  {
    file: "packageJson",
    code: "BACKEND_PROVIDER_GATE_MISSING",
    pattern: /gate:backend-provider-interface/,
    reason: "Provider interface guard must be wired into package scripts",
  },
];

const findings = [];

for (const check of checks) {
  const relPath = files[check.file];
  const absolute = path.join(repoRoot, relPath);
  if (!fs.existsSync(absolute)) {
    findings.push({
      code: "BACKEND_PROVIDER_FILE_MISSING",
      file: relPath,
      reason: "Required backend provider source file is missing",
    });
    continue;
  }
  const contents = fs.readFileSync(absolute, "utf8");
  if (!check.pattern.test(contents)) {
    findings.push({
      code: check.code,
      file: relPath,
      reason: check.reason,
    });
  }
}

const report = {
  version: "backend-provider-interface-guard.v1",
  status: findings.length === 0 ? "passed" : "failed",
  findings,
};

console.log(JSON.stringify(report, null, 2));

if (findings.length > 0) {
  process.exit(1);
}
