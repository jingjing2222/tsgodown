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

        if let Some(consumed) = analyze_call_chain(
            tail,
            file,
            instance_name,
            prefix,
            plugin_defs,
            handler_defs,
            routes,
            handlers,
            diagnostics,
        ) {
            idx = start + instance_name.len() + 1 + consumed;
            continue;
        }

        idx = start + 1;
    }
}

#[allow(clippy::too_many_arguments)]
fn analyze_call_chain(
    mut chain: &str,
    file: &str,
    instance_name: &str,
    prefix: &str,
    plugin_defs: &HashMap<String, PluginDef>,
    handler_defs: &HashMap<String, HandlerDef>,
    routes: &mut Vec<RouteIR>,
    handlers: &mut Vec<HandlerIR>,
    diagnostics: &mut Vec<DiagnosticIR>,
) -> Option<usize> {
    let original_len = chain.len();
    let mut matched_any = false;

    loop {
        let mut matched_here = false;

        for method in ["get", "post", "put", "delete", "patch"] {
            if let Some(args) = capture_call_args(chain, method) {
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
                let consumed = method.len() + args.len() + 2;
                chain = &chain[consumed..];
                matched_any = true;
                matched_here = true;
                break;
            }
        }

        if !matched_here {
            if let Some(args) = capture_call_args(chain, "route") {
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
                let consumed = "route".len() + args.len() + 2;
                chain = &chain[consumed..];
                matched_any = true;
                matched_here = true;
            }
        }

        if !matched_here {
            if let Some(args) = capture_call_args(chain, "register") {
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
                let consumed = "register".len() + args.len() + 2;
                chain = &chain[consumed..];
                matched_any = true;
                matched_here = true;
            }
        }

        if !matched_here {
            break;
        }

        let trimmed = chain.trim_start();
        if !trimmed.starts_with('.') {
            chain = trimmed;
            break;
        }
        chain = &trimmed[1..];
    }

    if matched_any {
        Some(original_len - chain.len())
    } else {
        None
    }
}
