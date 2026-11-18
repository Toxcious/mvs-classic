//! The project's collision profile table (DefaultEngine.ini's CollisionProfile section) and a body's resolved
//! collision: the channels' default responses, the game trace channels' names, and per
//! profile its CollisionEnabled, object type and responses. The stage bind's solids and platforms and the box bind's
//! shape classes read it.
//!
//! The lines are read as a regular expression would read them; here each is matched by hand (a greedy
//! `.*` takes the rightmost place the rest of the expression matches at).

use std::sync::Arc;

use indexmap::IndexMap;
use mvs_data::{Props, Value};

use super::shape::fail;
use crate::kind::{Cx, KindError};
use crate::kinds::config::{self, IniFile};

/// By channel name, a response (`ECR_Block`, ..).
pub type Responses = IndexMap<String, String>;

#[derive(Clone, Debug, PartialEq)]
pub struct CollisionProfile {
    pub enabled: String,
    pub obj: String,
    pub resp: Responses,
}

#[derive(Clone, Debug, PartialEq)]
pub struct CollisionTable {
    /// By channel name, its default response.
    pub defaults: Responses,
    /// By ECC_GameTraceChannel<n>, its channel name.
    pub chan: IndexMap<String, String>,
    /// By profile name.
    pub profiles: IndexMap<String, CollisionProfile>,
}

/// A body's profile name, object type's channel name, CollisionEnabled and responses by channel name (`None`: the
/// archetype's, or a profile the table lacks).
#[derive(Clone, Debug, PartialEq)]
pub struct ResolvedBody {
    pub prof: Option<String>,
    pub obj: Option<String>,
    pub enabled: Option<String>,
    pub resp: Option<Responses>,
}

pub const DEFAULT_ENGINE_INI: &str = "/MultiVersus/Config/DefaultEngine";
const SECTION: &str = "/Script/Engine.CollisionProfile";
/// The engine's object channels, each blocking by default.
const ENGINE_CHANNELS: [&str; 8] =
    ["WorldStatic", "WorldDynamic", "Pawn", "Visibility", "Camera", "PhysicsBody", "Vehicle", "Destructible"];

// ---- the expressions' pieces

fn is_word(b: u8) -> bool {
    b.is_ascii_alphanumeric() || b == b'_'
}

/// The length of the run of word characters at the start of `s`.
fn word_len(s: &str) -> usize {
    s.bytes().take_while(|b| is_word(*b)).count()
}

/// `\w+` at the start of `s`: the word and the rest.
fn word(s: &str) -> Option<(&str, &str)> {
    let n = word_len(s);
    (n > 0).then(|| s.split_at(n))
}

/// The values of `key` in `section` when the file is combined into an empty config, as FConfigFile::CombineFromBuffer
/// merges each line: keys compared without regard to case, values
/// too; '' replaces the key's first value or adds one, '+' adds unless an equal value is there, '.' adds, '-' removes
/// the first equal value, '!' removes them all.
fn config_values(file: &IniFile, section: &str, key: &str) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    let low = section.to_lowercase();
    let Some(s) = file.sections.iter().find(|s| s.name.to_lowercase() == low) else { return out };
    let k = key.to_lowercase();
    let same = |a: &str, b: &str| a.to_lowercase() == b.to_lowercase();
    for e in &s.entries {
        let (op, name, value) = (e.0.as_str(), e.1.as_str(), e.2.as_str());
        if name.to_lowercase() != k {
            continue;
        }
        match op {
            "" => {
                if out.is_empty() {
                    out.push(value.to_owned());
                } else {
                    out[0] = value.to_owned();
                }
            }
            "+" => {
                if !out.iter().any(|v| same(v, value)) {
                    out.push(value.to_owned());
                }
            }
            "." => out.push(value.to_owned()),
            "-" => {
                if let Some(at) = out.iter().position(|v| same(v, value)) {
                    out.remove(at);
                }
            }
            _ => out.clear(),
        }
    }
    out
}

/// `^\(Channel=(\w+),DefaultResponse=(\w+),.*Name="(\w+)"\)`: (channel, response, name).
fn default_channel(v: &str) -> Option<(&str, &str, &str)> {
    let (ch, rest) = word(v.strip_prefix("(Channel=")?)?;
    let (resp, rest) = word(rest.strip_prefix(",DefaultResponse=")?)?;
    let rest = rest.strip_prefix(',')?;
    rest.rmatch_indices("Name=\"").find_map(|(at, _)| {
        let (name, after) = word(&rest[at + 6..])?;
        after.starts_with("\")").then_some((ch, resp, name))
    })
}

