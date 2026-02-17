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
    extract_static_string_literal(v)
}

pub(crate) fn extract_static_string_literal(v: &str) -> Option<String> {
    let t = v.trim();
    if t.len() < 2 {
        return None;
    }

    if (t.starts_with('"') && t.ends_with('"')) || (t.starts_with('\'') && t.ends_with('\'')) {
        return Some(t[1..t.len() - 1].to_string());
    }

    if t.starts_with('`') && t.ends_with('`') {
        let inner = &t[1..t.len() - 1];
        if !inner.contains("${") {
            return Some(inner.to_string());
        }
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
        r#"(?s)(?:\b{}\b|"{}")\s*:\s*("[^"]*"|'[^']*'|`[^`]*`)"#,
        regex::escape(key),
        regex::escape(key)
    );
    let re = Regex::new(&pattern).unwrap();
    re.captures(obj).and_then(|c| extract_quoted_string(&c[1]))
}

pub(crate) fn extract_object_string_array_prop(obj: &str, key: &str) -> Vec<String> {
    let pattern = format!(
        r#"(?s)(?:\b{}\b|"{}")\s*:\s*\[(.*?)\]"#,
        regex::escape(key),
        regex::escape(key)
    );
    let re = Regex::new(&pattern).unwrap();
    let Some(captures) = re.captures(obj) else {
        return vec![];
    };
    split_top_level(&captures[1], ',')
        .into_iter()
        .filter_map(|part| extract_quoted_string(&part))
        .collect()
}

pub(crate) fn extract_object_prop_expr(obj: &str, key: &str) -> Option<String> {
    let pattern = format!(
        r#"(?s)(?:\b{}\b|"{}")\s*:\s*([^,}}]+)"#,
        regex::escape(key),
        regex::escape(key)
    );
    let re = Regex::new(&pattern).unwrap();
    re.captures(obj).map(|c| c[1].trim().to_string())
}

pub(crate) fn extract_object_handler_ref(obj: &str, key: &str) -> Option<String> {
    let expr = extract_object_prop_expr(obj, key)?;
    extract_handler_ref(&expr)
}

pub(crate) fn unwrap_single_call_arg(expr: &str) -> Option<String> {
    let t = expr.trim();
    let open = t.find('(')?;
    if !t.ends_with(')') {
        return None;
    }
    let args_src = &t[open + 1..t.len() - 1];
    let args = split_top_level(args_src, ',');
    if args.len() != 1 {
        return None;
    }
    Some(args[0].trim().to_string())
}

pub(crate) fn parse_inline_handler_signature(expr: &str) -> Option<(Vec<String>, bool)> {
    let t = expr.trim();

    let re_arrow = Regex::new(r"(?s)^(async\s+)?(?:\(([^)]*)\)|([A-Za-z_$][\w$]*))\s*=>")
        .unwrap();
    if let Some(cap) = re_arrow.captures(t) {
        let params_src = cap
            .get(2)
            .or_else(|| cap.get(3))
            .map(|m| m.as_str())
            .unwrap_or("");
        let params = split_top_level(params_src, ',')
            .into_iter()
            .map(|p| p.trim().to_string())
            .filter(|p| !p.is_empty())
            .collect::<Vec<_>>();
        return Some((params, cap.get(1).is_some()));
    }

    let re_fn =
        Regex::new(r"(?s)^(async\s+)?function(?:\s+[A-Za-z_$][\w$]*)?\s*\(([^)]*)\)").unwrap();
    re_fn.captures(t).map(|cap| {
        (
            split_top_level(&cap[2], ',')
                .into_iter()
                .map(|p| p.trim().to_string())
                .filter(|p| !p.is_empty())
                .collect::<Vec<_>>(),
            cap.get(1).is_some(),
        )
    })
}

#[derive(Clone, Debug)]
pub(crate) struct IfBlockInfo {
    pub then_range: (usize, usize),
    pub else_range: Option<(usize, usize)>,
    pub condition_value: Option<bool>,
}

