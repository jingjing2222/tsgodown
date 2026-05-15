use crate::backend::{BackendEmitRequest, BackendEmitResponse, BackendProvider};
use crate::emit_go::emit_go_project;

pub struct GoBackendProvider;

pub static GO_BACKEND_PROVIDER: GoBackendProvider = GoBackendProvider;

impl BackendProvider for GoBackendProvider {
    fn name(&self) -> &'static str {
        "go"
    }

    fn emit(&self, request: BackendEmitRequest) -> BackendEmitResponse {
        emit_go_project(request)
    }
}
