import fs from "node:fs";
import path from "node:path";
import type { HandlerIR, ProgramIR, RouteIR } from "@tsgodown/ir-core";

function routeHandlerName(index: number): string {
  return `route${index}`;
}

function quoteGo(value: string): string {
  return JSON.stringify(value);
}

function toServeMuxPath(pathname: string): string {
  return pathname.replaceAll(/:([A-Za-z_][A-Za-z0-9_]*)/g, "{$1}");
}

function toServeMuxPattern(route: RouteIR): string {
  return `${route.method} ${toServeMuxPath(route.path)}`;
}

function extractPathParamNames(pathname: string): string[] {
  const names: string[] = [];
  const seen = new Set<string>();

  for (const match of pathname.matchAll(/:([A-Za-z_][A-Za-z0-9_]*)/g)) {
    const name = match[1];
    if (!seen.has(name)) {
      seen.add(name);
      names.push(name);
    }
  }

  return names;
}

function emitPathParams(route: RouteIR): string[] {
  const names = extractPathParamNames(route.path);
  if (names.length === 0) {
    return [];
  }

  const lines = ["\t// Extracted path params:"];
  for (const name of names) {
    lines.push(`\t${name} := req.PathValue(${quoteGo(name)})`);
    lines.push(`\t_ = ${name}`);
  }
  lines.push("");

  return lines;
}

function formatHandlerParams(handler: HandlerIR | undefined): string {
  if (!handler || handler.params.length === 0) return "none";
  return handler.params.map((p) => `${p.role}:${p.name}`).join(", ");
}

function emitRoute(
  route: RouteIR,
  index: number,
  handler: HandlerIR | undefined,
): string[] {
  const todoMessage = `TODO implement handler ${route.handlerRef} for ${route.method} ${route.path}`;

  const lines: string[] = [
    `func ${routeHandlerName(index)}(w http.ResponseWriter, req *http.Request) {`,
    "",
    "\t// Route metadata:",
    `\t//   Method: ${route.method}`,
    `\t//   Path: ${quoteGo(route.path)}`,
    `\t//   Pattern: ${quoteGo(toServeMuxPattern(route))}`,
    `\t//   Handler: ${quoteGo(route.handlerRef)}`,
    `\t//   Handler params: ${formatHandlerParams(handler)}`,
    `\t//   Handler async: ${handler?.async ?? "unknown"}`,
    `\t//   Handler response mode: ${handler?.semantics?.responseMode ?? "unknown"}`,
  ];

  if ((route.middlewareRefs?.length ?? 0) > 0) {
    lines.push(`\t//   Middleware: ${JSON.stringify(route.middlewareRefs)}`);
  }

  lines.push(
    `\t// TODO(tsgodown): Implement handler ${quoteGo(route.handlerRef)} for ${route.method} ${route.path}.`,
    "\t//   - Replace this scaffold with application logic.",
    "\t//   - Validate request input and map to domain arguments.",
    "\t//   - Write response status, headers, and body.",
    "",
    ...emitPathParams(route),
    '\tw.Header().Set("Content-Type", "text/plain; charset=utf-8")',
    "\tw.WriteHeader(http.StatusNotImplemented)",
    `\tfmt.Fprintln(w, ${quoteGo(todoMessage)})`,
    "}",
    "",
  );

  return lines;
}

export function emitGoProject(ir: ProgramIR, outDir: string) {
  fs.mkdirSync(outDir, { recursive: true });
  const lines: string[] = [];

  lines.push("package main", "");
  lines.push("import (", '\t"fmt"', '\t"net/http"', ")", "");

  lines.push("func registerRoutes(mux *http.ServeMux) {");
  for (const [index, route] of ir.routes.entries()) {
    lines.push(
      `\tmux.HandleFunc(${quoteGo(toServeMuxPattern(route))}, ${routeHandlerName(index)})`,
    );
  }
  lines.push("}", "");

  lines.push("func main() {");
  lines.push("\tmux := http.NewServeMux()");
  lines.push("\tregisterRoutes(mux)");
  lines.push('\tfmt.Println("tsgodown scaffold listening on :18081")');
  lines.push('\tif err := http.ListenAndServe(":18081", mux); err != nil {');
  lines.push('\t\tfmt.Println("server exited:", err)');
  lines.push("\t}");
  lines.push("}", "");

  const handlerById = new Map(
    ir.handlers.map((handler) => [handler.id, handler]),
  );

  for (const [index, route] of ir.routes.entries()) {
    lines.push(...emitRoute(route, index, handlerById.get(route.handlerRef)));
  }

  fs.writeFileSync(path.join(outDir, "main.go"), `${lines.join("\n")}`);
}
