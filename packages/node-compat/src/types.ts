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
export type CapabilityBackend = "go";

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
