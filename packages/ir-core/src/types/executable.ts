export interface ExecutableModuleIR {
  stmts: JsStmtIR[];
}

export type JsStmtIR =
  | { kind: "expr"; expr: JsExprIR }
  | {
      kind: "function-decl";
      name: string;
      params: string[];
      async: boolean;
      body: JsStmtIR[];
    }
  | {
      kind: "if";
      test: JsExprIR;
      consequent: JsStmtIR[];
      alternate: JsStmtIR[];
    }
  | { kind: "return"; value?: JsExprIR }
  | { kind: "throw"; value: JsExprIR }
  | { kind: "var-decl"; name: string; init?: JsExprIR };

export type JsExprIR =
  | { kind: "value"; value: JsValueIR }
  | { kind: "ident"; name: string }
  | { kind: "array"; items: JsExprIR[] }
  | { kind: "object"; props: JsObjectPropIR[] }
  | {
      kind: "function";
      params: string[];
      async: boolean;
      body: JsStmtIR[];
    }
  | { kind: "unary"; op: string; arg: JsExprIR }
  | { kind: "binary"; op: string; left: JsExprIR; right: JsExprIR }
  | { kind: "call"; callee: JsExprIR; args: JsExprIR[] }
  | { kind: "member"; object: JsExprIR; property: string };

export interface JsObjectPropIR {
  key: string;
  value: JsExprIR;
}

export type JsValueIR =
  | { kind: "undefined" }
  | { kind: "null" }
  | { kind: "bool"; value: boolean }
  | { kind: "number"; value: string }
  | { kind: "string"; value: string };
