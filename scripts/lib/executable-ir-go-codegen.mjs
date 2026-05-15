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
  const functionDecls = collectTopLevelFunctionDecls(stmts);
  const classDecls = [
    ...stmts.filter((stmt) => stmt?.kind === "class-decl"),
    ...collectClassAliases(stmts),
  ];
  const classes = new Map(classDecls.map((stmt) => [stmt.name, stmt]));
  const ctx = {
    declared: new Set(),
    arrays: new Set(),
    classes,
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
      : stmts.filter(
          (stmt) =>
            !isTopLevelFunctionDecl(stmt) && stmt?.kind !== "class-decl",
        ),
    ctx,
    1,
  );
  const helperFunctions = functionDecls
    .filter((stmt) => stmt.name !== "main")
    .map((stmt) => renderFunctionDecl(stmt, ctx))
    .join("\n\n");
  const helperClasses = [
    ...classDecls.map((stmt) => renderClassDecl(stmt, ctx)),
    renderClassDispatch(classDecls),
  ]
    .filter(Boolean)
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
    'var jsModule = map[string]any{"exports": map[string]any{}}',
    "",
    "func jsRequire(name any) any {",
    "\tswitch fmt.Sprint(name) {",
    '\tcase "fs":',
    '\t\treturn map[string]any{"readFileSync": jsReadFileSync, "writeFileSync": jsWriteFileSync, "mkdtempSync": jsMkdtempSync, "rmSync": jsRmSync}',
    '\tcase "path":',
    '\t\treturn map[string]any{"join": jsPathJoin, "dirname": jsPathDirname}',
    '\tcase "os":',
    '\t\treturn map[string]any{"tmpdir": jsTmpdir}',
    '\tcase "crypto":',
    "\t\treturn map[string]any{}",
    "\tdefault:",
    "\t\treturn map[string]any{}",
    "\t}",
    "}",
    "",
    "func js_main() any {",
    body,
    "\treturn nil",
    "}",
    helperFunctions ? "" : null,
    helperFunctions,
    helperClasses ? "" : null,
    helperClasses,
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
    "func jsLogicalAnd(left any, right any) any {",
    "\tif !jsTruthy(left) {",
    "\t\treturn left",
    "\t}",
    "\treturn right",
    "}",
    "",
    "func jsLogicalOr(left any, right any) any {",
    "\tif jsTruthy(left) {",
    "\t\treturn left",
    "\t}",
    "\treturn right",
    "}",
    "",
    "func jsVoid(value any) any {",
    "\t_ = value",
    "\treturn nil",
    "}",
    "",
    "func jsSequence(values ...any) any {",
    "\tif len(values) == 0 {",
    "\t\treturn nil",
    "\t}",
    "\treturn values[len(values)-1]",
    "}",
    "",
    "func jsArg(args []any, index int) any {",
    "\tif index >= 0 && index < len(args) {",
    "\t\treturn args[index]",
    "\t}",
    "\treturn nil",
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
    "func jsBinaryArithmetic(op string, left any, right any) any {",
    "\tswitch op {",
    '\tcase "+":',
    "\t\tif _, ok := left.(string); ok {",
    "\t\t\treturn fmt.Sprint(left) + fmt.Sprint(right)",
    "\t\t}",
    "\t\tif _, ok := right.(string); ok {",
    "\t\t\treturn fmt.Sprint(left) + fmt.Sprint(right)",
    "\t\t}",
    "\t\treturn jsNumber(left) + jsNumber(right)",
    '\tcase "-":',
    "\t\treturn jsNumber(left) - jsNumber(right)",
    '\tcase "*":',
    "\t\treturn jsNumber(left) * jsNumber(right)",
    '\tcase "/":',
    "\t\treturn jsNumber(left) / jsNumber(right)",
    '\tcase "%":',
    "\t\treturn float64(int64(jsNumber(left)) % int64(jsNumber(right)))",
    "\tdefault:",
    '\t\tpanic(fmt.Sprintf("unsupported arithmetic op %s", op))',
    "\t}",
    "}",
    "",
    "func jsBitwise(op string, left any, right any) any {",
    "\tleftNumber := uint64(jsNumber(left))",
    "\trightNumber := uint(jsNumber(right))",
    "\tswitch op {",
    '\tcase "&":',
    "\t\treturn int64(leftNumber & uint64(jsNumber(right)))",
    '\tcase "|":',
    "\t\treturn int64(leftNumber | uint64(jsNumber(right)))",
    '\tcase "^":',
    "\t\treturn int64(leftNumber ^ uint64(jsNumber(right)))",
    '\tcase "<<":',
    "\t\treturn int64(leftNumber << rightNumber)",
    '\tcase ">>", ">>>":',
    "\t\treturn int64(leftNumber >> rightNumber)",
    "\tdefault:",
    '\t\tpanic(fmt.Sprintf("unsupported bitwise operator %s", op))',
    "\t}",
    "}",
    "",
    "func jsStrictEqual(left any, right any) bool {",
    "\treturn reflect.DeepEqual(left, right)",
    "}",
    "",
    "func jsLooseEqual(left any, right any) bool {",
    "\tif jsStrictEqual(left, right) {",
    "\t\treturn true",
    "\t}",
    "\tif left == nil || right == nil {",
    "\t\treturn left == nil && right == nil",
    "\t}",
    "\tif isJSNumberLike(left) || isJSNumberLike(right) {",
    "\t\treturn jsNumber(left) == jsNumber(right)",
    "\t}",
    "\treturn fmt.Sprint(left) == fmt.Sprint(right)",
    "}",
    "",
    "func isJSNumberLike(value any) bool {",
    "\tswitch value.(type) {",
    "\tcase int, int64, float64, bool:",
    "\t\treturn true",
    "\tdefault:",
    "\t\treturn false",
    "\t}",
    "}",
    "",
    "func jsRelationalCompare(op string, left any, right any) bool {",
    "\tleftNumber := jsNumber(left)",
    "\trightNumber := jsNumber(right)",
    "\tswitch op {",
    '\tcase "<":',
    "\t\treturn leftNumber < rightNumber",
    '\tcase "<=":',
    "\t\treturn leftNumber <= rightNumber",
    '\tcase ">":',
    "\t\treturn leftNumber > rightNumber",
    '\tcase ">=":',
    "\t\treturn leftNumber >= rightNumber",
    "\tdefault:",
    '\t\tpanic(fmt.Sprintf("unsupported relational operator %s", op))',
    "\t}",
    "}",
    "",
    "func jsInstanceOf(left any, right any) bool {",
    "\tinstance, ok := left.(*jsClassInstance)",
    "\tif !ok {",
    "\t\treturn false",
    "\t}",
    "\tswitch typed := right.(type) {",
    "\tcase *jsClassValue:",
    "\t\treturn instance.className == typed.name",
    "\tcase string:",
    "\t\treturn instance.className == typed",
    "\tdefault:",
    "\t\treturn false",
    "\t}",
    "}",
    "",
    "func jsAwait(value any) any {",
    "\treturn value",
    "}",
    "",
    "func jsCall(value any, args ...any) any {",
    "\tif callable, ok := value.(func(...any) any); ok {",
    "\t\treturn callable(args...)",
    "\t}",
    '\tpanic(fmt.Sprintf("value is not callable: %T", value))',
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
    "\tif instance, ok := value.(*jsClassInstance); ok {",
    "\t\treturn instance.fields[property]",
    "\t}",
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
    "func jsSymbol(description ...any) any {",
    '\tlabel := ""',
    "\tif len(description) > 0 {",
    "\t\tlabel = fmt.Sprint(description[0])",
    "\t}",
    '\treturn "Symbol(" + label + ")"',
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
    "func jsReadFileSync(target any, args ...any) any {",
    "\tbytes, err := os.ReadFile(fmt.Sprint(target))",
    "\tif err != nil {",
    "\t\tpanic(err)",
    "\t}",
    "\treturn string(bytes)",
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
    "\tif instance, ok := value.(*jsClassInstance); ok {",
    "\t\tinstance.fields[property] = next",
    "\t\treturn next",
    "\t}",
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
    "func jsHasProperty(value any, property any) bool {",
    "\tkey := fmt.Sprint(property)",
    "\tif instance, ok := value.(*jsClassInstance); ok {",
    "\t\t_, exists := instance.fields[key]",
    "\t\treturn exists",
    "\t}",
    "\tif object, ok := value.(map[string]any); ok {",
    "\t\t_, exists := object[key]",
    "\t\treturn exists",
    "\t}",
    "\treturn false",
    "}",
    "",
    "func jsNullishAssignMember(value any, property string, next any) any {",
    "\tcurrent := jsGet(value, property)",
    "\tif current == nil {",
    "\t\treturn jsSetMember(value, property, next)",
    "\t}",
    "\treturn current",
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
    "type jsSet struct {",
    "\titems []any",
    "}",
    "",
    "func jsNewSet(values ...any) any {",
    "\tset := &jsSet{items: []any{}}",
    "\tif len(values) > 0 {",
    "\t\tfor _, item := range jsIterable(values[0]) {",
    "\t\t\tset.add(item)",
    "\t\t}",
    "\t}",
    "\treturn set",
    "}",
    "",
    "func (set *jsSet) add(value any) {",
    "\tif !set.has(value) {",
    "\t\tset.items = append(set.items, value)",
    "\t}",
    "}",
    "",
    "func (set *jsSet) has(value any) bool {",
    "\tfor _, item := range set.items {",
    "\t\tif reflect.DeepEqual(item, value) {",
    "\t\t\treturn true",
    "\t\t}",
    "\t}",
    "\treturn false",
    "}",
    "",
    "func (set *jsSet) jsCallMember(property string, args ...any) any {",
    "\tswitch property {",
    '\tcase "add":',
    "\t\tset.add(jsArg(args, 0))",
    "\t\treturn set",
    '\tcase "has":',
    "\t\treturn set.has(jsArg(args, 0))",
    '\tcase "delete":',
    "\t\tfor index, item := range set.items {",
    "\t\t\tif reflect.DeepEqual(item, jsArg(args, 0)) {",
    "\t\t\t\tset.items = append(set.items[:index], set.items[index+1:]...)",
    "\t\t\t\treturn true",
    "\t\t\t}",
    "\t\t}",
    "\t\treturn false",
    '\tcase "values", "keys":',
    "\t\treturn append([]any{}, set.items...)",
    "\tdefault:",
    '\t\tpanic(fmt.Sprintf("unsupported Set member %s", property))',
    "\t}",
    "}",
    "",
    "type jsMapEntry struct {",
    "\tkey any",
    "\tvalue any",
    "}",
    "",
    "type jsMap struct {",
    "\tentries []jsMapEntry",
    "}",
    "",
    "func jsNewMap(values ...any) any {",
    "\tmapValue := &jsMap{entries: []jsMapEntry{}}",
    "\tif len(values) > 0 {",
    "\t\tfor _, item := range jsIterable(values[0]) {",
    "\t\t\tif pair, ok := item.([]any); ok && len(pair) >= 2 {",
    "\t\t\t\tmapValue.set(pair[0], pair[1])",
    "\t\t\t}",
    "\t\t}",
    "\t}",
    "\treturn mapValue",
    "}",
    "",
    "func (mapValue *jsMap) set(key any, value any) {",
    "\tfor index, entry := range mapValue.entries {",
    "\t\tif reflect.DeepEqual(entry.key, key) {",
    "\t\t\tmapValue.entries[index].value = value",
    "\t\t\treturn",
    "\t\t}",
    "\t}",
    "\tmapValue.entries = append(mapValue.entries, jsMapEntry{key: key, value: value})",
    "}",
    "",
    "func (mapValue *jsMap) get(key any) any {",
    "\tfor _, entry := range mapValue.entries {",
    "\t\tif reflect.DeepEqual(entry.key, key) {",
    "\t\t\treturn entry.value",
    "\t\t}",
    "\t}",
    "\treturn nil",
    "}",
    "",
    "func (mapValue *jsMap) has(key any) bool {",
    "\tfor _, entry := range mapValue.entries {",
    "\t\tif reflect.DeepEqual(entry.key, key) {",
    "\t\t\treturn true",
    "\t\t}",
    "\t}",
    "\treturn false",
    "}",
    "",
    "func (mapValue *jsMap) jsCallMember(property string, args ...any) any {",
    "\tswitch property {",
    '\tcase "set":',
    "\t\tmapValue.set(jsArg(args, 0), jsArg(args, 1))",
    "\t\treturn mapValue",
    '\tcase "get":',
    "\t\treturn mapValue.get(jsArg(args, 0))",
    '\tcase "has":',
    "\t\treturn mapValue.has(jsArg(args, 0))",
    '\tcase "keys":',
    "\t\tout := make([]any, 0, len(mapValue.entries))",
    "\t\tfor _, entry := range mapValue.entries {",
    "\t\t\tout = append(out, entry.key)",
    "\t\t}",
    "\t\treturn out",
    "\tdefault:",
    '\t\tpanic(fmt.Sprintf("unsupported Map member %s", property))',
    "\t}",
    "}",
    "",
    "func jsNewError(name string, args ...any) any {",
    '\tmessage := ""',
    "\tif len(args) > 0 {",
    "\t\tmessage = fmt.Sprint(args[0])",
    "\t}",
    '\treturn map[string]any{"name": name, "message": message}',
    "}",
    "",
    "func jsConstructFunction(name string, constructor func(any, ...any) any, args ...any) any {",
    "\tinstance := &jsClassInstance{className: name, fields: map[string]any{}}",
    "\tresult := constructor(instance, args...)",
    "\tif result != nil {",
    "\t\treturn result",
    "\t}",
    "\treturn instance",
    "}",
    "",
    "func jsNewOpaque(name string, args ...any) any {",
    "\t_ = args",
    "\treturn &jsClassInstance{className: name, fields: map[string]any{}}",
    "}",
    "",
    "func jsNewAbortController() any {",
    '\treturn map[string]any{"signal": map[string]any{"aborted": false}}',
    "}",
    "",
    "func jsNewURL(args ...any) any {",
    "\thref := fmt.Sprint(jsArg(args, 0))",
    '\treturn map[string]any{"href": href, "pathname": href}',
    "}",
    "",
    "func jsNewUint8Array(args ...any) any {",
    "\tif len(args) == 0 || args[0] == nil {",
    "\t\treturn []any{}",
    "\t}",
    "\tif length := int(jsNumber(args[0])); length > 0 && fmt.Sprint(args[0]) == fmt.Sprint(length) {",
    "\t\tout := make([]any, length)",
    "\t\tfor index := range out {",
    "\t\t\tout[index] = 0",
    "\t\t}",
    "\t\treturn out",
    "\t}",
    "\treturn jsIterable(args[0])",
    "}",
    "",
    "func jsNewArray(args ...any) any {",
    "\tif len(args) == 1 {",
    "\t\tif length := int(jsNumber(args[0])); length > 0 && fmt.Sprint(args[0]) == fmt.Sprint(length) {",
    "\t\t\tout := make([]any, length)",
    "\t\t\treturn out",
    "\t\t}",
    "\t}",
    "\treturn append([]any{}, args...)",
    "}",
    "",
    "type jsTextEncoder struct{}",
    "",
    "func (encoder *jsTextEncoder) jsCallMember(property string, args ...any) any {",
    "\tswitch property {",
    '\tcase "encode":',
    "\t\tbytes := []byte(fmt.Sprint(jsArg(args, 0)))",
    "\t\tout := make([]any, 0, len(bytes))",
    "\t\tfor _, value := range bytes {",
    "\t\t\tout = append(out, int(value))",
    "\t\t}",
    "\t\treturn out",
    "\tdefault:",
    '\t\tpanic(fmt.Sprintf("unsupported TextEncoder member %s", property))',
    "\t}",
    "}",
    "",
    "type jsTextDecoder struct{}",
    "",
    "func (decoder *jsTextDecoder) jsCallMember(property string, args ...any) any {",
    "\tswitch property {",
    '\tcase "decode":',
    "\t\titems := jsIterable(jsArg(args, 0))",
    "\t\tbytes := make([]byte, 0, len(items))",
    "\t\tfor _, item := range items {",
    "\t\t\tbytes = append(bytes, byte(jsNumber(item)))",
    "\t\t}",
    "\t\treturn string(bytes)",
    "\tdefault:",
    '\t\tpanic(fmt.Sprintf("unsupported TextDecoder member %s", property))',
    "\t}",
    "}",
    "",
    "func jsNewPromise(executor any) any {",
    "\tvar resolved any = nil",
    "\tresolve := func(args ...any) any {",
    "\t\tresolved = jsArg(args, 0)",
    "\t\treturn nil",
    "\t}",
    "\treject := func(args ...any) any {",
    "\t\tpanic(jsArg(args, 0))",
    "\t}",
    "\tjsCall(executor, resolve, reject)",
    "\treturn resolved",
    "}",
    "",
    "type jsMemberCallable interface {",
    "\tjsCallMember(property string, args ...any) any",
    "}",
    "",
    "type jsClassInstance struct {",
    "\tclassName string",
    "\tfields map[string]any",
    "}",
    "",
    "type jsControlFlow struct {",
    "\tkind string",
    "}",
    "",
    "type jsClassValue struct {",
    "\tname string",
    "}",
    "",
    "func (instance *jsClassInstance) jsCallMember(property string, args ...any) any {",
    "\treturn jsDispatchClassMember(instance, property, args...)",
    "}",
    "",
    "type jsRegExp struct {",
    "\tpattern string",
    "\tflags string",
    "}",
    "",
    "func (re *jsRegExp) jsCallMember(property string, args ...any) any {",
    "\tswitch property {",
    '\tcase "test":',
    "\t\tif len(args) == 0 {",
    "\t\t\treturn false",
    "\t\t}",
    "\t\treturn jsRegExpTest(re.pattern, re.flags, args[0])",
    "\tdefault:",
    '\t\tpanic(fmt.Sprintf("unsupported regexp member %s", property))',
    "\t}",
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