/// `^\(Name="(\w+)",CollisionEnabled=(\w+),.*ObjectTypeName="(\w*)",CustomResponses=\((.*)\),HelpMessage`:
/// (name, enabled, object type, responses).
fn profile_line(v: &str) -> Option<(&str, &str, &str, &str)> {
    let (name, rest) = word(v.strip_prefix("(Name=\"")?)?;
    let (enabled, rest) = word(rest.strip_prefix("\",CollisionEnabled=")?)?;
    let rest = rest.strip_prefix(',')?;
    rest.rmatch_indices("ObjectTypeName=\"").find_map(|(at, _)| {
        let s = &rest[at + 16..];
        let n = word_len(s);
        let (obj, after) = s.split_at(n);
        let body = after.strip_prefix("\",CustomResponses=(")?;
        let end = body.rfind("),HelpMessage")?;
        Some((name, enabled, obj, &body[..end]))
    })
}

/// `^\(Name="(\w+)",CustomResponses=\((.*)\)\)`: (name, responses).
fn edit_line(v: &str) -> Option<(&str, &str)> {
    let (name, rest) = word(v.strip_prefix("(Name=\"")?)?;
    let body = rest.strip_prefix("\",CustomResponses=(")?;
    let end = body.rfind("))")?;
    Some((name, &body[..end]))
}

/// Every `\(Channel="(\w+)"(?:,Response=(\w+))?\)` of `s`, left to right: (channel, response; a response left out is
/// ECR_Block, FResponseChannel's default).
fn responses(s: &str) -> Vec<(&str, &str)> {
    let mut out = Vec::new();
    let mut from = 0;
    while let Some(at) = s[from..].find("(Channel=\"").map(|a| from + a) {
        let one = || {
            let (ch, rest) = word(&s[at + 10..])?;
            let rest = rest.strip_prefix('"')?;
            if let Some(r) = rest.strip_prefix(",Response=")
                && let Some((resp, after)) = word(r)
                && after.starts_with(')')
            {
                return Some((ch, resp, s.len() - after.len() + 1));
            }
            rest.starts_with(')').then(|| (ch, "ECR_Block", s.len() - rest.len() + 1))
        };
        match one() {
            Some((ch, resp, end)) => {
                out.push((ch, resp));
                from = end;
            }
            None => from = at + 1,
        }
    }
    out
}

/// The table of a DefaultEngine.ini.
pub fn collision_table_of(ini: &IniFile) -> CollisionTable {
    let mut defaults: Responses = ENGINE_CHANNELS.iter().map(|c| ((*c).to_owned(), "ECR_Block".to_owned())).collect();
    let mut chan = IndexMap::new();
    for v in config_values(ini, SECTION, "DefaultChannelResponses") {
        let Some((ch, resp, name)) = default_channel(&v) else { continue };
        defaults.insert(name.to_owned(), resp.to_owned());
        chan.insert(ch.to_owned(), name.to_owned());
    }
    let mut profiles: IndexMap<String, CollisionProfile> = IndexMap::new();
    for v in config_values(ini, SECTION, "Profiles") {
        let Some((name, enabled, obj, body)) = profile_line(&v) else { continue };
        let mut resp = defaults.clone();
        for (c, r) in responses(body) {
            resp.insert(c.to_owned(), r.to_owned());
        }
        profiles.insert(name.to_owned(), CollisionProfile { enabled: enabled.to_owned(), obj: obj.to_owned(), resp });
    }
    for v in config_values(ini, SECTION, "EditProfiles") {
        let Some((name, body)) = edit_line(&v) else { continue };
        let Some(p) = profiles.get_mut(name) else { continue };
        for (c, r) in responses(body) {
            p.resp.insert(c.to_owned(), r.to_owned());
        }
    }
    CollisionTable { defaults, chan, profiles }
}

/// The run's table, read once.
struct Table(Result<Arc<CollisionTable>, String>);

