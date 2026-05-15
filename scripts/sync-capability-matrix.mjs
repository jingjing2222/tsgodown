#!/usr/bin/env node

import { spawnSync } from "node:child_process";
import fs from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";

const repoRoot = path.resolve(
  path.dirname(fileURLToPath(import.meta.url)),
  "..",
);
const checkOnly = process.argv.includes("--check");

const ledgerConfigs = [
  {
    kind: "node-lts",
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
    scope: "node api",
  },
  {
    kind: "tsdown-artifact",
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
    scope: "tsdown artifact",
  },
  {
    kind: "ecmascript",
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
    scope: "language",
  },
];

const legacyRows = [
  {
    key: "route.basic",
    scope: "HTTP route",
    status: "WIP",
    strategy: "legacy route compatibility alias",
    goStatus: "WIP",
    goStrategy: "legacy route compatibility alias",
  },
  {
    key: "handler.async",
    scope: "control-flow",
    status: "TODO",
    strategy: "legacy async handler alias; superseded by es.async.* ledgers",
    goStatus: "TODO",
    goStrategy: "legacy async handler alias; superseded by es.async.* ledgers",
  },
  {
    key: "module.esm",
    scope: "module",
    status: "WIP",
    strategy: "legacy ESM alias; superseded by node.module_esm and es.modules",
    goStatus: "WIP",
    goStrategy:
      "legacy ESM alias; superseded by node.module_esm and es.modules",
  },
  {
    key: "module.cjs",
    scope: "module",
    status: "TODO",
    strategy: "legacy CJS alias; superseded by node.module_cjs",
    goStatus: "TODO",
    goStrategy: "legacy CJS alias; superseded by node.module_cjs",
  },
  {
    key: "runtime.event_loop",
    scope: "runtime",
    status: "TODO",
    strategy:
      "legacy event-loop alias; superseded by es.async.* and node.timers",
    goStatus: "TODO",
    goStrategy:
      "legacy event-loop alias; superseded by es.async.* and node.timers",
  },
  {
    key: "node.fs.basic",
    scope: "node api",
    status: "TODO",
    strategy: "legacy fs alias; superseded by node.fs",
    goStatus: "TODO",
    goStrategy: "legacy fs alias; superseded by node.fs",
  },
  {
    key: "node.path.basic",
    scope: "node api",
    status: "WIP",
    strategy: "legacy path alias; superseded by node.path",
    goStatus: "WIP",
    goStrategy: "legacy path alias; superseded by node.path",
  },
  {
    key: "node.url.basic",
    scope: "node api",
    status: "WIP",
    strategy: "legacy url alias; superseded by node.url",
    goStatus: "WIP",
    goStrategy: "legacy url alias; superseded by node.url",
  },
  {
    key: "node.process.env",
    scope: "node api",
    status: "TODO",
    strategy:
      "legacy process.env alias; superseded by node.process and node.env_vars",
    goStatus: "TODO",
    goStrategy:
      "legacy process.env alias; superseded by node.process and node.env_vars",
  },
  {
    key: "node.buffer.basic",
    scope: "node api",
    status: "TODO",
    strategy: "legacy buffer alias; superseded by node.buffer",
    goStatus: "TODO",
    goStrategy: "legacy buffer alias; superseded by node.buffer",
  },
];

const ledgerRows = ledgerConfigs.flatMap((config) =>
  parseRows(read(config.path), config).map((row) => {
    const area = row.Area ?? row["Artifact Shape"];
    return {
      key: row.Key,
      scope: config.scope,
      status: row["Contract Status"],
      strategy: `${config.kind}: ${area}`,
      goStatus: row["Go Status"],
      goStrategy: row.Notes,
    };
  }),
);

const rows = [...legacyRows, ...ledgerRows];
const keys = rows.map((row) => row.key);
if (new Set(keys).size !== keys.length) {
  const duplicates = keys.filter((key, index) => keys.indexOf(key) !== index);
  fail("duplicate capability keys", { duplicates });
}

const outputs = new Map(
  [
    ["packages/node-compat/src/types.ts", renderTypes(keys)],
    ["packages/node-compat/src/matrix.ts", renderMatrix(rows)],
    ["docs/specs/CAPABILITY_MATRIX.md", renderDocs(rows)],
  ].map(([relativePath, contents]) => [
    relativePath,
    formatGenerated(relativePath, contents),
  ]),
);