function collectTopLevelFunctionDecls(stmts) {
  const decls = stmts
    .map((stmt) => {
      if (stmt?.kind === "function-decl") {
        return stmt;
      }
      if (stmt?.kind === "var-decl" && stmt.init?.kind === "function") {
        return {
          kind: "function-decl",
          name: stmt.name,
          params: stmt.init.params ?? [],
          async: Boolean(stmt.init.async),
          body: stmt.init.body ?? [],
        };
      }
      return null;
    })
    .filter(Boolean);
  const known = new Set(decls.map((decl) => decl.name));
  for (const stmt of stmts) {
    if (
      stmt?.kind === "var-decl" &&
      stmt.init?.kind === "ident" &&
      known.has(stmt.init.name) &&
      !known.has(stmt.name)
    ) {
      decls.push({
        kind: "function-decl",
        name: stmt.name,
        params: [],
        async: false,
        aliasTarget: stmt.init.name,
        body: [],
      });
      known.add(stmt.name);
    }
  }
  return decls;
}

function isTopLevelFunctionDecl(stmt) {
  return (
    stmt?.kind === "function-decl" ||
    (stmt?.kind === "var-decl" && stmt.init?.kind === "function")
  );
}

function collectClassAliases(stmts) {
  const aliases = [];
  const classes = new Map(
    stmts
      .filter((stmt) => stmt?.kind === "class-decl")
      .map((stmt) => [stmt.name, stmt]),
  );
  const seen = new Set(classes.keys());
  for (let index = 0; index < stmts.length - 1; index += 1) {
    const current = stmts[index];
    const next = stmts[index + 1];
    if (
      current?.kind === "var-decl" &&
      !current.init &&
      /^_[A-Za-z0-9]*$/.test(String(current.name)) &&
      next?.kind === "class-decl" &&
      !seen.has(current.name)
    ) {
      aliases.push({ ...next, name: current.name });
      seen.add(current.name);
    }
    if (
      current?.kind === "expr" &&
      current.expr?.kind === "assign" &&
      current.expr.op === "=" &&
      current.expr.left?.kind === "ident" &&
      current.expr.right?.kind === "ident" &&
      classes.has(current.expr.right.name) &&
      !seen.has(current.expr.left.name)
    ) {
      aliases.push({
        ...classes.get(current.expr.right.name),
        name: current.expr.left.name,
      });
      seen.add(current.expr.left.name);
    }
  }
  return aliases;
}

