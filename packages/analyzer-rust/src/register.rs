use std::collections::HashMap;

use crate::ast::{
    extract_handler_ref, extract_object_string_prop, parse_inline_plugin, split_top_level,
    unwrap_single_call_arg,
};
use crate::defs::{HandlerDef, PluginDef};
use crate::diagnostics::diag;
use crate::ir::{DiagnosticIR, HandlerIR, RouteIR};
use crate::routes::join_path;
use crate::traversal::analyze_scope;

#[allow(clippy::too_many_arguments)]
pub(crate) fn analyze_register_call(
    args: &str,
    file: &str,
    instance_name: &str,
    prefix: &str,
    plugin_defs: &HashMap<String, PluginDef>,
    handler_defs: &HashMap<String, HandlerDef>,
    routes: &mut Vec<RouteIR>,
    handlers: &mut Vec<HandlerIR>,
    diagnostics: &mut Vec<DiagnosticIR>,
) {
    let parts = split_top_level(args, ',');
    let plugin_expr = parts.first().map(|s| s.trim()).unwrap_or("");
    let options_expr = parts.get(1).map(|s| s.as_str()).unwrap_or("");
    let prefix_from_register =
        extract_object_string_prop(options_expr, "prefix").unwrap_or_default();
    let next_prefix = join_path(prefix, &prefix_from_register);

    let mut candidates = vec![plugin_expr.to_string()];
    let mut cursor = plugin_expr.to_string();
    while let Some(inner) = unwrap_single_call_arg(&cursor) {
        candidates.push(inner.clone());
        cursor = inner;
    }

    for candidate in &candidates {
        if let Some(inline) = parse_inline_plugin(candidate) {
            analyze_scope(
                &inline.body,
                file,
                &inline.param_name,
                &next_prefix,
                plugin_defs,
                handler_defs,
                routes,
                handlers,
                diagnostics,
            );
            return;
        }

        if let Some(plugin_ref) = extract_handler_ref(candidate) {
            if let Some(plugin) = plugin_defs.get(&plugin_ref) {
                analyze_scope(
                    &plugin.body,
                    file,
                    &plugin.param_name,
                    &next_prefix,
                    plugin_defs,
                    handler_defs,
                    routes,
                    handlers,
                    diagnostics,
                );
                return;
            }

            diagnostics.push(diag(
                file,
                "ANALYZER_UNRESOLVED_PLUGIN",
                &format!(
                    "register plugin '{}' could not be resolved in current file. Ensure plugin is declared in the same file or use an inline callback.",
                    plugin_ref
                ),
            ));
            return;
        }
    }

    diagnostics.push(diag(
        file,
        "ANALYZER_UNSUPPORTED_REGISTER_CALLBACK",
        &format!(
            "unsupported register callback pattern on {}.register(...). Use inline function(plugin) {{ ... }} or named local plugin reference.",
            instance_name
        ),
    ));
}
