//! The DataTable InputDefaults: per row the action names in table order with their key names in mapping order. No
//! mapping of the build carries a modifier, so a mapping is its key name.

use mvs_data::fighter::Map;
use mvs_data::{InputDefaults, Value};

use super::shape::{arr, fail, strukt, table_rows};
use crate::kind::{Cx, KindError};

pub const INPUT_DEFAULTS_TABLE: &str = "/Game/Panda_Main/UI/InputBinding/InputDefaults";

/// The InputDefaults table's rows.
pub fn input_defaults(cx: &Cx<'_>) -> Result<InputDefaults, KindError> {
    let mut rows = Map::new();
    for (name, row) in table_rows(&cx.object(INPUT_DEFAULTS_TABLE)?)? {
        let Some(row) = row.as_obj() else { return fail(format!("{name}: not a struct")) };
        let mut actions = Map::new();
        // (Actions: a map, as [key, value] pairs in entry order)
        for pair in arr(row, "Actions", &name)? {
            let Some([Value::Str(action), Value::Obj(value)]) = pair.as_arr() else {
                return fail(format!("{name}: Actions: not a [name, struct] pair"));
            };
            let at = format!("{name} {action}");
            let mut keys = Vec::new();
            for m in arr(value, "Mappings", &at)? {
                let Some(m) = m.as_obj() else { return fail(format!("{at}: a mapping is not a struct")) };
                match strukt(m, "Key", &at)?.get("KeyName") {
                    Some(Value::Str(k)) => keys.push(k.clone()),
                    _ => return fail(format!("{at}: a mapping's KeyName is not a string")),
                }
            }
            actions.insert(action.clone(), keys);
        }
        rows.insert(name, actions);
    }
    Ok(InputDefaults { rows })
}
