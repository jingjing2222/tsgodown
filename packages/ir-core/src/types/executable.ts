export interface ExecutableModuleIR {
  stmts: JsStmtIR[];
}

export type JsStmtIR =
  | { kind: "expr"; expr: JsExprIR }
  | { kind: "return"; value?: JsExprIR }
  | { kind: "throw"; value: JsExprIR }
  | { kind: "var-decl"; name: string; init?: JsExprIR };

export type JsExprIR =
  | { kind: "value"; value: JsValueIR }
  | { kind: "ident"; name: string }
  | { kind: "call"; callee: JsExprIR; args: JsExprIR[] }
  | { kind: "member"; object: JsExprIR; property: string };

export type JsValueIR =
  | { kind: "undefined" }
  | { kind: "null" }
  | { kind: "bool"; value: boolean }
  | { kind: "number"; value: string }
  | { kind: "string"; value: string };
