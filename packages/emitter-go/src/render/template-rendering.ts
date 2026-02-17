import type { HandlerIR, RouteIR } from "@tsgodown/ir-core";

import {
  extractPathParamNames,
  normalizeHttpMethod,
  normalizeRoutePath,
  toServeMuxPath,
  toServeMuxPattern,
} from "./route-normalization.js";

function routeHandlerName(index: number): string {
  return `route${index}`;
}

function quoteGo(value: string): string {
  return JSON.stringify(value);
}

const GO_KEYWORDS = new Set([
  "break",
  "default",
  "func",
  "interface",
  "select",
  "case",
  "defer",
  "go",
  "map",
  "struct",
  "chan",
  "else",
  "goto",
  "package",
  "switch",
  "const",
  "fallthrough",
  "if",
  "range",
  "type",
  "continue",
  "for",
  "import",
  "return",
  "var",
]);

const RESERVED_BINDING_NAMES = new Set(["w", "req", "_"]);

function capitalize(value: string): string {
  if (value.length === 0) return value;
  return `${value[0].toUpperCase()}${value.slice(1)}`;
}

function resolvePathParamBindingName(
  name: string,
  usedNames: Set<string>,
): string {
  let base = name;
  if (GO_KEYWORDS.has(base) || RESERVED_BINDING_NAMES.has(base)) {
    base = `pathParam${capitalize(name)}`;
  }

  let candidate = base;
  let suffix = 2;
  while (
    usedNames.has(candidate) ||
    GO_KEYWORDS.has(candidate) ||
    RESERVED_BINDING_NAMES.has(candidate)
  ) {
    candidate = `${base}${suffix}`;
    suffix += 1;
  }

  usedNames.add(candidate);
  return candidate;
}

function emitPathParams(route: RouteIR): string[] {
  const names = extractPathParamNames(route.path);
  if (names.length === 0) {
    return [];
  }

  const usedNames = new Set<string>([...RESERVED_BINDING_NAMES]);
  const lines = ["\t// Extracted path params:"];
  for (const name of names) {
    const bindingName = resolvePathParamBindingName(name, usedNames);
    lines.push(`\t${bindingName} := req.PathValue(${quoteGo(name)})`);
    lines.push(`\t_ = ${bindingName}`);
  }
  lines.push("");

  return lines;
}

function formatHandlerParams(handler: HandlerIR | undefined): string {
  if (!handler || handler.params.length === 0) return "none";
  return handler.params.map((p) => `${p.role}:${p.name}`).join(", ");
}

function toTodoHandlerDisplayName(handlerRef: string): string {
  return handlerRef.replace(/^handler[_-]+/, "");
}

function renderSemanticHandlerBehavior(
  route: RouteIR,
  handler: HandlerIR | undefined,
): string[] {
  const mode = handler?.semantics?.responseMode ?? "unknown";
  const normalizedMethod = normalizeHttpMethod(route.method);
  const normalizedPath = normalizeRoutePath(route.path);

  if (mode === "response-object") {
    return [
      '\tw.Header().Set("Content-Type", "application/json; charset=utf-8")',
      '\tw.Header().Set("X-TSGoDown-Handler", "response-object")',
      "\tw.WriteHeader(http.StatusOK)",
      "\tif err := json.NewEncoder(w).Encode(map[string]any{",
      `\t\t"handler": ${quoteGo(route.handlerRef)},`,
      `\t\t"method": ${quoteGo(normalizedMethod)},`,
      `\t\t"path": ${quoteGo(normalizedPath)},`,
      '\t\t"mode": "response-object",',
      "\t}); err != nil {",
      '\t\thttp.Error(w, "json encode failed", http.StatusInternalServerError)',
      "\t}",
    ];
  }

  if (mode === "return") {
    return [
      '\tw.Header().Set("Content-Type", "application/json; charset=utf-8")',
      '\tw.Header().Set("X-TSGoDown-Handler", "return")',
      "\tw.WriteHeader(http.StatusOK)",
      "\tif err := json.NewEncoder(w).Encode(map[string]any{",
      `\t\t"handler": ${quoteGo(route.handlerRef)},`,
      `\t\t"method": ${quoteGo(normalizedMethod)},`,
      `\t\t"path": ${quoteGo(normalizedPath)},`,
      '\t\t"mode": "return",',
      "\t}); err != nil {",
      '\t\thttp.Error(w, "json encode failed", http.StatusInternalServerError)',
      "\t}",
    ];
  }

  if (mode === "next-callback") {
    return [
      '\tw.Header().Set("X-TSGoDown-Handler", "next-callback")',
      "\tw.WriteHeader(http.StatusNoContent)",
    ];
  }

  const todoHandlerName = toTodoHandlerDisplayName(route.handlerRef);
  const todoMessage = `TODO implement handler ${todoHandlerName} for ${normalizedMethod} ${normalizedPath}`;
  return [
    `\t// TODO(tsgodown): Implement handler ${quoteGo(route.handlerRef)} for ${normalizedMethod} ${normalizedPath}.`,
    "\t//   - Replace this scaffold with application logic.",
    "\t//   - Validate request input and map to domain arguments.",
    "\t//   - Write response status, headers, and body.",
    "",
    '\tw.Header().Set("Content-Type", "text/plain; charset=utf-8")',
    "\tw.WriteHeader(http.StatusNotImplemented)",
    `\tfmt.Fprintln(w, ${quoteGo(todoMessage)})`,
  ];
}

export function renderRoute(
  route: RouteIR,
  index: number,
  handler: HandlerIR | undefined,
): string[] {
  const normalizedMethod = normalizeHttpMethod(route.method);
  const normalizedPath = normalizeRoutePath(route.path);

  const lines: string[] = [
    `func ${routeHandlerName(index)}(w http.ResponseWriter, req *http.Request) {`,
    "\t// Route metadata:",
    `\t//   Method: ${normalizedMethod}`,
    `\t//   Path: ${quoteGo(normalizedPath)}`,
    `\t//   Pattern: ${quoteGo(toServeMuxPattern(route))}`,
    `\t//   Handler: ${quoteGo(route.handlerRef)}`,
    `\t//   Handler params: ${formatHandlerParams(handler)}`,
    `\t//   Handler async: ${handler?.async ?? "unknown"}`,
    `\t//   Handler response mode: ${handler?.semantics?.responseMode ?? "unknown"}`,
  ];

  const middlewareRefs = route.middlewareRefs ?? [];
  if (middlewareRefs.length > 0) {
    const sortedMiddlewareRefs = [...middlewareRefs].sort((a, b) =>
      a.localeCompare(b),
    );
    lines.push(`\t//   Middleware: ${JSON.stringify(sortedMiddlewareRefs)}`);
  }

  lines.push(
    ...emitPathParams(route),
    ...renderSemanticHandlerBehavior(route, handler),
    "}",
    "",
  );

  return lines;
}

export function renderRouteRegistration(route: RouteIR, index: number): string {
  return `\trouter.handle(${quoteGo(normalizeHttpMethod(route.method))}, ${quoteGo(toServeMuxPath(route.path))}, ${routeHandlerName(index)})`;
}
