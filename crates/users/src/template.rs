//! `${name}` substitution, the same syntax as scenario files.
//!
//! A name is a letter or `_` followed by letters, digits or `_`. Anything else
//! (`$x`, `${}`, `${1x}`, an unclosed `${`) is left as written.

use std::collections::BTreeMap;

use serde_json::Value;

/// Calls `f(name, start, end)` for every `${name}` in `s` (byte offsets).
fn scan(s: &str, mut f: impl FnMut(&str, usize, usize)) {
    let b = s.as_bytes();
    let mut i = 0;
    while i + 1 < b.len() {
        if b[i] == b'$' && b[i + 1] == b'{' {
            let start = i + 2;
            let mut j = start;
            while j < b.len() && (b[j].is_ascii_alphanumeric() || b[j] == b'_') {
                j += 1;
            }
            let valid = j > start
                && j < b.len()
                && b[j] == b'}'
                && (b[start].is_ascii_alphabetic() || b[start] == b'_');
            if valid {
                f(&s[start..j], i, j + 1);
                i = j + 1;
                continue;
            }
        }
        i += 1;
    }
}

/// The names referenced in `s`, in order (with repeats).
pub fn refs(s: &str) -> Vec<String> {
    let mut out = Vec::new();
    scan(s, |name, _, _| out.push(name.to_string()));
    out
}

/// Every name referenced in the strings of a JSON tree (keys included).
pub fn refs_json(v: &Value, out: &mut Vec<String>) {
    match v {
        Value::String(s) => out.extend(refs(s)),
        Value::Array(a) => a.iter().for_each(|x| refs_json(x, out)),
        Value::Object(m) => {
            for (k, x) in m {
                out.extend(refs(k));
                refs_json(x, out);
            }
        }
        _ => {}
    }
}

/// Replace every `${name}` found in `vars`; unknown names are left as written.
pub fn substitute(s: &str, vars: &BTreeMap<String, String>) -> String {
    let mut out = String::with_capacity(s.len());
    let mut last = 0;
    scan(s, |name, start, end| {
        if let Some(v) = vars.get(name) {
            out.push_str(&s[last..start]);
            out.push_str(v);
            last = end;
        }
    });
    out.push_str(&s[last..]);
    out
}

/// [`substitute`] in every string of a JSON tree (keys included).
pub fn substitute_json(v: &Value, vars: &BTreeMap<String, String>) -> Value {
    match v {
        Value::String(s) => Value::String(substitute(s, vars)),
        Value::Array(a) => Value::Array(a.iter().map(|x| substitute_json(x, vars)).collect()),
        Value::Object(m) => Value::Object(
            m.iter()
                .map(|(k, x)| (substitute(k, vars), substitute_json(x, vars)))
                .collect(),
        ),
        other => other.clone(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn vars() -> BTreeMap<String, String> {
        [
            ("api", "http://127.0.0.1:3000"),
            ("user", "7"),
            ("_x1", "y"),
        ]
        .into_iter()
        .map(|(k, v)| (k.to_string(), v.to_string()))
        .collect()
    }

    #[test]
    fn finds_references() {
        assert_eq!(
            refs("${api}/u/${user}?t=${token}&${_x1}"),
            ["api", "user", "token", "_x1"]
        );
        assert!(refs("$api ${} ${1x} ${a-b} ${open").is_empty());
        let mut out = Vec::new();
        refs_json(&json!({"k${a}": ["${b}", 1, {"c": "${c}"}]}), &mut out);
        out.sort();
        assert_eq!(out, ["a", "b", "c"]);
    }

    #[test]
    fn substitutes_known_names_only() {
        let v = vars();
        assert_eq!(
            substitute("${api}/u/${user}", &v),
            "http://127.0.0.1:3000/u/7"
        );
        assert_eq!(substitute("${token} and ${_x1}", &v), "${token} and y");
        assert_eq!(substitute("no vars $ here ${", &v), "no vars $ here ${");
        assert_eq!(substitute("${user}${user}", &v), "77");
        assert_eq!(
            substitute_json(&json!({"email": "u${user}@t", "n": 1, "${user}": true}), &v),
            json!({"email": "u7@t", "n": 1, "7": true})
        );
    }

    #[test]
    fn handles_non_ascii_text() {
        let v = vars();
        assert_eq!(substitute("héllo ${user} → ✓", &v), "héllo 7 → ✓");
    }
}
