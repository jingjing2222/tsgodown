import fs from "node:fs";
import type { DiagnosticIR, ProgramIR, RouteIR } from "@tsgodown/ir-core";

export function analyzeFastifyEntry(entryFile: string): ProgramIR {
  const src = fs.readFileSync(entryFile, "utf-8");
  const routeRe =
    /fastify\.(get|post|put|delete|patch)\(\s*['\"]([^'\"]+)['\"]\s*,\s*([A-Za-z_][A-Za-z0-9_]*)/g;

  const routes: RouteIR[] = [];
  for (const m of src.matchAll(routeRe)) {
    routes.push({
      method: m[1].toUpperCase() as RouteIR["method"],
      path: m[2],
      handlerRef: m[3],
    });
  }

  const diagnostics: DiagnosticIR[] = [];
  if (src.includes("import(")) {
    diagnostics.push({
      level: "warn",
      code: "DYNAMIC_IMPORT_DETECTED",
      message: "dynamic import detected",
      source: { file: entryFile },
    });
  }

  return {
    modules: [],
    routes,
    handlers: [],
    diagnostics,
  };
}