function renderFunctionDecl(stmt, parentCtx) {
  if (stmt.aliasTarget) {
    return [
      `func js_${goIdent(stmt.name)}(jsArgs ...any) any {`,
      `\treturn js_${goIdent(stmt.aliasTarget)}(jsArgs...)`,
      "}",
      "",
      `func js_${goIdent(stmt.name)}_this(jsThis any, jsArgs ...any) any {`,
      `\treturn js_${goIdent(stmt.aliasTarget)}_this(jsThis, jsArgs...)`,
      "}",
    ].join("\n");
  }
  const params = (stmt.params ?? []).map(goIdent);
  const ctx = {
    declared: new Set(["jsThis", ...params]),
    arrays: new Set(),
    classes: parentCtx.classes,
    functions: parentCtx.functions,
    externalFunctions: parentCtx.externalFunctions,
    externalNamespaces: parentCtx.externalNamespaces,
    externalMembers: parentCtx.externalMembers,
    externalConstructors: parentCtx.externalConstructors,
    thisName: "jsThis",
  };
  return [
    `func js_${goIdent(stmt.name)}(jsArgs ...any) any {`,
    `\treturn js_${goIdent(stmt.name)}_this(nil, jsArgs...)`,
    "}",
    "",
    `func js_${goIdent(stmt.name)}_this(jsThis any, jsArgs ...any) any {`,
    "\tif jsThis == nil {",
    "\t\tjsThis = map[string]any{}",
    "\t}",
    ...params.map((param, index) => `\t${param} := jsArg(jsArgs, ${index})`),
    renderStmtBlock(stmt.body ?? [], ctx, 1),
    "\treturn nil",
    "}",
  ]
    .filter(Boolean)
    .join("\n");
}