const findings = [];
for (const [relativePath, contents] of outputs) {
  const absolutePath = path.join(repoRoot, relativePath);
  if (checkOnly) {
    const current = fs.existsSync(absolutePath)
      ? fs.readFileSync(absolutePath, "utf8")
      : "";
    if (current !== contents) {
      findings.push({ file: relativePath, code: "OUT_OF_SYNC" });
    }
  } else {
    fs.writeFileSync(absolutePath, contents);
  }
}

if (findings.length > 0) {
  fail("capability matrix out of sync", { findings });
}

console.log(
  JSON.stringify(
    {
      version: "capability-ledger-sync.v1",
      status: "passed",
      mode: checkOnly ? "check" : "write",
      rows: rows.length,
      outputs: [...outputs.keys()],
    },
    null,
    2,
  ),
);

function parseRows(markdown, config) {
  const expectedHeader = `| ${config.columns.join(" | ")} |`;
  const lines = markdown.split("\n").map((line) => line.trim());
  const headerIndex = lines.findIndex((line) => line === expectedHeader);
  if (headerIndex === -1) {
    fail("missing ledger table", { file: config.path, expectedHeader });
  }
  const rows = [];
  for (const line of lines.slice(headerIndex + 2)) {
    if (!line.startsWith("|")) break;
    const cells = line
      .split("|")
      .slice(1, -1)
      .map((cell) => cell.trim());
    rows.push(
      Object.fromEntries(
        config.columns.map((column, index) => [column, cells[index]]),
      ),
    );
  }
  return rows;
}

function renderTypes(keys) {
  return `import type { ProgramIR as CoreProgramIR } from "@tsgodown/ir-core";

export enum CapabilityStatus {
  TODO = "TODO",
  WIP = "WIP",
  DONE = "DONE",
  FAIL_CLOSED = "FAIL_CLOSED",
  BLOCKED = "BLOCKED",
}

export const CAPABILITY_KEYS = ${JSON.stringify(keys, null, 2)} as const;

export const CAPABILITY_BACKENDS = ["go", "rust", "cpp"] as const;

export type CapabilityKey = (typeof CAPABILITY_KEYS)[number];
export type CapabilityBackend = (typeof CAPABILITY_BACKENDS)[number];

export interface CapabilityBackendRule {
  status: CapabilityStatus;
  strategy: string;
}

export interface CapabilityRule {
  key: CapabilityKey;
  scope: string;
  status: CapabilityStatus;
  strategy: string;
  backends: Record<CapabilityBackend, CapabilityBackendRule>;
}

export interface CapabilitySource {
  file: string;
  line?: number;
  column?: number;
  viaSourceMap?: boolean;
}

export interface CapabilityRequirement {
  capability: CapabilityKey;
  reason: string;
  source?: CapabilitySource;
}

export interface CapabilityDiagnostic {
  level: "error";
  code: "CAPABILITY_UNMET";
  message: string;
  capability: CapabilityKey;
  status: CapabilityStatus;
  backend: CapabilityBackend;
  source?: CapabilitySource;
  cause?: string;
  guidance?: string;
}

export interface CapabilityCheckOptions {
  allowWip?: boolean;
  failFast?: boolean;
  targetBackend?: CapabilityBackend;
}

export interface CapabilityCheckResult {
  ok: boolean;
  required: CapabilityRequirement[];
  diagnostics: CapabilityDiagnostic[];
}

export type ProgramIRLike = CoreProgramIR | Record<string, unknown>;
`;
}

