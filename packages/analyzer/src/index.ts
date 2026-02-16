import fs from "node:fs";
import type { DiagnosticIR, ProgramIR, RouteIR } from "@tsgodown/ir-core";

type PluginDef = {
  paramName: string;
  body: string;
};

const HTTP_METHODS = ["GET", "POST", "PUT", "DELETE", "PATCH"] as const;

export function analyzeFastifyEntry(entryFile: string): ProgramIR {
  const src = fs.readFileSync(entryFile, "utf-8");
  const diagnostics: DiagnosticIR[] = [];
  const routes: RouteIR[] = [];

  const pluginDefs = collectPluginDefinitions(src);
  analyzeScope({
    src,
    file: entryFile,
    diagnostics,
    routes,
    pluginDefs,
    instanceName: "fastify",
    prefix: "",
  });

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

function analyzeScope(params: {
  src: string;
  file: string;
  diagnostics: DiagnosticIR[];
  routes: RouteIR[];
  pluginDefs: Map<string, PluginDef>;
  instanceName: string;
  prefix: string;
}) {
  const { src, file, diagnostics, routes, pluginDefs, instanceName, prefix } =
    params;

  const callRe = new RegExp(
    String.raw`${escapeRe(instanceName)}\.(get|post|put|delete|patch)\s*\(\s*([^,\n]+?)\s*,\s*([^,\n\)]+)`,
    "g",
  );

  for (const m of src.matchAll(callRe)) {
    const method = m[1].toUpperCase() as RouteIR["method"];
    const pathExpr = m[2].trim();
    const handlerExpr = m[3].trim();

    const path = extractQuoted(pathExpr);
    if (!path) {
      diagnostics.push({
        level: "warn",
        code: "ANALYZER_UNSUPPORTED_DYNAMIC_PATH",
        message: `unsupported dynamic path in ${instanceName}.${m[1]}(...)`,
        source: { file },
      });
      continue;
    }

    const handlerRef = extractHandlerRef(handlerExpr);
    if (!handlerRef) {
      diagnostics.push({
        level: "warn",
        code: "ANALYZER_UNSUPPORTED_INLINE_HANDLER",
        message: `unsupported non-reference handler in ${instanceName}.${m[1]}('${path}', handler)`,
        source: { file },
      });
      continue;
    }

    routes.push({ method, path: joinPath(prefix, path), handlerRef });
  }

  const routeObjRe = new RegExp(
    String.raw`${escapeRe(instanceName)}\.route\s*\(\s*\{([\s\S]*?)\}\s*\)`,
    "g",
  );
  for (const m of src.matchAll(routeObjRe)) {
    const body = m[1];
    const methodRaw = extractObjectStringProp(body, "method");
    const method = methodRaw ? methodRaw.toUpperCase() : undefined;
    const path =
      extractObjectStringProp(body, "url") ??
      extractObjectStringProp(body, "path");
    const handlerRef = extractHandlerRef(
      extractObjectValue(body, "handler") ?? "",
    );

    if (
      !method ||
      !HTTP_METHODS.includes(method as (typeof HTTP_METHODS)[number])
    ) {
      diagnostics.push({
        level: "warn",
        code: "ANALYZER_UNSUPPORTED_ROUTE_OBJECT",
        message: `unsupported route object method in ${instanceName}.route({...})`,
        source: { file },
      });
      continue;
    }
    if (!path) {
      diagnostics.push({
        level: "warn",
        code: "ANALYZER_UNSUPPORTED_DYNAMIC_PATH",
        message: `unsupported route object path in ${instanceName}.route({...})`,
        source: { file },
      });
      continue;
    }
    if (!handlerRef) {
      diagnostics.push({
        level: "warn",
        code: "ANALYZER_UNSUPPORTED_INLINE_HANDLER",
        message: `unsupported route object handler in ${instanceName}.route({...})`,
        source: { file },
      });
      continue;
    }

    routes.push({
      method: method as RouteIR["method"],
      path: joinPath(prefix, path),
      handlerRef,
    });
  }

  const registerNeedle = `${instanceName}.register`;
  let searchIndex = 0;
  while (true) {
    const at = src.indexOf(registerNeedle, searchIndex);
    if (at < 0) break;
    const openParen = src.indexOf("(", at + registerNeedle.length);
    if (openParen < 0) break;

    const closeParen = findMatching(src, openParen, "(", ")");
    if (closeParen < 0) break;

    const args = splitTopLevel(src.slice(openParen + 1, closeParen));
    const pluginExpr = (args[0] ?? "").trim();
    const optionsExpr = (args[1] ?? "").trim();
    const prefixFromRegister =
      extractObjectStringProp(optionsExpr, "prefix") ?? "";
    const nextPrefix = joinPath(prefix, prefixFromRegister);

    const inlinePlugin = parsePluginExpression(pluginExpr);
    if (inlinePlugin) {
      analyzeScope({
        src: inlinePlugin.body,
        file,
        diagnostics,
        routes,
        pluginDefs,
        instanceName: inlinePlugin.paramName,
        prefix: nextPrefix,
      });
      searchIndex = closeParen + 1;
      continue;
    }

    const pluginRef = extractHandlerRef(pluginExpr);
    if (pluginRef) {
      const def = pluginDefs.get(pluginRef);
      if (def) {
        analyzeScope({
          src: def.body,
          file,
          diagnostics,
          routes,
          pluginDefs,
          instanceName: def.paramName,
          prefix: nextPrefix,
        });
      } else {
        diagnostics.push({
          level: "warn",
          code: "ANALYZER_UNRESOLVED_PLUGIN",
          message: `register plugin '${pluginRef}' could not be resolved in current file`,
          source: { file },
        });
      }
      searchIndex = closeParen + 1;
      continue;
    }

    diagnostics.push({
      level: "warn",
      code: "ANALYZER_UNSUPPORTED_REGISTER_CALLBACK",
      message: `unsupported register callback pattern on ${instanceName}.register(...)`,
      source: { file },
    });

    searchIndex = closeParen + 1;
  }
}

function collectPluginDefinitions(src: string): Map<string, PluginDef> {
  const map = new Map<string, PluginDef>();

  const fnRe = /function\s+([A-Za-z_$][\w$]*)\s*\(([^)]*)\)\s*\{/g;
  for (const m of src.matchAll(fnRe)) {
    const name = m[1];
    const params = m[2];
    const openBrace = (m.index ?? 0) + m[0].length - 1;
    const closeBrace = findMatching(src, openBrace, "{", "}");
    if (closeBrace < 0) continue;
    const body = src.slice(openBrace + 1, closeBrace);
    const paramName = firstParam(params);
    if (!paramName) continue;
    map.set(name, { paramName, body });
  }

  const fnExprRe =
    /(const|let|var)\s+([A-Za-z_$][\w$]*)\s*=\s*(?:async\s+)?function(?:\s+[A-Za-z_$][\w$]*)?\s*\(([^)]*)\)\s*\{/g;
  for (const m of src.matchAll(fnExprRe)) {
    const name = m[2];
    const paramName = firstParam(m[3]);
    if (!paramName) continue;

    const openBrace = (m.index ?? 0) + m[0].length - 1;
    const closeBrace = findMatching(src, openBrace, "{", "}");
    if (closeBrace < 0) continue;
    const body = src.slice(openBrace + 1, closeBrace);
    map.set(name, { paramName, body });
  }

  const arrowExprRe =
    /(const|let|var)\s+([A-Za-z_$][\w$]*)\s*=\s*(?:async\s+)?(?:\(([^)]*)\)|([A-Za-z_$][\w$]*))\s*=>\s*\{/g;
  for (const m of src.matchAll(arrowExprRe)) {
    const name = m[2];
    const rawParams = m[3] ?? m[4] ?? "";
    const paramName = firstParam(rawParams);
    if (!paramName) continue;

    const openBrace = (m.index ?? 0) + m[0].length - 1;
    const closeBrace = findMatching(src, openBrace, "{", "}");
    if (closeBrace < 0) continue;
    const body = src.slice(openBrace + 1, closeBrace);
    map.set(name, { paramName, body });
  }

  return map;
}

function parsePluginExpression(expr: string): PluginDef | null {
  const fnRe =
    /^(?:async\s+)?function(?:\s+[A-Za-z_$][\w$]*)?\s*\(([^)]*)\)\s*\{([\s\S]*)\}$/;
  const fn = expr.match(fnRe);
  if (fn) {
    const paramName = firstParam(fn[1]);
    if (!paramName) return null;
    return { paramName, body: fn[2] };
  }

  const arrowBlockRe =
    /^(?:async\s+)?(?:\(([^)]*)\)|([A-Za-z_$][\w$]*))\s*=>\s*\{([\s\S]*)\}$/;
  const arrow = expr.match(arrowBlockRe);
  if (arrow) {
    const rawParams = arrow[1] ?? arrow[2] ?? "";
    const paramName = firstParam(rawParams);
    if (!paramName) return null;
    return { paramName, body: arrow[3] };
  }

  return null;
}