function renderClassDecl(stmt, parentCtx) {
  const className = goIdent(stmt.name);
  const methods = stmt.methods ?? [];
  const constructorMethod = methods.find(
    (method) => method.kind === "constructor",
  );
  const methodDecls = methods
    .filter((method) => method.kind !== "constructor")
    .map((method) => renderClassMethodDecl(stmt, method, parentCtx))
    .join("\n\n");
  return [
    `func js_new_${className}(jsArgs ...any) any {`,
    `\tinstance := &jsClassInstance{className: ${JSON.stringify(stmt.name)}, fields: map[string]any{}}`,
    constructorMethod
      ? `\tjs_${className}_constructor(instance, jsArgs...)`
      : "\t_ = jsArgs",
    "\treturn instance",
    "}",
    "",
    constructorMethod
      ? renderClassMethodDecl(stmt, constructorMethod, parentCtx)
      : null,
    methodDecls ? "" : null,
    methodDecls,
  ]
    .filter(Boolean)
    .join("\n");
}

function renderClassMethodDecl(classDecl, method, parentCtx) {
  const params = (method.params ?? []).map(goIdent);
  const ctx = {
    declared: new Set(["jsThis", ...params]),
    arrays: new Set(),
    classes: parentCtx.classes,
    functions: parentCtx.functions,
    externalFunctions: parentCtx.externalFunctions,
    externalNamespaces: parentCtx.externalNamespaces,
    externalMembers: parentCtx.externalMembers,
    externalConstructors: parentCtx.externalConstructors,
    thisName: "jsThis",
  };
  return [
    `func js_${goIdent(classDecl.name)}_${goIdent(method.name)}(jsThis any, jsArgs ...any) any {`,
    ...params.map((param, index) => `\t${param} := jsArg(jsArgs, ${index})`),
    renderStmtBlock(method.body ?? [], ctx, 1),
    "\treturn nil",
    "}",
  ]
    .filter(Boolean)
    .join("\n");
}