function renderMatrix(rows) {
  const entries = rows
    .map(
      (row) => `  ${JSON.stringify(row.key)}: {
    key: ${JSON.stringify(row.key)},
    scope: ${JSON.stringify(row.scope)},
    status: CapabilityStatus.${row.status},
    strategy: ${JSON.stringify(row.strategy)},
    backends: backendRules({
      status: CapabilityStatus.${row.goStatus},
      strategy: ${JSON.stringify(row.goStrategy)},
    }),
  },`,
    )
    .join("\n");
  return `import {
  CAPABILITY_BACKENDS,
  CAPABILITY_KEYS,
  type CapabilityBackend,
  type CapabilityBackendRule,
  type CapabilityKey,
  type CapabilityRule,
  CapabilityStatus,
} from "./types.js";

function backendRules(
  go: CapabilityBackendRule,
  overrides: Partial<Record<CapabilityBackend, CapabilityBackendRule>> = {},
): Record<CapabilityBackend, CapabilityBackendRule> {
  const defaults = Object.fromEntries(
    CAPABILITY_BACKENDS.map((backend) => [
      backend,
      { status: CapabilityStatus.TODO, strategy: "backend not implemented" },
    ]),
  ) as Record<CapabilityBackend, CapabilityBackendRule>;
  return {
    ...defaults,
    go,
    ...overrides,
  };
}

/**
 * Generated from docs/specs/* ledger files by scripts/sync-capability-matrix.mjs.
 * Do not edit this table manually.
 */
export const CAPABILITY_MATRIX: Record<CapabilityKey, CapabilityRule> = {
${entries}
};

if (Object.keys(CAPABILITY_MATRIX).join("\\n") !== CAPABILITY_KEYS.join("\\n")) {
  throw new Error("CAPABILITY_MATRIX keys are out of sync with CAPABILITY_KEYS");
}

export { CAPABILITY_BACKENDS, CAPABILITY_KEYS, CapabilityStatus };
`;
}

function renderDocs(rows) {
  const lines = [
    "# Capability Matrix (Generated)",
    "",
    "Generated from:",
    "",
    "- `docs/specs/NODE_LTS_COVERAGE_LEDGER.md`",
    "- `docs/specs/TSDOWN_ARTIFACT_CONTRACT.md`",
    "- `docs/specs/ECMASCRIPT_SEMANTICS_LEDGER.md`",
    "",
    "Do not edit this file manually. Run:",
    "",
    "```bash",
    "node scripts/sync-capability-matrix.mjs",
    "```",
    "",
    "| Capability Key | Scope | Contract Status | Contract Strategy | Go Status | Go Strategy | Rust Status | Rust Strategy | C++ Status | C++ Strategy |",
    "|---|---|---|---|---|---|---|---|---|---|",
  ];
  for (const row of rows) {
    lines.push(
      `| ${row.key} | ${row.scope} | ${row.status} | ${escapeCell(
        row.strategy,
      )} | ${row.goStatus} | ${escapeCell(
        row.goStrategy,
      )} | TODO | backend not implemented | TODO | backend not implemented |`,
    );
  }
  lines.push(
    "",
    "## Decision rules",
    "- If required capabilities for an IR node are not `DONE|WIP(allow)`, compilation fails.",
    "- On failure, include source-map-based original location in diagnostics.",
    "- Capability decision execution is based on the Rust core runtime path.",
    "- Do not bypass Rust path failure with TS analyzer fallback (no fallback).",
    "- `node-compat` capability checker defaults: `allowWip=true`, `failFast=true`, `targetBackend=go`.",
    "- IR capability keys and runtime helper contracts stay backend-neutral; backend-specific lowering status lives in backend columns.",
    "",
  );
  return `${lines.join("\n")}`;
}

function escapeCell(value) {
  return String(value).replaceAll("|", "\\|");
}

function formatGenerated(relativePath, contents) {
  if (!relativePath.endsWith(".ts") && !relativePath.endsWith(".mjs")) {
    return contents;
  }
  const result = spawnSync(
    "pnpm",
    ["exec", "biome", "format", "--stdin-file-path", relativePath],
    {
      cwd: repoRoot,
      input: contents,
      encoding: "utf8",
    },
  );
  if (result.status !== 0) {
    fail("failed to format generated source", {
      file: relativePath,
      stderr: result.stderr,
      stdout: result.stdout,
    });
  }
  return result.stdout;
}

function read(relativePath) {
  return fs.readFileSync(path.join(repoRoot, relativePath), "utf8");
}

function fail(message, details) {
  console.error(
    JSON.stringify(
      {
        version: "capability-ledger-sync.v1",
        status: "failed",
        message,
        ...details,
      },
      null,
      2,
    ),
  );
  process.exit(1);
}
