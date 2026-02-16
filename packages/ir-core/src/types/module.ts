export interface ModuleIR {
  id: string;
  sourcePath: string;
  exports: string[];
  imports: Array<{ spec: string; kind: "esm" | "cjs"; resolved?: string }>;
}
