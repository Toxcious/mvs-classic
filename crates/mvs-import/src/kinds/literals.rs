//! The `literals` kind: string and text constants of a Blueprint function's script, read where the decode found
//! them, JSON (statement offset -> the constant). Text a player reads that no locres line holds (the arcade bots'
//! names) is taken from the player's build here, never typed into the source.
//!
//! No Kismet reader walks the script: each constant is read at its byte offset in the serialized script, which
//! differs from the statement offset the decode names (object references and names are smaller as cooked), and its
//! op byte and form are checked there. The build's paks are checked against their hashes at setup, so the bytes are
//! the ones these offsets were taken from; a constant that is not where it should be fails the import, naming the
//! function and the statement.

use mvs_pak::paths::split_object_path;
use mvs_uasset::value::Value;

use crate::game::LITERALS_KIND_VERSION;
use crate::json::{Json, to_vec};
use crate::kind::{Cx, Item, Kind, KindError};

/// One constant the game reads: the byte offset of its op in the serialized script, and for a localized text the
/// key expected there (an id, not text a player reads); `None` for an FString literal.
pub struct LiteralRead {
    pub statement: u32,
    pub at: usize,
    pub text: Option<&'static str>,
}

/// Per function (`<package>.<class>:<function>`), the constants read (`READS`).
pub const READS: &[(&str, &[LiteralRead])] = &[(
    // PGI_CoOpVsAIManager_C GetArcadeBotLoadouts: the empty-DisplayName fallback MakeLiteralText (@4227), the Rivals
    // prefix Concat_StrStr (@5065), the Mini, Reign Dog and Giant Gizmo names set on L_DisplayName (@5145, @5182,
    // @5216)
    "/Game/Panda_Main/Blueprints/PandaGameInstance/PGI_CoOpVsAIManager.PGI_CoOpVsAIManager_C:GetArcadeBotLoadouts",
    &[
        LiteralRead { statement: 4227, at: 6382, text: Some("8F6DF8F5417F7777C6FCD08F2598F3ED") },
        LiteralRead { statement: 5065, at: 7648, text: None },
        LiteralRead { statement: 5145, at: 7767, text: None },
        LiteralRead { statement: 5182, at: 7820, text: None },
        LiteralRead { statement: 5216, at: 7870, text: None },
    ],
)];

/// Every function of [`READS`].
pub fn items() -> Vec<Item> {
    READS.iter().map(|(p, _)| Item::new(*p)).collect()
}

pub struct Literals;

impl Kind for Literals {
    fn id(&self) -> &'static str {
        "literals"
    }

    fn version(&self) -> u64 {
        LITERALS_KIND_VERSION
    }

    fn content_type(&self, _: &[u8]) -> &'static str {
        "application/json"
    }

    /// [`items`].
    fn list(&self, _: &Cx<'_>) -> Result<Vec<Item>, KindError> {
        Ok(items())
    }

    fn build(&self, cx: &Cx<'_>, item: &Item) -> Result<Vec<u8>, KindError> {
        let path = item.path.as_str();
        let Some((_, reads)) = READS.iter().find(|(p, _)| *p == path) else {
            return Err(KindError::NotFound(format!("literals: {path} is no function whose constants the game reads")));
        };
        let (pkg_path, object) = split_object_path(path);
        let object = object.unwrap_or_default();
        let colon = object.find(':').ok_or_else(|| KindError::Failed(format!("literals: {path} names no function")))?;
        let (cls, func) = (&object[..colon], &object[colon + 1..]);
        let (pkg, top) = cx.read(&format!("{pkg_path}.{cls}"))?;
        let fi = pkg
            .exports
            .iter()
            .position(|e| usize::try_from(e.outer).ok() == Some(top + 1) && e.object_name == func)
            .ok_or_else(|| KindError::Failed(format!("literals: {cls} has no function {func}")))?;
        let obj = cx.cache.object(&pkg, fi);
        if let Some(e) = obj.issues.first() {
            return Err(KindError::Failed(format!("{} export {fi}: {}", pkg.path, e.message)));
        }
        let script = script_bytes(&pkg, &obj)?;
        let mut got = Json::obj();
        for r in *reads {
            let fail = |m: String| KindError::Failed(format!("literals: {path} @{}: {m}", r.statement));
            let v = script_literal(script, r.at).map_err(fail)?;
            match (r.text, &v) {
                (None, Literal::Str(_)) => {}
                (Some(want), Literal::Text { key, .. }) if key == want => {}
                (Some(want), Literal::Text { key, .. }) => return Err(fail(format!("text key {key}, not {want}"))),
                (_, v) => {
                    let (is, was) = if matches!(v, Literal::Str(_)) { ("string", "text") } else { ("text", "string") };
                    return Err(fail(format!("a {is} constant where a {was} was read before")));
                }
            }
            got.set(r.statement.to_string(), v.json());
        }
        to_vec(&got).map_err(|e| KindError::Failed(e.to_string()))
    }
}

/// A script's constant as the entry writes it.
#[derive(Debug, PartialEq)]
pub enum Literal {
    Str(String),
    Text { source: String, namespace: String, key: String },
}

