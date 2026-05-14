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

export function renderExecutableIrGoProgram(ir, options = {}) {
  const packageName = options.packageName ?? "main";
  const stmts = Array.isArray(ir?.stmts) ? ir.stmts : [];
  const externalFunctions = new Set(options.externalFunctions ?? []);
  const externalNamespaces = normalizeExternalNamespaces(
    options.externalNamespaces ?? {},
  );
  const externalMembers = new Map(
    Object.entries(options.externalMembers ?? {}),
  );
  const mainFn = stmts.find(
    (stmt) => stmt?.kind === "function-decl" && stmt.name === "main",
  );
  const functionDecls = stmts.filter((stmt) => stmt?.kind === "function-decl");
  const ctx = {
    declared: new Set(),
    functions: new Set([
      ...functionDecls.map((stmt) => stmt.name),
      ...externalFunctions,
    ]),
    externalFunctions,
    externalNamespaces,
    externalMembers,
  };
  const body = renderStmtBlock(
    mainFn
      ? (mainFn.body ?? [])
      : stmts.filter((stmt) => stmt?.kind !== "function-decl"),
    ctx,
    1,
  );
  const helperFunctions = functionDecls
    .filter((stmt) => stmt.name !== "main")
    .map((stmt) => renderFunctionDecl(stmt, ctx))
    .join("\n\n");

  return [
    `package ${packageName}`,
    "",
    "import (",
    '\t"encoding/json"',
    '\t"fmt"',
    '\t"os"',
    '\t"regexp"',
    '\t"strings"',
    ...(options.extraImports ?? []).map(
      (importPath) => `\t${JSON.stringify(importPath)}`,
    ),
    ")",
    "",
    "func main() {",
    "\tresult := js_main()",
    "\tif result == nil {",
    "\t\treturn",
    "\t}",
    "\tbytes, err := json.Marshal(result)",
    "\tif err != nil {",
    "\t\tfmt.Fprintln(os.Stderr, err)",
    "\t\tos.Exit(1)",
    "\t}",
    "\tfmt.Println(string(bytes))",
    "}",
    "",
    "func js_main() any {",
    body,
    "\treturn nil",
    "}",
    helperFunctions ? "" : null,
    helperFunctions,
    options.helperSource ? "" : null,
    options.helperSource ?? null,
    "",
    "func jsTemplate(quasis []string, exprs []any) string {",
    '\tout := ""',
    "\tfor i, quasi := range quasis {",
    "\t\tout += quasi",
    "\t\tif i < len(exprs) {",
    "\t\t\tout += fmt.Sprint(exprs[i])",
    "\t\t}",
    "\t}",
    "\treturn out",
    "}",
    "",
    "func jsTernary(test bool, consequent any, alternate any) any {",
    "\tif test {",
    "\t\treturn consequent",
    "\t}",
    "\treturn alternate",
    "}",
    "",
    "func jsConsoleLog(args ...any) any {",
    "\tfor index, arg := range args {",
    "\t\tif index > 0 {",
    '\t\t\tfmt.Print(" ")',
    "\t\t}",
    "\t\tfmt.Print(arg)",
    "\t}",
    '\tfmt.Print("\\n")',
    "\treturn nil",
    "}",
    "",
    "func jsJSONStringify(value any, args ...any) string {",
    "\tif len(args) >= 2 && args[1] != nil {",
    '\t\tbytes, _ := json.MarshalIndent(value, "", jsJSONIndent(args[1]))',
    "\t\treturn string(bytes)",
    "\t}",
    "\tbytes, _ := json.Marshal(value)",
    "\treturn string(bytes)",
    "}",
    "",
    "func jsJSONIndent(value any) string {",
    "\tswitch typed := value.(type) {",
    "\tcase int:",
    '\t\treturn strings.Repeat(" ", typed)',
    "\tcase int64:",
    '\t\treturn strings.Repeat(" ", int(typed))',
    "\tcase float64:",
    '\t\treturn strings.Repeat(" ", int(typed))',
    "\tcase string:",
    "\t\treturn typed",
    "\tdefault:",
    "\t\treturn fmt.Sprint(value)",
    "\t}",
    "}",
    "",
    "func jsGet(value any, property string) any {",
    "\tif object, ok := value.(map[string]any); ok {",
    "\t\treturn object[property]",
    "\t}",
    "\treturn nil",
    "}",
    "",
    "func jsRegExpTest(pattern string, flags string, value any) bool {",
    "\tcompiled := regexp.MustCompile(pattern)",
    "\treturn compiled.MatchString(fmt.Sprint(value))",
    "}",
    "",
    "type jsArrayItem struct {",
    "\tspread bool",
    "\tvalue any",
    "}",
    "",
    "func jsArraySpread(items ...jsArrayItem) []any {",
    "\tout := []any{}",
    "\tfor _, item := range items {",
    "\t\tif item.spread {",
    "\t\t\tif values, ok := item.value.([]any); ok {",
    "\t\t\t\tout = append(out, values...)",
    "\t\t\t}",
    "\t\t\tcontinue",
    "\t\t}",
    "\t\tout = append(out, item.value)",
    "\t}",
    "\treturn out",
    "}",
    "",
  ]
    .filter((line) => line !== null)
    .join("\n");
}

