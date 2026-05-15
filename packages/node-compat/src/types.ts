import type { ProgramIR as CoreProgramIR } from "@tsgodown/ir-core";

export enum CapabilityStatus {
  TODO = "TODO",
  WIP = "WIP",
  DONE = "DONE",
  FAIL_CLOSED = "FAIL_CLOSED",
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
] as const;

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
