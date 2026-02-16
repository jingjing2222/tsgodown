import fs from "node:fs";
import path from "node:path";
import type { ProgramIR, RouteIR } from "@tsgodown/ir-core";

function routeHandlerName(index: number): string {
  return `route${index}`;
}

function toGoHttpMethod(method: RouteIR["method"]): string {
  switch (method) {
    case "GET":
      return "http.MethodGet";
    case "POST":
      return "http.MethodPost";
    case "PUT":
      return "http.MethodPut";
    case "DELETE":
      return "http.MethodDelete";
    case "PATCH":
      return "http.MethodPatch";
    default:
      return JSON.stringify(method);
  }
}

function quoteGo(value: string): string {
  return JSON.stringify(value);
}

function emitRoute(route: RouteIR, index: number): string[] {
  const methodRef = toGoHttpMethod(route.method);
  const todoMessage = `TODO implement handler ${route.handlerRef} for ${route.method} ${route.path}`;

  return [
    `func ${routeHandlerName(index)}(w http.ResponseWriter, req *http.Request) {`,
    `\tif req.Method != ${methodRef} {`,
    `\t\tw.Header().Set("Allow", ${methodRef})`,
    '\t\thttp.Error(w, "method not allowed", http.StatusMethodNotAllowed)',
    "\t\treturn",
    "\t}",
    "",
    "\t// Route metadata:",
    `\t//   Method: ${route.method}`,
    `\t//   Path: ${quoteGo(route.path)}`,
    `\t//   Handler: ${quoteGo(route.handlerRef)}`,
    `\t// TODO(tsgodown): Implement handler ${quoteGo(route.handlerRef)} for ${route.method} ${route.path}.`,
    "\t//   - Replace this scaffold with application logic.",
    "\t//   - Validate request input and map to domain arguments.",
    "\t//   - Write response status, headers, and body.",
    '\tw.Header().Set("Content-Type", "text/plain; charset=utf-8")',
    "\tw.WriteHeader(http.StatusNotImplemented)",
    `\tfmt.Fprintln(w, ${quoteGo(todoMessage)})`,
    "}",
    "",
  ];
}

export function emitGoProject(ir: ProgramIR, outDir: string) {
  fs.mkdirSync(outDir, { recursive: true });
  const lines: string[] = [];

  lines.push("package main", "");
  lines.push("import (", '\t"fmt"', '\t"net/http"', ")", "");

  lines.push("func registerRoutes(mux *http.ServeMux) {");
  for (const [index, route] of ir.routes.entries()) {
    lines.push(
      `\tmux.HandleFunc(${quoteGo(route.path)}, ${routeHandlerName(index)})`,
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

  for (const [index, route] of ir.routes.entries()) {
    lines.push(...emitRoute(route, index));
  }

  fs.writeFileSync(path.join(outDir, "main.go"), `${lines.join("\n")}`);
}
