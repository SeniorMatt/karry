//! Lossless KConfig reader/writer: every line keeps its raw text, so untouched files round-trip byte-identically.
use crate::{err, Result};
use regex::Regex;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

#[derive(Clone, Debug, PartialEq)]
enum Line {
    Other(String),
    Group(String, String), // raw, canonical "[A][B]" (flags like [$i] dropped)
    Entry { raw: String, lhs: String, key: String, val: String },
}
impl Line {
    fn raw(&self) -> &str {
        match self {
            Line::Other(r) | Line::Group(r, _) => r,
            Line::Entry { raw, .. } => raw,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Default)]
pub struct KConfig {
    lines: Vec<Line>,
    final_newline: bool,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct KeyChange {
    pub group: String,
    pub key: String,
    pub old: Option<String>,
    pub new: Option<String>,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Conflict {
    pub store_path: String,
    pub group: String,
    pub key: String, // empty for a whole-file conflict
    pub base: Option<String>,
    pub local: Option<String>,
    pub remote: Option<String>,
}

pub fn group_path(canon: &str) -> Vec<String> {
    canon.trim_start_matches('[').trim_end_matches(']').split("][").filter(|s| !s.is_empty()).map(String::from).collect()
}
fn new_entry(key: &str, val: &str) -> Line {
    Line::Entry { raw: format!("{key}={val}"), lhs: key.into(), key: key.into(), val: val.into() }
}
pub fn unescape(v: &str) -> String {
    let mut o = String::new();
    let mut it = v.chars();
    while let Some(c) = it.next() {
        if c != '\\' {
            o.push(c);
            continue;
        }
        match it.next() {
            Some('s') => o.push(' '),
            Some('t') => o.push('\t'),
            Some('n') => o.push('\n'),
            Some('r') => o.push('\r'),
            Some('\\') => o.push('\\'),
            Some(x) => {
                o.push('\\');
                o.push(x)
            }
            None => o.push('\\'),
        }
    }
    o
}
pub fn escape(v: &str) -> String {
    let mut o = v.replace('\\', "\\\\").replace('\n', "\\n").replace('\t', "\\t").replace('\r', "\\r");
    if o.starts_with(' ') {
        o.replace_range(..1, "\\s");
    }
    if o.ends_with(' ') {
        o.replace_range(o.len() - 1.., "\\s");
    }
    o
}

impl KConfig {
    pub fn parse(text: &str) -> Result<KConfig> {
        let final_newline = text.ends_with('\n');
        let body = text.strip_suffix('\n').unwrap_or(text);
        let mut lines = vec![];
        if !text.is_empty() {
            for (n, raw) in body.split('\n').enumerate() {
                let t = raw.trim_end_matches('\r');
                let trimmed = t.trim();
                if trimmed.is_empty() || trimmed.starts_with('#') {
                    lines.push(Line::Other(raw.into()));
                } else if t.starts_with('[') && t.ends_with(']') {
                    let mut path = group_path(t);
                    if path.last().is_some_and(|l| l.len() > 1 && l.starts_with('$') && l[1..].chars().all(|c| "idear".contains(c))) {
                        path.pop();
                    }
                    if path.is_empty() {
                        lines.push(Line::Other(raw.into())); // file-wide flag header like [$i]
                        continue;
                    }
                    lines.push(Line::Group(raw.into(), path.iter().map(|p| format!("[{p}]")).collect()));
                } else if let Some((lhs, rest)) = t.split_once('=') {
                    if lhs.trim().is_empty() {
                        return err(format!("line {}: empty key", n + 1));
                    }
                    lines.push(Line::Entry { raw: raw.into(), lhs: lhs.into(), key: lhs.trim().into(), val: rest.trim_start().into() });
                } else {
                    return err(format!("line {}: not a group, comment or key=value", n + 1));
                }
            }
        }
        Ok(KConfig { lines, final_newline })
    }
    pub fn to_text(&self) -> String {
        let mut s = self.lines.iter().map(|l| l.raw()).collect::<Vec<_>>().join("\n");
        if self.final_newline && !self.lines.is_empty() {
            s.push('\n');
        }
        s
    }
    /// (group, key) -> raw value; later duplicates win, like KConfig does.
    pub fn flat(&self) -> BTreeMap<(String, String), String> {
        let mut m = BTreeMap::new();
        let mut cur = String::new();
        for l in &self.lines {
            match l {
                Line::Group(_, n) => cur = n.clone(),
                Line::Entry { key, val, .. } => {
                    m.insert((cur.clone(), key.clone()), val.clone());
                }
                _ => {}
            }
        }
        m
    }
    pub fn get(&self, group: &str, key: &str) -> Option<String> {
        self.flat().remove(&(group.to_string(), key.to_string()))
    }
    pub fn groups(&self) -> Vec<String> {
        let mut v: Vec<String> = vec![];
        for l in &self.lines {
            if let Line::Group(_, n) = l {
                if !v.contains(n) {
                    v.push(n.clone());
                }
            }
        }
        v
    }
    pub fn set(&mut self, group: &str, key: &str, val: &str) {
        let (mut cur, mut found, mut last) = (String::new(), None, None);
        for (i, l) in self.lines.iter().enumerate() {
            match l {
                Line::Group(_, n) => {
                    cur = n.clone();
                    if cur == group {
                        last = Some(i);
                    }
                }
                Line::Entry { key: k, .. } if cur == group => {
                    last = Some(i);
                    if k == key {
                        found = Some(i);
                    }
                }
                Line::Other(r) if cur == group && !r.trim().is_empty() => last = Some(i),
                _ => {}
            }
        }
        if let Some(i) = found {
            if let Line::Entry { raw, lhs, val: v, .. } = &mut self.lines[i] {
                *raw = format!("{lhs}={val}");
                *v = val.into();
            }
        } else if let Some(i) = last {
            self.lines.insert(i + 1, new_entry(key, val));
        } else if group.is_empty() {
            self.lines.insert(0, new_entry(key, val));
        } else {
            if self.lines.last().is_some_and(|l| !l.raw().trim().is_empty()) {
                self.lines.push(Line::Other(String::new()));
            }
            self.lines.push(Line::Group(group.into(), group.into()));
            self.lines.push(new_entry(key, val));
        }
        self.final_newline = true;
    }
    pub fn delete(&mut self, group: &str, key: &str) -> bool {
        let mut cur = String::new();
        let n = self.lines.len();
        self.lines.retain(|l| match l {
            Line::Group(_, g) => {
                cur = g.clone();
                true
            }
            Line::Entry { key: k, .. } => !(cur == group && k == key),
            _ => true,
        });
        self.lines.len() != n
    }
    /// Remove every section named exactly `group`, with its lines.
    pub fn remove_group(&mut self, group: &str) {
        let mut skip = false;
        self.lines.retain(|l| {
            if let Line::Group(_, g) = l {
                skip = g == group;
            }
            !skip
        });
    }
    /// Split into sections whose group matches `pred` (second) and the rest (first).
    pub fn split(&self, pred: &dyn Fn(&str) -> bool) -> (KConfig, KConfig) {
        let (mut keep, mut moved) = (vec![], vec![]);
        let mut to_moved = false;
        for l in &self.lines {
            if let Line::Group(_, g) = l {
                to_moved = pred(g);
            }
            if to_moved { &mut moved } else { &mut keep }.push(l.clone());
        }
        (KConfig { lines: keep, final_newline: self.final_newline }, KConfig { lines: moved, final_newline: self.final_newline })
    }
    pub fn strip_keys(&mut self, re: &Regex) {
        self.lines.retain(|l| !matches!(l, Line::Entry { key, .. } if re.is_match(key)));
    }
    pub fn apply(&mut self, changes: &[KeyChange]) {
        for c in changes {
            match &c.new {
                Some(v) => self.set(&c.group, &c.key, v),
                None => {
                    self.delete(&c.group, &c.key);
                }
            }
        }
    }
}

pub fn diff(old: &KConfig, new: &KConfig) -> Vec<KeyChange> {
    let (a, b) = (old.flat(), new.flat());
    let mut out = vec![];
    for (k, v) in &b {
        if a.get(k) != Some(v) {
            out.push(KeyChange { group: k.0.clone(), key: k.1.clone(), old: a.get(k).cloned(), new: Some(v.clone()) });
        }
    }
    for (k, v) in &a {
        if !b.contains_key(k) {
            out.push(KeyChange { group: k.0.clone(), key: k.1.clone(), old: Some(v.clone()), new: None });
        }
    }
    out
}

/// Key-level 3-way merge. Result starts from `local`; a key changed on both sides differently is a
/// conflict (local kept unless `remote_wins(group, key)`).
pub fn merge3(base: &KConfig, local: &KConfig, remote: &KConfig, remote_wins: &dyn Fn(&str, &str) -> bool) -> (KConfig, Vec<Conflict>) {
    let (b, l, r) = (base.flat(), local.flat(), remote.flat());
    let keys: std::collections::BTreeSet<_> = b.keys().chain(l.keys()).chain(r.keys()).cloned().collect();
    let (mut out, mut conflicts, mut changes) = (local.clone(), vec![], vec![]);
    for k in keys {
        let (bv, lv, rv) = (b.get(&k), l.get(&k), r.get(&k));
        let pick = if lv == rv || rv == bv {
            lv
        } else if lv == bv {
            rv
        } else {
            conflicts.push(Conflict { store_path: String::new(), group: k.0.clone(), key: k.1.clone(), base: bv.cloned(), local: lv.cloned(), remote: rv.cloned() });
            if remote_wins(&k.0, &k.1) { rv } else { lv }
        };
        if pick != lv {
            changes.push(KeyChange { group: k.0, key: k.1, old: lv.cloned(), new: pick.cloned() });
        }
    }
    out.apply(&changes);
    (out, conflicts)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roundtrip_fixtures() {
        let fixtures = [
            "# top comment\n[General]\nColorScheme=BreezeDark\nName[de]=Hallo\nFont=Noto Sans,10,-1,5,50,0,0,0,0,0\n\n[Colors:View][Inner]\nBackgroundNormal=1,2,3\n\n[Colors:View][Inner]\nDup=1\n",
            "[$i]\n[Group][$i]\nkey[$i]=locked\nenv[$e]=$HOME/x\n[A][B][C]\nempty=\nspaced = value with space\n",
            "[Esc]\na=lead\\strail\\s\nb=tab\\there\\nnl\\\\back\nlist=a\\;b\\;c\n   \n# c\n",
            "[NoFinalNewline]\nk=v",
            "top=level\n[G]\nk=v\r\n",
            "",
        ];
        for f in fixtures {
            assert_eq!(KConfig::parse(f).unwrap().to_text(), f);
        }
        assert!(KConfig::parse("[G]\nnonsense line\n").is_err());
        let k = KConfig::parse(fixtures[2]).unwrap();
        assert_eq!(unescape(&k.get("[Esc]", "b").unwrap()), "tab\there\nnl\\back");
        assert_eq!(unescape(&k.get("[Esc]", "a").unwrap()), "lead trail ");
        assert_eq!(escape(" x\t"), "\\sx\\t");
        let d = KConfig::parse(fixtures[0]).unwrap();
        assert_eq!(d.get("[Colors:View][Inner]", "Dup").as_deref(), Some("1"));
        assert_eq!(d.get("[General]", "Name[de]").as_deref(), Some("Hallo"));
        assert_eq!(KConfig::parse(fixtures[1]).unwrap().groups(), vec!["[Group]", "[A][B][C]"]);
    }

    #[test]
    fn edit_is_minimal() {
        let mut k = KConfig::parse("[A]\nx=1\n\n[B]\ny=2\n").unwrap();
        k.set("[A]", "z", "3");
        k.set("[B]", "y", "9");
        k.set("[C][D]", "n", "1");
        assert_eq!(k.to_text(), "[A]\nx=1\nz=3\n\n[B]\ny=9\n\n[C][D]\nn=1\n");
        k.delete("[A]", "x");
        k.remove_group("[B]");
        assert_eq!(k.to_text(), "[A]\nz=3\n\n[C][D]\nn=1\n");
    }

    #[test]
    fn three_way_merge() {
        let p = |s| KConfig::parse(s).unwrap();
        let base = p("[A]\nx=1\ny=1\nz=1\n");
        let local = p("[A]\nx=2\ny=1\nz=1\n");
        let remote = p("[A]\nx=1\ny=3\nz=1\nnew=1\n");
        let (m, c) = merge3(&base, &local, &remote, &|_, _| false);
        assert!(c.is_empty());
        assert_eq!(m.flat().into_iter().map(|((_, k), v)| format!("{k}={v}")).collect::<Vec<_>>(), ["new=1", "x=2", "y=3", "z=1"]);
        let remote2 = p("[A]\nx=5\ny=1\nz=1\n");
        let (m, c) = merge3(&base, &local, &remote2, &|_, _| false);
        assert_eq!((c.len(), c[0].key.as_str(), m.get("[A]", "x").as_deref()), (1, "x", Some("2")));
        let (m, _) = merge3(&base, &local, &remote2, &|_, _| true);
        assert_eq!(m.get("[A]", "x").as_deref(), Some("5"));
    }
}
