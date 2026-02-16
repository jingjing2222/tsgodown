import type { ProgramIR as CoreProgramIR } from "@tsgodown/ir-core";

export enum CapabilityStatus {
  TODO = "TODO",
  WIP = "WIP",
  DONE = "DONE",
  BLOCKED = "BLOCKED",
}

export const CAPABILITY_KEYS = [
  "route.basic",
  "handler.async",
  "module.esm",
  "module.cjs",
  "runtime.event_loop",
  "node.fs.basic",
  "node.path.basic",
  "node.url.basic",
  "node.process.env",
  "node.buffer.basic",
] as const;

export type CapabilityKey = (typeof CAPABILITY_KEYS)[number];

export interface CapabilityRule {
  key: CapabilityKey;
  scope: string;
  status: CapabilityStatus;
  strategy: string;
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
  source?: CapabilitySource;
  cause?: string;
  guidance?: string;
}

export interface CapabilityCheckOptions {
  allowWip?: boolean;
  failFast?: boolean;
}

export interface CapabilityCheckResult {
  ok: boolean;
  required: CapabilityRequirement[];
  diagnostics: CapabilityDiagnostic[];
}

/**
 * SSoT mirror of docs/specs/CAPABILITY_MATRIX.md
 */
export const CAPABILITY_MATRIX: Record<CapabilityKey, CapabilityRule> = {
  "route.basic": {
    key: "route.basic",
    scope: "HTTP route",
    status: CapabilityStatus.WIP,
    strategy: "direct mapping",
  },
  "handler.async": {
    key: "handler.async",
    scope: "control-flow",
    status: CapabilityStatus.TODO,
    strategy: "goroutine + await shim",
  },
  "module.esm": {
    key: "module.esm",
    scope: "module",
    status: CapabilityStatus.WIP,
    strategy: "static link graph",
  },
  "module.cjs": {
    key: "module.cjs",
    scope: "module",
    status: CapabilityStatus.TODO,
    strategy: "cjs bridge",
  },
  "runtime.event_loop": {
    key: "runtime.event_loop",
    scope: "runtime",
    status: CapabilityStatus.TODO,
    strategy: "scheduler shim",
  },
  "node.fs.basic": {
    key: "node.fs.basic",
    scope: "node api",
    status: CapabilityStatus.TODO,
    strategy: "os/io adapter",
  },
  "node.path.basic": {
    key: "node.path.basic",
    scope: "node api",
    status: CapabilityStatus.WIP,
    strategy: "filepath adapter (join/resolve/dirname/basename)",
  },
  "node.url.basic": {
    key: "node.url.basic",
    scope: "node api",
    status: CapabilityStatus.WIP,
    strategy: "net/url adapter (URL + URLSearchParams)",
  },
  "node.process.env": {
    key: "node.process.env",
    scope: "node api",
    status: CapabilityStatus.TODO,
    strategy: "runtime env map",
  },
  "node.buffer.basic": {
    key: "node.buffer.basic",
    scope: "node api",
    status: CapabilityStatus.TODO,
    strategy: "[]byte wrapper",
  },
};

type ProgramIRLike = CoreProgramIR | Record<string, unknown>;

const DIAGNOSTIC_CAPABILITY_MAP: Record<string, CapabilityKey> = {
  NODE_FS_BASIC_REQUIRED: "node.fs.basic",
  NODE_PATH_BASIC_REQUIRED: "node.path.basic",
  NODE_URL_BASIC_REQUIRED: "node.url.basic",
  NODE_PROCESS_ENV_REQUIRED: "node.process.env",
  NODE_BUFFER_BASIC_REQUIRED: "node.buffer.basic",
  RUNTIME_EVENT_LOOP_REQUIRED: "runtime.event_loop",
};

function sourceFromUnknown(value: unknown): CapabilitySource | undefined {
  if (!value || typeof value !== "object") return undefined;
  const v = value as Record<string, unknown>;
  const file = typeof v.file === "string" ? v.file : undefined;
  if (!file) return undefined;
  return {
    file,
    line: typeof v.line === "number" ? v.line : undefined,
    column: typeof v.column === "number" ? v.column : undefined,
    viaSourceMap:
      typeof v.viaSourceMap === "boolean" ? v.viaSourceMap : undefined,
  };
}

function pushUnique(
  out: CapabilityRequirement[],
  seen: Set<string>,
  requirement: CapabilityRequirement,
) {
  const key = `${requirement.capability}::${requirement.reason}::${requirement.source?.file ?? ""}::${requirement.source?.line ?? ""}::${requirement.source?.column ?? ""}`;
  if (seen.has(key)) return;
  seen.add(key);
  out.push(requirement);
}

