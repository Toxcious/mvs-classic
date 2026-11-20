//! (the port's own) A release's notes as the launcher shows them: the Markdown of the release's text on GitHub read
//! into headings, list items, paragraphs and quotes with bold runs. A link keeps its words and code its text; a
//! picture, a rule and the release's download section (the page's, of no use inside the launcher) are left out.
//! Each release shows one of a few of the beta's pictures, chosen by its version.

/// A release among the notes, against the build that runs.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum NoteAge {
    /// Later than this build: an update waits.
    New,
    /// This build's.
    Current,
    #[default]
    Old,
}

/// A release's notes.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct PatchNote {
    /// `x.y.z`: the card's words.
    pub version: String,
    /// The release's name (the notes' heading).
    pub title: String,
    /// The day it came out, as shown ('' unknown).
    pub date: String,
    pub age: NoteAge,
    /// Its picture: the note picture's index (`LauncherUi::set_note_picture`), `PICTURES`' own.
    pub picture: usize,
    pub blocks: Vec<Block>,
}

/// The pictures a release's card and notes show, entries of the texture kind of the player's store: the beta's own
/// pictures of its stages (the pre-match screen's) and of its main menu's news, its patch notes' first.
pub const PICTURES: [&str; 12] = [
    "/Game/Panda_Main/UI/MainMenu/PromoTicker/PatchNotes/PatchNotes_Thumbnail.PatchNotes_Thumbnail",
    "/Game/Panda_Main/PreMatch/Maps/MapBG_Batcave.MapBG_Batcave",
    "/Game/Panda_Main/UI/MainMenu/PromoTicker/Preseason/UI_Promo_Preseason_Thumbnail.UI_Promo_Preseason_Thumbnail",
    "/Game/Panda_Main/PreMatch/Maps/MapBG_M011.MapBG_M011",
    "/Game/Panda_Main/UI/MainMenu/PromoTicker/MonsterMash/UI_Promo_MonsterMash_Thumbnail.UI_Promo_MonsterMash_Thumbnail",
    "/Game/Panda_Main/PreMatch/Maps/MapBG_TreeFort.MapBG_TreeFort",
    "/Game/Panda_Main/UI/MainMenu/PromoTicker/ScoobyEvent/Assets/ShaggyCarousel_Thumbnail.ShaggyCarousel_Thumbnail",
    "/Game/Panda_Main/PreMatch/Maps/MapBG_M010.MapBG_M010",
    "/Game/Panda_Main/UI/MainMenu/PromoTicker/C021_S02/UI_Promo_C021_S02_carouselThumbnail.UI_Promo_C021_S02_carouselThumbnail",
    "/Game/Panda_Main/PreMatch/Maps/MapBG_TrophyRoom.MapBG_TrophyRoom",
    "/Game/Panda_Main/PreMatch/Maps/MapBG_M009.MapBG_M009",
    "/Game/Panda_Main/PreMatch/Maps/MapBG_M008.MapBG_M008",
];

/// The picture of each release of `versions` (their `x.y.z`, the newest first), `PICTURES`' index: a release's
/// numbers choose it, so it keeps its picture from one day to the next and each release moves on by one; a release
/// that would show the picture of the one before it takes the next.
pub fn pictures(versions: &[[u32; 3]]) -> Vec<usize> {
    let n = PICTURES.len();
    let mut out = vec![0; versions.len()];
    for (k, [major, minor, patch]) in versions.iter().enumerate().rev() {
        let mut pick = (*major as usize * 97 + *minor as usize * 13 + *patch as usize) % n;
        if out.get(k + 1) == Some(&pick) {
            pick = (pick + 1) % n;
        }
        out[k] = pick;
    }
    out
}

/// Words of one weight.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Run {
    pub bold: bool,
    pub text: String,
}

/// A part of the notes.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Block {
    Heading(String),
    /// A list's item; `depth` 1: of a list inside an item.
    Item {
        depth: u8,
        runs: Vec<Run>,
    },
    Para(Vec<Run>),
    Quote(Vec<Run>),
}

/// The block a line belongs to while its lines are gathered.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Open {
    Item(u8),
    Para,
    Quote,
}

