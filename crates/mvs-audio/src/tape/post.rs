//! The post tape: every call the game made on the sound engine, in order, with the sim tick it was made on. The
//! oracle records it during a match run; the glue fed the same match's events must make the same calls.
//!
//! One line per call: `{"tick": <sim tick>, "call": "<name>", ...arguments}`, the arguments by the oracle's
//! WwiseAudio method's parameter names, names as the game passed them (not hashed), a parameter's default filled
//! in where the method has one (setRtpc's interpMs, stopKey's and stopAll's fade), another missing optional
//! argument left out. The glue makes these lines and [`crate::Audio::call`] takes them:
//!
//! ```text
//! {"tick":120,"call":"post","ev":"Play_Cha_Shaggy_Jump","owner":0,"key":"jump:3","at":[0,10,20]}
//! {"tick":120,"call":"setRtpc","name":"Fighter_Damage","value":42,"owner":0,"interpMs":0}
//! {"tick":120,"call":"setSwitch","group":"Surface","value":"Grass","owner":0}
//! {"tick":0,"call":"setState","group":"Music_State","value":"InMatch"}
//! {"tick":121,"call":"setEmitter","owner":0,"pos":[0,0,0],"front":[1,0,0]}
//! {"tick":121,"call":"setListener","pos":[..],"right":[..],"up":[..],"front":[..]}
//! {"tick":0,"call":"setReverbVolumes","list":[{"bus":"Rev_Small","loc":[..],"scale":[..],"level":1,"rate":1,"priority":0}]}
//! {"tick":300,"call":"stopKey","key":"jump:3","fade":0.05,"curve":"Linear"}
//! {"tick":900,"call":"stopAll","fade":0.1,"keep":"STAGE_AMBIENCE"}
//! {"tick":0,"call":"scope","name":"match","keep":["play_..."],"lazy":[]}
//! {"tick":0,"call":"scopeMusic","ev":"Play_Music","values":["Intro"],"flow":{"group":"..","start":"..","next":{"..":[".."]}}}
//! {"tick":0,"call":"preloadMusic","ev":"Play_Music","values":["Intro"]}
//! {"tick":0,"call":"releaseMusic","ev":"Play_Music","values":["Intro"]}
//! {"tick":0,"call":"resume"}
//! ```

use indexmap::IndexMap;
use serde::{Deserialize, Serialize};

use crate::V3;

/// A reverb volume as the game passes it.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ReverbArg {
    pub bus: String,
    pub loc: Vec<f64>,
    pub scale: Vec<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub level: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rate: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub priority: Option<f64>,
}

/// A music flow as the game passes it.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct FlowArg {
    pub group: String,
    pub start: String,
    pub next: IndexMap<String, Vec<String>>,
}

/// A call on the sound engine.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "call", rename_all = "camelCase")]
pub enum Call {
    Post {
        ev: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        owner: Option<i32>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        key: Option<String>,
        #[serde(default, rename = "stopKey", skip_serializing_if = "Option::is_none")]
        stop_key: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        at: Option<V3>,
    },
    /// (the port's own) A post on `owner` whose Play actions skip their probability's roll: a sound the build plays
    /// by chance that a feature of the port's own wants each time.
    PostSure {
        ev: String,
        owner: i32,
    },
    SetRtpc {
        name: String,
        value: f64,
        owner: i32,
        #[serde(rename = "interpMs")]
        interp_ms: f64,
    },
    SetSwitch {
        group: String,
        value: String,
        owner: i32,
    },
    SetState {
        group: String,
        value: String,
    },
    SetEmitter {
        owner: i32,
        pos: V3,
        front: V3,
    },
    SetListener {
        pos: V3,
        right: V3,
        up: V3,
        front: V3,
    },
    SetReverbVolumes {
        list: Vec<ReverbArg>,
    },
    StopKey {
        key: String,
        fade: f64,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        curve: Option<String>,
    },
    StopAll {
        fade: f64,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        keep: Option<String>,
    },
    Scope {
        name: String,
        keep: Vec<String>,
        #[serde(default)]
        lazy: Vec<String>,
    },
    ScopeMusic {
        ev: String,
        values: Vec<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        flow: Option<FlowArg>,
    },
    PreloadMusic {
        ev: String,
        values: Vec<String>,
    },
    ReleaseMusic {
        ev: String,
        values: Vec<String>,
    },
    Resume,
}

/// A line of the post tape.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Line {
    pub tick: u32,
    #[serde(flatten)]
    pub call: Call,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_lines() {
        let text = concat!(
            r#"{"tick":120,"call":"post","ev":"Play_X","owner":0,"key":"jump:3","at":[0,10,20]}"#,
            "\n",
            r#"{"tick":121,"call":"setRtpc","name":"P","value":0.1,"owner":-1,"interpMs":0}"#,
            "\n",
            r#"{"tick":122,"call":"resume"}"#,
            "\n",
        );
        let lines: Vec<Line> = crate::tape::parse(text, "test").unwrap();
        assert_eq!(lines.len(), 3);
        let Call::Post { ev, owner, key, stop_key, at } = &lines[0].call else { panic!("{:?}", lines[0]) };
        assert_eq!(
            (ev.as_str(), *owner, key.as_deref(), stop_key, *at),
            ("Play_X", Some(0), Some("jump:3"), &None, Some([0.0, 10.0, 20.0]))
        );
        assert_eq!(lines[1].call, Call::SetRtpc { name: "P".into(), value: 0.1, owner: -1, interp_ms: 0.0 });
        assert_eq!(lines[2], Line { tick: 122, call: Call::Resume });
    }
}
