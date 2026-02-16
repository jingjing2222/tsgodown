import type {
  CapabilityKey,
  CapabilityRequirement,
  CapabilitySource,
  ProgramIRLike,
} from "./types.js";

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