function renderClassDispatch(classDecls) {
  const cases = [];
  for (const classDecl of classDecls) {
    for (const method of classDecl.methods ?? []) {
      if (method.kind === "constructor" || method.isStatic) {
        continue;
      }
      cases.push([
        `\tcase instance.className == ${JSON.stringify(classDecl.name)} && property == ${JSON.stringify(method.name)}:`,
        `\t\treturn js_${goIdent(classDecl.name)}_${goIdent(method.name)}(instance, args...)`,
      ]);
    }
  }
  return [
    "func jsDispatchClassMember(instance *jsClassInstance, property string, args ...any) any {",
    "\tswitch {",
    ...cases.flat(),
    "\tdefault:",
    '\t\tpanic(fmt.Sprintf("unsupported class member %s.%s", instance.className, property))',
    "\t}",
    "}",
  ].join("\n");
}

function renderStmtBlock(stmts, ctx, indentLevel) {
  return stmts
    .map((stmt) => renderStmt(stmt, ctx, indentLevel))
    .filter(Boolean)
    .join("\n");
}

function renderLoopBody(stmts, ctx, indentLevel) {
  const indent = "\t".repeat(indentLevel);
  const bodyCtx = {
    ...ctx,
    declared: new Set(ctx.declared),
    arrays: new Set(ctx.arrays),
    controlAsPanic: true,
  };
  return [
    `${indent}jsLoopControl := ""`,
    `${indent}func() {`,
    `${indent}\tdefer func() {`,
    `${indent}\t\tif recovered := recover(); recovered != nil {`,
    `${indent}\t\t\tif control, ok := recovered.(jsControlFlow); ok {`,
    `${indent}\t\t\t\tjsLoopControl = control.kind`,
    `${indent}\t\t\t\treturn`,
    `${indent}\t\t\t}`,
    `${indent}\t\t\tpanic(recovered)`,
    `${indent}\t\t}`,
    `${indent}\t}()`,
    renderStmtBlock(stmts, bodyCtx, indentLevel + 1),
    `${indent}}()`,
    `${indent}if jsLoopControl == "break" {`,
    `${indent}\tbreak`,
    `${indent}}`,
    `${indent}if jsLoopControl == "continue" {`,
    `${indent}\tcontinue`,
    `${indent}}`,
  ]
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
      if (!isArrayInit) {
        return `${indent}var ${name} any = ${init}`;
      }
      return `${indent}${name} := ${init}`;
    }
    case "return":
      return `${indent}return ${stmt.value ? renderExpr(stmt.value, ctx) : "nil"}`;
    case "expr":
      return `${indent}${renderExpr(stmt.expr, ctx)}`;
    case "function-decl":
      return "";
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
        `${indent}if jsTruthy(${renderExpr(stmt.test, ctx)}) {`,
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
      const initStmts = stmt.init ?? [];
      const init =
        initStmts.length === 1 && initStmts[0]?.kind !== "var-decl"
          ? renderSimpleStmt(initStmts[0], ctx)
          : "";
      const test = stmt.test ? `jsTruthy(${renderExpr(stmt.test, ctx)})` : "";
      const update = stmt.update ? renderExpr(stmt.update, ctx) : "";
      const prefix =
        initStmts.length > 1 || initStmts[0]?.kind === "var-decl"
          ? initStmts
              .map((child) => renderStmt(child, ctx, indentLevel))
              .filter(Boolean)
          : [];
      return [
        ...prefix,
        `${indent}for ${init}; ${test}; ${update} {`,
        renderLoopBody(stmt.body ?? [], ctx, indentLevel + 1),
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
        renderLoopBody(stmt.body ?? [], bodyCtx, indentLevel + 1),
        `${indent}}`,
      ]
        .filter(Boolean)
        .join("\n");
    }
    case "while":
      return [
        `${indent}for jsTruthy(${renderExpr(stmt.test, ctx)}) {`,
        renderLoopBody(stmt.body ?? [], ctx, indentLevel + 1),
        `${indent}}`,
      ]
        .filter(Boolean)
        .join("\n");
    case "break":
      if (ctx.controlAsPanic) {
        return `${indent}panic(jsControlFlow{kind: "break"})`;
      }
      return `${indent}break`;
    case "continue":
      if (ctx.controlAsPanic) {
        return `${indent}panic(jsControlFlow{kind: "continue"})`;
      }
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
        `${indent}\t\t\tif control, ok := recovered.(jsControlFlow); ok {`,
        `${indent}\t\t\t\tpanic(control)`,
        `${indent}\t\t\t}`,
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
      if (!isArrayInit) {
        return `var ${name} any = ${init}`;
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
    case "this":
      return ctx.thisName ?? "jsThis";
    case "ident":
      if (expr.name === "undefined") {
        return "nil";
      }
      if (expr.name === "require") {
        return "jsRequire";
      }
      if (expr.name === "module") {
        return "jsModule";
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
    case "function":
      return renderFunctionExpression(expr, ctx);
    case "class":
      return '&jsClassValue{name: ""}';
    case "sequence":
      return `jsSequence(${(expr.exprs ?? expr.items ?? [])
        .map((item) => renderExpr(item, ctx))
        .join(", ")})`;
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
      if (expr.op === "delete") {
        if (expr.arg?.kind === "member") {
          return `jsReflectDeleteProperty(${renderExpr(
            expr.arg.object,
            ctx,
          )}, ${JSON.stringify(expr.arg.property)})`;
        }
        return "true";
      }
      if (expr.op === "void") {
        return `jsVoid(${renderExpr(expr.arg, ctx)})`;
      }
      throw new Error(`EXECUTABLE_IR_UNSUPPORTED_UNARY:${expr.op}`);
    case "binary":
      if (expr.op === "&&") {
        return `jsLogicalAnd(${renderExpr(expr.left, ctx)}, ${renderExpr(
          expr.right,
          ctx,
        )})`;
      }
      if (expr.op === "||") {
        return `jsLogicalOr(${renderExpr(expr.left, ctx)}, ${renderExpr(
          expr.right,
          ctx,
        )})`;
      }
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
      if (expr.op === "==") {
        return `jsLooseEqual(${renderExpr(expr.left, ctx)}, ${renderExpr(
          expr.right,
          ctx,
        )})`;
      }
      if (expr.op === "!=") {
        return `(!jsLooseEqual(${renderExpr(expr.left, ctx)}, ${renderExpr(
          expr.right,
          ctx,
        )}))`;
      }
      if (["+", "-", "*", "/", "%"].includes(expr.op)) {
        return `jsBinaryArithmetic(${JSON.stringify(expr.op)}, ${renderExpr(
          expr.left,
          ctx,
        )}, ${renderExpr(expr.right, ctx)})`;
      }
      if (["&", "|", "^", "<<", ">>", ">>>"].includes(expr.op)) {
        return `jsBitwise(${JSON.stringify(expr.op)}, ${renderExpr(
          expr.left,
          ctx,
        )}, ${renderExpr(expr.right, ctx)})`;
      }
      if (expr.op === "instanceof") {
        return `jsInstanceOf(${renderExpr(expr.left, ctx)}, ${renderExpr(
          expr.right,
          ctx,
        )})`;
      }
      if (expr.op === "in") {
        return `jsHasProperty(${renderExpr(expr.right, ctx)}, ${renderExpr(
          expr.left,
          ctx,
        )})`;
      }
      if (["<", "<=", ">", ">="].includes(expr.op)) {
        return `jsRelationalCompare(${JSON.stringify(expr.op)}, ${renderExpr(
          expr.left,
          ctx,
        )}, ${renderExpr(expr.right, ctx)})`;
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
      return `jsTernary(jsTruthy(${renderExpr(expr.test, ctx)}), ${renderExpr(
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
        return `jsCall(${renderExpr(expr.callee, ctx)}${
          (expr.args ?? []).length
            ? `, ${(expr.args ?? []).map((arg) => renderExpr(arg, ctx)).join(", ")}`
            : ""
        })`;
      }
      return `js_${goIdent(expr.callee.name)}(${(expr.args ?? [])
        .map((arg) => renderExpr(arg, ctx))
        .join(", ")})`;
    }
    case "new":
      if (expr.callee?.kind === "ident" && expr.callee.name === "URL") {
        return `jsNewURL(${(expr.args ?? [])
          .map((arg) => renderExpr(arg, ctx))
          .join(", ")})`;
      }
      if (expr.callee?.kind === "ident" && expr.callee.name === "Array") {
        return `jsNewArray(${(expr.args ?? [])
          .map((arg) => renderExpr(arg, ctx))
          .join(", ")})`;
      }
      if (expr.callee?.kind === "ident" && expr.callee.name === "RegExp") {
        const args = expr.args ?? [];
        return `&jsRegExp{pattern: fmt.Sprint(${args[0] ? renderExpr(args[0], ctx) : '""'}), flags: fmt.Sprint(${args[1] ? renderExpr(args[1], ctx) : '""'})}`;
      }
      if (expr.callee?.kind === "ident" && expr.callee.name === "Promise") {
        return `jsNewPromise(${(expr.args ?? [])
          .map((arg) => renderExpr(arg, ctx))
          .join(", ")})`;
      }
      if (expr.callee?.kind === "ident" && expr.callee.name === "Set") {
        return `jsNewSet(${(expr.args ?? [])
          .map((arg) => renderExpr(arg, ctx))
          .join(", ")})`;
      }
      if (
        expr.callee?.kind === "ident" &&
        ["Map", "WeakMap"].includes(expr.callee.name)
      ) {
        return `jsNewMap(${(expr.args ?? [])
          .map((arg) => renderExpr(arg, ctx))
          .join(", ")})`;
      }
      if (expr.callee?.kind === "ident" && expr.callee.name === "WeakSet") {
        return `jsNewSet(${(expr.args ?? [])
          .map((arg) => renderExpr(arg, ctx))
          .join(", ")})`;
      }
      if (
        expr.callee?.kind === "ident" &&
        ["Error", "TypeError"].includes(expr.callee.name)
      ) {
        return `jsNewError(${JSON.stringify(expr.callee.name)}, ${(
          expr.args ?? []
        )
          .map((arg) => renderExpr(arg, ctx))
          .join(", ")})`;
      }
      if (
        expr.callee?.kind === "ident" &&
        expr.callee.name === "AbortController"
      ) {
        return "jsNewAbortController()";
      }
      if (expr.callee?.kind === "ident" && expr.callee.name === "Uint8Array") {
        return `jsNewUint8Array(${(expr.args ?? [])
          .map((arg) => renderExpr(arg, ctx))
          .join(", ")})`;
      }
      if (expr.callee?.kind === "ident" && expr.callee.name === "TextEncoder") {
        return "&jsTextEncoder{}";
      }
      if (expr.callee?.kind === "ident" && expr.callee.name === "TextDecoder") {
        return "&jsTextDecoder{}";
      }
      if (
        expr.callee?.kind === "ident" &&
        ctx.functions.has(expr.callee.name)
      ) {
        return `jsConstructFunction(${JSON.stringify(
          expr.callee.name,
        )}, js_${goIdent(expr.callee.name)}_this${
          (expr.args ?? []).length
            ? `, ${(expr.args ?? [])
                .map((arg) => renderExpr(arg, ctx))
                .join(", ")}`
            : ""
        })`;
      }
      if (expr.callee?.kind === "ident" && ctx.classes.has(expr.callee.name)) {
        return `js_new_${goIdent(expr.callee.name)}(${(expr.args ?? [])
          .map((arg) => renderExpr(arg, ctx))
          .join(", ")})`;
      }
      if (
        expr.callee?.kind === "ident" &&
        ctx.externalConstructors.has(expr.callee.name)
      ) {
        return `js_new_${goIdent(expr.callee.name)}(${(expr.args ?? [])
          .map((arg) => renderExpr(arg, ctx))
          .join(", ")})`;
      }
      if (expr.callee?.kind === "ident" && /^[A-Z]/.test(expr.callee.name)) {
        return `jsNewOpaque(${JSON.stringify(expr.callee.name)}${
          (expr.args ?? []).length
            ? `, ${(expr.args ?? [])
                .map((arg) => renderExpr(arg, ctx))
                .join(", ")}`
            : ""
        })`;
      }
      throw new Error(
        `EXECUTABLE_IR_UNSUPPORTED_NEW:${expr.callee?.name ?? "unknown"}`,
      );
    case "assign":
      return renderAssignmentExpression(expr, ctx);
    case "update":
      return renderUpdateExpression(expr, ctx);
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

function renderAssignmentExpression(expr, ctx) {
  const right = renderExpr(expr.right, ctx);
  if (expr.left?.kind === "ident") {
    const name = goIdent(expr.left.name);
    if (expr.op === "=") {
      return `func() any { ${name} = ${right}; return ${name} }()`;
    }
    if (expr.op === "??=") {
      return `func() any { if ${name} == nil { ${name} = ${right} }; return ${name} }()`;
    }
    const arithmeticOp = compoundArithmeticOperator(expr.op);
    if (arithmeticOp) {
      return `func() any { ${name} = jsBinaryArithmetic(${JSON.stringify(arithmeticOp)}, ${name}, ${right}); return ${name} }()`;
    }
  }
  if (expr.left?.kind === "member") {
    const object = renderExpr(expr.left.object, ctx);
    const property = JSON.stringify(expr.left.property);
    if (expr.op === "=") {
      return `jsSetMember(${object}, ${property}, ${right})`;
    }
    if (expr.op === "??=") {
      return `jsNullishAssignMember(${object}, ${property}, ${right})`;
    }
    const arithmeticOp = compoundArithmeticOperator(expr.op);
    if (arithmeticOp) {
      return `func() any { jsObject := ${object}; jsProperty := ${property}; jsNext := jsBinaryArithmetic(${JSON.stringify(arithmeticOp)}, jsGet(jsObject, jsProperty), ${right}); return jsSetMember(jsObject, jsProperty, jsNext) }()`;
    }
  }
  throw new Error(`EXECUTABLE_IR_UNSUPPORTED_ASSIGN:${expr.op}`);
}

function renderUpdateExpression(expr, ctx) {
  const delta = expr.op === "--" ? "-1" : "1";
  if (expr.arg?.kind === "ident") {
    const name = goIdent(expr.arg.name);
    if (expr.prefix) {
      return `func() any { ${name} = jsBinaryArithmetic("+", ${name}, ${delta}); return ${name} }()`;
    }
    return `func() any { jsOld := ${name}; ${name} = jsBinaryArithmetic("+", ${name}, ${delta}); return jsOld }()`;
  }
  if (expr.arg?.kind === "member") {
    const object = renderExpr(expr.arg.object, ctx);
    const property = JSON.stringify(expr.arg.property);
    if (expr.prefix) {
      return `func() any { jsObject := ${object}; jsProperty := ${property}; jsNext := jsBinaryArithmetic("+", jsGet(jsObject, jsProperty), ${delta}); jsSetMember(jsObject, jsProperty, jsNext); return jsNext }()`;
    }
    return `func() any { jsObject := ${object}; jsProperty := ${property}; jsOld := jsGet(jsObject, jsProperty); jsSetMember(jsObject, jsProperty, jsBinaryArithmetic("+", jsOld, ${delta})); return jsOld }()`;
  }
  throw new Error(`EXECUTABLE_IR_UNSUPPORTED_UPDATE:${expr.op}`);
}

function compoundArithmeticOperator(op) {
  return {
    "+=": "+",
    "-=": "-",
    "*=": "*",
    "/=": "/",
    "%=": "%",
  }[op];
}

function renderFunctionExpression(expr, parentCtx) {
  const params = (expr.params ?? []).map(goIdent);
  const ctx = {
    ...parentCtx,
    declared: new Set([...parentCtx.declared, ...params]),
    arrays: new Set(parentCtx.arrays),
  };
  return [
    "func(jsArgs ...any) any {",
    ...params.map((param, index) => `\t${param} := jsArg(jsArgs, ${index})`),
    renderStmtBlock(expr.body ?? [], ctx, 1),
    "\treturn nil",
    "}",
  ]
    .filter(Boolean)
    .join("\n");
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
  const args = (expr.args ?? []).map((arg) => renderExpr(arg, ctx)).join(", ");
  if (
    expr.callee?.kind === "member" &&
    expr.callee.object?.kind === "ident" &&
    ctx.classes.has(expr.callee.object.name)
  ) {
    const classDecl = ctx.classes.get(expr.callee.object.name);
    const method = (classDecl.methods ?? []).find(
      (candidate) =>
        candidate.isStatic && candidate.name === expr.callee.property,
    );
    if (method) {
      return `js_${goIdent(classDecl.name)}_${goIdent(method.name)}(nil${
        args ? `, ${args}` : ""
      })`;
    }
  }
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
  if (expr.callee?.kind === "ident" && expr.callee.name === "String") {
    return `jsString(${args})`;
  }
  if (expr.callee?.kind === "ident" && expr.callee.name === "Symbol") {
    return `jsSymbol(${args})`;
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
    classes: parentCtx.classes,
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
    case "bigint":
      return String(value.value).replace(/n$/, "");
    case "string":
      return JSON.stringify(value.value);
    case "regexp":
      return `&jsRegExp{pattern: ${JSON.stringify(value.pattern)}, flags: ${JSON.stringify(
        value.flags ?? "",
      )}}`;
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
