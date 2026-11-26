//! (the port's own) The launcher's patch notes: the text of the repository's releases, the same a release's page
//! on GitHub shows. The GitHub API's list of releases beside the update feed is read off the frame's thread when
//! the launcher opens and kept in the data folder (`patch-notes.json`), so the next start shows the notes at once
//! and a start with no network still shows them. A build without an update feed asks nothing and shows what it
//! kept.

use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, Receiver};

#[cfg(windows)]
use mvs_release::{NOTES_KEPT, ReleaseNotes, newer, notes_feed, notes_from_github, parse_version};
#[cfg(not(windows))]
use mvs_release::{NOTES_KEPT, ReleaseNotes, newer, notes_feed, parse_version};
use mvs_ui::launcher::notes::{self as ui, NoteAge, PatchNote};
use mvs_ui::launcher::words as w;

#[cfg(not(windows))]
use crate::update::UpdateError;
#[cfg(windows)]
use crate::update::{UpdateError, http};

/// The kept list's file in the data folder.
pub const FILE: &str = "patch-notes.json";
/// A developer's run names the releases' list with this (a dev build has no update feed): the list's whole
/// address, `https://api.github.com/repos/<owner>/<repo>/releases`.
pub const FEED_ENV: &str = "MVS_NOTES_FEED";

/// The list's answer is a few KB a release: one that answers more is not it.
#[cfg(windows)]
const LIST_MAX: usize = 1024 * 1024;

/// Where the releases' list is read: beside this build's update feed, or where a developer's run says. None:
/// never read.
pub fn feed() -> Option<String> {
    std::env::var(FEED_ENV).ok().filter(|f| !f.is_empty()).or_else(|| mvs_release::FEED.and_then(notes_feed))
}

/// The list kept in `data` (none, or one that does not read: empty).
pub fn kept(data: &Path) -> Vec<ReleaseNotes> {
    let mut list: Vec<ReleaseNotes> =
        std::fs::read(data.join(FILE)).ok().and_then(|b| serde_json::from_slice(&b).ok()).unwrap_or_default();
    list.retain(|n| parse_version(&n.version).is_some());
    list.truncate(NOTES_KEPT);
    list
}

fn keep(data: &Path, list: &[ReleaseNotes]) -> std::io::Result<()> {
    std::fs::create_dir_all(data)?;
    std::fs::write(data.join(FILE), serde_json::to_vec_pretty(list).map_err(std::io::Error::other)?)
}

#[cfg(windows)]
fn get(url: &str) -> Result<Vec<ReleaseNotes>, String> {
    let mut body = Vec::new();
    http::get(
        url,
        |s| if (200..300).contains(&s) { Ok(()) } else { Err(UpdateError::FeedStatus(s)) },
        |chunk| {
            if body.len() + chunk.len() > LIST_MAX {
                return Err(UpdateError::Latest(w::LATEST_TOO_LONG.to_owned()));
            }
            body.extend_from_slice(chunk);
            Ok(())
        },
    )
    .map_err(|e| e.to_string())?;
    notes_from_github(&String::from_utf8(body).map_err(|e| e.to_string())?)
}

/// The list at `url`: never read on Linux, where the updater (WinHTTP) is off.
#[cfg(not(windows))]
fn get(_url: &str) -> Result<Vec<ReleaseNotes>, String> {
    Err(UpdateError::NoFeed.to_string())
}

/// Reads the list at `url` on a thread of its own and keeps it in `data`; the answer comes once. None: the thread
/// did not start.
pub fn fetch(url: String, data: PathBuf) -> Option<Receiver<Result<Vec<ReleaseNotes>, String>>> {
    let (tx, rx) = mpsc::channel();
    let run = move || {
        let got = get(&url);
        if let Ok(list) = &got
            && let Err(e) = keep(&data, list)
        {
            eprintln!("mvs-launcher: {}: {e}", data.join(FILE).display());
        }
        let _ = tx.send(got);
    };
    std::thread::Builder::new().name("patch notes".into()).spawn(run).ok().map(|_| rx)
}

/// The launcher's notes of `list` (the latest first) for a build of `version`.
pub fn model(list: &[ReleaseNotes], version: &str) -> Vec<PatchNote> {
    let versions: Vec<[u32; 3]> = list.iter().map(|n| parse_version(&n.version).unwrap_or_default()).collect();
    list.iter()
        .zip(ui::pictures(&versions))
        .map(|(n, picture)| PatchNote {
            version: n.version.clone(),
            title: if n.name.trim().is_empty() { n.version.clone() } else { n.name.trim().to_owned() },
            date: ui::day(&n.published_at).map(|(y, m, d)| w::note_day(y, m, d)).unwrap_or_default(),
            age: if newer(&n.version, version) {
                NoteAge::New
            } else if n.version == version {
                NoteAge::Current
            } else {
                NoteAge::Old
            },
            picture,
            blocks: ui::parse(&n.body),
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn release(version: &str, name: &str, at: &str, body: &str) -> ReleaseNotes {
        ReleaseNotes { version: version.into(), name: name.into(), published_at: at.into(), body: body.into() }
    }

    #[test]
    fn the_releases_become_the_launchers_notes() {
        let list = [
            release("0.2.0", "MVS Classic 0.2.0", "2026-03-05T09:30:00Z", "## New\n\n- **A thing.** Its words.\n"),
            release("0.1.9", "", "", ""),
            release("0.1.8", " Eight ", "soon", "Words.\n\n## Download\n\nThe zip.\n"),
        ];
        let m = model(&list, "0.1.9");
        assert_eq!(m.iter().map(|n| n.age).collect::<Vec<_>>(), [NoteAge::New, NoteAge::Current, NoteAge::Old]);
        assert_eq!(m[0].title, "MVS Classic 0.2.0");
        assert_eq!(m[0].date, w::note_day(2026, 3, 5));
        assert_eq!(m[0].blocks.len(), 2);
        assert_eq!((m[1].title.as_str(), m[1].date.as_str(), m[1].blocks.len()), ("0.1.9", "", 0));
        assert_eq!((m[2].title.as_str(), m[2].date.as_str(), m[2].blocks.len()), ("Eight", "", 1));
        assert!(m.windows(2).all(|p| p[0].picture != p[1].picture));
    }

    #[test]
    fn the_list_is_kept_and_read_again() {
        let data = std::env::temp_dir().join(format!("mvs-patch-notes-{}", std::process::id()));
        assert_eq!(kept(&data), vec![]);
        let list = vec![release("0.1.1", "One", "2026-01-01T00:00:00Z", "- a"), release("x", "", "", "")];
        keep(&data, &list).unwrap();
        // (a version that does not read is not shown)
        assert_eq!(kept(&data), list[..1]);
        std::fs::write(data.join(FILE), b"not json").unwrap();
        assert_eq!(kept(&data), vec![]);
        let _ = std::fs::remove_dir_all(&data);
    }
}
