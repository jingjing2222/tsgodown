import type { RouteIR } from "./route.js";

export interface ProgramIR {
  routes: RouteIR[];
  warnings: string[];
}
