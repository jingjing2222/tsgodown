#!/usr/bin/env node

import fs from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";

const repoRoot = path.resolve(
  path.dirname(fileURLToPath(import.meta.url)),
  "..",
);
const args = process.argv.slice(2);
const ledgerName = args.find((arg) => !arg.startsWith("--"));
const finalMode = args.includes("--final");

const STATUSES = new Set(["DONE", "WIP", "TODO", "FAIL_CLOSED", "BLOCKED"]);

const LEDGERS = {
  "node-lts": {
    path: "docs/specs/NODE_LTS_COVERAGE_LEDGER.md",
    columns: [
      "Key",
      "Area",
      "Stability",
      "Contract Status",
      "Go Status",
      "Diagnostic",
      "Evidence",
      "Notes",
    ],
    requiredKeys: [
      "node.assert",
      "node.async_context",
      "node.async_hooks",
      "node.buffer",
      "node.addons_cpp",
      "node.addons_node_api",
      "node.embedder_api",
      "node.child_process",
      "node.cluster",
      "node.cli_options",
      "node.console",
      "node.crypto",
      "node.debugger",
      "node.deprecated",
      "node.diagnostics_channel",
      "node.dns",
      "node.domain",
      "node.env_vars",
      "node.errors",
      "node.events",
      "node.fs",
      "node.globals",
      "node.http",
      "node.http2",
      "node.https",
      "node.inspector",
      "node.intl",
      "node.module_cjs",
      "node.module_esm",
      "node.module_api",
      "node.packages",
      "node.typescript",
      "node.net",
      "node.os",
      "node.path",
      "node.perf_hooks",
      "node.permissions",
      "node.process",
      "node.punycode",
      "node.querystring",
      "node.readline",
      "node.repl",
      "node.report",
      "node.sea",
      "node.sqlite",
      "node.stream",
      "node.string_decoder",
      "node.test_runner",
      "node.timers",
      "node.tls",
      "node.trace_events",
      "node.tty",
      "node.dgram",
      "node.url",
      "node.util",
      "node.v8",
      "node.vm",
      "node.wasi",
      "node.webcrypto",
      "node.webstreams",
      "node.worker_threads",
      "node.zlib",
    ],
  },
  "tsdown-artifact": {
    path: "docs/specs/TSDOWN_ARTIFACT_CONTRACT.md",
    columns: [
      "Key",
      "Artifact Shape",
      "Contract Status",
      "Go Status",
      "Diagnostic",
      "Evidence",
      "Notes",
    ],
    requiredKeys: [
      "tsdown.esm_bundle",
      "tsdown.cjs_bundle",
      "tsdown.dual_package",
      "tsdown.dts",
      "tsdown.declaration_map",
      "tsdown.sourcemap",
      "tsdown.package_exports",
      "tsdown.package_imports",
      "tsdown.package_main_module_type",
      "tsdown.node_builtins",
      "tsdown.json_modules",
      "tsdown.import_attributes",
      "tsdown.dynamic_import",
      "tsdown.top_level_await",
      "tsdown.code_splitting",
      "tsdown.externals",
      "tsdown.assets",
      "tsdown.cli_shebang",
      "tsdown.platform_target",
      "tsdown.package_manager",
      "tsdown.diagnostics_mapping",
    ],
  },
  ecmascript: {
    path: "docs/specs/ECMASCRIPT_SEMANTICS_LEDGER.md",
    columns: [
      "Key",
      "Area",
      "Contract Status",
      "Go Status",
      "Diagnostic",
      "Evidence",
      "Notes",
    ],
    requiredKeys: [
      "es.values.primitives",
      "es.values.bigint",
      "es.values.symbol",
      "es.values.object_identity",
      "es.coercion",
      "es.scope.lexical",
      "es.scope.hoist_tdz",
      "es.functions.calls",
      "es.functions.this_bind",
      "es.functions.construct",
      "es.classes",
      "es.objects.properties",
      "es.objects.prototype",
      "es.objects.destructuring",
      "es.objects.spread_rest",
      "es.arrays",
      "es.typed_arrays",
      "es.control.block_if_switch",
      "es.control.loops_labels",
      "es.control.try_finally",
      "es.iteration",
      "es.generators",
      "es.async.promises",
      "es.async.async_await",
      "es.async.async_iteration",
      "es.modules",
      "es.regexp",
      "es.date",
      "es.json",
      "es.error",
      "es.map_set",
      "es.intl",
      "es.proxy_reflect",
      "es.eval_dynamic",
    ],
  },
  "aot-emission": {
    path: "docs/specs/AOT_EMISSION_LEDGER.md",
    columns: [
      "Key",
      "Area",
      "Contract Status",
      "Go Status",
      "Diagnostic",
      "Evidence",
      "Notes",
    ],
    requiredKeys: [
      "aot.entry.module",
      "aot.module.registry",
      "aot.module.init_order",
      "aot.function.decl",
      "aot.function.call",
      "aot.scope.lexical_slots",
      "aot.scope.captured_slots",
      "aot.control.if_return",
      "aot.control.loops",
      "aot.expr.numeric",
      "aot.expr.boolean",
      "aot.property.static",
      "aot.property.dynamic",
      "aot.value.model",
      "aot.node.builtins",
      "aot.async.promise_timer",
      "aot.diagnostics.fail_closed",
      "aot.no_ir_json_interpreter",
      "aot.holdout.parity",
      "aot.benchmarks",
    ],
  },
};

