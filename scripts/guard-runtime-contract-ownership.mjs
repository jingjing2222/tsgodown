#!/usr/bin/env node

import fs from "node:fs";
import path from "node:path";

const repoRoot = path.resolve(import.meta.dirname, "..");

const files = {
  runtimeContract: "crates/engine-core/src/runtime_contract.rs",
  emitGo: "crates/engine-core/src/emit_go.rs",
  packageJson: "package.json",
};

const checks = [
  {
    file: "runtimeContract",
    code: "RUNTIME_CONTRACT_TYPE_MISSING",
    pattern: /pub struct RuntimeContract\b/,
    reason: "Runtime semantics must have backend-neutral contract type",
  },
  {
    file: "runtimeContract",
    code: "RUNTIME_OPERATION_TYPE_MISSING",
    pattern: /pub struct RuntimeOperation\b/,
    reason: "Runtime operation ownership must be explicit",
  },
  {
    file: "runtimeContract",
    code: "RUNTIME_CONTRACT_FACTORY_MISSING",
    pattern: /pub fn runtime_contract\b/,
    reason: "Runtime contract must be accessible to providers and emitters",
  },
  {
    file: "runtimeContract",
    code: "NODE_BUILTINS_CONTRACT_MISSING",
    pattern: /pub const SUPPORTED_NODE_BUILTINS\b/,
    reason: "Node builtin support policy belongs in runtime contract",
  },
  {
    file: "runtimeContract",
    code: "SEMANTIC_OPERATION_CONTRACT_MISSING",
    pattern: /js\.value-model[\s\S]*node\.process[\s\S]*node\.child-process/,
    reason: "JS and Node semantic operation keys must live in runtime contract",
  },
  {
    file: "emitGo",
    code: "GO_RUNTIME_CONTRACT_METADATA_MISSING",
    pattern: /render_runtime_contract_go_metadata/,
    reason:
      "Generated Go runtime must consume contract metadata from runtime_contract",
  },
  {
    file: "emitGo",
    code: "RUNTIME_CONTRACT_IMPORT_MISSING",
    pattern: /runtime_contract/,
    reason: "Go emitter must read runtime contract instead of inventing policy",
  },
  {
    file: "packageJson",
    code: "RUNTIME_CONTRACT_GATE_MISSING",
    pattern: /gate:runtime-contract-ownership/,
    reason:
      "Runtime contract ownership guard must be wired into package scripts",
  },
];

const forbidden = [
  {
    file: "emitGo",
    code: "SUPPORTED_BUILTIN_POLICY_IN_GO_EMITTER",
    pattern:
      /SUPPORTED_NODE_BUILTINS|is_supported_builtin_import|external module import/,
    reason: "Node builtin support policy belongs in runtime_contract.rs",
  },
];

const findings = [];

for (const check of checks) {
  const relPath = files[check.file];
  const absolute = path.join(repoRoot, relPath);
  if (!fs.existsSync(absolute)) {
    findings.push({
      code: "RUNTIME_CONTRACT_FILE_MISSING",
      file: relPath,
      reason: "Required runtime contract source file is missing",
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

for (const check of forbidden) {
  const relPath = files[check.file];
  const absolute = path.join(repoRoot, relPath);
  if (!fs.existsSync(absolute)) {
    continue;
  }
  const contents = fs.readFileSync(absolute, "utf8");
  if (check.pattern.test(contents)) {
    findings.push({
      code: check.code,
      file: relPath,
      reason: check.reason,
    });
  }
}

const report = {
  version: "runtime-contract-ownership-guard.v1",
  status: findings.length === 0 ? "passed" : "failed",
  findings,
};

console.log(JSON.stringify(report, null, 2));

if (findings.length > 0) {
  process.exit(1);
}
