use regex::Regex;

use crate::defs::PluginDef;

pub(crate) fn capture_call_args(tail: &str, method: &str) -> Option<String> {
    let needle = format!("{}(", method);
    let mut s = tail.trim_start();
    if !s.starts_with(&needle) {
        return None;
    }
    s = &s[needle.len()..];
    let (inside, _) = capture_balanced(s, '(', ')')?;
    Some(inside)
}

pub(crate) fn capture_balanced(src: &str, open: char, close: char) -> Option<(String, usize)> {
    let mut depth = 1i32;
    let mut i = 0usize;
    let chars = src.chars().collect::<Vec<_>>();
    while i < chars.len() {
        match chars[i] {
            '"' | '\'' => {
                i = skip_string(&chars, i);
                continue;
            }
            c if c == open => depth += 1,
            c if c == close => {
                depth -= 1;
                if depth == 0 {
                    let inside = chars[..i].iter().collect::<String>();
                    return Some((inside, i + 1));
                }
            }
            _ => {}
        }
        i += 1;
    }
    None
}

fn skip_string(chars: &[char], start: usize) -> usize {
    let quote = chars[start];
    let mut i = start + 1;
    while i < chars.len() {
        if chars[i] == '\\' {
            i += 2;
            continue;
        }
        if chars[i] == quote {
            return i + 1;
        }
        i += 1;
    }
    chars.len()
}

pub(crate) fn split_top_level(src: &str, delim: char) -> Vec<String> {
    let mut out = vec![];
    let mut cur = String::new();
    let mut paren = 0i32;
    let mut brace = 0i32;
    let mut bracket = 0i32;
    let chars = src.chars().collect::<Vec<_>>();
    let mut i = 0usize;
    while i < chars.len() {
        match chars[i] {
            '"' | '\'' => {
                let next = skip_string(&chars, i);
                cur.push_str(&chars[i..next].iter().collect::<String>());
                i = next;
                continue;
            }
            '(' => paren += 1,
            ')' => paren -= 1,
            '{' => brace += 1,
            '}' => brace -= 1,
            '[' => bracket += 1,
            ']' => bracket -= 1,
            c if c == delim && paren == 0 && brace == 0 && bracket == 0 => {
                out.push(cur.trim().to_string());
                cur.clear();
                i += 1;
                continue;
            }
            _ => {}
        }
        cur.push(chars[i]);
        i += 1;
    }
    if !cur.trim().is_empty() {
        out.push(cur.trim().to_string());
    }
    out
}

pub(crate) fn parse_inline_plugin(expr: &str) -> Option<PluginDef> {
    let re_arrow = Regex::new(
        r"(?s)^(?:async\s+)?(?:\(\s*([A-Za-z_$][\w$]*)\s*\)|([A-Za-z_$][\w$]*))\s*=>\s*\{(.*)\}$",
    )
    .unwrap();
    if let Some(cap) = re_arrow.captures(expr.trim()) {
        let param_name = cap
            .get(1)
            .or_else(|| cap.get(2))
            .map(|m| m.as_str())
            .unwrap_or_default()
            .to_string();
        return Some(PluginDef {
            param_name,
            body: cap[3].to_string(),
        });
    }

    let re_fn = Regex::new(r"(?s)^(?:async\s+)?function(?:\s+[A-Za-z_$][\w$]*)?\s*\(\s*([A-Za-z_$][\w$]*)\s*[^)]*\)\s*\{(.*)\}$").unwrap();
    re_fn.captures(expr.trim()).map(|cap| PluginDef {
        param_name: cap[1].to_string(),
        body: cap[2].to_string(),
    })
}

pub(crate) fn first_object_literal(src: &str) -> Option<String> {
    let t = src.trim_start();
    if !t.starts_with('{') {
        return None;
    }
    let (obj, _) = capture_balanced(&t[1..], '{', '}')?;
    Some(obj)
}

pub(crate) fn extract_quoted_string(v: &str) -> Option<String> {
    let t = v.trim();
    if t.len() >= 2
        && ((t.starts_with('"') && t.ends_with('"')) || (t.starts_with('\'') && t.ends_with('\'')))
    {
        return Some(t[1..t.len() - 1].to_string());
    }
    None
}

pub(crate) fn extract_handler_ref(v: &str) -> Option<String> {
    let t = v.trim();
    let re = Regex::new(r"^[A-Za-z_$][\w$]*(?:\.[A-Za-z_$][\w$]*)*$").unwrap();
    if re.is_match(t) {
        Some(t.to_string())
    } else {
        None
    }
}

pub(crate) fn extract_object_string_prop(obj: &str, key: &str) -> Option<String> {
    let pattern = format!(
        r#"(?s)(?:\b{}\b|"{}")\s*:\s*("[^"]*"|'[^']*')"#,
        regex::escape(key),
        regex::escape(key)
    );
    let re = Regex::new(&pattern).unwrap();
    re.captures(obj).and_then(|c| extract_quoted_string(&c[1]))
}

pub(crate) fn extract_object_handler_ref(obj: &str, key: &str) -> Option<String> {
    let pattern = format!(
        r#"(?s)(?:\b{}\b|"{}")\s*:\s*([A-Za-z_$][\w$]*(?:\.[A-Za-z_$][\w$]*)*)"#,
        regex::escape(key),
        regex::escape(key)
    );
    let re = Regex::new(&pattern).unwrap();
    re.captures(obj).map(|c| c[1].to_string())
}
