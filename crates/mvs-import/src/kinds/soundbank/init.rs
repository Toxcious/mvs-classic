//! What the InitBank entry holds of the InitBank's own tree: the buses with their
//! effects, curves and states, the game parameters' defaults and ramps, the state groups, the volume threshold and
//! the voice limit.

use std::collections::BTreeMap;

use mvs_data::sound::*;
use mvs_uasset::value::Value;

use super::compact::{limit_flags, rtpcs};
use super::fx::fx_params;
use super::tree::*;

/// What the InitBank's tree gives, apart from the attenuations and the event index.
pub(super) struct InitParts {
    pub buses: BTreeMap<Id, Bus>,
    pub rtpc_defaults: BTreeMap<Id, f64>,
    pub rtpc_ramps: BTreeMap<Id, RtpcRamp>,
    pub state_groups: BTreeMap<Id, f64>,
    pub volume_threshold: f64,
    pub max_voices: u32,
}

/// The first value of `key` found by a depth-first walk (a null one does not count).
fn find_key<'a>(v: &'a Value, key: &str) -> Option<&'a Value> {
    match v {
        Value::Array(a) => a.iter().find_map(|x| find_key(x, key)),
        Value::Struct(p) => {
            if let Some(x) = p.get(key) {
                return Some(x);
            }
            p.values().find_map(|x| find_key(x, key))
        }
        _ => None,
    }
}

/// Every struct of the tree, outer before inner.
fn walk<'a>(v: &'a Value, visit: &mut impl FnMut(&'a Value)) {
    match v {
        Value::Array(a) => a.iter().for_each(|x| walk(x, visit)),
        Value::Struct(p) => {
            visit(v);
            p.values().for_each(|x| walk(x, visit));
        }
        _ => {}
    }
}

fn ramp_type(v: Option<&Value>, what: &str) -> R<u8> {
    match v {
        Some(Value::Int(n)) => Ok(u8::try_from(*n).unwrap_or(0)),
        Some(Value::UInt(n)) => Ok(u8::try_from(*n).unwrap_or(0)),
        Some(Value::Str(s)) => match s.rsplit("::").next().unwrap_or("") {
            "None" => Ok(0),
            "SlewRate" => Ok(1),
            "FilteringOverTime" => Ok(2),
            k => fail(what, format!("ramp type {k}")),
        },
        _ => fail(what, format!("ramp type {v:?}")),
    }
}

