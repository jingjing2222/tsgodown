import fs from "node:fs";
import type {
  DiagnosticIR,
  HandlerIR,
  HandlerResponseMode,
  ProgramIR,
  RouteIR,
} from "@tsgodown/ir-core";
import ts from "typescript";

type PluginDef = {
  paramName: string;
  statements: readonly ts.Statement[];
};

type HandlerDef = {
  params: string[];
  async: boolean;
  body: string;
};

const HTTP_METHODS = new Set(["GET", "POST", "PUT", "DELETE", "PATCH"]);

export function analyzeFastifyEntry(entryFile: string): ProgramIR {
  const src = fs.readFileSync(entryFile, "utf-8");
  const sourceFile = ts.createSourceFile(
    entryFile,
    src,
    ts.ScriptTarget.Latest,
    true,
    ts.ScriptKind.TS,
  );

  const diagnostics: DiagnosticIR[] = [];
  const routes: RouteIR[] = [];
  const handlers: HandlerIR[] = [];
  const pluginDefs = collectPluginDefinitions(sourceFile);
  const handlerDefs = collectHandlerDefinitions(sourceFile);

  analyzeScope({
    statements: sourceFile.statements,
    file: entryFile,
    diagnostics,
    routes,
    handlers,
    pluginDefs,
    handlerDefs,
    instanceName: detectRootInstanceName(sourceFile) ?? "fastify",
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
    handlers,
    diagnostics,
  };
}

function analyzeScope(params: {
  statements: readonly ts.Statement[];
  file: string;
  diagnostics: DiagnosticIR[];
  routes: RouteIR[];
  handlers: HandlerIR[];
  pluginDefs: Map<string, PluginDef>;
  handlerDefs: Map<string, HandlerDef>;
  instanceName: string;
  prefix: string;
}) {
  const {
    statements,
    file,
    diagnostics,
    routes,
    handlers,
    pluginDefs,
    handlerDefs,
    instanceName,
    prefix,
  } = params;

  for (const statement of statements) {
    walkNode(statement, (node) => {
      if (!ts.isCallExpression(node)) return;
      if (!ts.isPropertyAccessExpression(node.expression)) return;
      if (!ts.isIdentifier(node.expression.expression)) return;
      if (node.expression.expression.text !== instanceName) return;

      const member = node.expression.name.text;
      if (isHttpMethod(member)) {
        extractShorthandRoute({
          call: node,
          file,
          diagnostics,
          routes,
          handlers,
          handlerDefs,
          method: member.toUpperCase() as RouteIR["method"],
          prefix,
          instanceName,
        });
        return;
      }

      if (member === "route") {
        extractRouteObject({
          call: node,
          file,
          diagnostics,
          routes,
          handlers,
          handlerDefs,
          prefix,
          instanceName,
        });
        return;
      }

      if (member === "register") {
        analyzeRegisterCall({
          call: node,
          file,
          diagnostics,
          routes,
          handlers,
          pluginDefs,
          handlerDefs,
          prefix,
          instanceName,
        });
      }
    });
  }
}

function extractShorthandRoute(params: {
  call: ts.CallExpression;
  file: string;
  diagnostics: DiagnosticIR[];
  routes: RouteIR[];
  handlers: HandlerIR[];
  handlerDefs: Map<string, HandlerDef>;
  method: RouteIR["method"];
  prefix: string;
  instanceName: string;
}) {
  const {
    call,
    file,
    diagnostics,
    routes,
    handlers,
    handlerDefs,
    method,
    prefix,
    instanceName,
  } = params;
  const pathExpr = call.arguments[0];
  const handlerExpr = call.arguments[1];

  const path = pathExpr ? extractStringLiteral(pathExpr) : null;
  if (!path) {
    diagnostics.push({
      level: "warn",
      code: "ANALYZER_UNSUPPORTED_DYNAMIC_PATH",
      message: `unsupported dynamic path in ${instanceName}.${method.toLowerCase()}(...)`,
      source: { file },
    });
    return;
  }

  const handlerRef = handlerExpr ? extractHandlerRef(handlerExpr) : null;
  if (!handlerRef) {
    diagnostics.push({
      level: "warn",
      code: "ANALYZER_UNSUPPORTED_INLINE_HANDLER",
      message: `unsupported non-reference handler in ${instanceName}.${method.toLowerCase()}('${path}', handler)`,
      source: { file },
    });
    return;
  }

  routes.push({ method, path: joinPath(prefix, path), handlerRef });
  upsertHandler(handlers, handlerDefs, handlerRef);
}

function extractRouteObject(params: {
  call: ts.CallExpression;
  file: string;
  diagnostics: DiagnosticIR[];
  routes: RouteIR[];
  handlers: HandlerIR[];
  handlerDefs: Map<string, HandlerDef>;
  prefix: string;
  instanceName: string;
}) {
  const {
    call,
    file,
    diagnostics,
    routes,
    handlers,
    handlerDefs,
    prefix,
    instanceName,
  } = params;
  const firstArg = call.arguments[0];
  if (!firstArg || !ts.isObjectLiteralExpression(firstArg)) {
    diagnostics.push({
      level: "warn",
      code: "ANALYZER_UNSUPPORTED_ROUTE_OBJECT",
      message: `unsupported route object method in ${instanceName}.route({...})`,
      source: { file },
    });
    return;
  }

  const methodRaw = extractObjectStringProp(firstArg, "method");
  const method = methodRaw?.toUpperCase();
  const path =
    extractObjectStringProp(firstArg, "url") ??
    extractObjectStringProp(firstArg, "path");
  const handlerRef = extractObjectHandlerRef(firstArg, "handler");

  if (!method || !HTTP_METHODS.has(method)) {
    diagnostics.push({
      level: "warn",
      code: "ANALYZER_UNSUPPORTED_ROUTE_OBJECT",
      message: `unsupported route object method in ${instanceName}.route({...})`,
      source: { file },
    });
    return;
  }

  if (!path) {
    diagnostics.push({
      level: "warn",
      code: "ANALYZER_UNSUPPORTED_DYNAMIC_PATH",
      message: `unsupported route object path in ${instanceName}.route({...})`,
      source: { file },
    });
    return;
  }

  if (!handlerRef) {
    diagnostics.push({
      level: "warn",
      code: "ANALYZER_UNSUPPORTED_INLINE_HANDLER",
      message: `unsupported route object handler in ${instanceName}.route({...})`,
      source: { file },
    });
    return;
  }

  routes.push({
    method: method as RouteIR["method"],
    path: joinPath(prefix, path),
    handlerRef,
  });
  upsertHandler(handlers, handlerDefs, handlerRef);
}

function analyzeRegisterCall(params: {
  call: ts.CallExpression;
  file: string;
  diagnostics: DiagnosticIR[];
  routes: RouteIR[];
  handlers: HandlerIR[];
  pluginDefs: Map<string, PluginDef>;
  handlerDefs: Map<string, HandlerDef>;
  prefix: string;
  instanceName: string;
}) {
  const {
    call,
    file,
    diagnostics,
    routes,
    handlers,
    pluginDefs,
    handlerDefs,
    prefix,
    instanceName,
  } = params;

  const pluginExpr = call.arguments[0];
  const optionsExpr = call.arguments[1];
  const prefixFromRegister =
    optionsExpr && ts.isObjectLiteralExpression(optionsExpr)
      ? (extractObjectStringProp(optionsExpr, "prefix") ?? "")
      : "";
  const nextPrefix = joinPath(prefix, prefixFromRegister);

  const inlinePlugin = pluginExpr ? parsePluginExpression(pluginExpr) : null;
  if (inlinePlugin) {
    analyzeScope({
      statements: inlinePlugin.statements,
      file,
      diagnostics,
      routes,
      handlers,
      pluginDefs,
      handlerDefs,
      instanceName: inlinePlugin.paramName,
      prefix: nextPrefix,
    });
    return;
  }

  const pluginRef = pluginExpr ? extractHandlerRef(pluginExpr) : null;
  if (pluginRef) {
    const def = pluginDefs.get(pluginRef);
    if (def) {
      analyzeScope({
        statements: def.statements,
        file,
        diagnostics,
        routes,
        handlers,
        pluginDefs,
        handlerDefs,
        instanceName: def.paramName,
        prefix: nextPrefix,
      });
      return;
    }

    diagnostics.push({
      level: "warn",
      code: "ANALYZER_UNRESOLVED_PLUGIN",
      message: `register plugin '${pluginRef}' could not be resolved in current file`,
      source: { file },
    });
    return;
  }

  diagnostics.push({
    level: "warn",
    code: "ANALYZER_UNSUPPORTED_REGISTER_CALLBACK",
    message: `unsupported register callback pattern on ${instanceName}.register(...)`,
    source: { file },
  });
}

function upsertHandler(
  handlers: HandlerIR[],
  handlerDefs: Map<string, HandlerDef>,
  handlerRef: string,
) {
  if (!/^[A-Za-z_$][\w$]*$/.test(handlerRef)) return;
  if (handlers.some((h) => h.id === handlerRef)) return;

  const def = handlerDefs.get(handlerRef);
  if (!def) return;

  handlers.push({
    id: handlerRef,
    params: def.params.map((name) => ({ name, role: inferParamRole(name) })),
    async: def.async,
    semantics: {
      responseMode: inferResponseMode(def.body, def.params),
    },
  });
}

function inferParamRole(name: string): HandlerIR["params"][number]["role"] {
  const n = name.toLowerCase();
  if (n === "req" || n === "request") return "request";
  if (n === "res" || n === "reply" || n === "response") return "response";
  if (n === "next") return "next";
  return "custom";
}

function inferResponseMode(
  body: string,
  params: string[],
): HandlerResponseMode {
  const responseParam = params.find((p) => {
    const n = p.toLowerCase();
    return n === "res" || n === "reply" || n === "response";
  });
  if (
    responseParam &&
    new RegExp(`\\b${escapeRe(responseParam)}\\s*\\.`).test(body)
  ) {
    return "response-object";
  }

  const nextParam = params.find((p) => p.toLowerCase() === "next");
  if (nextParam && new RegExp(`\\b${escapeRe(nextParam)}\\s*\\(`).test(body)) {
    return "next-callback";
  }

  if (/\breturn\b/i.test(body)) return "return";
  return "unknown";
}

function collectPluginDefinitions(
  sourceFile: ts.SourceFile,
): Map<string, PluginDef> {
  const map = new Map<string, PluginDef>();

  for (const statement of sourceFile.statements) {
    if (
      ts.isFunctionDeclaration(statement) &&
      statement.name &&
      statement.body
    ) {
      const paramName = firstParam(statement.parameters);
      if (!paramName) continue;
      map.set(statement.name.text, {
        paramName,
        statements: statement.body.statements,
      });
      continue;
    }

    if (!ts.isVariableStatement(statement)) continue;
    for (const declaration of statement.declarationList.declarations) {
      if (!ts.isIdentifier(declaration.name) || !declaration.initializer)
        continue;
      const plugin = parsePluginExpression(declaration.initializer);
      if (!plugin) continue;
      map.set(declaration.name.text, plugin);
    }
  }

  return map;
}

function collectHandlerDefinitions(
  sourceFile: ts.SourceFile,
): Map<string, HandlerDef> {
  const map = new Map<string, HandlerDef>();

  for (const statement of sourceFile.statements) {
    if (
      ts.isFunctionDeclaration(statement) &&
      statement.name &&
      statement.body
    ) {
      map.set(statement.name.text, {
        params: parseParamNames(statement.parameters),
        async: hasAsyncModifier(statement),
        body: statement.body.getText(sourceFile),
      });
      continue;
    }

    if (!ts.isVariableStatement(statement)) continue;
    for (const declaration of statement.declarationList.declarations) {
      if (!ts.isIdentifier(declaration.name) || !declaration.initializer)
        continue;

      const init = declaration.initializer;
      if (
        (ts.isFunctionExpression(init) || ts.isArrowFunction(init)) &&
        ts.isBlock(init.body)
      ) {
        map.set(declaration.name.text, {
          params: parseParamNames(init.parameters),
          async: hasAsyncModifier(init),
          body: init.body.getText(sourceFile),
        });
      }
    }
  }

  return map;
}

function parsePluginExpression(expr: ts.Expression): PluginDef | null {
  if (
    (ts.isFunctionExpression(expr) || ts.isArrowFunction(expr)) &&
    ts.isBlock(expr.body)
  ) {
    const paramName = firstParam(expr.parameters);
    if (!paramName) return null;
    return { paramName, statements: expr.body.statements };
  }

  return null;
}

function parseParamNames(params: readonly ts.ParameterDeclaration[]): string[] {
  return params
    .map((p) => {
      if (!ts.isIdentifier(p.name)) return null;
      return p.name.text;
    })
    .filter((v): v is string => typeof v === "string");
}

function hasAsyncModifier(node: ts.Node): boolean {
  return Boolean(
    (node as { modifiers?: ts.NodeArray<ts.ModifierLike> }).modifiers?.some(
      (m) => m.kind === ts.SyntaxKind.AsyncKeyword,
    ),
  );
}

function firstParam(params: readonly ts.ParameterDeclaration[]): string | null {
  const first = params[0];
  if (!first) return null;
  return ts.isIdentifier(first.name) ? first.name.text : null;
}

function extractStringLiteral(node: ts.Expression): string | null {
  if (ts.isStringLiteral(node) || ts.isNoSubstitutionTemplateLiteral(node)) {
    return node.text;
  }

  return null;
}

function extractHandlerRef(node: ts.Expression): string | null {
  if (ts.isIdentifier(node)) return node.text;

  if (ts.isPropertyAccessExpression(node)) {
    const left = extractHandlerRef(node.expression);
    if (!left) return null;
    return `${left}.${node.name.text}`;
  }

  return null;
}

function extractObjectStringProp(
  objectLiteral: ts.ObjectLiteralExpression,
  key: string,
): string | null {
  for (const prop of objectLiteral.properties) {
    if (!ts.isPropertyAssignment(prop) || !isNamedProperty(prop.name, key)) {
      continue;
    }

    return extractStringLiteral(prop.initializer);
  }

  return null;
}

function extractObjectHandlerRef(
  objectLiteral: ts.ObjectLiteralExpression,
  key: string,
): string | null {
  for (const prop of objectLiteral.properties) {
    if (!ts.isPropertyAssignment(prop) || !isNamedProperty(prop.name, key)) {
      continue;
    }

    return extractHandlerRef(prop.initializer);
  }

  return null;
}

function isNamedProperty(name: ts.PropertyName, key: string): boolean {
  return (
    (ts.isIdentifier(name) && name.text === key) ||
    (ts.isStringLiteral(name) && name.text === key)
  );
}

function isHttpMethod(member: string): boolean {
  return HTTP_METHODS.has(member.toUpperCase());
}

function detectRootInstanceName(sourceFile: ts.SourceFile): string | null {
  for (const statement of sourceFile.statements) {
    if (!ts.isVariableStatement(statement)) continue;

    for (const declaration of statement.declarationList.declarations) {
      if (!ts.isIdentifier(declaration.name) || !declaration.initializer)
        continue;
      if (!ts.isCallExpression(declaration.initializer)) continue;

      const callee = declaration.initializer.expression;
      if (ts.isIdentifier(callee) && callee.text === "Fastify") {
        return declaration.name.text;
      }
    }
  }

  return null;
}

function walkNode(node: ts.Node, visit: (node: ts.Node) => void): void {
  visit(node);

  if (ts.isFunctionLike(node) && !ts.isSourceFile(node)) {
    return;
  }

  node.forEachChild((child) => {
    walkNode(child, visit);
  });
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
