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
        r#async: bool,
        body: Vec<JsStmt>,
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
    #[serde(rename = "return")]
    Return {
        #[serde(skip_serializing_if = "Option::is_none")]
        value: Option<JsExpr>,
    },
    #[serde(rename = "throw")]
    Throw { value: JsExpr },
    #[serde(rename = "var-decl")]
    VarDecl {
        name: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        init: Option<JsExpr>,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "kind")]
pub enum JsExpr {
    #[serde(rename = "value")]
    Value { value: JsValue },
    #[serde(rename = "ident")]
    Ident { name: String },
    #[serde(rename = "array")]
    Array { items: Vec<JsExpr> },
    #[serde(rename = "object")]
    Object { props: Vec<JsObjectProp> },
    #[serde(rename = "function")]
    Function {
        params: Vec<String>,
        r#async: bool,
        body: Vec<JsStmt>,
    },
    #[serde(rename = "unary")]
    Unary { op: String, arg: Box<JsExpr> },
    #[serde(rename = "binary")]
    Binary {
        op: String,
        left: Box<JsExpr>,
        right: Box<JsExpr>,
    },
    #[serde(rename = "assign")]
    Assign {
        op: String,
        left: Box<JsExpr>,
        right: Box<JsExpr>,
    },
    #[serde(rename = "call")]
    Call {
        callee: Box<JsExpr>,
        args: Vec<JsExpr>,
    },
    #[serde(rename = "member")]
    Member {
        object: Box<JsExpr>,
        property: String,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct JsObjectProp {
    pub key: String,
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
