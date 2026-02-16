export interface DiagnosticIR {
  level: "error" | "warn" | "info";
  code: string;
  message: string;
  source?: {
    file: string;
    line?: number;
    column?: number;
    viaSourceMap?: boolean;
  };
}
