use std::collections::HashMap;

use crate::ast::capture_call_args;
use crate::defs::{HandlerDef, PluginDef};
use crate::ir::{DiagnosticIR, HandlerIR, RouteIR};
use crate::register::analyze_register_call;
use crate::routes::{extract_route_object, extract_shorthand_route};

#[allow(clippy::too_many_arguments)]
pub(crate) fn analyze_scope(
    body: &str,
    file: &str,
    instance_name: &str,
    prefix: &str,
    plugin_defs: &HashMap<String, PluginDef>,
    handler_defs: &HashMap<String, HandlerDef>,
    routes: &mut Vec<RouteIR>,
    handlers: &mut Vec<HandlerIR>,
    diagnostics: &mut Vec<DiagnosticIR>,
) {
    let mut idx = 0usize;
    while let Some(pos) = body[idx..].find(&format!("{}.", instance_name)) {
        let start = idx + pos;
        let tail = &body[start + instance_name.len() + 1..];
        let mut advanced = false;

        for method in ["get", "post", "put", "delete", "patch"] {
            if let Some(args) = capture_call_args(tail, method) {
                extract_shorthand_route(
                    &args,
                    file,
                    method,
                    instance_name,
                    prefix,
                    routes,
                    handlers,
                    handler_defs,
                    diagnostics,
                );
                idx = start + instance_name.len() + 1 + method.len() + args.len() + 2;
                advanced = true;
                break;
            }
        }
        if advanced {
            continue;
        }

        if let Some(args) = capture_call_args(tail, "route") {
            extract_route_object(
                &args,
                file,
                instance_name,
                prefix,
                routes,
                handlers,
                handler_defs,
                diagnostics,
            );
            idx = start + instance_name.len() + 1 + "route".len() + args.len() + 2;
            continue;
        }

        if let Some(args) = capture_call_args(tail, "register") {
            analyze_register_call(
                &args,
                file,
                instance_name,
                prefix,
                plugin_defs,
                handler_defs,
                routes,
                handlers,
                diagnostics,
            );
            idx = start + instance_name.len() + 1 + "register".len() + args.len() + 2;
            continue;
        }

        idx = start + 1;
    }
}
