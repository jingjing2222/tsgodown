#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProgramIR {
    pub modules: Vec<ModuleIR>,
    pub routes: Vec<RouteIR>,
    pub handlers: Vec<HandlerIR>,
    pub diagnostics: Vec<DiagnosticIR>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ModuleIR {
    pub id: String,
    pub source_path: String,
    pub exports: Vec<String>,
    pub imports: Vec<ImportIR>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ImportIR {
    pub spec: String,
    pub kind: String,
    pub resolved: Option<String>,
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
}
