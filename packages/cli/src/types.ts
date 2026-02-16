export type Command = "build" | "check" | "report" | "stages";

export type TargetDiagnostics = {
  routes: number;
  warnings: string[];
};

export type TargetSummary = {
  configIndex: number;
  entry: string;
  outDir: string;
  artifact: string;
  emitted: boolean;
  diagnostics: TargetDiagnostics;
};

export type CommandResult = {
  cwd: string;
  stages?: readonly string[];
  targets: TargetSummary[];
};

export type StagesResult = {
  cwd: string;
  stages: readonly string[];
  targets: Array<{
    configIndex: number;
    entry: string;
    outDir: string;
    artifact: string;
    emitted: boolean;
  }>;
};
