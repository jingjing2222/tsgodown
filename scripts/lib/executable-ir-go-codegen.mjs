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
  const mainFn = stmts.find(
    (stmt) => stmt?.kind === "function-decl" && stmt.name === "main",
  );
  if (!mainFn) {
    throw new Error("EXECUTABLE_IR_MAIN_FUNCTION_REQUIRED");
  }

  const ctx = {
    declared: new Set(),
    functions: new Set(["main"]),
  };
  const body = renderStmtBlock(mainFn.body ?? [], ctx, 1);

  return [
    `package ${packageName}`,
    "",
    "import (",
    '\t"encoding/json"',
    '\t"fmt"',
    '\t"os"',
    ")",
    "",
    "func main() {",
    "\tresult := js_main()",
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
  ].join("\n");
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
    default:
      throw new Error(
        `EXECUTABLE_IR_UNSUPPORTED_STMT:${stmt?.kind ?? "unknown"}`,
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
    default:
      throw new Error(
        `EXECUTABLE_IR_UNSUPPORTED_EXPR:${expr?.kind ?? "unknown"}`,
      );
  }
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