function firstParam(params: string): string | null {
  const first = params
    .split(",")
    .map((v) => v.trim())
    .filter(Boolean)[0];
  if (!first) return null;
  const cleaned = first
    .replace(/^\.\.\./, "")
    .split(/[=:]/)[0]
    .trim();
  return /^[A-Za-z_$][\w$]*$/.test(cleaned) ? cleaned : null;
}

function findMatching(
  src: string,
  openIndex: number,
  open: string,
  close: string,
): number {
  let depth = 0;
  let quote: string | null = null;
  let escaped = false;

  for (let i = openIndex; i < src.length; i++) {
    const ch = src[i];
    if (quote) {
      if (!escaped && ch === quote) quote = null;
      escaped = !escaped && ch === "\\";
      continue;
    }

    if (ch === '"' || ch === "'" || ch === "`") {
      quote = ch;
      escaped = false;
      continue;
    }

    if (ch === open) depth++;
    if (ch === close) {
      depth--;
      if (depth === 0) return i;
    }
  }

  return -1;
}

function splitTopLevel(src: string): string[] {
  const out: string[] = [];
  let start = 0;
  let depthParen = 0;
  let depthBrace = 0;
  let depthBracket = 0;
  let quote: string | null = null;
  let escaped = false;

  for (let i = 0; i < src.length; i++) {
    const ch = src[i];
    if (quote) {
      if (!escaped && ch === quote) quote = null;
      escaped = !escaped && ch === "\\";
      continue;
    }

    if (ch === '"' || ch === "'" || ch === "`") {
      quote = ch;
      escaped = false;
      continue;
    }

    if (ch === "(") depthParen++;
    else if (ch === ")") depthParen--;
    else if (ch === "{") depthBrace++;
    else if (ch === "}") depthBrace--;
    else if (ch === "[") depthBracket++;
    else if (ch === "]") depthBracket--;

    if (
      ch === "," &&
      depthParen === 0 &&
      depthBrace === 0 &&
      depthBracket === 0
    ) {
      out.push(src.slice(start, i).trim());
      start = i + 1;
    }
  }

  out.push(src.slice(start).trim());
  return out;
}

