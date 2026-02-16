import type { ProgramIR as CoreProgramIR } from "@tsgodown/ir-core";
export declare enum CapabilityStatus {
    TODO = "TODO",
    WIP = "WIP",
    DONE = "DONE",
    BLOCKED = "BLOCKED"
}
export type CapabilityKey = "route.basic" | "handler.async" | "module.esm" | "module.cjs" | "runtime.event_loop" | "node.fs.basic" | "node.path.basic" | "node.url.basic" | "node.process.env" | "node.buffer.basic";
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
export declare const CAPABILITY_MATRIX: Record<CapabilityKey, CapabilityRule>;
type ProgramIRLike = CoreProgramIR | Record<string, unknown>;
/**
 * Minimal feature extraction from ProgramIR:
 * - routes -> route.basic
 * - modules.imports.kind(esm/cjs) -> module.esm/module.cjs
 * - handlers.async -> handler.async
 */
export declare function collectRequiredCapabilities(ir: ProgramIRLike): CapabilityRequirement[];
export declare function checkCapabilities(ir: ProgramIRLike, options?: CapabilityCheckOptions): CapabilityCheckResult;
export {};
