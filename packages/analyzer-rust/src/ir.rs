#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProgramIR {
    pub modules: Vec<ModuleIR>,
    pub routes: Vec<RouteIR>,
    pub handlers: Vec<HandlerIR>,
    pub diagnostics: Vec<DiagnosticIR>,
}

impl ProgramIR {
    pub fn normalize(mut self) -> Self {
        for module in &mut self.modules {
            module.exports.sort();
            module.imports.sort_by(|a, b| {
                (&a.spec, &a.kind, &a.resolved, &a.bindings).cmp(&(
                    &b.spec,
                    &b.kind,
                    &b.resolved,
                    &b.bindings,
                ))
            });
        }

        self.modules
            .sort_by(|a, b| (&a.id, &a.source_path).cmp(&(&b.id, &b.source_path)));
        self.routes.sort_by(|a, b| {
            (&a.path, &a.method, &a.handler_ref).cmp(&(&b.path, &b.method, &b.handler_ref))
        });
        self.handlers.sort_by(|a, b| a.id.cmp(&b.id));
        self.diagnostics.sort_by(|a, b| {
            (
                diagnostic_level_rank(&a.level),
                &a.code,
                &a.message,
                diagnostic_source_sort_key(&a.source),
            )
                .cmp(&(
                    diagnostic_level_rank(&b.level),
                    &b.code,
                    &b.message,
                    diagnostic_source_sort_key(&b.source),
                ))
        });

        self
    }
}

fn diagnostic_level_rank(level: &str) -> u8 {
    match level {
        "error" => 0,
        "warn" => 1,
        "info" => 2,
        _ => 3,
    }
}

fn diagnostic_source_sort_key(source: &Option<DiagnosticSourceIR>) -> (&str, i32, i32) {
    match source {
        Some(source) => (
            &source.file,
            source.line.unwrap_or(i32::MAX),
            source.column.unwrap_or(i32::MAX),
        ),
        None => ("", i32::MAX, i32::MAX),
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ModuleIR {
    pub id: String,
    pub source_path: String,
    pub exports: Vec<String>,
    pub imports: Vec<ImportIR>,
    pub executable: Option<ExecutableModuleIR>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ImportIR {
    pub spec: String,
    pub kind: String,
    pub resolved: Option<String>,
    pub bindings: Vec<ImportBindingIR>,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct ImportBindingIR {
    pub local: String,
    pub imported: Option<String>,
    pub kind: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExecutableModuleIR {
    pub stmts: Vec<JsStmtIR>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum JsStmtIR {
    Expr(JsExprIR),
    FunctionDecl {
        name: String,
        params: Vec<String>,
        rest_param: Option<String>,
        r#async: bool,
        generator: bool,
        body: Vec<JsStmtIR>,
    },
    ClassDecl {
        name: String,
        super_class: Option<JsExprIR>,
        methods: Vec<JsClassMethodIR>,
    },
    If {
        test: JsExprIR,
        consequent: Vec<JsStmtIR>,
        alternate: Vec<JsStmtIR>,
    },
    For {
        init: Vec<JsStmtIR>,
        test: Option<JsExprIR>,
        update: Option<JsExprIR>,
        body: Vec<JsStmtIR>,
    },
    ForOf {
        left: String,
        right: JsExprIR,
        body: Vec<JsStmtIR>,
    },
    While {
        test: JsExprIR,
        body: Vec<JsStmtIR>,
    },
    DoWhile {
        body: Vec<JsStmtIR>,
        test: JsExprIR,
    },
    Switch {
        discriminant: JsExprIR,
        cases: Vec<JsSwitchCaseIR>,
    },
    Try {
        body: Vec<JsStmtIR>,
        catch_param: Option<String>,
        catch_body: Vec<JsStmtIR>,
        finally_body: Vec<JsStmtIR>,
    },
    Label {
        label: String,
        body: Vec<JsStmtIR>,
    },
    Break(Option<String>),
    Continue(Option<String>),
    Return(Option<JsExprIR>),
    Throw(JsExprIR),
    Yield {
        value: Option<JsExprIR>,
        delegate: bool,
    },
    VarDecl {
        name: String,
        init: Option<JsExprIR>,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct JsSwitchCaseIR {
    pub test: Option<JsExprIR>,
    pub consequent: Vec<JsStmtIR>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct JsClassMethodIR {
    pub name: String,
    pub kind: String,
    pub is_static: bool,
    pub params: Vec<String>,
    pub rest_param: Option<String>,
    pub r#async: bool,
    pub generator: bool,
    pub body: Vec<JsStmtIR>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum JsExprIR {
    Value(JsValueIR),
    Ident(String),
    This,
    Super,
    Array(Vec<JsExprIR>),
    ArraySpread(Vec<JsArrayElementIR>),
    Object(Vec<JsObjectPropIR>),
    ObjectRest {
        object: Box<JsExprIR>,
        excluded: Vec<String>,
    },
    Function {
        params: Vec<String>,
        rest_param: Option<String>,
        r#async: bool,
        generator: bool,
        lexical_this: bool,
        body: Vec<JsStmtIR>,
    },
    Class {
        super_class: Option<Box<JsExprIR>>,
        methods: Vec<JsClassMethodIR>,
    },
    Unary {
        op: String,
        arg: Box<JsExprIR>,
    },
    Await {
        arg: Box<JsExprIR>,
    },
    Binary {
        op: String,
        left: Box<JsExprIR>,
        right: Box<JsExprIR>,
    },
    Conditional {
        test: Box<JsExprIR>,
        consequent: Box<JsExprIR>,
        alternate: Box<JsExprIR>,
    },
    Assign {
        op: String,
        left: Box<JsExprIR>,
        right: Box<JsExprIR>,
    },
    Update {
        op: String,
        arg: Box<JsExprIR>,
        prefix: bool,
    },
    Call {
        callee: Box<JsExprIR>,
        args: Vec<JsExprIR>,
        optional: bool,
    },
    Spread {
        arg: Box<JsExprIR>,
    },
    New {
        callee: Box<JsExprIR>,
        args: Vec<JsExprIR>,
    },
    Member {
        object: Box<JsExprIR>,
        property: String,
        computed: Option<Box<JsExprIR>>,
        optional: bool,
    },
    Template {
        quasis: Vec<String>,
        exprs: Vec<JsExprIR>,
    },
    Sequence(Vec<JsExprIR>),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct JsArrayElementIR {
    pub spread: bool,
    pub value: JsExprIR,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct JsObjectPropIR {
    pub key: String,
    pub key_expr: Option<JsExprIR>,
    pub value: JsExprIR,
    pub spread: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum JsValueIR {
    Undefined,
    Null,
    Bool(bool),
    Number(String),
    String(String),
    BigInt(String),
    RegExp { pattern: String, flags: String },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RouteIR {
    pub method: String,
    pub path: String,
    pub handler_ref: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HandlerIR {
    pub id: String,
    pub params: Vec<HandlerParamIR>,
    pub r#async: bool,
    pub semantics: Option<HandlerSemanticsIR>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HandlerParamIR {
    pub name: String,
    pub role: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HandlerSemanticsIR {
    pub response_mode: String,
    pub request_param: Option<String>,
    pub response_param: Option<String>,
    pub uses_status: bool,
    pub uses_body: bool,
    pub uses_headers: bool,
    pub uses_json: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DiagnosticIR {
    pub level: String,
    pub code: String,
    pub message: String,
    pub source: Option<DiagnosticSourceIR>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DiagnosticSourceIR {
    pub file: String,
    pub line: Option<i32>,
    pub column: Option<i32>,
    pub via_source_map: Option<bool>,
}
