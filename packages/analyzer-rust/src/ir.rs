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
                (&a.spec, &a.kind, &a.resolved).cmp(&(&b.spec, &b.kind, &b.resolved))
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
    pub line: Option<i32>,
    pub column: Option<i32>,
    pub via_source_map: Option<bool>,
}
