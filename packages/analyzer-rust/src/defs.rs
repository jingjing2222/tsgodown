use regex::Regex;
use std::collections::HashMap;

use crate::ast::{capture_balanced, split_top_level};

#[derive(Clone, Debug)]
pub(crate) struct PluginDef {
    pub(crate) param_name: String,
    pub(crate) body: String,
}

#[derive(Clone, Debug)]
pub(crate) struct HandlerDef {
    pub(crate) params: Vec<String>,
    pub(crate) is_async: bool,
}

pub(crate) fn collect_plugin_definitions(src: &str) -> HashMap<String, PluginDef> {
    let mut map = HashMap::new();

    let fn_re = Regex::new(
        r"(?:async\s+)?function\s+([A-Za-z_$][\w$]*)\s*\(([^)]*)\)\s*(?::\s*[^\{=;]+)?\s*\{",
    )
    .unwrap();
    for cap in fn_re.captures_iter(src) {
        let Some(m0) = cap.get(0) else { continue };
        let open_idx = m0.end() - 1;
        let Some((body, _)) = capture_balanced(&src[open_idx + 1..], '{', '}') else {
            continue;
        };
        let params = cap[2]
            .split(',')
            .map(|v| v.trim())
            .filter(|v| !v.is_empty())
            .collect::<Vec<_>>();
        if let Some(first) = params.first() {
            map.insert(
                cap[1].to_string(),
                PluginDef {
                    param_name: (*first).to_string(),
                    body,
                },
            );
        }
    }

    let arrow_re = Regex::new(
        r"(?:const|let|var)\s+([A-Za-z_$][\w$]*)\s*(?::\s*[^=]+?)?\s*=\s*(?:async\s+)?(?:\(([^)]*)\)\s*(?::\s*[^\{=;]+)?|([A-Za-z_$][\w$]*)\s*(?::\s*[^\{=;]+)?)\s*=>\s*\{",
    )
    .unwrap();
    for cap in arrow_re.captures_iter(src) {
        let Some(m0) = cap.get(0) else { continue };
        let open_idx = m0.end() - 1;
        let Some((body, _)) = capture_balanced(&src[open_idx + 1..], '{', '}') else {
            continue;
        };
        let params = cap
            .get(2)
            .map(|m| m.as_str())
            .or_else(|| cap.get(3).map(|m| m.as_str()))
            .unwrap_or("")
            .split(',')
            .map(|v| v.trim())
            .filter(|v| !v.is_empty())
            .collect::<Vec<_>>();
        if let Some(first) = params.first() {
            map.insert(
                cap[1].to_string(),
                PluginDef {
                    param_name: (*first).to_string(),
                    body,
                },
            );
        }
    }

    let var_fn_re = Regex::new(
        r"(?:const|let|var)\s+([A-Za-z_$][\w$]*)\s*(?::\s*[^=]+?)?\s*=\s*(?:async\s+)?function\s*\(([^)]*)\)\s*(?::\s*[^\{=;]+)?\s*\{",
    )
    .unwrap();
    for cap in var_fn_re.captures_iter(src) {
        let Some(m0) = cap.get(0) else { continue };
        let open_idx = m0.end() - 1;
        let Some((body, _)) = capture_balanced(&src[open_idx + 1..], '{', '}') else {
            continue;
        };
        let params = cap[2]
            .split(',')
            .map(|v| v.trim())
            .filter(|v| !v.is_empty())
            .collect::<Vec<_>>();
        if let Some(first) = params.first() {
            map.insert(
                cap[1].to_string(),
                PluginDef {
                    param_name: (*first).to_string(),
                    body,
                },
            );
        }
    }

    map
}

pub(crate) fn collect_handler_definitions(src: &str) -> HashMap<String, HandlerDef> {
    let mut map = HashMap::new();

    let fn_re = Regex::new(
        r"(?s)(async\s+)?function\s+([A-Za-z_$][\w$]*)\s*\(([^)]*)\)\s*(?::\s*[^\{=;]+)?\s*\{",
    )
    .unwrap();
    for cap in fn_re.captures_iter(src) {
        map.insert(
            cap[2].to_string(),
            HandlerDef {
                params: split_params(&cap[3]),
                is_async: cap.get(1).is_some(),
            },
        );
    }

    let var_fn_re = Regex::new(
        r"(?s)(?:const|let|var)\s+([A-Za-z_$][\w$]*)\s*(?::\s*[^=]+?)?\s*=\s*(async\s+)?(?:function\s*\(([^)]*)\)\s*(?::\s*[^\{=;]+)?|\(([^)]*)\)\s*(?::\s*[^\{=;]+)?=>|([A-Za-z_$][\w$]*)\s*(?::\s*[^\{=;]+)?=>)\s*\{",
    )
    .unwrap();
    for cap in var_fn_re.captures_iter(src) {
        let params_src = cap
            .get(3)
            .or_else(|| cap.get(4))
            .or_else(|| cap.get(5))
            .map(|m| m.as_str())
            .unwrap_or("");
        map.insert(
            cap[1].to_string(),
            HandlerDef {
                params: split_params(params_src),
                is_async: cap.get(2).is_some(),
            },
        );
    }

    collect_object_literal_handlers(src, &mut map);
    collect_class_instance_handlers(src, &mut map);

    map
}