/// The project's collision profile table, read once per run.
pub fn collision_table(cx: &Cx<'_>) -> Result<Arc<CollisionTable>, KindError> {
    let t = cx.shared.get_or_init(|| {
        Table(
            config::ini(cx, DEFAULT_ENGINE_INI)
                .map(|ini| Arc::new(collision_table_of(&ini)))
                .map_err(|e| e.to_string()),
        )
    });
    t.0.clone().map_err(KindError::Failed)
}

/// A BodyInstance's (`bi`, the bag) collision over `table`. A named profile: the table's (the
/// body's own array is not read: the profile is loaded over it). 'Custom' or no name: the body's ObjectType and
/// ResponseArray over the channels' defaults; an enabled or an object type the body does not serialise is `None`
/// (the archetype's). A profile the table lacks: responses `None`.
pub fn resolve_body(bi: &Props, table: &CollisionTable) -> Result<ResolvedBody, KindError> {
    let s = |k: &str| match bi.get(k) {
        Some(Value::Str(v)) => Some(v.clone()),
        _ => None,
    };
    let prof = s("CollisionProfileName");
    if let Some(p) = prof.as_deref().filter(|p| *p != "Custom") {
        return Ok(match table.profiles.get(p) {
            Some(t) => ResolvedBody {
                prof: prof.clone(),
                obj: Some(t.obj.clone()),
                enabled: Some(t.enabled.clone()),
                resp: Some(t.resp.clone()),
            },
            None => ResolvedBody { prof: prof.clone(), obj: None, enabled: None, resp: None },
        });
    }
    let mut resp = table.defaults.clone();
    let ra = bi.get("CollisionResponses").and_then(Value::as_obj).and_then(|c| c.get("ResponseArray"));
    for r in ra.and_then(Value::as_arr).unwrap_or_default() {
        let r = r.as_obj();
        let field = |k: &str| r.and_then(|r| r.get(k)).and_then(Value::as_str);
        let ch = field("Channel").unwrap_or("undefined");
        if !resp.contains_key(ch) {
            return fail(format!("stage data: response channel {ch} is not in DefaultEngine.ini"));
        }
        let Some(v) = field("Response") else {
            return fail(format!("stage data: response channel {ch}: no Response"));
        };
        resp.insert(ch.to_owned(), v.to_owned());
    }
    let obj = s("ObjectType").map(|o| match table.chan.get(&o) {
        Some(n) => n.clone(),
        None => o.strip_prefix("ECC_").map_or_else(|| o.chars().skip(4).collect(), str::to_owned),
    });
    let enabled = s("CollisionEnabled").map(|e| e.rsplit("::").next().unwrap_or_default().to_owned());
    Ok(ResolvedBody { prof, obj, enabled, resp: Some(resp) })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lines_as_the_expressions_read_them() {
        assert_eq!(
            default_channel(
                r#"(Channel=ECC_GameTraceChannel1,DefaultResponse=ECR_Ignore,bTraceType=False,bStaticObject=False,Name="Hurtbox")"#
            ),
            Some(("ECC_GameTraceChannel1", "ECR_Ignore", "Hurtbox"))
        );
        let p = r#"(Name="NoCollision",CollisionEnabled=NoCollision,ObjectTypeName="WorldStatic",CustomResponses=((Channel="Visibility",Response=ECR_Ignore),(Channel="Camera")),HelpMessage="No collision",bCanModify=False)"#;
        let (name, en, obj, body) = profile_line(p).unwrap();
        assert_eq!((name, en, obj), ("NoCollision", "NoCollision", "WorldStatic"));
        assert_eq!(responses(body), [("Visibility", "ECR_Ignore"), ("Camera", "ECR_Block")]);
        // (no parenthesis after CustomResponses=: the expression does not match)
        let none = r#"(Name="BlockAll",CollisionEnabled=QueryAndPhysics,ObjectTypeName="WorldStatic",CustomResponses=,HelpMessage="x",bCanModify=False)"#;
        assert_eq!(profile_line(none), None);
        assert_eq!(
            edit_line(r#"(Name="Pawn",CustomResponses=((Channel="Hurtbox",Response=ECR_Overlap)))"#),
            Some(("Pawn", r#"(Channel="Hurtbox",Response=ECR_Overlap)"#))
        );
        assert_eq!(responses(r#"(Channel="A",Response=)(Channel="B")"#), [("B", "ECR_Block")]);
    }
}
