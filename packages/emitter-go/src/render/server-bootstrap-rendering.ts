import type { RouteIR } from "@tsgodown/ir-core";

import { renderRouteRegistration } from "./template-rendering";

export function renderGoImports(): string[] {
  return [
    "import (",
    '\t"fmt"',
    '\t"net/http"',
    '\t"os"',
    '\t"sort"',
    '\t"strings"',
    '\t"time"',
    ")",
    "",
  ];
}

export function renderRuntimeRouter(): string[] {
  return [
    "type runtimeRouter struct {",
    "\tmethodMux *http.ServeMux",
    "\tpathMux *http.ServeMux",
    "\tmethodsByPathPattern map[string]map[string]struct{}",
    "\tpathPatternRegistered map[string]struct{}",
    "}",
    "",
    "func newRuntimeRouter() *runtimeRouter {",
    "\treturn &runtimeRouter{",
    "\t\tmethodMux:             http.NewServeMux(),",
    "\t\tpathMux:               http.NewServeMux(),",
    "\t\tmethodsByPathPattern:  map[string]map[string]struct{}{},",
    "\t\tpathPatternRegistered: map[string]struct{}{},",
    "\t}",
    "}",
    "",
    "func (r *runtimeRouter) handle(method string, pathPattern string, handler http.HandlerFunc) {",
    '\tr.methodMux.HandleFunc(method+" "+pathPattern, handler)',
    "\tif _, ok := r.pathPatternRegistered[pathPattern]; !ok {",
    "\t\tr.pathMux.HandleFunc(pathPattern, func(http.ResponseWriter, *http.Request) {})",
    "\t\tr.pathPatternRegistered[pathPattern] = struct{}{}",
    "\t}",
    "\tmethodSet, ok := r.methodsByPathPattern[pathPattern]",
    "\tif !ok {",
    "\t\tmethodSet = map[string]struct{}{}",
    "\t\tr.methodsByPathPattern[pathPattern] = methodSet",
    "\t}",
    "\tmethodSet[method] = struct{}{}",
    "}",
    "",
    "func (r *runtimeRouter) ServeHTTP(w http.ResponseWriter, req *http.Request) {",
    "\thandler, methodPattern := r.methodMux.Handler(req)",
    '\tif methodPattern != "" {',
    "\t\thandler.ServeHTTP(w, req)",
    "\t\treturn",
    "\t}",
    "",
    "\t_, pathPattern := r.pathMux.Handler(req)",
    '\tif pathPattern != "" {',
    "\t\tallow := make([]string, 0, len(r.methodsByPathPattern[pathPattern]))",
    "\t\tfor method := range r.methodsByPathPattern[pathPattern] {",
    "\t\t\tallow = append(allow, method)",
    "\t\t}",
    "\t\tsort.Strings(allow)",
    '\t\tw.Header().Set("Allow", strings.Join(allow, ", "))',
    "\t\thttp.Error(w, http.StatusText(http.StatusMethodNotAllowed), http.StatusMethodNotAllowed)",
    "\t\treturn",
    "\t}",
    "",
    "\thttp.NotFound(w, req)",
    "}",
    "",
  ];
}

export function renderRouteRegistry(routes: RouteIR[]): string[] {
  const lines = ["func registerRoutes(router *runtimeRouter) {"];
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
    "\trouter := newRuntimeRouter()",
    "\tregisterRoutes(router)",
    "\taddr := resolveListenAddr()",
    '\tfmt.Println("tsgodown scaffold listening on", addr)',
    "\tserver := &http.Server{",
    "\t\tAddr:              addr,",
    "\t\tHandler:           router,",
    "\t\tReadHeaderTimeout: 5 * time.Second,",
    "\t}",
    "\tif err := server.ListenAndServe(); err != nil && err != http.ErrServerClosed {",
    '\t\tfmt.Println("server exited:", err)',
    "\t}",
    "}",
    "",
  ];
}