fn collect_object_literal_handlers(src: &str, map: &mut HashMap<String, HandlerDef>) {
    let obj_re = Regex::new(r"(?:const|let|var)\s+([A-Za-z_$][\w$]*)\s*=\s*\{").unwrap();
    for cap in obj_re.captures_iter(src) {
        let obj_name = cap[1].to_string();
        let Some(m0) = cap.get(0) else { continue };
        let open_idx = m0.end() - 1;
        let Some((body, _)) = capture_balanced(&src[open_idx + 1..], '{', '}') else {
            continue;
        };

        for member in split_top_level(&body, ',') {
            let Some((key, def)) = parse_object_member_handler(&member) else {
                continue;
            };
            map.insert(format!("{}.{}", obj_name, key), def);
        }
    }
}

fn parse_object_member_handler(member: &str) -> Option<(String, HandlerDef)> {
    let t = member.trim();
    if t.is_empty() {
        return None;
    }

    let re_method =
        Regex::new(r#"^(?:(async)\s+)?([A-Za-z_$][\w$]*|"[A-Za-z_$][\w$]*"|'[A-Za-z_$][\w$]*')\s*\(([^)]*)\)\s*\{"#)
            .unwrap();
    if let Some(cap) = re_method.captures(t) {
        return Some((
            normalize_prop_key(&cap[2]),
            HandlerDef {
                params: split_params(&cap[3]),
                is_async: cap.get(1).is_some(),
            },
        ));
    }

    let re_arrow = Regex::new(
        r#"^([A-Za-z_$][\w$]*|"[A-Za-z_$][\w$]*"|'[A-Za-z_$][\w$]*')\s*:\s*(async\s+)?(?:\(([^)]*)\)|([A-Za-z_$][\w$]*))\s*=>\s*\{"#,
    )
    .unwrap();
    if let Some(cap) = re_arrow.captures(t) {
        let params_src = cap
            .get(3)
            .or_else(|| cap.get(4))
            .map(|m| m.as_str())
            .unwrap_or("");
        return Some((
            normalize_prop_key(&cap[1]),
            HandlerDef {
                params: split_params(params_src),
                is_async: cap.get(2).is_some(),
            },
        ));
    }

    let re_fn = Regex::new(
        r#"^([A-Za-z_$][\w$]*|"[A-Za-z_$][\w$]*"|'[A-Za-z_$][\w$]*')\s*:\s*(async\s+)?function\s*\(([^)]*)\)\s*\{"#,
    )
    .unwrap();
    re_fn.captures(t).map(|cap| {
        (
            normalize_prop_key(&cap[1]),
            HandlerDef {
                params: split_params(&cap[3]),
                is_async: cap.get(2).is_some(),
            },
        )
    })
}

fn collect_class_instance_handlers(src: &str, map: &mut HashMap<String, HandlerDef>) {
    let mut class_methods: HashMap<String, Vec<(String, HandlerDef)>> = HashMap::new();

    let class_re = Regex::new(r"class\s+([A-Za-z_$][\w$]*)\s*\{").unwrap();
    let method_re = Regex::new(
        r"(?m)^\s*(?:(?:public|private|protected|readonly|static)\s+)*(async\s+)?([A-Za-z_$][\w$]*)\s*\(([^)]*)\)\s*\{",
    )
    .unwrap();
    for cap in class_re.captures_iter(src) {
        let class_name = cap[1].to_string();
        let Some(m0) = cap.get(0) else { continue };
        let open_idx = m0.end() - 1;
        let Some((body, _)) = capture_balanced(&src[open_idx + 1..], '{', '}') else {
            continue;
        };

        let mut methods = vec![];
        for m in method_re.captures_iter(&body) {
            methods.push((
                m[2].to_string(),
                HandlerDef {
                    params: split_params(&m[3]),
                    is_async: m.get(1).is_some(),
                },
            ));
        }

        if !methods.is_empty() {
            class_methods.insert(class_name, methods);
        }
    }

    let instance_re =
        Regex::new(r"(?:const|let|var)\s+([A-Za-z_$][\w$]*)\s*=\s*new\s+([A-Za-z_$][\w$]*)\s*\(")
            .unwrap();
    for cap in instance_re.captures_iter(src) {
        let instance_name = cap[1].to_string();
        let class_name = cap[2].to_string();
        let Some(methods) = class_methods.get(&class_name) else {
            continue;
        };

        for (method_name, def) in methods {
            map.insert(format!("{}.{}", instance_name, method_name), def.clone());
        }
    }
}

fn normalize_prop_key(v: &str) -> String {
    let t = v.trim();
    if t.len() >= 2
        && ((t.starts_with('"') && t.ends_with('"')) || (t.starts_with('\'') && t.ends_with('\'')))
    {
        t[1..t.len() - 1].to_string()
    } else {
        t.to_string()
    }
}

pub(crate) fn detect_root_instance_name(src: &str) -> Option<String> {
    let re = Regex::new(r"(?:const|let|var)\s+([A-Za-z_$][\w$]*)\s*=\s*Fastify\s*\(").unwrap();
    re.captures(src).map(|c| c[1].to_string())
}

fn split_params(src: &str) -> Vec<String> {
    src.split(',')
        .map(|s| s.trim())
        .filter(|s| !s.is_empty())
        .map(normalize_param_name)
        .filter(|s| !s.is_empty())
        .collect()
}

fn normalize_param_name(param: &str) -> String {
    let without_default = param.split('=').next().unwrap_or(param).trim();
    let without_type = without_default
        .split(':')
        .next()
        .unwrap_or(without_default)
        .trim();
    without_type.trim_start_matches("...").trim().to_string()
}
