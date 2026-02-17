import fs from "node:fs";
import path from "node:path";
import type { ProgramIR } from "@tsgodown/ir-core";

import {
  renderDiagnosticsComments,
  renderGoImports,
  renderMainFunction,
  renderResolveListenAddr,
  renderRoute,
  renderRouteRegistry,
  renderRuntimeRouter,
} from "./render/index";

export function emitGoProject(ir: ProgramIR, outDir: string) {
  fs.mkdirSync(outDir, { recursive: true });
  const lines: string[] = [];

  lines.push("package main", "");
  lines.push(...renderGoImports());
  lines.push(...renderDiagnosticsComments(ir.diagnostics));
  lines.push(...renderRuntimeRouter());
  lines.push(...renderRouteRegistry(ir.routes));
  lines.push(...renderResolveListenAddr());
  lines.push(...renderMainFunction());

  const handlerById = new Map(
    ir.handlers.map((handler) => [handler.id, handler]),
  );

  for (const [index, route] of ir.routes.entries()) {
    lines.push(...renderRoute(route, index, handlerById.get(route.handlerRef)));
  }

  fs.writeFileSync(path.join(outDir, "main.go"), `${lines.join("\n")}`);
}
