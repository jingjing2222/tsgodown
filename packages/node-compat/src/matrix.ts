import {
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
 * SSoT mirror of docs/specs/CAPABILITY_MATRIX.md
 */
export const CAPABILITY_MATRIX: Record<CapabilityKey, CapabilityRule> = {
  "route.basic": {
    key: "route.basic",
    scope: "HTTP route",
    status: CapabilityStatus.WIP,
    strategy: "direct mapping",
    backends: backendRules({
      status: CapabilityStatus.WIP,
      strategy: "direct mapping",
    }),
  },
  "handler.async": {
    key: "handler.async",
    scope: "control-flow",
    status: CapabilityStatus.TODO,
    strategy: "goroutine + await shim",
    backends: backendRules({
      status: CapabilityStatus.TODO,
      strategy: "goroutine + await shim",
    }),
  },
  "module.esm": {
    key: "module.esm",
    scope: "module",
    status: CapabilityStatus.WIP,
    strategy: "static link graph",
    backends: backendRules({
      status: CapabilityStatus.WIP,
      strategy: "static link graph",
    }),
  },
  "module.cjs": {
    key: "module.cjs",
    scope: "module",
    status: CapabilityStatus.TODO,
    strategy: "cjs bridge",
    backends: backendRules({
      status: CapabilityStatus.TODO,
      strategy: "cjs bridge",
    }),
  },
  "runtime.event_loop": {
    key: "runtime.event_loop",
    scope: "runtime",
    status: CapabilityStatus.TODO,
    strategy: "scheduler shim",
    backends: backendRules({
      status: CapabilityStatus.TODO,
      strategy: "scheduler shim",
    }),
  },
  "node.fs.basic": {
    key: "node.fs.basic",
    scope: "node api",
    status: CapabilityStatus.TODO,
    strategy: "os/io adapter",
    backends: backendRules({
      status: CapabilityStatus.TODO,
      strategy: "os/io adapter",
    }),
  },
  "node.path.basic": {
    key: "node.path.basic",
    scope: "node api",
    status: CapabilityStatus.WIP,
    strategy: "filepath adapter (join/resolve/dirname/basename)",
    backends: backendRules({
      status: CapabilityStatus.WIP,
      strategy: "filepath adapter (join/resolve/dirname/basename)",
    }),
  },
  "node.url.basic": {
    key: "node.url.basic",
    scope: "node api",
    status: CapabilityStatus.WIP,
    strategy: "net/url adapter (URL + URLSearchParams)",
    backends: backendRules({
      status: CapabilityStatus.WIP,
      strategy: "net/url adapter (URL + URLSearchParams)",
    }),
  },
  "node.process.env": {
    key: "node.process.env",
    scope: "node api",
    status: CapabilityStatus.TODO,
    strategy: "runtime env map",
    backends: backendRules({
      status: CapabilityStatus.TODO,
      strategy: "runtime env map",
    }),
  },
  "node.buffer.basic": {
    key: "node.buffer.basic",
    scope: "node api",
    status: CapabilityStatus.TODO,
    strategy: "[]byte wrapper",
    backends: backendRules({
      status: CapabilityStatus.TODO,
      strategy: "[]byte wrapper",
    }),
  },
};

export { CAPABILITY_BACKENDS, CAPABILITY_KEYS, CapabilityStatus };
