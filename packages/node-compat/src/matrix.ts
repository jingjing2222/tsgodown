import {
  CAPABILITY_KEYS,
  type CapabilityKey,
  type CapabilityRule,
  CapabilityStatus,
} from "./types.js";

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

export { CAPABILITY_KEYS, CapabilityStatus };