impl Literal {
    fn json(&self) -> Json {
        match self {
            Literal::Str(s) => Json::Str(s.clone()),
            Literal::Text { source, namespace, key } => Json::object([
                ("source", Json::Str(source.clone())),
                ("namespace", Json::Str(namespace.clone())),
                ("key", Json::Str(key.clone())),
            ]),
        }
    }
}

// EExprToken (UE 4.26 Script.h)
const EX_STRING_CONST: u8 = 0x1f;
const EX_TEXT_CONST: u8 = 0x29;
const EX_UNICODE_STRING_CONST: u8 = 0x34;
/// EBlueprintTextLiteralType::LocalizedText
const TEXT_LOCALIZED: u8 = 1;

/// A string or text constant of a serialized script, its op byte at `at`: an
/// EX_StringConst (Latin-1 up to a 0) or EX_UnicodeStringConst (UTF-16 up to a 0) as its string; an EX_TextConst of a
/// LocalizedText (its source, key and namespace, each one of those two) as a text. An error naming the place for any
/// other op or a constant that runs past the script's end.
pub fn script_literal(script: &[u8], at: usize) -> Result<Literal, String> {
    let mut p = at;
    let fail = |what: &str| format!("script constant at {at}: {what}");
    fn byte(script: &[u8], p: &mut usize, fail: &dyn Fn(&str) -> String) -> Result<u8, String> {
        let b = *script.get(*p).ok_or_else(|| fail("runs past the end of the script"))?;
        *p += 1;
        Ok(b)
    }
    fn string(script: &[u8], p: &mut usize, fail: &dyn Fn(&str) -> String) -> Result<String, String> {
        let op = byte(script, p, fail)?;
        if op == EX_STRING_CONST {
            let end = script[*p..]
                .iter()
                .position(|&b| b == 0)
                .map(|i| *p + i)
                .ok_or_else(|| fail("an EX_StringConst runs past the end of the script"))?;
            let s: String = script[*p..end].iter().map(|&b| char::from(b)).collect();
            *p = end + 1;
            return Ok(s);
        }
        if op == EX_UNICODE_STRING_CONST {
            let mut end = *p;
            while end + 1 < script.len() && (script[end] != 0 || script[end + 1] != 0) {
                end += 2;
            }
            if end + 1 >= script.len() {
                return Err(fail("an EX_UnicodeStringConst runs past the end of the script"));
            }
            let units: Vec<u16> = script[*p..end].as_chunks::<2>().0.iter().map(|c| u16::from_le_bytes(*c)).collect();
            *p = end + 2;
            return Ok(String::from_utf16_lossy(&units));
        }
        Err(fail(&format!("op 0x{op:x} at {}, not a string constant", *p - 1)))
    }
    if script.get(at) == Some(&EX_TEXT_CONST) {
        p += 1;
        let ty = byte(script, &mut p, &fail)?;
        if ty != TEXT_LOCALIZED {
            return Err(fail(&format!("an EX_TextConst of literal type {ty}, not LocalizedText")));
        }
        let source = string(script, &mut p, &fail)?;
        let key = string(script, &mut p, &fail)?;
        let namespace = string(script, &mut p, &fail)?;
        return Ok(Literal::Text { source, namespace, key });
    }
    string(script, &mut p, &fail).map(Literal::Str)
}

/// A function's serialized script: the bytes as cooked, where the object kind's UStruct
/// reader noted them (`ScriptAt`, `ScriptSize`).
fn script_bytes<'p>(pkg: &'p mvs_uasset::Package, obj: &mvs_uasset::objects::UObj) -> Result<&'p [u8], KindError> {
    let (Some(Value::Int(at)), Some(Value::Int(size))) = (obj.data.get("ScriptAt"), obj.data.get("ScriptSize")) else {
        return Err(KindError::Failed("literals: no script (not a UStruct)".into()));
    };
    let uexp = pkg.uexp()?;
    let (at, size) = (usize::try_from(*at).unwrap_or(usize::MAX), usize::try_from(*size).unwrap_or(usize::MAX));
    uexp.get(at..at.saturating_add(size))
        .ok_or_else(|| KindError::Failed("literals: the script runs past the data".into()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn constants() {
        let mut s = vec![0u8, EX_STRING_CONST];
        s.extend(b"Rivals: \0");
        assert_eq!(script_literal(&s, 1), Ok(Literal::Str("Rivals: ".into())));
        let mut u = vec![EX_UNICODE_STRING_CONST];
        u.extend("Mini".encode_utf16().flat_map(u16::to_le_bytes));
        u.extend([0, 0]);
        assert_eq!(script_literal(&u, 0), Ok(Literal::Str("Mini".into())));
        let mut t = vec![EX_TEXT_CONST, TEXT_LOCALIZED];
        for part in ["src", "KEY", "ns"] {
            t.push(EX_STRING_CONST);
            t.extend(part.bytes());
            t.push(0);
        }
        assert_eq!(
            script_literal(&t, 0),
            Ok(Literal::Text { source: "src".into(), namespace: "ns".into(), key: "KEY".into() })
        );
        assert!(script_literal(&[0x10], 0).is_err());
        assert!(script_literal(&[EX_STRING_CONST, b'a'], 0).is_err());
        assert!(script_literal(&[EX_TEXT_CONST, 0], 0).is_err());
    }
}
