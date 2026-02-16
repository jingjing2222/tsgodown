import type { HandlerIR, RouteIR } from "@tsgodown/ir-core";

import {
  extractPathParamNames,
  normalizeHttpMethod,
  normalizeRoutePath,
  toServeMuxPattern,
} from "./route-normalization";

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

export function renderRoute(
  route: RouteIR,
  index: number,
  handler: HandlerIR | undefined,
): string[] {
  const normalizedMethod = normalizeHttpMethod(route.method);
  const normalizedPath = normalizeRoutePath(route.path);
  const todoMessage = `TODO implement handler ${route.handlerRef} for ${normalizedMethod} ${normalizedPath}`;

  const lines: string[] = [
    `func ${routeHandlerName(index)}(w http.ResponseWriter, req *http.Request) {`,
    "",
    "\t// Route metadata:",
    `\t//   Method: ${normalizedMethod}`,
    `\t//   Path: ${quoteGo(normalizedPath)}`,
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
    `\t// TODO(tsgodown): Implement handler ${quoteGo(route.handlerRef)} for ${normalizedMethod} ${normalizedPath}.`,
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

export function renderRouteRegistration(route: RouteIR, index: number): string {
  return `\tmux.HandleFunc(${quoteGo(toServeMuxPattern(route))}, ${routeHandlerName(index)})`;
}
