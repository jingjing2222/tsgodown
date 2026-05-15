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
  const importPaths = [
    ...new Set([
      "encoding/json",
      "fmt",
      "os",
      "path/filepath",
      "regexp",
      "reflect",
      "strconv",
      "strings",
      ...(options.extraImports ?? []),
    ]),
  ];
  const externalFunctions = new Set(options.externalFunctions ?? []);
  const externalNamespaces = normalizeExternalNamespaces(
    options.externalNamespaces ?? {},
  );
  const externalMembers = new Map(
    Object.entries(options.externalMembers ?? {}),
  );
  const externalConstructors = new Set(options.externalConstructors ?? []);
  const mainFn = stmts.find(
    (stmt) => stmt?.kind === "function-decl" && stmt.name === "main",
  );
  const functionDecls = stmts.filter((stmt) => stmt?.kind === "function-decl");
  const ctx = {
    declared: new Set(),
    arrays: new Set(),
    functions: new Set([
      ...functionDecls.map((stmt) => stmt.name),
      ...externalFunctions,
    ]),
    externalFunctions,
    externalNamespaces,
    externalMembers,
    externalConstructors,
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
    ...importPaths.map((importPath) => `\t${JSON.stringify(importPath)}`),
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
    "func jsNullish(value any, fallback any) any {",
    "\tif value == nil {",
    "\t\treturn fallback",
    "\t}",
    "\treturn value",
    "}",
    "",
    "func jsTruthy(value any) bool {",
    "\tswitch typed := value.(type) {",
    "\tcase nil:",
    "\t\treturn false",
    "\tcase bool:",
    "\t\treturn typed",
    "\tcase int:",
    "\t\treturn typed != 0",
    "\tcase int64:",
    "\t\treturn typed != 0",
    "\tcase float64:",
    "\t\treturn typed != 0",
    "\tcase string:",
    '\t\treturn typed != ""',
    "\tdefault:",
    "\t\treturn true",
    "\t}",
    "}",
    "",
    "func jsTypeof(value any) string {",
    "\tswitch value.(type) {",
    "\tcase nil:",
    '\t\treturn "undefined"',
    "\tcase bool:",
    '\t\treturn "boolean"',
    "\tcase int, int64, float64:",
    '\t\treturn "number"',
    "\tcase string:",
    '\t\treturn "string"',
    "\tdefault:",
    '\t\treturn "object"',
    "\t}",
    "}",
    "",
    "func jsNumber(value any) float64 {",
    "\tswitch typed := value.(type) {",
    "\tcase int:",
    "\t\treturn float64(typed)",
    "\tcase int64:",
    "\t\treturn float64(typed)",
    "\tcase float64:",
    "\t\treturn typed",
    "\tcase bool:",
    "\t\tif typed {",
    "\t\t\treturn 1",
    "\t\t}",
    "\t\treturn 0",
    "\tcase string:",
    "\t\tparsed, err := strconv.ParseFloat(typed, 64)",
    "\t\tif err == nil {",
    "\t\t\treturn parsed",
    "\t\t}",
    "\t}",
    "\treturn 0",
    "}",
    "",
    "func jsStrictEqual(left any, right any) bool {",
    "\treturn reflect.DeepEqual(left, right)",
    "}",
    "",
    "func jsAwait(value any) any {",
    "\treturn value",
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
    "\tif items, ok := value.([]any); ok {",
    "\t\tindex, err := strconv.Atoi(property)",
    "\t\tif err == nil && index >= 0 && index < len(items) {",
    "\t\t\treturn items[index]",
    "\t\t}",
    "\t}",
    "\treturn nil",
    "}",
    "",
    "func jsString(value any) string {",
    "\treturn fmt.Sprint(value)",
    "}",
    "",
    "func jsPathJoin(parts ...any) string {",
    "\tclean := make([]string, 0, len(parts))",
    "\tfor _, part := range parts {",
    "\t\tclean = append(clean, fmt.Sprint(part))",
    "\t}",
    "\treturn filepath.Join(clean...)",
    "}",
    "",
    "func jsPathDirname(value any) string {",
    "\treturn filepath.Dir(fmt.Sprint(value))",
    "}",
    "",
    "func jsTmpdir() string {",
    "\treturn os.TempDir()",
    "}",
    "",
    "func jsMkdtempSync(prefix any) string {",
    '\tdir, err := os.MkdirTemp(filepath.Dir(fmt.Sprint(prefix)), filepath.Base(fmt.Sprint(prefix))+"*")',
    "\tif err != nil {",
    "\t\tpanic(err)",
    "\t}",
    "\treturn dir",
    "}",
    "",
    "func jsRmSync(target any, options ...any) any {",
    "\tif len(options) > 0 {",
    '\t\tif object, ok := options[0].(map[string]any); ok && jsTruthy(object["recursive"]) {',
    "\t\t\t_ = os.RemoveAll(fmt.Sprint(target))",
    "\t\t\treturn nil",
    "\t\t}",
    "\t}",
    "\t_ = os.Remove(fmt.Sprint(target))",
    "\treturn nil",
    "}",
    "",
    "func jsWriteFileSync(target any, data any) any {",
    "\tif err := os.WriteFile(fmt.Sprint(target), []byte(fmt.Sprint(data)), 0o666); err != nil {",
    "\t\tpanic(err)",
    "\t}",
    "\treturn nil",
    "}",
    "",
    "func jsStringSplit(value any, separator any) []any {",
    "\tparts := strings.Split(fmt.Sprint(value), fmt.Sprint(separator))",
    "\tout := make([]any, 0, len(parts))",
    "\tfor _, part := range parts {",
    "\t\tout = append(out, part)",
    "\t}",
    "\treturn out",
    "}",
    "",
    "func jsIterable(value any) []any {",
    "\tswitch typed := value.(type) {",
    "\tcase []any:",
    "\t\treturn typed",
    "\tcase string:",
    "\t\tout := make([]any, 0, len(typed))",
    "\t\tfor _, char := range typed {",
    "\t\t\tout = append(out, string(char))",
    "\t\t}",
    "\t\treturn out",
    "\tdefault:",
    '\t\tpanic(fmt.Sprintf("value is not iterable: %T", value))',
    "\t}",
    "}",
    "",
    "func jsRecoverValue(value any) any {",
    "\treturn value",
    "}",
    "",
    "func jsSetMember(value any, property string, next any) any {",
    "\tif object, ok := value.(map[string]any); ok {",
    "\t\tobject[property] = next",
    "\t\treturn next",
    "\t}",
    '\tpanic(fmt.Sprintf("unsupported member assignment %T.%s", value, property))',
    "}",
    "",
    "func jsReflectDeleteProperty(value any, property any) any {",
    "\tif object, ok := value.(map[string]any); ok {",
    "\t\tdelete(object, fmt.Sprint(property))",
    "\t\treturn true",
    "\t}",
    "\treturn false",
    "}",
    "",
    "func jsArrayJoin(value any, separator any) string {",
    "\titems, ok := value.([]any)",
    "\tif !ok {",
    "\t\treturn fmt.Sprint(value)",
    "\t}",
    "\tparts := make([]string, 0, len(items))",
    "\tfor _, item := range items {",
    "\t\tparts = append(parts, fmt.Sprint(item))",
    "\t}",
    "\treturn strings.Join(parts, fmt.Sprint(separator))",
    "}",
    "",
    "func jsArrayMap(value any, mapper func(...any) any) []any {",
    "\titems, ok := value.([]any)",
    "\tif !ok {",
    "\t\treturn []any{}",
    "\t}",
    "\tout := make([]any, 0, len(items))",
    "\tfor _, item := range items {",
    "\t\tif tuple, ok := item.([]any); ok {",
    "\t\t\tout = append(out, mapper(tuple...))",
    "\t\t\tcontinue",
    "\t\t}",
    "\t\tout = append(out, mapper(item))",
    "\t}",
    "\treturn out",
    "}",
    "",
    "func jsArrayFilter(value any, predicate func(...any) any) []any {",
    "\titems, ok := value.([]any)",
    "\tif !ok {",
    "\t\treturn []any{}",
    "\t}",
    "\tout := make([]any, 0, len(items))",
    "\tfor _, item := range items {",
    "\t\targs := []any{item}",
    "\t\tif tuple, ok := item.([]any); ok {",
    "\t\t\targs = tuple",
    "\t\t}",
    "\t\tif keep, ok := predicate(args...).(bool); ok && keep {",
    "\t\t\tout = append(out, item)",
    "\t\t}",
    "\t}",
    "\treturn out",
    "}",
    "",
    "func jsArrayPush(target *[]any, values ...any) any {",
    "\t*target = append(*target, values...)",
    "\treturn len(*target)",
    "}",
    "",
    "type jsMemberCallable interface {",
    "\tjsCallMember(property string, args ...any) any",
    "}",
    "",
    "func jsCallMember(value any, property string, args ...any) any {",
    "\tif target, ok := value.(jsMemberCallable); ok {",
    "\t\treturn target.jsCallMember(property, args...)",
    "\t}",
    '\tpanic(fmt.Sprintf("unsupported member call %T.%s", value, property))',
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
    arrays: new Set(),
    functions: parentCtx.functions,
    externalFunctions: parentCtx.externalFunctions,
    externalNamespaces: parentCtx.externalNamespaces,
    externalMembers: parentCtx.externalMembers,
    externalConstructors: parentCtx.externalConstructors,
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
      const isArrayInit =
        stmt.init?.kind === "array" || stmt.init?.kind === "array-spread";
      if (ctx.declared.has(name)) {
        if (isArrayInit) {
          ctx.arrays.add(name);
        } else {
          ctx.arrays.delete(name);
        }
        return `${indent}${name} = ${init}`;
      }
      ctx.declared.add(name);
      if (isArrayInit) {
        ctx.arrays.add(name);
      }
      if (init === "nil") {
        return `${indent}var ${name} any = nil`;
      }
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
    case "switch": {
      const discriminant = renderExpr(stmt.discriminant, ctx);
      const lines = [`${indent}switch {`];
      for (const switchCase of stmt.cases ?? []) {
        if (switchCase.test) {
          lines.push(
            `${indent}case jsStrictEqual(${discriminant}, ${renderExpr(
              switchCase.test,
              ctx,
            )}):`,
          );
        } else {
          lines.push(`${indent}default:`);
        }
        lines.push(
          renderStmtBlock(switchCase.consequent ?? [], ctx, indentLevel + 1),
        );
      }
      lines.push(`${indent}}`);
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
    case "for-of": {
      const name = goIdent(stmt.left);
      const right = renderExpr(stmt.right, ctx);
      const bodyCtx = {
        ...ctx,
        declared: new Set([...ctx.declared, name]),
        arrays: new Set(ctx.arrays),
      };
      ctx.declared.add(name);
      return [
        `${indent}for _, ${name} := range jsIterable(${right}) {`,
        renderStmtBlock(stmt.body ?? [], bodyCtx, indentLevel + 1),
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
    case "throw":
      return `${indent}panic(${renderExpr(stmt.value, ctx)})`;
    case "try": {
      const catchParam = goIdent(stmt.catchParam ?? "error");
      const catchCtx = {
        ...ctx,
        declared: new Set([...ctx.declared, catchParam]),
        arrays: new Set(ctx.arrays),
      };
      const catchBody = renderStmtBlock(
        stmt.catchBody ?? [],
        catchCtx,
        indentLevel + 2,
      );
      const finallyBody = renderStmtBlock(
        stmt.finallyBody ?? [],
        ctx,
        indentLevel,
      );
      const lines = [
        `${indent}func() {`,
        `${indent}\tdefer func() {`,
        `${indent}\t\tif recovered := recover(); recovered != nil {`,
        `${indent}\t\t\t${catchParam} := jsRecoverValue(recovered)`,
        catchBody,
        `${indent}\t\t}`,
        `${indent}\t}()`,
        renderStmtBlock(stmt.body ?? [], ctx, indentLevel + 1),
        `${indent}}()`,
        finallyBody,
      ];
      return lines.filter(Boolean).join("\n");
    }
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
      const isArrayInit =
        stmt.init?.kind === "array" || stmt.init?.kind === "array-spread";
      if (ctx.declared.has(name)) {
        if (isArrayInit) {
          ctx.arrays.add(name);
        } else {
          ctx.arrays.delete(name);
        }
        return `${name} = ${init}`;
      }
      ctx.declared.add(name);
      if (isArrayInit) {
        ctx.arrays.add(name);
      }
      if (init === "nil") {
        return `var ${name} any = nil`;
      }
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
      if (expr.name === "undefined") {
        return "nil";
      }
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
    case "unary":
      if (expr.op === "!") {
        return `(!jsTruthy(${renderExpr(expr.arg, ctx)}))`;
      }
      if (expr.op === "typeof") {
        return `jsTypeof(${renderExpr(expr.arg, ctx)})`;
      }
      if (expr.op === "+") {
        return `jsNumber(${renderExpr(expr.arg, ctx)})`;
      }
      if (expr.op === "-") {
        return `(-jsNumber(${renderExpr(expr.arg, ctx)}))`;
      }
      throw new Error(`EXECUTABLE_IR_UNSUPPORTED_UNARY:${expr.op}`);
    case "binary":
      if (expr.op === "??") {
        return `jsNullish(${renderExpr(expr.left, ctx)}, ${renderExpr(
          expr.right,
          ctx,
        )})`;
      }
      if (expr.op === "===") {
        return `jsStrictEqual(${renderExpr(expr.left, ctx)}, ${renderExpr(
          expr.right,
          ctx,
        )})`;
      }
      if (expr.op === "!==") {
        return `(!jsStrictEqual(${renderExpr(expr.left, ctx)}, ${renderExpr(
          expr.right,
          ctx,
        )}))`;
      }
      return `(${renderExpr(expr.left, ctx)} ${expr.op} ${renderExpr(expr.right, ctx)})`;
    case "await": {
      const timerPromise = renderTimerPromise(expr.arg, ctx);
      if (timerPromise) {
        return timerPromise;
      }
      return `jsAwait(${renderExpr(expr.arg, ctx)})`;
    }
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
      if (expr.callee?.kind === "member") {
        const args = (expr.args ?? []).map((arg) => renderExpr(arg, ctx));
        if (
          expr.callee.property === "push" &&
          expr.callee.object?.kind === "ident" &&
          ctx.arrays.has(goIdent(expr.callee.object.name))
        ) {
          return `jsArrayPush(&${goIdent(expr.callee.object.name)}, ${args.join(
            ", ",
          )})`;
        }
        return `jsCallMember(${renderExpr(
          expr.callee.object,
          ctx,
        )}, ${JSON.stringify(expr.callee.property)}${
          args.length ? `, ${args.join(", ")}` : ""
        })`;
      }
      if (
        expr.callee?.kind !== "ident" ||
        !ctx.functions.has(expr.callee.name)
      ) {
        throw new Error(
          `EXECUTABLE_IR_UNSUPPORTED_CALL:${expr.callee?.name ?? expr.callee?.kind ?? "unknown"}`,
        );
      }
      return `js_${goIdent(expr.callee.name)}(${(expr.args ?? [])
        .map((arg) => renderExpr(arg, ctx))
        .join(", ")})`;
    }
    case "new":
      if (
        expr.callee?.kind === "ident" &&
        ctx.externalConstructors.has(expr.callee.name)
      ) {
        return `js_new_${goIdent(expr.callee.name)}(${(expr.args ?? [])
          .map((arg) => renderExpr(arg, ctx))
          .join(", ")})`;
      }
      throw new Error(
        `EXECUTABLE_IR_UNSUPPORTED_NEW:${expr.callee?.name ?? "unknown"}`,
      );
    case "assign":
      if (expr.op === "=" && expr.left?.kind === "member") {
        return `jsSetMember(${renderExpr(
          expr.left.object,
          ctx,
        )}, ${JSON.stringify(expr.left.property)}, ${renderExpr(
          expr.right,
          ctx,
        )})`;
      }
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

function renderTimerPromise(expr, ctx) {
  if (
    expr?.kind !== "new" ||
    expr.callee?.kind !== "ident" ||
    expr.callee.name !== "Promise" ||
    expr.args?.length !== 1
  ) {
    return null;
  }
  const executor = expr.args[0];
  const returned = executor?.body?.find((stmt) => stmt?.kind === "return");
  const call = returned?.value;
  if (
    call?.kind !== "call" ||
    call.callee?.kind !== "ident" ||
    call.callee.name !== "setTimeout" ||
    call.args?.length < 2
  ) {
    return null;
  }
  return `jsSleepPromise(${renderExpr(call.args[1], ctx)})`;
}

function renderBuiltinCall(expr, ctx) {
  if (
    expr.callee?.kind === "member" &&
    expr.callee.property === "map" &&
    expr.args?.length === 1 &&
    expr.args[0]?.kind === "function"
  ) {
    return `jsArrayMap(${renderExpr(expr.callee.object, ctx)}, ${renderMapperFunction(
      expr.args[0],
      ctx,
    )})`;
  }
  if (
    expr.callee?.kind === "member" &&
    expr.callee.property === "filter" &&
    expr.args?.length === 1 &&
    expr.args[0]?.kind === "function"
  ) {
    return `jsArrayFilter(${renderExpr(expr.callee.object, ctx)}, ${renderMapperFunction(
      expr.args[0],
      ctx,
    )})`;
  }
  if (
    expr.callee?.kind === "member" &&
    expr.callee.property === "join" &&
    expr.args?.length === 1
  ) {
    return `jsArrayJoin(${renderExpr(expr.callee.object, ctx)}, ${renderExpr(
      expr.args[0],
      ctx,
    )})`;
  }
  if (
    expr.callee?.kind === "member" &&
    expr.callee.property === "split" &&
    expr.args?.length === 1
  ) {
    return `jsStringSplit(${renderExpr(expr.callee.object, ctx)}, ${renderExpr(
      expr.args[0],
      ctx,
    )})`;
  }
  const args = (expr.args ?? []).map((arg) => renderExpr(arg, ctx)).join(", ");
  if (expr.callee?.kind === "ident" && expr.callee.name === "String") {
    return `jsString(${args})`;
  }
  if (expr.callee?.kind === "ident" && expr.callee.name === "join") {
    return `jsPathJoin(${args})`;
  }
  if (expr.callee?.kind === "ident" && expr.callee.name === "dirname") {
    return `jsPathDirname(${args})`;
  }
  if (expr.callee?.kind === "ident" && expr.callee.name === "tmpdir") {
    return "jsTmpdir()";
  }
  if (expr.callee?.kind === "ident" && expr.callee.name === "mkdtempSync") {
    return `jsMkdtempSync(${args})`;
  }
  if (expr.callee?.kind === "ident" && expr.callee.name === "rmSync") {
    return `jsRmSync(${args})`;
  }
  if (expr.callee?.kind === "ident" && expr.callee.name === "writeFileSync") {
    return `jsWriteFileSync(${args})`;
  }
  if (
    expr.callee?.kind === "member" &&
    expr.callee.object?.kind === "ident" &&
    expr.callee.object.name === "Reflect" &&
    expr.callee.property === "deleteProperty"
  ) {
    return `jsReflectDeleteProperty(${args})`;
  }
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

function renderMapperFunction(expr, parentCtx) {
  const params = expr.params ?? [];
  const ctx = {
    declared: new Set(params.map(goIdent)),
    arrays: new Set(),
    functions: parentCtx.functions,
    externalFunctions: parentCtx.externalFunctions,
    externalNamespaces: parentCtx.externalNamespaces,
    externalMembers: parentCtx.externalMembers,
    externalConstructors: parentCtx.externalConstructors,
  };
  const paramDecls = params
    .map((param, index) => `\t${goIdent(param)} := jsArgs[${index}]`)
    .join("\n");
  return [
    "func(jsArgs ...any) any {",
    paramDecls,
    renderStmtBlock(expr.body ?? [], ctx, 1),
    "\treturn nil",
    "}",
  ]
    .filter(Boolean)
    .join("\n");
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
