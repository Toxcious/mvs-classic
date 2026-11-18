//! The `config` kind: one of the build's config files (`.ini` under a `Config/` folder), parsed as the engine
//! reads one file of its hierarchy: its sections and key lines in file order, each line's merge
//! command kept; JSON `{ sections: [{ name, entries: [[op, key, value], ...] }, ...] }`.
//!
//! Unverified: the parser's rules are UE 4.26's as its source gives them, not a read of this build's exe
//! (FConfigFile::CombineFromBuffer, FParse::LineExtended, FParse::QuotedString); over the build's 33 ini files only
//! the plain cases occur.

use std::collections::HashMap;

use mvs_pak::paths::split_object_path;
use serde::Serialize;

use crate::json;
use crate::kind::{Cx, Item, Kind, KindError};

/// The kind's version.
pub const CONFIG_KIND_VERSION: u64 = 1;

/// One key line: its merge command (`""`, `+`, `.`, `-` or `!`), its key without the command character, and its
/// value as UE reads it.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct IniEntry(pub String, pub String, pub String);

/// One section: its name as the file first spells it and its key lines in file order.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct IniSection {
    pub name: String,
    pub entries: Vec<IniEntry>,
}

/// One config file: its sections in the order the file first opens them.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct IniFile {
    pub sections: Vec<IniSection>,
}

/// `<stem><ext>` (the extension without regard to case) as a path of non-empty segments: the stem, its last
/// segment, and the segments before the last (the regex `^((?:[^/]+\/)*([^/]+))\.ext$`).
fn stem_of<'a>(file: &'a str, ext: &str) -> Option<(&'a str, &'a str, Vec<&'a str>)> {
    let cut = file.len().checked_sub(ext.len())?;
    if !file.is_char_boundary(cut) || !file[cut..].eq_ignore_ascii_case(ext) {
        return None;
    }
    let stem = &file[..cut];
    let mut segments: Vec<&str> = stem.split('/').collect();
    if segments.iter().any(|s| s.is_empty()) {
        return None;
    }
    let name = segments.pop()?;
    Some((stem, name, segments))
}

/// A path of the paks `<...>/Config/<...>/<name>.ini` as the kind names it: `(stem path, name)`.
fn config_name(file: &str) -> Option<(&str, &str)> {
    let (stem, name, dirs) = stem_of(file, ".ini")?;
    dirs.iter().any(|d| d.eq_ignore_ascii_case("Config")).then_some((stem, name))
}

/// A `.png` of the paks: `(stem path, name)`.
pub(crate) fn png_name(file: &str) -> Option<(&str, &str)> {
    stem_of(file, ".png").map(|(stem, name, _)| (stem, name))
}

/// The items of every file of the paks `find` accepts: `/<stem>.<name>`.
pub(crate) fn file_items(cx: &Cx<'_>, find: impl Fn(&str) -> Option<(&str, &str)>) -> Vec<Item> {
    cx.paks.paths().filter_map(|f| find(f).map(|(stem, name)| Item::new(format!("/{stem}.{name}")))).collect()
}

/// The pak path of the file a kind item names: the path's package under the root
/// with `ext`, the export none or the file's name, the file one `find` accepts.
pub(crate) fn file_of(path: &str, ext: &str, find: impl Fn(&str) -> Option<(&str, &str)>) -> Option<String> {
    let (pkg, object) = split_object_path(path);
    let name = &pkg[pkg.rfind('/').map_or(0, |s| s + 1)..];
    let file = format!("{}{ext}", pkg.strip_prefix('/')?);
    (object.is_none_or(|o| o.eq_ignore_ascii_case(name)) && find(&file).is_some()).then_some(file)
}

/// The `config` kind.
pub struct Config;

impl Kind for Config {
    fn id(&self) -> &'static str {
        "config"
    }

    fn version(&self) -> u64 {
        CONFIG_KIND_VERSION
    }

    fn content_type(&self, _: &[u8]) -> &'static str {
        "application/json"
    }

    /// Every `.ini` under a `Config/` folder of the paks.
    fn list(&self, cx: &Cx<'_>) -> Result<Vec<Item>, KindError> {
        Ok(file_items(cx, config_name))
    }

    fn build(&self, cx: &Cx<'_>, item: &Item) -> Result<Vec<u8>, KindError> {
        Ok(json::to_vec(&ini(cx, &item.path)?)?)
    }
}

