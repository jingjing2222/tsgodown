import type { ExecutableModuleIR } from "./executable.js";

export interface ModuleIR {
  id: string;
  sourcePath: string;
  exports: string[];
  imports: ImportIR[];
  executable?: ExecutableModuleIR;
}

export interface ImportIR {
  spec: string;
  kind: "esm" | "cjs" | "dynamic";
  resolved?: string;
  bindings?: ImportBindingIR[];
}

export interface ImportBindingIR {
  local: string;
  imported?: string;
  kind: "default" | "namespace" | "named" | "require" | "destructure";
}
