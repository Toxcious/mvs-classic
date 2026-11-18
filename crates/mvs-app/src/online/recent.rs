//! Recently played: everyone this player met in a custom game lobby or an online match,
//! newest first, kept on this PC. It stands in for the build's GetRecentlyPlayedSocialList (UI_SocialPanel's
//! socialListRecent), which the backend filled.
//!
//! Deviation: kept in `recent.json` in the data folder, written at
//! each change.

use std::fs;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};
use serde_json::Value;

/// A player met.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct RecentPlayer {
    /// Their id: the one an invitation goes to.
    pub id: String,
    pub name: String,
    /// Their equipped profile icon's ProfileIconData asset name ('' none known).
    pub icon: String,
    /// When last met (ms since 1970).
    pub at: f64,
}

/// A player of the lobby or the match now (`icon` None: not given).
#[derive(Clone, Debug, PartialEq)]
pub struct MetPlayer {
    pub id: String,
    pub name: String,
    pub icon: Option<String>,
}

pub const RECENT_MAX: usize = 50;
pub const FILE: &str = "recent.json";

/// The list as a file gave it: repeated ids, entries without an id or a name dropped (an unreadable one: empty).
fn read(text: Option<&str>) -> Vec<RecentPlayer> {
    let Some(Ok(Value::Array(raw))) = text.map(serde_json::from_str::<Value>) else { return Vec::new() };
    let mut out: Vec<RecentPlayer> = Vec::new();
    for p in &raw {
        let (Some(id), Some(name)) = (p.get("id").and_then(Value::as_str), p.get("name").and_then(Value::as_str))
        else {
            continue;
        };
        if id.is_empty() || out.iter().any(|x| x.id == id) {
            continue;
        }
        let icon = p.get("icon").and_then(Value::as_str).unwrap_or_default().to_owned();
        let at = p.get("at").and_then(Value::as_f64).filter(|n| n.is_finite()).unwrap_or(0.0);
        out.push(RecentPlayer { id: id.to_owned(), name: name.to_owned(), icon, at });
        if out.len() == RECENT_MAX {
            break;
        }
    }
    out
}

pub struct RecentPlayers {
    list: Vec<RecentPlayer>,
    /// The file (None: none, the list lasts for the run).
    file: Option<PathBuf>,
}

impl RecentPlayers {
    pub fn open(file: Option<PathBuf>) -> RecentPlayers {
        let text = file.as_ref().and_then(|f| fs::read_to_string(f).ok());
        RecentPlayers { list: read(text.as_deref()), file }
    }

    /// Newest first.
    pub fn players(&self) -> &[RecentPlayer] {
        &self.list
    }

    /// Those met now (`me` and anyone without an id left out), in front in their order; a name or icon not given
    /// keeps the one kept. True when the list shows differently (who, their order, a name or an icon).
    pub fn met(&mut self, players: &[MetPlayer], me: &str, at: f64) -> bool {
        let mut front: Vec<RecentPlayer> = Vec::new();
        for p in players {
            if p.id.is_empty() || p.id == me || front.iter().any(|x| x.id == p.id) {
                continue;
            }
            let old = self.list.iter().find(|x| x.id == p.id);
            let name = if p.name.is_empty() { old.map(|o| o.name.clone()).unwrap_or_default() } else { p.name.clone() };
            let icon = p.icon.clone().or_else(|| old.map(|o| o.icon.clone())).unwrap_or_default();
            front.push(RecentPlayer { id: p.id.clone(), name, icon, at });
        }
        if front.is_empty() {
            return false;
        }
        let mut next = front.clone();
        next.extend(self.list.iter().filter(|x| !front.iter().any(|f| f.id == x.id)).cloned());
        next.truncate(RECENT_MAX);
        let changed = next.len() != self.list.len()
            || next.iter().zip(&self.list).any(|(a, b)| a.id != b.id || a.name != b.name || a.icon != b.icon);
        self.list = next;
        if let Some(f) = &self.file {
            // (no file written: the list lasts for the run)
            let text = serde_json::to_string(&self.list).expect("the list is JSON");
            if let Err(e) = fs::write(f, text) {
                eprintln!("online: {} could not be written: {e}", f.display());
            }
        }
        changed
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn met(id: &str, name: &str, icon: Option<&str>) -> MetPlayer {
        MetPlayer { id: id.into(), name: name.into(), icon: icon.map(Into::into) }
    }

    fn ids(r: &RecentPlayers) -> String {
        r.players().iter().map(|p| p.id.as_str()).collect::<Vec<_>>().join(",")
    }

    #[test]
    fn the_list() {
        let dir = std::env::temp_dir().join(format!("mvs-recent-{}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        let file = dir.join(FILE);
        let _ = fs::remove_file(&file);
        let mut r = RecentPlayers::open(Some(file.clone()));
        assert!(r.players().is_empty());
        let c1 = r.met(
            &[
                met("me", "Me", None),
                met("a", "Ann", Some("ProfileIcon_A")),
                met("", "Nobody", None),
                met("b", "Bob", None),
            ],
            "me",
            1000.0,
        );
        assert!(c1 && ids(&r) == "a,b" && r.players().iter().all(|p| p.at == 1000.0));
        let c2 = r.met(&[met("a", "Ann", Some("ProfileIcon_A")), met("b", "Bob", None)], "me", 2000.0);
        assert!(!c2 && r.players()[0].at == 2000.0);
        let c3 = r.met(&[met("b", "", None)], "me", 2000.0);
        assert!(c3 && ids(&r) == "b,a" && r.players()[0].name == "Bob");
        let c4 = r.met(&[met("a", "Ann2", None)], "me", 2000.0);
        assert!(c4 && r.players()[0].name == "Ann2" && r.players()[0].icon == "ProfileIcon_A");
        let again = RecentPlayers::open(Some(file.clone()));
        assert!(ids(&again) == "a,b" && again.players()[0].name == "Ann2");
        for i in 0..RECENT_MAX + 5 {
            r.met(&[met(&format!("p{i}"), &format!("P{i}"), None)], "me", 3000.0);
        }
        assert_eq!(r.players().len(), RECENT_MAX);
        assert!(r.players()[0].id == format!("p{}", RECENT_MAX + 4) && !r.players().iter().any(|p| p.id == "a"));
        fs::write(&file, "{not json").unwrap();
        assert!(RecentPlayers::open(Some(file.clone())).players().is_empty());
        fs::write(&file, r#"[{"id":"x","name":"X"},{"id":"x","name":"X2"},{"name":"no id"},5]"#).unwrap();
        let odd = RecentPlayers::open(Some(file.clone()));
        assert!(ids(&odd) == "x" && odd.players()[0].icon.is_empty());
        assert!(RecentPlayers::open(None).met(&[met("a", "A", None)], "me", 0.0));
        let _ = fs::remove_dir_all(&dir);
    }
}