if (!ledgerName || !LEDGERS[ledgerName]) {
  fail("unknown-ledger", {
    usage:
      "node scripts/check-ledger.mjs <node-lts|tsdown-artifact|ecmascript|aot-emission> [--final]",
    received: ledgerName ?? null,
  });
}

const config = LEDGERS[ledgerName];
const markdownPath = path.join(repoRoot, config.path);
const markdown = fs.readFileSync(markdownPath, "utf8");
const rows = parseRows(markdown, config.columns);
const findings = validateRows(rows, config);

if (finalMode) {
  for (const row of rows) {
    if (row["Contract Status"] === "TODO" || row["Contract Status"] === "WIP") {
      findings.push({
        code: "FINAL_CONTRACT_INCOMPLETE",
        key: row.Key,
        status: row["Contract Status"],
      });
    }
    if (row["Go Status"] === "TODO" || row["Go Status"] === "WIP") {
      findings.push({
        code: "FINAL_GO_INCOMPLETE",
        key: row.Key,
        status: row["Go Status"],
      });
    }
  }
}

const summary = {
  version: "ledger-check.v1",
  ledger: ledgerName,
  finalMode,
  status: findings.length === 0 ? "passed" : "failed",
  rows: rows.length,
  counts: countStatuses(rows),
  findings,
};

console.log(JSON.stringify(summary, null, 2));

if (findings.length > 0) {
  process.exit(1);
}

function parseRows(markdown, columns) {
  const expectedHeader = `| ${columns.join(" | ")} |`;
  const lines = markdown.split("\n").map((line) => line.trim());
  const headerIndex = lines.findIndex((line) => line === expectedHeader);
  if (headerIndex === -1) {
    fail("missing-header", { expectedHeader });
  }

  const rows = [];
  for (const line of lines.slice(headerIndex + 2)) {
    if (!line.startsWith("|")) {
      break;
    }
    const cells = line
      .split("|")
      .slice(1, -1)
      .map((cell) => cell.trim());
    if (cells.length !== columns.length) {
      fail("bad-row-width", {
        line,
        expected: columns.length,
        actual: cells.length,
      });
    }
    rows.push(
      Object.fromEntries(
        columns.map((column, index) => [column, cells[index]]),
      ),
    );
  }
  return rows;
}

function validateRows(rows, config) {
  const findings = [];
  const seen = new Set();
  for (const row of rows) {
    if (!row.Key) {
      findings.push({ code: "EMPTY_KEY", row });
      continue;
    }
    if (seen.has(row.Key)) {
      findings.push({ code: "DUPLICATE_KEY", key: row.Key });
    }
    seen.add(row.Key);
    for (const field of config.columns) {
      if (!row[field]) {
        findings.push({ code: "EMPTY_FIELD", key: row.Key, field });
      }
    }
    for (const field of ["Contract Status", "Go Status"]) {
      if (!STATUSES.has(row[field])) {
        findings.push({
          code: "INVALID_STATUS",
          key: row.Key,
          field,
          status: row[field],
        });
      }
    }
    if (row.Diagnostic === "-" || row.Diagnostic === "planned") {
      findings.push({ code: "MISSING_DIAGNOSTIC_CODE", key: row.Key });
    }
  }

  const required = new Set(config.requiredKeys);
  for (const key of required) {
    if (!seen.has(key)) {
      findings.push({ code: "MISSING_REQUIRED_KEY", key });
    }
  }
  for (const key of seen) {
    if (!required.has(key)) {
      findings.push({ code: "UNEXPECTED_KEY", key });
    }
  }
  return findings;
}

function countStatuses(rows) {
  const counts = {};
  for (const row of rows) {
    const contract = row["Contract Status"];
    const go = row["Go Status"];
    counts[`contract:${contract}`] = (counts[`contract:${contract}`] ?? 0) + 1;
    counts[`go:${go}`] = (counts[`go:${go}`] ?? 0) + 1;
  }
  return counts;
}

function fail(code, details) {
  console.error(
    JSON.stringify(
      {
        version: "ledger-check.v1",
        status: "failed",
        code,
        ...details,
      },
      null,
      2,
    ),
  );
  process.exit(1);
}
