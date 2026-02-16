export var CapabilityStatus;
(function (CapabilityStatus) {
    CapabilityStatus["TODO"] = "TODO";
    CapabilityStatus["WIP"] = "WIP";
    CapabilityStatus["DONE"] = "DONE";
    CapabilityStatus["BLOCKED"] = "BLOCKED";
})(CapabilityStatus || (CapabilityStatus = {}));
/**
 * SSoT mirror of docs/specs/CAPABILITY_MATRIX.md
 */
export const CAPABILITY_MATRIX = {
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
        status: CapabilityStatus.TODO,
        strategy: "filepath adapter",
    },
    "node.url.basic": {
        key: "node.url.basic",
        scope: "node api",
        status: CapabilityStatus.TODO,
        strategy: "net/url adapter",
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
function sourceFromUnknown(value) {
    if (!value || typeof value !== "object")
        return undefined;
    const v = value;
    const file = typeof v.file === "string" ? v.file : undefined;
    if (!file)
        return undefined;
    return {
        file,
        line: typeof v.line === "number" ? v.line : undefined,
        column: typeof v.column === "number" ? v.column : undefined,
        viaSourceMap: typeof v.viaSourceMap === "boolean" ? v.viaSourceMap : undefined,
    };
}
function pushUnique(out, seen, requirement) {
    const key = `${requirement.capability}::${requirement.reason}::${requirement.source?.file ?? ""}::${requirement.source?.line ?? ""}::${requirement.source?.column ?? ""}`;
    if (seen.has(key))
        return;
    seen.add(key);
    out.push(requirement);
}
/**
 * Minimal feature extraction from ProgramIR:
 * - routes -> route.basic
 * - modules.imports.kind(esm/cjs) -> module.esm/module.cjs
 * - handlers.async -> handler.async
 */
export function collectRequiredCapabilities(ir) {
    const required = [];
    const seen = new Set();
    const rec = ir;
    const routes = Array.isArray(rec.routes) ? rec.routes : [];
    if (routes.length > 0) {
        pushUnique(required, seen, {
            capability: "route.basic",
            reason: "ProgramIR.routes is non-empty",
            source: sourceFromUnknown(routes[0]?.source),
        });
    }
    const handlers = Array.isArray(rec.handlers) ? rec.handlers : [];
    for (const h of handlers) {
        const hr = h;
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
        const mr = m;
        const imports = Array.isArray(mr.imports) ? mr.imports : [];
        for (const i of imports) {
            const imp = i;
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
    return required;
}
function isSupportedStatus(status, allowWip) {
    if (status === CapabilityStatus.DONE)
        return true;
    if (status === CapabilityStatus.WIP && allowWip)
        return true;
    return false;
}
export function checkCapabilities(ir, options = {}) {
    const allowWip = options.allowWip ?? true;
    const failFast = options.failFast ?? true;
    const required = collectRequiredCapabilities(ir);
    const diagnostics = [];
    for (const req of required) {
        const rule = CAPABILITY_MATRIX[req.capability];
        if (isSupportedStatus(rule.status, allowWip))
            continue;
        diagnostics.push({
            level: "error",
            code: "CAPABILITY_UNMET",
            message: `Capability '${req.capability}' is required (${req.reason}) but current status is ${rule.status}.`,
            capability: req.capability,
            status: rule.status,
            source: req.source,
        });
        if (failFast)
            break;
    }
    return {
        ok: diagnostics.length === 0,
        required,
        diagnostics,
    };
}
