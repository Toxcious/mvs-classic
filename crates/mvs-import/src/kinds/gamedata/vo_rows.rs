//! A DTVO DataTable's rows as Comp_Actor_VO InitVOData takes them, with each event's length: one bind for every VO
//! table the port reads (a fighter's default skin, the other skins, an announcer pack).

use mvs_data::fighter::VoRow;
use mvs_data::{Props, Value};

use super::shape::{fail, properties, short_name, table_rows};
use crate::kind::{Cx, KindError};
use crate::objview::ObjView;

const EVENT_DATA: &str = "/Script/AkAudio.AkAudioEventData";
/// The localized event package a VO event's length is read from: the store's map key is `English(US)`.
// Unverified: the build's own pick is the player's language (not read natively).
const VO_LANGUAGE: &str = "English(US)";

/// An event's own AkAudioEventData objects (its direct inner objects of that class), in export order, as bags.
fn event_datas(o: &ObjView<'_>) -> Result<Vec<Props>, KindError> {
    let mut out = Vec::new();
    for i in o.inner()? {
        if i.class() == EVENT_DATA && !i.chain().contains(['.', ':']) {
            out.push(properties(&i)?);
        }
    }
    Ok(out)
}

/// An AkAudioEvent's length: its AkAudioEventData's MaximumDuration. The data is the event's own, or for a localized
/// event with no own data or only a stub (no MediaList, no MaximumDuration: Finn's vo_finn_sys_victory /
/// matchstart_ctx_21-24), the VO_LANGUAGE package's. It stands for the time from the post to the end-of-event
/// callback: exact for one sound, the longest variation for a random container (Unverified). `None` where the data
/// states none.
pub fn event_length(cx: &Cx<'_>, event: &str) -> Result<Option<f64>, KindError> {
    let ev = cx.object(event)?;
    let mut datas = event_datas(&ev)?;
    let props = properties(&ev)?;
    let loc = props.get("LocalizedPlatformAssetDataMap").and_then(Value::as_arr).filter(|l| !l.is_empty());
    let stub = datas.iter().all(|p| !p.contains_key("MediaList") && !p.contains_key("MaximumDuration"));
    if datas.is_empty() || (loc.is_some() && stub) {
        let lang = loc.and_then(|l| {
            l.iter().find_map(|e| match e.as_arr() {
                Some([Value::Str(k), Value::Str(v)]) if k == VO_LANGUAGE => Some(v.clone()),
                _ => None,
            })
        });
        if let Some(lang) = lang {
            datas = event_datas(&cx.object(&lang)?)?;
        }
    }
    Ok(datas.last().and_then(|p| p.get("MaximumDuration")).and_then(Value::as_f64))
}

/// A DTVO DataTable's rows in table order (Comp_Actor_VO InitVOData), those `keep` passes (by alias).
pub fn vo_rows(cx: &Cx<'_>, table: &str, keep: &dyn Fn(&str) -> bool) -> Result<Vec<VoRow>, KindError> {
    let o = cx.object(table)?;
    let mut out = Vec::new();
    for (name, r) in table_rows(&o)? {
        let Some(r) = r.as_obj() else { return fail(format!("{table}: row {name} is not a struct")) };
        let text = |k: &str| match r.get(k) {
            Some(Value::Str(s)) => Ok(s.clone()),
            _ => fail(format!("{table}: row {name}: {k} is not a string")),
        };
        let alias = text("Alias")?;
        if !keep(&alias) {
            continue;
        }
        let tags = text("Tags")?;
        let Some(priority) = r.get("Priority").and_then(Value::as_f64) else {
            return fail(format!("{table}: row {name}: Priority is not a number"));
        };
        let ev = match r.get("AkAudioEvent") {
            Some(Value::Str(s)) if !s.is_empty() => Some(s.as_str()),
            _ => None,
        };
        let len = match ev {
            Some(e) => event_length(cx, e)?,
            None => None,
        };
        out.push((alias, tags, priority, ev.map(|e| short_name(e).to_owned()), len));
    }
    Ok(out)
}