/// The heading of the section left out.
fn is_download(heading: &str) -> bool {
    let h = heading.trim().to_ascii_lowercase();
    h == "download" || h == "downloads"
}

/// `#`s and the heading's text of a heading's line.
fn heading(line: &str) -> Option<(usize, &str)> {
    let level = line.bytes().take_while(|b| *b == b'#').count();
    let rest = &line[level..];
    ((1..=6).contains(&level) && rest.starts_with([' ', '\t']))
        .then(|| (level, rest.trim().trim_end_matches('#').trim()))
}

/// The text of a list item's line: after `- `, `* `, `+ `, or from its number on (`1. `, `1) `).
fn item(line: &str) -> Option<&str> {
    for mark in ["- ", "* ", "+ "] {
        if let Some(rest) = line.strip_prefix(mark) {
            return Some(rest.trim());
        }
    }
    let digits = line.bytes().take_while(u8::is_ascii_digit).count();
    let rest = &line[digits..];
    (digits > 0 && digits <= 3 && (rest.starts_with(". ") || rest.starts_with(") "))).then_some(line)
}

fn rule(line: &str) -> bool {
    let marks: Vec<char> = line.chars().filter(|c| !c.is_whitespace()).collect();
    marks.len() >= 3 && ['-', '*', '_'].iter().any(|m| marks.iter().all(|c| c == m))
}

/// The end of a link's or a picture's `[words](address)` that opens at `k`: its `]` and its `)`.
fn link(s: &[char], k: usize) -> Option<(usize, usize)> {
    let close = k + s[k..].iter().position(|c| *c == ']')?;
    if s.get(close + 1) != Some(&'(') {
        return None;
    }
    // (an address may hold brackets of its own, in pairs)
    let mut depth = 0;
    for (i, c) in s.iter().enumerate().skip(close + 1) {
        match c {
            '(' => depth += 1,
            ')' if depth == 1 => return Some((close, i)),
            ')' => depth -= 1,
            _ => {}
        }
    }
    None
}

/// The runs of a block's text: `**bold**`, the words of `[a link](its address)`, the text of `code`; a picture
/// (`![..](..)`) and an emphasis' single `*` are dropped; `\` keeps the character after it.
pub fn runs(text: &str) -> Vec<Run> {
    let s: Vec<char> = text.chars().collect();
    let mut out: Vec<Run> = Vec::new();
    let mut put = |bold: bool, c: char| match out.last_mut() {
        Some(r) if r.bold == bold => r.text.push(c),
        _ => out.push(Run { bold, text: c.to_string() }),
    };
    let (mut bold, mut code, mut k) = (false, false, 0);
    // (a link's words end at this `]`, and its address is passed over to this `)`)
    let mut link_end: Option<(usize, usize)> = None;
    while k < s.len() {
        let c = s[k];
        if let Some((close, end)) = link_end
            && k == close
        {
            link_end = None;
            k = end + 1;
            continue;
        }
        if c == '`' {
            code = !code;
        } else if code {
            put(bold, c);
        } else if c == '\\' && k + 1 < s.len() {
            k += 1;
            put(bold, s[k]);
        } else if c == '*' && s.get(k + 1) == Some(&'*') {
            bold = !bold;
            k += 1;
        } else if c == '*' {
            // (an emphasis' mark touches a word; one between spaces is a star)
            let touches = |i: Option<&char>| i.is_some_and(|c| !c.is_whitespace());
            if !(touches(k.checked_sub(1).and_then(|i| s.get(i))) || touches(s.get(k + 1))) {
                put(bold, c);
            }
        } else if c == '!'
            && s.get(k + 1) == Some(&'[')
            && let Some((_, end)) = link(&s, k + 1)
        {
            k = end;
        } else if c == '['
            && link_end.is_none()
            && let Some(ends) = link(&s, k)
        {
            link_end = Some(ends);
        } else {
            put(bold, c);
        }
        k += 1;
    }
    // One space between words, none at the ends.
    let mut tidy: Vec<Run> = Vec::new();
    let mut space = true;
    for r in out {
        let mut text = String::new();
        for c in r.text.chars() {
            if c.is_whitespace() {
                if !space {
                    text.push(' ');
                }
                space = true;
            } else {
                text.push(c);
                space = false;
            }
        }
        if !text.is_empty() {
            tidy.push(Run { bold: r.bold, text });
        }
    }
    if let Some(last) = tidy.last_mut() {
        let len = last.text.trim_end().len();
        last.text.truncate(len);
    }
    tidy.retain(|r| !r.text.is_empty());
    tidy
}