function extractQuoted(value: string): string | null {
  const m = value.trim().match(/^['"]([^'"]+)['"]$/);
  return m ? m[1] : null;
}

function extractHandlerRef(value: string): string | null {
  const v = value.trim();
  if (/^[A-Za-z_$][\w$]*(?:\.[A-Za-z_$][\w$]*)*$/.test(v)) return v;
  return null;
}

function extractObjectStringProp(src: string, key: string): string | null {
  const re = new RegExp(`${escapeRe(key)}\\s*:\\s*['\"]([^'\"]+)['\"]`);
  const m = src.match(re);
  return m ? m[1] : null;
}

function extractObjectValue(src: string, key: string): string | null {
  const re = new RegExp(`${escapeRe(key)}\\s*:\\s*([^,\\n}]+)`);
  const m = src.match(re);
  return m ? m[1].trim() : null;
}

function joinPath(prefix: string, path: string): string {
  const prefixNorm = prefix.trim();
  const pathNorm = path.trim();
  if (!prefixNorm) return ensureSlash(pathNorm);
  if (!pathNorm) return ensureSlash(prefixNorm);
  const left = ensureSlash(prefixNorm).replace(/\/$/, "");
  const right = ensureSlash(pathNorm);
  return `${left}${right}`;
}

function ensureSlash(v: string): string {
  return v.startsWith("/") ? v : `/${v}`;
}

function escapeRe(v: string): string {
  return v.replace(/[.*+?^${}()|[\]\\]/g, "\\$&");
}