/**
 * Minimal feature extraction from ProgramIR:
 * - routes -> route.basic
 * - modules.imports.kind(esm/cjs) -> module.esm/module.cjs
 * - handlers.async -> handler.async
 * - diagnostics.code -> node/runtime feature requirements
 */
export function collectRequiredCapabilities(
  ir: ProgramIRLike,
): CapabilityRequirement[] {
  const required: CapabilityRequirement[] = [];
  const seen = new Set<string>();

  const rec = ir as Record<string, unknown>;

  const routes = Array.isArray(rec.routes) ? rec.routes : [];
  if (routes.length > 0) {
    pushUnique(required, seen, {
      capability: "route.basic",
      reason: "ProgramIR.routes is non-empty",
      source: sourceFromUnknown((routes[0] as Record<string, unknown>)?.source),
    });
  }

  const handlers = Array.isArray(rec.handlers) ? rec.handlers : [];
  for (const h of handlers) {
    const hr = h as Record<string, unknown>;
    if (hr.async === true) {
      pushUnique(required, seen, {
        capability: "handler.async",
        reason: "HandlerIR.async is true",
        source: sourceFromUnknown(hr.source),
      });
    }
  }

  const modules = Array.isArray(rec.modules) ? rec.modules : [];
  for (const m of modules) {
    const mr = m as Record<string, unknown>;
    const imports = Array.isArray(mr.imports) ? mr.imports : [];
    for (const i of imports) {
      const imp = i as Record<string, unknown>;
      if (imp.kind === "esm") {
        pushUnique(required, seen, {
          capability: "module.esm",
          reason: "ModuleIR.imports includes kind='esm'",
          source: sourceFromUnknown(mr.source),
        });
      }
      if (imp.kind === "cjs") {
        pushUnique(required, seen, {
          capability: "module.cjs",
          reason: "ModuleIR.imports includes kind='cjs'",
          source: sourceFromUnknown(mr.source),
        });
      }
    }
  }

  const diagnostics = Array.isArray(rec.diagnostics) ? rec.diagnostics : [];
  for (const d of diagnostics) {
    const dr = d as Record<string, unknown>;
    const code = typeof dr.code === "string" ? dr.code : "";
    const mapped = DIAGNOSTIC_CAPABILITY_MAP[code];
    if (!mapped) continue;
    const detail =
      typeof dr.message === "string" && dr.message.length > 0
        ? ` (${dr.message})`
        : "";
    pushUnique(required, seen, {
      capability: mapped,
      reason: `Analyzer diagnostic ${code}${detail}`,
      source: sourceFromUnknown(dr.source),
    });
  }

  return required;
}

function isSupportedStatus(
  status: CapabilityStatus,
  allowWip: boolean,
): boolean {
  if (status === CapabilityStatus.DONE) return true;
  if (status === CapabilityStatus.WIP && allowWip) return true;
  return false;
}

function formatSource(source?: CapabilitySource): string {
  if (!source) return "unknown";
  const line = source.line ?? "?";
  const column = source.column ?? "?";
  const suffix = source.viaSourceMap ? " (via source map)" : "";
  return `${source.file}:${line}:${column}${suffix}`;
}

function buildGuidance(rule: CapabilityRule): string {
  return `Update source to avoid '${rule.key}' usage, or implement ${rule.scope} strategy '${rule.strategy}'. See docs/specs/CAPABILITY_MATRIX.md.`;
}

export function checkCapabilities(
  ir: ProgramIRLike,
  options: CapabilityCheckOptions = {},
): CapabilityCheckResult {
  const allowWip = options.allowWip ?? true;
  const failFast = options.failFast ?? true;

  const required = collectRequiredCapabilities(ir);
  const diagnostics: CapabilityDiagnostic[] = [];

  for (const req of required) {
    const rule = CAPABILITY_MATRIX[req.capability];
    if (isSupportedStatus(rule.status, allowWip)) continue;

    const cause = `Capability status is ${rule.status} for required '${req.capability}' (${req.reason}).`;
    const guidance = buildGuidance(rule);
    const sourceLocation = formatSource(req.source);

    diagnostics.push({
      level: "error",
      code: "CAPABILITY_UNMET",
      message:
        `Capability '${req.capability}' is not supported (status=${rule.status}) at ${sourceLocation}. ` +
        `Cause: ${cause} Guidance: ${guidance}`,
      capability: req.capability,
      status: rule.status,
      source: req.source,
      cause,
      guidance,
    });

    if (failFast) break;
  }

  return {
    ok: diagnostics.length === 0,
    required,
    diagnostics,
  };
}