fn plain(text: &str) -> String {
    runs(text).into_iter().map(|r| r.text).collect()
}

/// The block gathered, added when it holds words.
fn close(open: &mut Option<(Open, String)>, out: &mut Vec<Block>) {
    let Some((kind, text)) = open.take() else { return };
    let runs = runs(&text);
    if runs.is_empty() {
        return;
    }
    out.push(match kind {
        Open::Item(depth) => Block::Item { depth, runs },
        Open::Para => Block::Para(runs),
        Open::Quote => Block::Quote(runs),
    });
}

/// The notes of a release's text.
pub fn parse(body: &str) -> Vec<Block> {
    let mut out = Vec::new();
    let mut open: Option<(Open, String)> = None;
    // (the level of the download section's heading while its lines are passed over)
    let mut skip: Option<usize> = None;
    let mut fence = false;
    for raw in body.lines() {
        let indent =
            raw.chars().take_while(|c| c.is_whitespace()).map(|c| if c == '\t' { 4 } else { 1 }).sum::<usize>();
        let line = raw.trim();
        if line.starts_with("```") || line.starts_with("~~~") {
            close(&mut open, &mut out);
            fence = !fence;
            continue;
        }
        if let Some((level, text)) = heading(line).filter(|_| !fence) {
            close(&mut open, &mut out);
            if skip.is_some_and(|s| level > s) {
                continue;
            }
            let text = plain(text);
            skip = is_download(&text).then_some(level);
            if skip.is_none() && !text.is_empty() {
                out.push(Block::Heading(text));
            }
            continue;
        }
        if skip.is_some() {
            continue;
        }
        if fence {
            // (a line of code is a paragraph of its own, its marks kept)
            if !line.is_empty() {
                out.push(Block::Para(vec![Run { bold: false, text: line.to_owned() }]));
            }
            continue;
        }
        if line.is_empty() || rule(line) || (line.starts_with('<') && line.ends_with('>')) {
            close(&mut open, &mut out);
            continue;
        }
        if let Some(rest) = line.strip_prefix('>') {
            let rest = rest.trim();
            // (GitHub's alert mark, `[!NOTE]`: the quote's own line, not its words)
            let rest = if rest.starts_with("[!") && rest.ends_with(']') { "" } else { rest };
            match &mut open {
                Some((Open::Quote, text)) => {
                    text.push(' ');
                    text.push_str(rest);
                }
                _ => {
                    close(&mut open, &mut out);
                    open = Some((Open::Quote, rest.to_owned()));
                }
            }
            continue;
        }
        if let Some(text) = item(line) {
            close(&mut open, &mut out);
            open = Some((Open::Item(u8::from(indent >= 2)), text.to_owned()));
            continue;
        }
        match &mut open {
            // (a line under a block goes on with it)
            Some((_, text)) => {
                text.push(' ');
                text.push_str(line);
            }
            None => open = Some((Open::Para, line.to_owned())),
        }
    }
    close(&mut open, &mut out);
    out
}

/// The words of `runs` for wrapping: each a word's pieces (bold, text), drawn with no space between them (a bold
/// word and the full stop after it), a space between words.
pub fn words(runs: &[Run]) -> Vec<Vec<(bool, &str)>> {
    let mut out: Vec<Vec<(bool, &str)>> = Vec::new();
    // (the next piece goes on with the last word: no space lay between them)
    let mut joined = false;
    for r in runs {
        for (k, part) in r.text.split(' ').enumerate() {
            joined &= k == 0;
            if part.is_empty() {
                continue;
            }
            match out.last_mut() {
                Some(word) if joined => word.push((r.bold, part)),
                _ => out.push(vec![(r.bold, part)]),
            }
            joined = true;
        }
    }
    out
}

