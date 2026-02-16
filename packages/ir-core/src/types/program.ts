import type { DiagnosticIR } from "./diagnostic.js";
import type { HandlerIR } from "./handler.js";
import type { ModuleIR } from "./module.js";
import type { RouteIR } from "./route.js";

export interface ProgramIR {
  modules: ModuleIR[];
  routes: RouteIR[];
  handlers: HandlerIR[];
  diagnostics: DiagnosticIR[];
}
