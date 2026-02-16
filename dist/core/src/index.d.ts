export declare const ACTIVE_STAGES: readonly ["load-config", "analyze", "emit", "onSuccess"];
export type BuildStage = (typeof ACTIVE_STAGES)[number];
export interface BuildTargetPlan {
    configIndex: number;
    entry: string;
    outDir: string;
    artifact: string;
}
export interface BuildTargetDiagnostics {
    routes: number;
    warnings: string[];
}
export interface BuildTargetResult extends BuildTargetPlan {
    diagnostics: BuildTargetDiagnostics;
    emitted: boolean;
}
export interface BuildSummary {
    ok: boolean;
    cwd: string;
    command: "build" | "check" | "report";
    stages: readonly BuildStage[];
    targets: BuildTargetResult[];
}
export declare function build(cwd: string): Promise<BuildSummary>;
export declare function check(cwd: string): Promise<BuildSummary>;
export declare function report(cwd: string): Promise<BuildSummary>;
export declare function stages(cwd: string): Promise<Pick<BuildSummary, "cwd" | "stages" | "targets">>;