function renderFunctionDecl(stmt, parentCtx) {
  const params = (stmt.params ?? []).map(goIdent);
  const ctx = {
    declared: new Set(params),
    functions: parentCtx.functions,
    externalFunctions: parentCtx.externalFunctions,
    externalNamespaces: parentCtx.externalNamespaces,
    externalMembers: parentCtx.externalMembers,
  };
  const renderedParams = params.map((param) => `${param} any`).join(", ");
  return [
    `func js_${goIdent(stmt.name)}(${renderedParams}) any {`,
    renderStmtBlock(stmt.body ?? [], ctx, 1),
    "\treturn nil",
    "}",
  ]
    .filter(Boolean)
    .join("\n");
}

function renderStmtBlock(stmts, ctx, indentLevel) {
  return stmts
    .map((stmt) => renderStmt(stmt, ctx, indentLevel))
    .filter(Boolean)
    .join("\n");
}

function renderStmt(stmt, ctx, indentLevel) {
  const indent = "\t".repeat(indentLevel);
  switch (stmt?.kind) {
    case "var-decl": {
      const name = goIdent(stmt.name);
      const init = stmt.init ? renderExpr(stmt.init, ctx) : "nil";
      if (ctx.declared.has(name)) {
        return `${indent}${name} = ${init}`;
      }
      ctx.declared.add(name);
      return `${indent}${name} := ${init}`;
    }
    case "return":
      return `${indent}return ${stmt.value ? renderExpr(stmt.value, ctx) : "nil"}`;
    case "expr":
      return `${indent}${renderExpr(stmt.expr, ctx)}`;
    case "if": {
      const consequent = renderStmtBlock(
        stmt.consequent ?? [],
        ctx,
        indentLevel + 1,
      );
      const alternate = renderStmtBlock(
        stmt.alternate ?? [],
        ctx,
        indentLevel + 1,
      );
      const lines = [
        `${indent}if ${renderExpr(stmt.test, ctx)} {`,
        consequent,
        `${indent}}`,
      ];
      if (alternate) {
        lines[2] = `${indent}} else {`;
        lines.push(alternate, `${indent}}`);
      }
      return lines.filter(Boolean).join("\n");
    }
    case "for": {
      const init = (stmt.init ?? [])
        .map((child) => renderSimpleStmt(child, ctx))
        .join(", ");
      const test = stmt.test ? renderExpr(stmt.test, ctx) : "";
      const update = stmt.update ? renderExpr(stmt.update, ctx) : "";
      return [
        `${indent}for ${init}; ${test}; ${update} {`,
        renderStmtBlock(stmt.body ?? [], ctx, indentLevel + 1),
        `${indent}}`,
      ]
        .filter(Boolean)
        .join("\n");
    }
    case "while":
      return [
        `${indent}for ${renderExpr(stmt.test, ctx)} {`,
        renderStmtBlock(stmt.body ?? [], ctx, indentLevel + 1),
        `${indent}}`,
      ]
        .filter(Boolean)
        .join("\n");
    case "break":
      return `${indent}break`;
    case "continue":
      return `${indent}continue`;
    default:
      throw new Error(
        `EXECUTABLE_IR_UNSUPPORTED_STMT:${stmt?.kind ?? "unknown"}`,
      );
  }
}

function renderSimpleStmt(stmt, ctx) {
  switch (stmt?.kind) {
    case "var-decl": {
      const name = goIdent(stmt.name);
      const init = stmt.init ? renderExpr(stmt.init, ctx) : "nil";
      if (ctx.declared.has(name)) {
        return `${name} = ${init}`;
      }
      ctx.declared.add(name);
      return `${name} := ${init}`;
    }
    case "expr":
      return renderExpr(stmt.expr, ctx);
    default:
      throw new Error(
        `EXECUTABLE_IR_UNSUPPORTED_SIMPLE_STMT:${stmt?.kind ?? "unknown"}`,
      );
  }
}

