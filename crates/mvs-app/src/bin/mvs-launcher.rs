//! The launcher: setup, the import, Home with its Play (which starts `mvs.exe`), the settings, the updater, in a
//! small window of its own. The program a player starts.
//!
//! mvs-launcher [--data <dir>] [--store <dir>] [--online-defaults <file>] [--adapter <text>] [--play] [<game args>]
//!   `--store`: a store imported already (a developer's), played as it is: Home at once, no import, no rebuild
//!   `--play`: the game at once when it is ready, the launcher closing (else the launcher shows what is needed)
//!   any other argument is passed on to the game each time the launcher starts it (`--no-audio`, `--size 1920x1080`)
//! mvs-launcher --import-worker <op>   (its import process: its request on stdin, its events on stdout)
//! mvs-launcher --finish-update <dir> --wait-pid <pid>   (an unpacked release copying itself over <dir>)
//! mvs-launcher --version   (prints `mvs-launcher <version>` and leaves)
//! mvs-launcher --link <url>   (a join link, `mvsclassic://join/<code>`, and nothing after it: handed to the running
//!   game, or the game started on it: `mvs_app::launcher::link`)

// a desktop program: no console window of its own (mvs_app::console)
#![windows_subsystem = "windows"]

use std::path::PathBuf;
use std::process::ExitCode;

use mvs_app::launcher::game::GameArgs;
use mvs_app::launcher::host::{Options, run};

const USAGE: &str = "mvs-launcher [--data <dir>] [--store <dir>] [--online-defaults <file>] [--adapter <text>] [--play] \
[<game args>]\n\
mvs-launcher --link <url>\n\
mvs-launcher --version";

fn parse(mut it: impl Iterator<Item = String>) -> Result<Options, String> {
    let mut data = None;
    let mut o = Options {
        online_defaults: std::env::current_exe()
            .ok()
            .and_then(|e| e.parent().map(|d| mvs_release::online_json(&mvs_release::app_root(d))))
            .unwrap_or_else(|| PathBuf::from(mvs_release::ONLINE_JSON)),
        game: GameArgs::beside_this_exe(),
        ..Default::default()
    };
    while let Some(arg) = it.next() {
        match arg.as_str() {
            "--data" => data = Some(PathBuf::from(it.next().ok_or("--data: no value")?)),
            "--store" => {
                let dir = PathBuf::from(it.next().ok_or("--store: no value")?);
                // (the game is started in its own folder: the store's path made whole)
                o.store = Some(std::path::absolute(&dir).map_err(|e| format!("--store: {e}"))?);
            }
            "--online-defaults" => {
                let file = PathBuf::from(it.next().ok_or("--online-defaults: no value")?);
                o.online_defaults = std::path::absolute(&file).map_err(|e| format!("--online-defaults: {e}"))?;
            }
            "--adapter" => o.adapter = Some(it.next().ok_or("--adapter: no value")?),
            "--play" => o.play = true,
            _ => o.pass_on.push(arg),
        }
    }
    o.data_given = data.is_some();
    o.data = mvs_settings::data_dir(data.as_deref());
    if o.data_given {
        o.data = std::path::absolute(&o.data).map_err(|e| format!("--data: {e}"))?;
    }
    Ok(o)
}

fn main() -> ExitCode {
    // (DXVK-native's settings, before any thread: the device reads them)
    mvs_d3d::set_linux_wsi();
    mvs_app::console::attach_parent();
    let mut args = std::env::args().skip(1).peekable();
    if args.peek().is_some_and(|a| a == "--import-worker") {
        let op = args.nth(1).unwrap_or_default();
        return ExitCode::from(u8::try_from(mvs_app::import::worker_main(&op)).unwrap_or(1));
    }
    // (the updater's hand-over runs the unpacked release's launcher: mvs_app::update)
    if args.peek().is_some_and(|a| a == "--finish-update") {
        let job: Vec<String> = args.skip(1).collect();
        let pid = match job.get(1).map(String::as_str) {
            Some("--wait-pid") => job.get(2).and_then(|p| p.parse().ok()),
            _ => None,
        };
        let (Some(target), true) = (job.first(), job.len() == 1 || pid.is_some() && job.len() == 3) else {
            eprintln!("mvs-launcher: usage: mvs-launcher --finish-update <dir> [--wait-pid <pid>]");
            return ExitCode::from(2);
        };
        return ExitCode::from(mvs_app::update::finish_update_main(std::path::Path::new(target), pid));
    }
    if args.peek().is_some_and(|a| a == "--version") {
        println!("{}", mvs_release::launcher_version_line());
        return ExitCode::SUCCESS;
    }
    if args.peek().is_some_and(|a| a == "--link") {
        // (exactly the link: a link's text cannot add another option)
        let rest: Vec<String> = args.skip(1).collect();
        let [url] = rest.as_slice() else {
            eprintln!("mvs-launcher: --link: one value and nothing after it\nusage: {USAGE}");
            return ExitCode::from(2);
        };
        return match parse(std::iter::empty()) {
            Ok(o) => ExitCode::from(mvs_app::launcher::link::forward(url, &o)),
            Err(e) => {
                eprintln!("mvs-launcher: {e}\nusage: {USAGE}");
                ExitCode::from(2)
            }
        };
    }
    let o = match parse(args) {
        Ok(o) => o,
        Err(e) => {
            eprintln!("mvs-launcher: {e}\nusage: {USAGE}");
            return ExitCode::from(2);
        }
    };
    match run(&o) {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("mvs-launcher: {e}");
            ExitCode::FAILURE
        }
    }
}
