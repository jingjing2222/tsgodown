use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct InputManifest {
    pub entry: String,
    #[serde(default)]
    pub framework: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
pub struct AnalyzeConfig {
    #[serde(default)]
    pub profile: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct AnalyzeRequest {
    pub manifest: InputManifest,
    #[serde(default)]
    pub cwd: Option<String>,
    #[serde(default)]
    pub config: AnalyzeConfig,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct IrDocument {
    pub version: String,
    pub entry: String,
    #[serde(default)]
    pub modules: Vec<Module>,
    #[serde(default)]
    pub routes: Vec<Route>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct Module {
    pub id: String,
    pub source_path: String,
    #[serde(default)]
    pub exports: Vec<String>,
    #[serde(default)]
    pub imports: Vec<Import>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub executable: Option<ExecutableModule>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Import {
    pub spec: String,
    pub kind: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub resolved: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub bindings: Vec<ImportBinding>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ImportBinding {
    pub local: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub imported: Option<String>,
    pub kind: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ExecutableModule {
    #[serde(default)]
    pub stmts: Vec<JsStmt>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "kind")]
pub enum JsStmt {
    #[serde(rename = "expr")]
    Expr { expr: JsExpr },
    #[serde(rename = "function-decl")]
    FunctionDecl {
        name: String,
        params: Vec<String>,
        #[serde(default, skip_serializing_if = "Option::is_none", rename = "restParam")]
        rest_param: Option<String>,
        r#async: bool,
        #[serde(default, skip_serializing_if = "is_false")]
        generator: bool,
        body: Vec<JsStmt>,
    },
    #[serde(rename = "class-decl")]
    ClassDecl {
        name: String,
        #[serde(skip_serializing_if = "Option::is_none", rename = "superClass")]
        super_class: Option<JsExpr>,
        methods: Vec<JsClassMethod>,
    },
    #[serde(rename = "if")]
    If {
        test: JsExpr,
        consequent: Vec<JsStmt>,
        alternate: Vec<JsStmt>,
    },
    #[serde(rename = "for")]
    For {
        init: Vec<JsStmt>,
        #[serde(skip_serializing_if = "Option::is_none")]
        test: Option<JsExpr>,
        #[serde(skip_serializing_if = "Option::is_none")]
        update: Option<JsExpr>,
        body: Vec<JsStmt>,
    },
    #[serde(rename = "for-of")]
    ForOf {
        left: String,
        right: JsExpr,
        body: Vec<JsStmt>,
    },
    #[serde(rename = "while")]
    While { test: JsExpr, body: Vec<JsStmt> },
    #[serde(rename = "switch")]
    Switch {
        discriminant: JsExpr,
        cases: Vec<JsSwitchCase>,
    },
    #[serde(rename = "try")]
    Try {
        body: Vec<JsStmt>,
        #[serde(skip_serializing_if = "Option::is_none", rename = "catchParam")]
        catch_param: Option<String>,
        #[serde(rename = "catchBody")]
        catch_body: Vec<JsStmt>,
        #[serde(rename = "finallyBody")]
        finally_body: Vec<JsStmt>,
    },
    #[serde(rename = "break")]
    Break {
        #[serde(skip_serializing_if = "Option::is_none")]
        label: Option<String>,
    },
    #[serde(rename = "continue")]
    Continue {
        #[serde(skip_serializing_if = "Option::is_none")]
        label: Option<String>,
    },
    #[serde(rename = "return")]
    Return {
        #[serde(skip_serializing_if = "Option::is_none")]
        value: Option<JsExpr>,
    },
    #[serde(rename = "throw")]
    Throw { value: JsExpr },
    #[serde(rename = "yield")]
    Yield {
        #[serde(skip_serializing_if = "Option::is_none")]
        value: Option<JsExpr>,
    },
    #[serde(rename = "var-decl")]
    VarDecl {
        name: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        init: Option<JsExpr>,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct JsSwitchCase {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub test: Option<JsExpr>,
    pub consequent: Vec<JsStmt>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct JsClassMethod {
    pub name: String,
    pub kind: String,
    pub is_static: bool,
    pub params: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none", rename = "restParam")]
    pub rest_param: Option<String>,
    pub r#async: bool,
    #[serde(default, skip_serializing_if = "is_false")]
    pub generator: bool,
    pub body: Vec<JsStmt>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "kind")]
pub enum JsExpr {
    #[serde(rename = "value")]
    Value { value: JsValue },
    #[serde(rename = "ident")]
    Ident { name: String },
    #[serde(rename = "this")]
    This,
    #[serde(rename = "array")]
    Array { items: Vec<JsExpr> },
    #[serde(rename = "array-spread")]
    ArraySpread { items: Vec<JsArrayElement> },
    #[serde(rename = "object")]
    Object { props: Vec<JsObjectProp> },
    #[serde(rename = "function")]
    Function {
        params: Vec<String>,
        #[serde(default, skip_serializing_if = "Option::is_none", rename = "restParam")]
        rest_param: Option<String>,
        r#async: bool,
        #[serde(default, skip_serializing_if = "is_false")]
        generator: bool,
        #[serde(default, rename = "lexicalThis")]
        lexical_this: bool,
        body: Vec<JsStmt>,
    },
    #[serde(rename = "class")]
    Class {
        #[serde(skip_serializing_if = "Option::is_none", rename = "superClass")]
        super_class: Option<Box<JsExpr>>,
        methods: Vec<JsClassMethod>,
    },
    #[serde(rename = "unary")]
    Unary { op: String, arg: Box<JsExpr> },
    #[serde(rename = "await")]
    Await { arg: Box<JsExpr> },
    #[serde(rename = "binary")]
    Binary {
        op: String,
        left: Box<JsExpr>,
        right: Box<JsExpr>,
    },
    #[serde(rename = "conditional")]
    Conditional {
        test: Box<JsExpr>,
        consequent: Box<JsExpr>,
        alternate: Box<JsExpr>,
    },
    #[serde(rename = "assign")]
    Assign {
        op: String,
        left: Box<JsExpr>,
        right: Box<JsExpr>,
    },
    #[serde(rename = "update")]
    Update {
        op: String,
        arg: Box<JsExpr>,
        prefix: bool,
    },
    #[serde(rename = "call")]
    Call {
        callee: Box<JsExpr>,
        args: Vec<JsExpr>,
        #[serde(default, skip_serializing_if = "is_false")]
        optional: bool,
    },
    #[serde(rename = "spread")]
    Spread { arg: Box<JsExpr> },
    #[serde(rename = "new")]
    New {
        callee: Box<JsExpr>,
        args: Vec<JsExpr>,
    },
    #[serde(rename = "member")]
    Member {
        object: Box<JsExpr>,
        property: String,
        #[serde(skip_serializing_if = "Option::is_none", rename = "propertyExpr")]
        property_expr: Option<Box<JsExpr>>,
        #[serde(default, skip_serializing_if = "is_false")]
        optional: bool,
    },
    #[serde(rename = "template")]
    Template {
        quasis: Vec<String>,
        exprs: Vec<JsExpr>,
    },
    #[serde(rename = "sequence")]
    Sequence { exprs: Vec<JsExpr> },
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct JsObjectProp {
    pub key: String,
    #[serde(skip_serializing_if = "Option::is_none", rename = "keyExpr")]
    pub key_expr: Option<JsExpr>,
    pub value: JsExpr,
    #[serde(default, skip_serializing_if = "is_false")]
    pub spread: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct JsArrayElement {
    pub spread: bool,
    pub value: JsExpr,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "kind")]
pub enum JsValue {
    #[serde(rename = "undefined")]
    Undefined,
    #[serde(rename = "null")]
    Null,
    #[serde(rename = "bool")]
    Bool { value: bool },
    #[serde(rename = "number")]
    Number { value: String },
    #[serde(rename = "string")]
    String { value: String },
    #[serde(rename = "bigint")]
    BigInt { value: String },
    #[serde(rename = "regexp")]
    RegExp { pattern: String, flags: String },
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Route {
    pub method: String,
    pub path: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Diagnostic {
    pub level: DiagnosticLevel,
    pub code: String,
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source: Option<DiagnosticSource>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct DiagnosticSource {
    pub file: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub line: Option<i32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub column: Option<i32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub via_source_map: Option<bool>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum DiagnosticLevel {
    Info,
    Warning,
    Error,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct AnalyzeResponse {
    pub ir: IrDocument,
    #[serde(default)]
    pub diagnostics: Vec<Diagnostic>,
}

fn is_false(value: &bool) -> bool {
    !*value
}