pub(crate) fn find_if_block_infos(src: &str) -> Vec<IfBlockInfo> {
    let bytes = src.as_bytes();
    let mut out = vec![];
    let mut i = 0usize;

    while i + 2 <= bytes.len() {
        if i + 1 < bytes.len()
            && bytes[i] == b'i'
            && bytes[i + 1] == b'f'
            && (i == 0 || !is_ident_char(bytes[i - 1]))
            && (i + 2 == bytes.len() || !is_ident_char(bytes[i + 2]))
        {
            let mut j = i + 2;
            while j < bytes.len() && bytes[j].is_ascii_whitespace() {
                j += 1;
            }
            if j >= bytes.len() || bytes[j] != b'(' {
                i += 2;
                continue;
            }
            let Some((condition_raw, cond_consumed)) = capture_balanced(&src[j + 1..], '(', ')')
            else {
                i += 2;
                continue;
            };
            let condition_value = eval_static_bool_expr(&condition_raw);

            j = j + 1 + cond_consumed;
            while j < bytes.len() && bytes[j].is_ascii_whitespace() {
                j += 1;
            }

            let mut then_range = None;
            if j < bytes.len() && bytes[j] == b'{' {
                if let Some((_, body_consumed)) = capture_balanced(&src[j + 1..], '{', '}') {
                    then_range = Some((j + 1, j + body_consumed));
                    j = j + 1 + body_consumed;
                }
            }

            let mut else_range = None;
            while j < bytes.len() && bytes[j].is_ascii_whitespace() {
                j += 1;
            }
            if j + 4 <= bytes.len() && &src[j..j + 4] == "else" {
                let mut k = j + 4;
                while k < bytes.len() && bytes[k].is_ascii_whitespace() {
                    k += 1;
                }
                if k < bytes.len() && bytes[k] == b'{' {
                    if let Some((_, else_consumed)) = capture_balanced(&src[k + 1..], '{', '}') {
                        else_range = Some((k + 1, k + else_consumed));
                        j = k + 1 + else_consumed;
                    }
                }
            }

            if let Some(then_range) = then_range {
                out.push(IfBlockInfo {
                    then_range,
                    else_range,
                    condition_value,
                });
            }

            i = j;
            continue;
        }

        i += 1;
    }

    out
}

// Intentionally no range-only helper; use find_if_block_infos for branch-aware handling.

fn eval_static_bool_expr(src: &str) -> Option<bool> {
    struct Parser<'a> {
        s: &'a [u8],
        i: usize,
    }

    impl<'a> Parser<'a> {
        fn new(src: &'a str) -> Self {
            Self {
                s: src.as_bytes(),
                i: 0,
            }
        }

        fn parse(mut self) -> Option<bool> {
            let value = self.parse_or()?;
            self.ws();
            if self.i == self.s.len() {
                Some(value)
            } else {
                None
            }
        }

        fn parse_or(&mut self) -> Option<bool> {
            let mut left = self.parse_and()?;
            loop {
                self.ws();
                if self.consume("||") {
                    let right = self.parse_and()?;
                    left = left || right;
                } else {
                    break;
                }
            }
            Some(left)
        }

        fn parse_and(&mut self) -> Option<bool> {
            let mut left = self.parse_not()?;
            loop {
                self.ws();
                if self.consume("&&") {
                    let right = self.parse_not()?;
                    left = left && right;
                } else {
                    break;
                }
            }
            Some(left)
        }

        fn parse_not(&mut self) -> Option<bool> {
            self.ws();
            if self.consume("!") {
                return Some(!self.parse_not()?);
            }
            self.parse_primary()
        }

        fn parse_primary(&mut self) -> Option<bool> {
            self.ws();
            if self.consume("(") {
                let v = self.parse_or()?;
                self.ws();
                if !self.consume(")") {
                    return None;
                }
                return Some(v);
            }
            if self.consume_word("true") {
                return Some(true);
            }
            if self.consume_word("false") {
                return Some(false);
            }
            None
        }

        fn ws(&mut self) {
            while self.i < self.s.len() && self.s[self.i].is_ascii_whitespace() {
                self.i += 1;
            }
        }

        fn consume(&mut self, token: &str) -> bool {
            let b = token.as_bytes();
            if self.i + b.len() <= self.s.len() && &self.s[self.i..self.i + b.len()] == b {
                self.i += b.len();
                true
            } else {
                false
            }
        }

        fn consume_word(&mut self, word: &str) -> bool {
            let b = word.as_bytes();
            if self.i + b.len() > self.s.len() || &self.s[self.i..self.i + b.len()] != b {
                return false;
            }
            let next = self.i + b.len();
            if next < self.s.len() && is_ident_char(self.s[next]) {
                return false;
            }
            self.i = next;
            true
        }
    }

    Parser::new(src).parse()
}

fn is_ident_char(b: u8) -> bool {
    b.is_ascii_alphanumeric() || b == b'_' || b == b'$'
}