/// `er`: the early reflection taps of a RoomVerb pattern index.
pub(super) fn init_parts(bank: &Value, er: &dyn Fn(u32) -> R<ErTaps>, what: &str) -> R<InitParts> {
    let Some(volume_threshold) = f(find_key(bank, "VolumeThreshold")) else {
        return fail(what, "no VolumeThreshold");
    };
    let Some(max_voices) = int::<u32>(find_key(bank, "MaxNumVoicesLimitInternal")) else {
        return fail(what, "no MaxNumVoicesLimitInternal");
    };
    let mut rtpc_defaults = BTreeMap::new();
    let mut rtpc_ramps = BTreeMap::new();
    for x in items(find_key(bank, "RTPCRampingParams")) {
        let id: Id = req_int(x, "RtpcId", what)?;
        rtpc_defaults.insert(id, req_f(x, "Value", what)?);
        let kind = ramp_type(get(x, "RampType"), what)?;
        if kind != 0 {
            rtpc_ramps.insert(id, RtpcRamp { kind, up: req_f(x, "RampUp", what)?, down: req_f(x, "RampDown", what)? });
        }
    }
    let mut hier: BTreeMap<Id, &Value> = BTreeMap::new();
    walk(bank, &mut |o| {
        if let (Some(_), Some(_), Some(id)) = (get(o, "Type"), get(o, "Data"), get(o, "Id"))
            && let Some(id) = str_of(Some(id)).and_then(|s| s.parse().ok()).or_else(|| int(Some(id)))
        {
            hier.insert(id, o);
        }
    });
    let mut buses = BTreeMap::new();
    for (&id, &h) in &hier {
        let aux = match str_of(get(h, "Type")) {
            Some("AudioBus") => false,
            Some("AuxiliaryBus") => true,
            _ => continue,
        };
        let Some(d) = get(h, "Data") else { continue };
        let mut pr: BTreeMap<u32, f64> = BTreeMap::new();
        for p in list(d, "Props") {
            if let (Some(k), Some(v)) = (int(get(p, "Id")), f(get(p, "Value"))) {
                pr.insert(k, v);
            }
        }
        let mut b = Bus { aux, ..Bus::default() };
        b.parent = int::<Id>(get(d, "OverrideBusId")).filter(|&i| i != 0);
        b.vol = pr.get(&5).copied().unwrap_or(0.0);
        // CAkBus::SetInitialParams: limit byte 1 kill newest, 2 use virtual voice behaviour, 4 ignore parent limit,
        // then the u16 maximum
        let max_inst: u16 = int(get(d, "MaxNumInstance")).unwrap_or(0);
        if max_inst != 0 {
            b.max_inst = max_inst;
            b.lim = limit_flags(get(d, "AdvSettingsParams"), what)? & 7;
        }
        // bit 0 bIsHdrBus, bit 1 bHdrReleaseModeExponential; props 27 / 28 / 29 = HDRBusThreshold / Ratio /
        // ReleaseTime
        if let Some(flags) = int::<i64>(get(d, "HdrEnvelopeFlags"))
            && flags & 1 != 0
        {
            b.hdr = Some(Hdr {
                threshold: pr.get(&27).copied().unwrap_or(96.0),
                ratio: pr.get(&28).copied().unwrap_or(16.0),
                release: pr.get(&29).copied().unwrap_or(1.0),
                exponential: flags & 2 != 0,
            });
        }
        if let Some(fb) = get(d, "FxBusParams") {
            for ch in list(fb, "FxChunks") {
                let Some(x) = int::<Id>(get(ch, "FxId")).and_then(|i| hier.get(&i)) else { continue };
                let Some(xd) = get(x, "Data") else { continue };
                let plugin = get(xd, "Plugin").and_then(|p| str_of(get(p, "PluginId"))).unwrap_or("None");
                let params = get(xd, "PluginParams").and_then(|p| get(p, "Params"));
                let fx = fx_params(plugin, params, what)?;
                let er_taps = match (&fx, params) {
                    (FxParams::AkRoomVerbFX(_), Some(p)) => {
                        let Some(i) = get(p, "InvariantParams").and_then(|v| int::<u32>(get(v, "ERPattern"))) else {
                            return fail(what, "a RoomVerb without an ERPattern");
                        };
                        Some(er(i)?)
                    }
                    _ => None,
                };
                b.fx.push(BusFx { fx, rtpc: rtpcs(list(xd, "RTPCs"), true, what)?, er: er_taps });
            }
        }
        b.rtpc = rtpcs(list(d, "RTPCs"), true, what)?;
        for g in list(d, "StateGroups") {
            let gid: Id = req_int(g, "Id", what)?;
            for s in list(g, "States") {
                let sid: Id = req_int(s, "Id", what)?;
                let inst: Id = req_int(s, "StateInstanceId", what)?;
                let Some(se) = hier.get(&inst).and_then(|h| get(h, "Data")).and_then(|d| get(d, "Settings")) else {
                    continue;
                };
                let Value::Struct(members) = se else { continue };
                // CUE4Parse names the settings by property id; the bank numbers them as RTPC parameters, so its
                // PlaybackPriority (7) is MakeUpGain and its HdrEnvelopeActiveRange (0x21) is BypassAllFX
                let mut st = StateSet::default();
                let mut any = false;
                for (k, v) in members {
                    let slot = match k.as_str() {
                        "BusVolume" => &mut st.vol,
                        "VoiceLowPass" => &mut st.lpf,
                        "PlaybackPriority" => &mut st.mk,
                        "HdrEnvelopeActiveRange" => &mut st.bypass,
                        k => return fail(what, format!("state setting {k} not handled")),
                    };
                    if let Some(x) = f(Some(v)).filter(|&x| x != 0.0) {
                        *slot = x;
                        any = true;
                    }
                }
                if any {
                    b.states.entry(gid).or_default().insert(sid, st);
                }
            }
        }
        buses.insert(id, b);
    }
    let mut state_groups = BTreeMap::new();
    walk(bank, &mut |o| {
        if let (Some(g), Some(t)) = (get(o, "StateGroupId"), get(o, "DefaultTransitionTime"))
            && let (Some(g), Some(t)) = (int::<Id>(Some(g)), f(Some(t)))
        {
            state_groups.insert(g, t);
        }
    });
    Ok(InitParts { buses, rtpc_defaults, rtpc_ramps, state_groups, volume_threshold, max_voices })
}
