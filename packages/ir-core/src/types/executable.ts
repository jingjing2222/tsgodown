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
      kind: "class-decl";
      name: string;
      superClass?: JsExprIR;
      methods: JsClassMethodIR[];
    }
  | {
      kind: "if";
      test: JsExprIR;
      consequent: JsStmtIR[];
      alternate: JsStmtIR[];
    }
  | {
      kind: "for";
      init: JsStmtIR[];
      test?: JsExprIR;
      update?: JsExprIR;
      body: JsStmtIR[];
    }
  | { kind: "for-of"; left: string; right: JsExprIR; body: JsStmtIR[] }
  | { kind: "while"; test: JsExprIR; body: JsStmtIR[] }
  | { kind: "switch"; discriminant: JsExprIR; cases: JsSwitchCaseIR[] }
  | {
      kind: "try";
      body: JsStmtIR[];
      catchParam?: string;
      catchBody: JsStmtIR[];
      finallyBody: JsStmtIR[];
    }
  | { kind: "break"; label?: string }
  | { kind: "continue"; label?: string }
  | { kind: "return"; value?: JsExprIR }
  | { kind: "throw"; value: JsExprIR }
  | { kind: "var-decl"; name: string; init?: JsExprIR };

export interface JsSwitchCaseIR {
  test?: JsExprIR;
  consequent: JsStmtIR[];
}

export interface JsClassMethodIR {
  name: string;
  kind: string;
  isStatic: boolean;
  params: string[];
  async: boolean;
  body: JsStmtIR[];
}

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
  | {
      kind: "class";
      superClass?: JsExprIR;
      methods: JsClassMethodIR[];
    }
  | { kind: "unary"; op: string; arg: JsExprIR }
  | { kind: "binary"; op: string; left: JsExprIR; right: JsExprIR }
  | { kind: "assign"; op: string; left: JsExprIR; right: JsExprIR }
  | { kind: "update"; op: string; arg: JsExprIR; prefix: boolean }
  | { kind: "call"; callee: JsExprIR; args: JsExprIR[] }
  | { kind: "new"; callee: JsExprIR; args: JsExprIR[] }
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