/// The ini file of config entry `path` (an item of the kind: `/<stem>.<name>`), parsed as the kind writes it: what a
/// game kind reads of a config in-process (the material's texture LOD groups) instead of from the store.
pub fn ini(cx: &Cx<'_>, path: &str) -> Result<IniFile, KindError> {
    let entry = file_of(path, ".ini", config_name).and_then(|f| cx.paks.entry(&f).map(|e| e.path.clone()));
    let Some(entry) = entry else {
        return Err(KindError::NotFound(format!("config: the build has no {path} (.ini)")));
    };
    let bytes = cx.paks.read(&entry).map_err(|e| KindError::Failed(e.to_string()))?;
    Ok(parse_ini(&ini_text(&bytes)))
}

/// FFileHelper::BufferToString: UTF-16 by its byte order mark, else UTF-8 (a UTF-8 mark dropped).
pub fn ini_text(bytes: &[u8]) -> String {
    let utf16 = |b: &[u8], le: bool| {
        let units =
            b.as_chunks::<2>().0.iter().map(|&c| if le { u16::from_le_bytes(c) } else { u16::from_be_bytes(c) });
        let mut s: String = char::decode_utf16(units).map(|r| r.unwrap_or('\u{fffd}')).collect();
        if b.len() % 2 == 1 {
            s.push('\u{fffd}');
        }
        s
    };
    match bytes {
        [0xff, 0xfe, rest @ ..] => utf16(rest, true),
        [0xfe, 0xff, rest @ ..] => utf16(rest, false),
        _ => {
            let s = String::from_utf8_lossy(bytes);
            s.strip_prefix('\u{feff}').map_or_else(|| s.to_string(), str::to_owned)
        }
    }
}

fn is_break(c: Option<&char>) -> bool {
    matches!(c, Some('\n' | '\r'))
}

fn is_space(c: char) -> bool {
    matches!(c, ' ' | '\t' | '\n' | '\r' | '\u{c}' | '\u{b}')
}

/// FParse::LineExtended with Exact (no `//` comment, no `|` chaining), as the config reader calls it: a line ends at
/// a line break outside braces; a backslash before a line break, and a line break inside `{ }`, join the next line
/// with one space; braces outside quotes are dropped; `\"` and `\\` inside quotes are taken as two characters.
pub fn ini_lines(text: &str) -> Vec<String> {
    let t: Vec<char> = text.chars().collect();
    let n = t.len();
    let mut lines = Vec::new();
    let mut i = 0;
    while i < n {
        let mut out = String::new();
        let mut quoted = false;
        let mut depth = 0;
        while i < n && (!is_break(t.get(i)) || depth > 0) {
            let c = t[i];
            if is_break(t.get(i)) {
                out.push(' ');
                i += 1;
                if is_break(t.get(i)) {
                    i += 1;
                }
            } else if !quoted && c == '\\' && is_break(t.get(i + 1)) {
                out.push(' ');
                i += 2;
                if is_break(t.get(i)) {
                    i += 1;
                }
            } else if !quoted && c == '{' {
                depth += 1;
                i += 1;
            } else if !quoted && c == '}' && depth > 0 {
                depth -= 1;
                i += 1;
            } else if quoted && c == '\\' && matches!(t.get(i + 1), Some('"' | '\\')) {
                out.push(c);
                out.push(t[i + 1]);
                i += 2;
            } else {
                if c == '"' {
                    quoted = !quoted;
                }
                out.push(c);
                i += 1;
            }
        }
        if t.get(i) == Some(&'\r') {
            i += 1;
        }
        if t.get(i) == Some(&'\n') {
            i += 1;
        }
        lines.push(out);
    }
    lines
}

/// FParse::QuotedString: the text between the opening quote and the next unescaped quote, `\\`, `\"`, `\'`, `\n`,
/// `\r`, `\t` unescaped; any other escape keeps its backslash; an unclosed quote gives what was read.
pub fn quoted_string(s: &str) -> String {
    let t: Vec<char> = s.chars().collect();
    let mut out = String::new();
    let mut i = 1;
    while i < t.len() && t[i] != '"' && !is_break(t.get(i)) {
        let c = t[i];
        if c != '\\' {
            out.push(c);
            i += 1;
            continue;
        }
        let esc = match t.get(i + 1) {
            Some('\\') => Some('\\'),
            Some('"') => Some('"'),
            Some('\'') => Some('\''),
            Some('n') => Some('\n'),
            Some('r') => Some('\r'),
            Some('t') => Some('\t'),
            _ => None,
        };
        if let Some(e) = esc {
            out.push(e);
            i += 2;
        } else {
            out.push(c);
            i += 1;
        }
    }
    out
}