/// Year, month (1..12) and day of an ISO 8601 time (`2026-01-31T12:00:00Z`).
pub fn day(iso: &str) -> Option<(u32, u32, u32)> {
    let mut it = iso.get(..10)?.split('-');
    let (y, m, d) = (it.next()?.parse().ok()?, it.next()?.parse().ok()?, it.next()?.parse().ok()?);
    ((1..=12).contains(&m) && (1..=31).contains(&d) && it.next().is_none()).then_some((y, m, d))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn run(bold: bool, text: &str) -> Run {
        Run { bold, text: text.into() }
    }

    #[test]
    fn a_release_text_is_read() {
        let body = "First words of the release,\nwrapped onto a second line.\r\n\r\n\
## New\n\n\
- **A bold lead.** The rest of the item with `code` in it.\n\
- A plain item that goes on\n  on its next line.\n  - An item inside it\n\
1. A numbered one\n\n\
---\n\n\
## Download\n\nDownload `x.zip` and run it.\n\n### More\n\nStill the download section.\n\n\
## After\n\nKept.\n";
        assert_eq!(
            parse(body),
            vec![
                Block::Para(vec![run(false, "First words of the release, wrapped onto a second line.")]),
                Block::Heading("New".into()),
                Block::Item {
                    depth: 0,
                    runs: vec![run(true, "A bold lead."), run(false, " The rest of the item with code in it.")],
                },
                Block::Item { depth: 0, runs: vec![run(false, "A plain item that goes on on its next line.")] },
                Block::Item { depth: 1, runs: vec![run(false, "An item inside it")] },
                Block::Item { depth: 0, runs: vec![run(false, "1. A numbered one")] },
                Block::Heading("After".into()),
                Block::Para(vec![run(false, "Kept.")]),
            ]
        );
    }

    #[test]
    fn a_quote_keeps_its_words_and_a_link_its_text() {
        let body =
            "> [!IMPORTANT]\n> Read the [install guide](https://example.org/a_(b)) first,\n> then start.\n\nAfter.";
        assert_eq!(
            parse(body),
            vec![
                Block::Quote(vec![run(false, "Read the install guide first, then start.")]),
                Block::Para(vec![run(false, "After.")]),
            ]
        );
    }

    #[test]
    fn marks_are_read_in_a_line() {
        assert_eq!(runs("a **b** c"), vec![run(false, "a "), run(true, "b"), run(false, " c")]);
        assert_eq!(runs("press **PLAY**"), vec![run(false, "press "), run(true, "PLAY")]);
        assert_eq!(runs("an *emphasis* and 2 * 3"), vec![run(false, "an emphasis and 2 * 3")]);
        assert_eq!(runs("`a*b` \\*c ![shot](x.png)end"), vec![run(false, "a*b *c end")]);
        assert_eq!(runs("[not a link] here"), vec![run(false, "[not a link] here")]);
        assert_eq!(runs("  **  "), vec![]);
    }

    #[test]
    fn a_word_keeps_the_mark_after_its_bold_part() {
        let r = runs("press **PLAY**, then **wait a bit** more");
        assert_eq!(
            words(&r),
            vec![
                vec![(false, "press")],
                vec![(true, "PLAY"), (false, ",")],
                vec![(false, "then")],
                vec![(true, "wait")],
                vec![(true, "a")],
                vec![(true, "bit")],
                vec![(false, "more")],
            ]
        );
    }

    #[test]
    fn a_release_keeps_its_picture_and_differs_from_the_one_before() {
        let n = PICTURES.len() as u32;
        let all = pictures(&[[0, 1, 5], [0, 1, 4], [0, 1, 3]]);
        assert_eq!(all[1..], pictures(&[[0, 1, 4], [0, 1, 3]]));
        assert!(all.iter().all(|p| *p < PICTURES.len()));
        assert!(all.windows(2).all(|w| w[0] != w[1]));
        // (two releases whose numbers choose the same picture: the later takes the next)
        assert_eq!(pictures(&[[0, 0, n], [0, 0, 0]]), vec![1, 0]);
    }

    #[test]
    fn a_day_is_read_from_a_time() {
        assert_eq!(day("2026-01-31T12:00:00Z"), Some((2026, 1, 31)));
        assert_eq!(day("2026-13-01T00:00:00Z"), None);
        assert_eq!(day("soon"), None);
    }
}