function renderExpr(expr, ctx) {
  switch (expr?.kind) {
    case "value":
      return renderValue(expr.value);
    case "ident":
      return goIdent(expr.name);
    case "array":
      return `[]any{${(expr.items ?? []).map((item) => renderExpr(item, ctx)).join(", ")}}`;
    case "array-spread":
      return `jsArraySpread(${(expr.items ?? [])
        .map(
          (item) =>
            `jsArrayItem{spread: ${item.spread ? "true" : "false"}, value: ${renderExpr(
              item.value,
              ctx,
            )}}`,
        )
        .join(", ")})`;
    case "object":
      return `map[string]any{${(expr.props ?? [])
        .map(
          (prop) =>
            `${JSON.stringify(prop.key)}: ${renderExpr(prop.value, ctx)}`,
        )
        .join(", ")}}`;
    case "template":
      return `jsTemplate([]string{${(expr.quasis ?? [])
        .map((quasi) => JSON.stringify(quasi))
        .join(", ")}}, []any{${(expr.exprs ?? [])
        .map((child) => renderExpr(child, ctx))
        .join(", ")}})`;
    case "binary":
      return `(${renderExpr(expr.left, ctx)} ${expr.op} ${renderExpr(expr.right, ctx)})`;
    case "conditional":
      return `jsTernary(${renderExpr(expr.test, ctx)}, ${renderExpr(
        expr.consequent,
        ctx,
      )}, ${renderExpr(expr.alternate, ctx)})`;
    case "call": {
      const builtinCall = renderBuiltinCall(expr, ctx);
      if (builtinCall) {
        return builtinCall;
      }
      if (
        expr.callee?.kind !== "ident" ||
        !ctx.functions.has(expr.callee.name)
      ) {
        throw new Error("EXECUTABLE_IR_UNSUPPORTED_CALL");
      }
      return `js_${goIdent(expr.callee.name)}(${(expr.args ?? [])
        .map((arg) => renderExpr(arg, ctx))
        .join(", ")})`;
    }
    case "assign":
      return `${renderExpr(expr.left, ctx)} ${expr.op} ${renderExpr(
        expr.right,
        ctx,
      )}`;
    case "update": {
      const arg = renderExpr(expr.arg, ctx);
      return expr.prefix ? `${expr.op}${arg}` : `${arg}${expr.op}`;
    }
    case "member":
      if (expr.object?.kind === "ident") {
        const key = `${expr.object.name}.${expr.property}`;
        if (ctx.externalMembers.has(key)) {
          return ctx.externalMembers.get(key);
        }
      }
      return `jsGet(${renderExpr(expr.object, ctx)}, ${JSON.stringify(
        expr.property,
      )})`;
    default:
      throw new Error(
        `EXECUTABLE_IR_UNSUPPORTED_EXPR:${expr?.kind ?? "unknown"}`,
      );
  }
}

function renderBuiltinCall(expr, ctx) {
  const args = (expr.args ?? []).map((arg) => renderExpr(arg, ctx)).join(", ");
  if (
    expr.callee?.kind === "member" &&
    expr.callee.object?.kind === "value" &&
    expr.callee.object.value?.kind === "regexp" &&
    expr.callee.property === "test"
  ) {
    return `jsRegExpTest(${JSON.stringify(
      expr.callee.object.value.pattern,
    )}, ${JSON.stringify(expr.callee.object.value.flags ?? "")}, ${args})`;
  }
  if (
    expr.callee?.kind === "member" &&
    expr.callee.object?.kind === "ident" &&
    ctx.externalNamespaces
      .get(expr.callee.object.name)
      ?.has(expr.callee.property)
  ) {
    return `js_${goIdent(expr.callee.object.name)}_${goIdent(expr.callee.property)}(${args})`;
  }
  if (
    expr.callee?.kind === "member" &&
    expr.callee.object?.kind === "ident" &&
    expr.callee.object.name === "console" &&
    expr.callee.property === "log"
  ) {
    return `jsConsoleLog(${args})`;
  }
  if (
    expr.callee?.kind === "member" &&
    expr.callee.object?.kind === "ident" &&
    expr.callee.object.name === "JSON" &&
    expr.callee.property === "stringify"
  ) {
    return `jsJSONStringify(${args})`;
  }
  return null;
}

function normalizeExternalNamespaces(namespaces) {
  return new Map(
    Object.entries(namespaces).map(([name, members]) => [
      name,
      new Set(members),
    ]),
  );
}

function renderValue(value) {
  switch (value?.kind) {
    case "undefined":
    case "null":
      return "nil";
    case "bool":
      return value.value ? "true" : "false";
    case "number":
      return String(value.value);
    case "string":
      return JSON.stringify(value.value);
    default:
      throw new Error(
        `EXECUTABLE_IR_UNSUPPORTED_VALUE:${value?.kind ?? "unknown"}`,
      );
  }
}

function goIdent(name) {
  const ident = String(name ?? "")
    .replace(/[^A-Za-z0-9_]/g, "_")
    .replace(/^[^A-Za-z_]/, "_$&");
  if (!ident || GO_KEYWORDS.has(ident)) {
    return `${ident}_`;
  }
  return ident;
}