/// FConfigFile::CombineFromBuffer over one file, without a base: each line's trailing
/// white space trimmed; `[Name]` opens a section (found again by name without regard to case); before the first
/// section a line is skipped; in a section a line that starts with `;` or holds no `=` is skipped, else it splits at
/// the first `=`: the key loses its leading white space, then its command character and its trailing white space,
/// and the value loses white space on both ends and, when it opens with a quote, is read as a quoted string. Nothing
/// is merged: every line stays with its command. A line starting `#` is a key whose name starts with `#`.
pub fn parse_ini(text: &str) -> IniFile {
    let mut sections: Vec<IniSection> = Vec::new();
    let mut by_name: HashMap<String, usize> = HashMap::new();
    let mut cur: Option<usize> = None;
    for raw in ini_lines(text) {
        let line = raw.trim_end_matches(is_space);
        if line.starts_with('[') && line.ends_with(']') && line.len() >= 2 {
            let name = &line[1..line.len() - 1];
            let at = *by_name.entry(name.to_lowercase()).or_insert_with(|| {
                sections.push(IniSection { name: name.to_owned(), entries: Vec::new() });
                sections.len() - 1
            });
            cur = Some(at);
            continue;
        }
        let Some(at) = cur else { continue };
        if line.is_empty() || line.starts_with(';') {
            continue;
        }
        let Some(eq) = line.find('=') else { continue };
        let mut key = line[..eq].trim_start_matches(is_space);
        let mut op = "";
        if let Some(c) = key.chars().next().filter(|c| matches!(c, '+' | '-' | '.' | '!')) {
            op = &key[..c.len_utf8()];
            key = &key[c.len_utf8()..];
        }
        let key = key.trim_end_matches(is_space);
        let mut value = line[eq + 1..].trim_start_matches(is_space).to_owned();
        if value.starts_with('"') {
            value = quoted_string(&value);
        }
        sections[at].entries.push(IniEntry(op.to_owned(), key.to_owned(), value));
    }
    IniFile { sections }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_lines_and_commands() {
        let f = parse_ini("junk=1\n[A]\n+K = \"x \\\"y\\\"\"  \n;c\n!Z=\n[a]\nK2=v\n#+N=1\n[B]\n");
        assert_eq!(f.sections.len(), 2);
        assert_eq!(f.sections[0].name, "A");
        let e = &f.sections[0].entries;
        assert_eq!(e[0], IniEntry("+".into(), "K".into(), "x \"y\"".into()));
        assert_eq!(e[1], IniEntry("!".into(), "Z".into(), String::new()));
        assert_eq!(e[2].1, "K2");
        assert_eq!(e[3].1, "#+N");
    }

    #[test]
    fn joins_continued_lines() {
        assert_eq!(ini_lines("a=1\\\nb\nc"), ["a=1 b", "c"]);
        assert_eq!(ini_lines("a={\nb}\r\nc"), ["a= b", "c"]);
    }

    #[test]
    fn utf16_and_bom() {
        assert_eq!(ini_text(&[0xff, 0xfe, b'a', 0, b'b', 0]), "ab");
        assert_eq!(ini_text(&[0xef, 0xbb, 0xbf, b'a']), "a");
    }

    #[test]
    fn names() {
        assert_eq!(
            config_name("MultiVersus/Config/DefaultEngine.ini"),
            Some(("MultiVersus/Config/DefaultEngine", "DefaultEngine"))
        );
        assert_eq!(config_name("Engine/Plugins/FX/Niagara/Config/BaseNiagara.INI").map(|x| x.1), Some("BaseNiagara"));
        assert_eq!(config_name("Engine/Content/X.ini"), None);
        assert_eq!(config_name("Config/X.ini").map(|x| x.1), Some("X"));
        assert_eq!(config_name("Config.ini"), None);
        let f = file_of("/Engine/Config/BaseEngine.BaseEngine", ".ini", config_name);
        assert_eq!(f.as_deref(), Some("Engine/Config/BaseEngine.ini"));
        assert_eq!(file_of("/Engine/Config/BaseEngine.Other", ".ini", config_name), None);
    }
}
