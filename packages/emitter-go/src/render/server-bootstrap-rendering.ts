import type { RouteIR } from "@tsgodown/ir-core";

import { renderRouteRegistration } from "./template-rendering";

export function renderGoImports(): string[] {
  return [
    "import (",
    '\t"fmt"',
    '\t"net/http"',
    '\t"os"',
    '\t"strings"',
    '\t"time"',
    ")",
    "",
  ];
}

export function renderRouteRegistry(routes: RouteIR[]): string[] {
  const lines = ["func registerRoutes(mux *http.ServeMux) {"];
  for (const [index, route] of routes.entries()) {
    lines.push(renderRouteRegistration(route, index));
  }
  lines.push("}", "");
  return lines;
}

export function renderResolveListenAddr(): string[] {
  return [
    "func resolveListenAddr() string {",
    '\tif addr := strings.TrimSpace(os.Getenv("TSGODOWN_ADDR")); addr != "" {',
    "\t\treturn addr",
    "\t}",
    '\tif port := strings.TrimSpace(os.Getenv("PORT")); port != "" {',
    '\t\tif strings.Contains(port, ":") {',
    "\t\t\treturn port",
    "\t\t}",
    '\t\treturn ":" + port',
    "\t}",
    '\treturn ":18081"',
    "}",
    "",
  ];
}

export function renderMainFunction(): string[] {
  return [
    "func main() {",
    "\tmux := http.NewServeMux()",
    "\tregisterRoutes(mux)",
    "\taddr := resolveListenAddr()",
    '\tfmt.Println("tsgodown scaffold listening on", addr)',
    "\tserver := &http.Server{",
    "\t\tAddr:              addr,",
    "\t\tHandler:           mux,",
    "\t\tReadHeaderTimeout: 5 * time.Second,",
    "\t}",
    "\tif err := server.ListenAndServe(); err != nil && err != http.ErrServerClosed {",
    '\t\tfmt.Println("server exited:", err)',
    "\t}",
    "}",
    "",
  ];
}
