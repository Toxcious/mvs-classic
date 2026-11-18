//! The ForceFeedbackEffect assets of Blueprints/ForceFeedback by asset name: per channel the motors its four bAffects
//! flags name and its curve's keys [time, value]; the effect's duration by GetDuration's rule

use mvs_data::{RumbleChannel, RumbleEffect, RumbleTable};
use mvs_math::js;

use super::shape::{arr, bool, fail, num, package_name, properties, strukt};
use crate::kind::{Cx, KindError};

pub const RUMBLE_FOLDER: &str = "/Game/Panda_Main/Blueprints/ForceFeedback";
pub const RUMBLE_CLASS: &str = "/Script/Engine.ForceFeedbackEffect";

/// The ForceFeedbackEffects by name, in code-unit order of their names (the folder's listing is
/// the store's, and the game looks an effect up by name).
pub fn rumble(cx: &Cx<'_>) -> Result<RumbleTable, KindError> {
    let mut effects = RumbleTable::new();
    for path in cx.catalog.list_packages(RUMBLE_FOLDER, false, None) {
        let o = cx.object(&path)?;
        if o.class() != RUMBLE_CLASS {
            continue;
        }
        let name = package_name(&path).to_owned();
        let p = properties(&o)?;
        let mut channels = Vec::new();
        for (i, c) in arr(&p, "ChannelDetails", &name)?.iter().enumerate() {
            let at = format!("{name} channel {i}");
            let Some(c) = c.as_obj() else { return fail(format!("{at}: not a struct")) };
            let curve = strukt(strukt(c, "Curve", &at)?, "EditorCurveData", &at)?;
            let mut keys = Vec::new();
            for k in arr(curve, "Keys", &at)? {
                let Some(k) = k.as_obj() else { return fail(format!("{at}: a key is not a struct")) };
                keys.push((num(k, "Time", &at)?, num(k, "Value", &at)?));
            }
            // FForceFeedbackChannelDetails' four bAffects flags as the motor bits the mixer writes (GetValues
            // 142a5b430: 1 LeftLarge, 2 LeftSmall, 4 RightLarge, 8 RightSmall)
            let mut motors = 0u8;
            for (k, b) in [
                ("bAffectsLeftLarge", 1),
                ("bAffectsLeftSmall", 2),
                ("bAffectsRightLarge", 4),
                ("bAffectsRightSmall", 8),
            ] {
                if bool(c, k, &at)? {
                    motors |= b;
                }
            }
            channels.push(RumbleChannel { motors, keys });
        }
        // UForceFeedbackEffect GetDuration (142a58260): the largest last key time over the channels
        let duration = channels.iter().fold(0.0, |m, c| js::max(m, c.keys.last().map_or(0.0, |k| k.0)));
        effects.insert(name, RumbleEffect { duration, channels });
    }
    effects.sort_unstable_keys();
    Ok(effects)
}
