use std::io::Read;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver, Sender};
use std::time::{Duration, Instant, SystemTime};

use nucleo_matcher::pattern::{CaseMatching, Normalization, Pattern};
use nucleo_matcher::{Config, Matcher, Utf32Str};

use super::Event;
use crate::config::{ListPreview, Position};
use crate::term::{self, Click, Key};
use crate::theme::{Palette, bg, fg};

pub const CAP: usize = 10_000;
const HEAD_BYTES: u64 = 4096;
const BATCH: Duration = Duration::from_millis(30);
const HEADER_ROWS: usize = 3;
/// A smaller preview is hidden.
const MIN_PREVIEW_COLS: u16 = 30;
const MIN_PREVIEW_ROWS: u16 = 8;
/// The list keeps this much, whatever `size` asks.
const MIN_LIST_COLS: u16 = 40;
const MIN_LIST_ROWS: u16 = 10;

/// A block of cells, from 0.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Area {
    pub col: u16,
    pub row: u16,
    pub cols: u16,
    pub rows: u16,
}

/// How the rows between the header and the hint row are shared. Cells count from 0.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Split {
    pub list_col: u16,
    pub list_row: u16,
    pub list_cols: usize,
    pub list_rows: usize,
    pub preview: Option<Area>,
}

impl Split {
    /// The list row under the mouse at `col`, `row`, which count from 1.
    fn row_at(&self, col: u16, row: u16) -> Option<usize> {
        let c = usize::from(col.checked_sub(1)?);
        let in_cols = (usize::from(self.list_col)..usize::from(self.list_col) + self.list_cols).contains(&c);
        row.checked_sub(self.list_row + 1).map(usize::from).filter(|&r| r < self.list_rows && in_cols)
    }

    /// The line between the list and the preview: a column (true) or a row (false), and its index.
    fn border(&self) -> Option<(bool, u16)> {
        let a = self.preview?;
        Some(if a.col > self.list_col {
            (true, a.col - 1)
        } else if self.list_col > a.col {
            (true, a.col + a.cols)
        } else if a.row > self.list_row {
            (false, a.row - 1)
        } else {
            (false, a.row + a.rows)
        })
    }
}

/// `hidden` is Ctrl-t's.
pub fn split(pv: &ListPreview, hidden: bool, cols: u16, rows: u16) -> Split {
    let top = HEADER_ROWS as u16;
    let body = rows.saturating_sub(top + 1).max(1);
    let whole = Split { list_col: 0, list_row: top, list_cols: usize::from(cols), list_rows: usize::from(body), preview: None };
    let (side, size) = if cols >= pv.narrow_columns { (Some(pv.position), pv.size) } else { (pv.narrow_position.side(), pv.narrow_size.unwrap_or(pv.size)) };
    let Some(side) = side.filter(|_| !hidden) else { return whole };
    let picked = match side {
        Position::Right | Position::Left => {
            let room = size.of(cols).min(cols.saturating_sub(MIN_LIST_COLS + 1));
            let list = cols.saturating_sub(room + 1);
            (room >= MIN_PREVIEW_COLS && body >= MIN_PREVIEW_ROWS).then(|| {
                let (list_col, col) = if side == Position::Right { (0, list + 1) } else { (room + 1, 0) };
                Split { list_col, list_row: top, list_cols: usize::from(list), list_rows: usize::from(body), preview: Some(Area { col, row: top, cols: room, rows: body }) }
            })
        }
        Position::Top | Position::Bottom => {
            let room = size.of(body).min(body.saturating_sub(MIN_LIST_ROWS + 1));
            let list = body.saturating_sub(room + 1);
            (room >= MIN_PREVIEW_ROWS && cols >= MIN_PREVIEW_COLS).then(|| {
                let (list_row, row) = if side == Position::Bottom { (top, top + list + 1) } else { (top + room + 1, top) };
                Split { list_col: 0, list_row, list_cols: usize::from(cols), list_rows: usize::from(list), preview: Some(Area { col: 0, row, cols, rows: room }) }
            })
        }
    };
    picked.unwrap_or(whole)
}

pub struct Item {
    pub path: PathBuf,
    /// The path relative to the root, without control characters.
    pub rel: String,
    pub title: String,
    pub mtime: SystemTime,
}

#[derive(Debug, PartialEq, Eq)]
pub enum Pick {
    Nothing,
    Redraw,
    Open(PathBuf),
    Close,
    Suspend,
    Quit,
}

struct Row {
    item: usize,
    hits: Vec<u32>,
}

pub struct Picker {
    pub root: PathBuf,
    /// A one-off message shown in the header, such as a file that failed to open.
    pub note: Option<String>,
    items: Vec<Item>,
    rows: Vec<Row>,
    query: String,
    sel: usize,
    top: usize,
    done: bool,
    rx: Option<Receiver<(Vec<Item>, bool)>>,
    stop: Arc<AtomicBool>,
    matcher: Matcher,
    /// A press landed in the list, so the next release is a click. A release alone ends a drag begun on the page.
    pressed: bool,
    pv: ListPreview,
    hidden: bool,
}

/// The folder the list starts from: the git repo that holds `start`, else `cwd`.
pub fn scope(start: &Path, cwd: &Path) -> PathBuf {
    start.ancestors().find(|d| d.join(".git").exists()).map_or_else(|| cwd.to_path_buf(), Path::to_path_buf)
}

pub fn listed(p: &Path) -> bool {
    crate::config::get().file_list.shows(p)
}

pub fn readmes(folder: &Path) -> Vec<PathBuf> {
    let Ok(entries) = std::fs::read_dir(folder) else { return Vec::new() };
    let mut found: Vec<PathBuf> =
        entries.filter_map(|e| e.ok().map(|e| e.path())).filter(|p| p.is_file() && p.file_stem().and_then(|s| s.to_str()).is_some_and(|s| s.eq_ignore_ascii_case("readme")) && listed(p)).collect();
    found.sort_by_key(|p| (!p.extension().is_some_and(|e| e.eq_ignore_ascii_case("md")), p.clone()));
    found
}

/// The text of the first ATX heading, outside front matter and code fences.
pub fn title(text: &str) -> String {
    let mut lines = text.lines().peekable();
    if lines.peek().is_some_and(|l| l.trim_end() == "---") {
        lines.next();
        for l in lines.by_ref() {
            if matches!(l.trim_end(), "---" | "...") {
                break;
            }
        }
    }
    let run = |t: &str, c: u8| t.bytes().take_while(|&b| b == c).count();
    let mut fence: Option<(u8, usize)> = None;
    for l in lines {
        let t = l.trim_start();
        if let Some((c, n)) = fence {
            if run(t, c) >= n && t.trim_start_matches(c as char).trim().is_empty() {
                fence = None;
            }
            continue;
        }
        if let Some(c) = b"`~".iter().copied().find(|&c| run(t, c) >= 3) {
            fence = Some((c, run(t, c)));
            continue;
        }
        if l.len() - t.len() > 3 {
            continue;
        }
        let hashes = t.bytes().take_while(|&b| b == b'#').count();
        if (1..=6).contains(&hashes) && t[hashes..].starts_with([' ', '\t']) {
            let h = t[hashes..].trim();
            // A closing run of # counts only after a space: `# C#` keeps its #.
            let bare = h.trim_end_matches('#');
            return clean(if bare.is_empty() || bare.ends_with([' ', '\t']) { bare.trim_end() } else { h });
        }
    }
    String::new()
}

fn clean(s: &str) -> String {
    s.chars().filter(|c| !c.is_control()).collect()
}

fn read_item(root: &Path, path: PathBuf) -> Option<Item> {
    let meta = std::fs::metadata(&path).ok()?;
    let mut head = Vec::new();
    std::fs::File::open(&path).ok()?.take(HEAD_BYTES).read_to_end(&mut head).ok()?;
    let rel = clean(&path.strip_prefix(root).unwrap_or(&path).to_string_lossy());
    Some(Item { title: title(&String::from_utf8_lossy(&head)), rel, mtime: meta.modified().unwrap_or(SystemTime::UNIX_EPOCH), path })
}

/// Lists the Markdown files under `root` on a thread, skipping hidden and gitignored ones, and
/// sends them in batches. Each batch also wakes the event loop. It stops once `stop` is set.
fn walk(root: PathBuf, tx: Sender<(Vec<Item>, bool)>, wake: Sender<Event>, stop: Arc<AtomicBool>) {
    std::thread::spawn(move || {
        let mut batch = Vec::new();
        let mut sent = Instant::now();
        let mut count = 0;
        let walker = ignore::WalkBuilder::new(&root).require_git(false).build();
        for e in walker.flatten() {
            if count >= CAP || stop.load(Ordering::Relaxed) {
                break;
            }
            if !e.file_type().is_some_and(|t| t.is_file()) || !listed(e.path()) {
                continue;
            }
            let Some(item) = read_item(&root, e.into_path()) else { continue };
            batch.push(item);
            count += 1;
            if sent.elapsed() >= BATCH {
                if tx.send((std::mem::take(&mut batch), false)).is_err() || wake.send(Event::Listed).is_err() {
                    return;
                }
                sent = Instant::now();
            }
        }
        let _ = tx.send((batch, true));
        let _ = wake.send(Event::Listed);
    });
}

fn age(d: Duration) -> String {
    let s = d.as_secs();
    match s {
        0..60 => "now".into(),
        60..3600 => format!("{}m", s / 60),
        3600..86_400 => format!("{}h", s / 3600),
        86_400..1_209_600 => format!("{}d", s / 86_400),
        1_209_600..5_184_000 => format!("{}w", s / 604_800),
        5_184_000..31_536_000 => format!("{}mo", s / 2_592_000),
        _ => format!("{}y", s / 31_536_000),
    }
}

/// `text` cut to `max` cells, with the chars at `hits` (sorted char indices) in `hit`, the rest in `base`.
fn styled(text: &str, hits: &[u32], max: usize, base: &str, hit: &str) -> (String, usize) {
    let mut out = String::from(base);
    let mut w = 0;
    let total = term::width(text);
    let room = if total > max { max.saturating_sub(1) } else { max };
    let mut on = false;
    for (i, c) in text.chars().enumerate() {
        let cw = term::width(c.encode_utf8(&mut [0; 4]));
        if w + cw > room {
            break;
        }
        let want = hits.binary_search(&(i as u32)).is_ok();
        if want != on {
            out.push_str(if want { hit } else { base });
            on = want;
        }
        out.push(c);
        w += cw;
    }
    if total > max && max > 0 {
        out.push_str(base);
        out.push('…');
        w += 1;
    }
    (out, w)
}

/// `styled`, but cut at the start, so a long path keeps its file name.
fn styled_tail(text: &str, hits: &[u32], max: usize, base: &str, hit: &str) -> (String, usize) {
    if term::width(text) <= max || max < 2 {
        return styled(text, hits, max, base, hit);
    }
    let chars: Vec<char> = text.chars().collect();
    let mut w = 0;
    let mut k = chars.len();
    while k > 0 {
        let cw = term::width(chars[k - 1].encode_utf8(&mut [0; 4]));
        if w + cw > max - 1 {
            break;
        }
        w += cw;
        k -= 1;
    }
    let rest: String = chars[k..].iter().collect();
    let shifted: Vec<u32> = hits.iter().filter_map(|&h| h.checked_sub(k as u32)).collect();
    let (s, sw) = styled(&rest, &shifted, max - 1, base, hit);
    (format!("{base}…{s}"), sw + 1)
}

/// What the preview shows, for its label. Names are paths in the list.
pub enum Shows<'a> {
    Nothing,
    File(&'a str),
    Stale(&'a str),
    Loading,
    Failed(&'a str),
}

impl<'a> Shows<'a> {
    pub fn of(selected: Option<&'a str>, shown: Option<&'a str>, failed: bool) -> Shows<'a> {
        match (shown, selected) {
            (Some(s), Some(c)) if s == c => Shows::File(s),
            (Some(s), _) => Shows::Stale(s),
            (None, Some(c)) if failed => Shows::Failed(c),
            (None, Some(_)) => Shows::Loading,
            (None, None) => Shows::Nothing,
        }
    }
}

/// Names the file the preview shows, so a preview kept from the last file is never taken for the selected one.
/// Empty when it doesn't fit in `max` cells.
pub fn label(shows: Shows, max: usize) -> String {
    let (name, suffix) = match shows {
        Shows::Nothing => return String::new(),
        Shows::File(n) => (n, ""),
        Shows::Stale(n) => (n, " · loading…"),
        Shows::Loading => ("", "loading…"),
        Shows::Failed(n) => (n, " · can't show"),
    };
    let room = max.saturating_sub(2 + term::width(suffix));
    if (name.is_empty() && max < 2 + term::width(suffix)) || (!name.is_empty() && room < 4) {
        return String::new();
    }
    format!(" {}{suffix} ", tail(name, room).0)
}

/// The end of `s` in `max` cells, after a `…` when cut, so a long path keeps its folder name.
fn tail(s: &str, max: usize) -> (String, usize) {
    if term::width(s) <= max {
        return (s.to_string(), term::width(s));
    }
    let mut w = 1;
    let mut kept: Vec<char> = Vec::new();
    for c in s.chars().rev() {
        let cw = term::width(c.encode_utf8(&mut [0; 4]));
        if w + cw > max {
            break;
        }
        kept.push(c);
        w += cw;
    }
    if max == 0 {
        return (String::new(), 0);
    }
    (std::iter::once('…').chain(kept.into_iter().rev()).collect(), w)
}

fn home_short(p: &Path) -> String {
    let s = p.display().to_string();
    match std::env::var("HOME") {
        Ok(h) if !h.is_empty() && (s == h || s.starts_with(&format!("{h}/"))) => format!("~{}", &s[h.len()..]),
        _ => s,
    }
}

impl Picker {
    pub fn new(root: PathBuf) -> Picker {
        Picker {
            root,
            note: None,
            items: Vec::new(),
            rows: Vec::new(),
            query: String::new(),
            sel: 0,
            top: 0,
            done: false,
            rx: None,
            stop: Arc::default(),
            matcher: Matcher::new(Config::DEFAULT.match_paths()),
            pressed: false,
            pv: crate::config::get().file_list.preview,
            hidden: !crate::config::get().file_list.preview.show_by_default,
        }
    }

    pub fn split(&self, cols: u16, rows: u16) -> Split {
        split(&self.pv, self.hidden, cols, rows)
    }

    /// The selected file first, then its neighbors.
    pub fn around(&self) -> Vec<(&Path, SystemTime)> {
        let at = |i: usize| self.rows.get(i).map(|r| (self.items[r.item].path.as_path(), self.items[r.item].mtime));
        [Some(self.sel), self.sel.checked_add(1), self.sel.checked_sub(1)].into_iter().flatten().filter_map(at).collect()
    }

    /// Moves the list to the folder `start` belongs in, unless `start` is already under the root.
    /// A new root clears the filter. Returns whether the root changed.
    pub fn follow(&mut self, start: &Path, cwd: &Path) -> bool {
        if start.starts_with(&self.root) {
            return false;
        }
        let root = scope(start, cwd);
        if root == self.root {
            return false;
        }
        self.root = root;
        self.query.clear();
        true
    }

    /// Stops the walk, if one is running. The next `list` starts over.
    pub fn halt(&self) {
        self.stop.store(true, Ordering::Relaxed);
    }

    /// Starts listing the files again, keeping the filter.
    pub fn list(&mut self, wake: Sender<Event>) {
        let (tx, rx) = mpsc::channel();
        self.rx = Some(rx);
        self.items.clear();
        self.rows.clear();
        (self.sel, self.top) = (0, 0);
        self.done = false;
        self.pressed = false;
        self.stop.store(true, Ordering::Relaxed);
        self.stop = Arc::default();
        walk(self.root.clone(), tx, wake, self.stop.clone());
    }

    /// Waits up to `limit` for the walker's first batch, so a small folder shows whole at once.
    pub fn first_batch(&mut self, limit: Duration) {
        if let Some(Ok((items, done))) = self.rx.as_ref().map(|rx| rx.recv_timeout(limit)) {
            self.items.extend(items);
            self.done |= done;
        }
        self.receive();
        self.resort();
    }

    /// Takes the batches the walker sent. Returns whether the list changed.
    pub fn receive(&mut self) -> bool {
        let Some(rx) = &self.rx else { return false };
        let mut got = false;
        while let Ok((items, done)) = rx.try_recv() {
            self.items.extend(items);
            self.done |= done;
            got = true;
        }
        if got {
            self.resort();
        }
        got
    }

    /// Sorts the items, newest first, and filters them again, keeping the selected file selected.
    fn resort(&mut self) {
        let keep = self.selected().map(Path::to_path_buf);
        self.items.sort_by(|a, b| b.mtime.cmp(&a.mtime).then_with(|| a.rel.cmp(&b.rel)));
        self.filter();
        if let Some(i) = keep.and_then(|k| self.rows.iter().position(|r| self.items[r.item].path == k)) {
            self.sel = i;
        }
    }

    fn filter(&mut self) {
        let pattern = Pattern::parse(&self.query, CaseMatching::Smart, Normalization::Smart);
        let (mut buf, mut hay) = (Vec::new(), String::new());
        let mut scored: Vec<(u32, Row)> = Vec::new();
        for (i, it) in self.items.iter().enumerate() {
            if self.query.is_empty() {
                scored.push((0, Row { item: i, hits: vec![] }));
                continue;
            }
            hay.clear();
            hay.push_str(&it.rel);
            hay.push(' ');
            hay.push_str(&it.title);
            // Utf32Str::new counts graphemes, or bytes, in a name with combining marks (common in
            // macOS file names), but the hits are drawn per char, so it gets the chars.
            buf.clear();
            buf.extend(hay.chars());
            let text = if hay.is_ascii() { Utf32Str::Ascii(hay.as_bytes()) } else { Utf32Str::Unicode(&buf) };
            let mut hits = Vec::new();
            if let Some(score) = pattern.indices(text, &mut self.matcher, &mut hits) {
                hits.sort_unstable();
                hits.dedup();
                scored.push((score, Row { item: i, hits }));
            }
        }
        scored.sort_by_key(|a| std::cmp::Reverse(a.0));
        self.rows = scored.into_iter().map(|(_, r)| r).collect();
        self.sel = self.sel.min(self.rows.len().saturating_sub(1));
    }

    pub fn selected(&self) -> Option<&Path> {
        self.rows.get(self.sel).map(|r| self.items[r.item].path.as_path())
    }

    fn move_by(&mut self, d: isize) -> Pick {
        let last = self.rows.len().saturating_sub(1);
        let to = self.sel.saturating_add_signed(d).min(last);
        if to == self.sel {
            return Pick::Nothing;
        }
        self.sel = to;
        Pick::Redraw
    }

    fn edit(&mut self, f: impl FnOnce(&mut String)) -> Pick {
        let before = self.query.clone();
        f(&mut self.query);
        if self.query == before {
            return Pick::Nothing;
        }
        self.sel = 0;
        self.top = 0;
        self.filter();
        Pick::Redraw
    }

    pub fn handle(&mut self, key: Key, cols: u16, rows: u16) -> Pick {
        let noted = self.note.take().is_some();
        match self.key(key, self.split(cols, rows)) {
            Pick::Nothing if noted => Pick::Redraw,
            p => p,
        }
    }

    fn key(&mut self, key: Key, split: Split) -> Pick {
        let page = split.list_rows as isize;
        match key {
            Key::Enter => self.selected().map_or(Pick::Nothing, |p| Pick::Open(p.to_path_buf())),
            Key::Esc => Pick::Close,
            Key::Ctrl('c') => Pick::Quit,
            Key::Ctrl('z') => Pick::Suspend,
            Key::Ctrl('t') => {
                self.hidden = !self.hidden;
                Pick::Redraw
            }
            Key::Up | Key::Ctrl('p') | Key::WheelUp(..) => self.move_by(-1),
            Key::Down | Key::Ctrl('n') | Key::WheelDown(..) => self.move_by(1),
            Key::PageUp => self.move_by(-page),
            Key::PageDown => self.move_by(page),
            Key::Home => self.move_by(isize::MIN / 2),
            Key::End => self.move_by(isize::MAX / 2),
            Key::Backspace => self.edit(|q| {
                q.pop();
            }),
            Key::Ctrl('u') => self.edit(String::clear),
            Key::Ctrl('w') => self.edit(|q| {
                let keep = q.trim_end().char_indices().rfind(|(_, c)| c.is_whitespace()).map_or(0, |(i, c)| i + c.len_utf8());
                q.truncate(keep);
            }),
            Key::Char(c) if !c.is_control() => self.edit(|q| q.push(c)),
            Key::Mouse(Click::Press, col, row) => {
                self.pressed = split.row_at(col, row).is_some();
                Pick::Nothing
            }
            Key::Mouse(Click::Release, _, _) if !std::mem::take(&mut self.pressed) => Pick::Nothing,
            Key::Mouse(Click::Release, col, row) => match split.row_at(col, row).map(|r| r + self.top).filter(|&i| i < self.rows.len()) {
                Some(i) => {
                    self.sel = i;
                    Pick::Open(self.items[self.rows[i].item].path.clone())
                }
                None => Pick::Nothing,
            },
            _ => Pick::Nothing,
        }
    }

    /// Draws the list over the whole screen, then puts the cursor at the end of the filter.
    pub fn draw(&mut self, out: &mut String, cols: u16, rows: u16, now: SystemTime, pal: &Palette) {
        let split = self.split(cols, rows);
        let cols = usize::from(cols);
        let shown = split.list_rows;
        let list_w = split.list_cols;
        if self.sel < self.top {
            self.top = self.sel;
        } else if self.sel >= self.top + shown {
            self.top = self.sel + 1 - shown;
        }
        let (dim, text, bright) = (fg(pal.fg_muted), fg(pal.fg), format!("\x1b[1m{}", fg(pal.fg_bright)));
        let reset = pal.reset();
        // Not ESC[2J: Rex deletes unplaced images on a full clear, and the kept previews are unplaced.
        out.push_str(&reset);
        for r in 1..=rows {
            out.push_str(&format!("\x1b[{r};1H\x1b[2K"));
        }
        let count = if let Some(n) = &self.note {
            term::printable(n)
        } else if !self.done {
            format!("{} files, listing…", self.items.len())
        } else if self.items.len() >= CAP {
            format!("first {CAP} files")
        } else if self.query.is_empty() {
            format!("{} files", self.items.len())
        } else {
            format!("{} of {} files", self.rows.len(), self.items.len())
        };
        let (where_, ww) = tail(&term::printable(&home_short(&self.root)), cols.saturating_sub(term::width(&count) + 4));
        let pad = cols.saturating_sub(ww + term::width(&count) + 2);
        out.push_str(&format!("\x1b[1;1H {bright}{where_}{reset}{}{dim}{count}{reset}", " ".repeat(pad)));
        let (q, qw) = term::fit(&term::printable(&self.query), cols.saturating_sub(4));
        out.push_str(&format!("\x1b[2;1H {bright}>{reset} {text}{q}{reset}"));
        out.push_str(&format!("\x1b[3;1H {}{}{reset}", fg(pal.border), "─".repeat(cols.saturating_sub(2))));
        if self.done && self.rows.is_empty() {
            let msg = if self.items.is_empty() { "No Markdown files here" } else { "No files match" };
            out.push_str(&format!("\x1b[{};{}H{dim}{msg}{reset}", split.list_row + 1, split.list_col + 3));
        }
        self.top = self.top.min(self.rows.len());
        let visible = &self.rows[self.top..self.rows.len().min(self.top + shown)];
        let path_w = visible.iter().map(|r| term::width(&self.items[r.item].rel)).max().unwrap_or(0).min(list_w * 3 / 5);
        for (n, r) in visible.iter().enumerate() {
            let it = &self.items[r.item];
            let active = self.top + n == self.sel;
            let bg = if active { bg(pal.selection_bg) } else { String::new() };
            let marker = if active { "▌" } else { " " };
            let ago = age(now.duration_since(it.mtime).unwrap_or_default());
            let seam = it.rel.chars().count() as u32 + 1;
            let (path_hits, title_hits): (Vec<u32>, Vec<u32>) = r.hits.iter().partition(|&&h| h < seam);
            let title_hits: Vec<u32> = title_hits.iter().map(|h| h - seam).collect();
            let base = format!("{bg}{text}");
            let hit = format!("{bg}{bright}");
            let (p, pw) = styled_tail(&it.rel, &path_hits, path_w.min(list_w.saturating_sub(4)), &format!("{reset}{base}"), &hit);
            let mut line = format!("{bg}{bright}{marker}{p}{}", " ".repeat(path_w.saturating_sub(pw)));
            let mut w = 2 + path_w;
            let title_room = list_w.saturating_sub(w + 2 + ago.len() + 2);
            if title_room >= 8 && !it.title.is_empty() {
                let (t, tw) = styled(&it.title, &title_hits, title_room, &format!("{reset}{bg}{dim}"), &hit);
                line.push_str(&format!("{bg}  {t}"));
                w += 2 + tw;
            }
            let gap = list_w.saturating_sub(w + ago.len() + 1);
            line.push_str(&format!("{reset}{bg}{}{dim}{ago} {reset}", " ".repeat(gap)));
            out.push_str(&format!("\x1b[{};{}H {line}", usize::from(split.list_row) + 1 + n, split.list_col + 1));
        }
        if let (Some(a), Some((vertical, at))) = (split.preview, split.border()) {
            let line = fg(pal.border);
            if vertical {
                for r in a.row..a.row + a.rows {
                    out.push_str(&format!("\x1b[{};{}H{line}│{reset}", r + 1, at + 1));
                }
            } else {
                out.push_str(&format!("\x1b[{};2H{line}{}{reset}", at + 1, "─".repeat(cols.saturating_sub(2))));
            }
        }
        let hints = [("↑↓", "move"), ("Enter", "open"), ("Esc", "back"), ("Ctrl-u", "clear"), ("Ctrl-t", "preview")];
        let mut bottom = String::new();
        let mut bw = 1;
        for (k, l) in hints {
            let w = term::width(k) + term::width(l) + 3;
            if bw + w > cols {
                break;
            }
            bottom.push_str(&format!("{}{k}{dim} {l}  ", fg(pal.fg_bright)));
            bw += w;
        }
        out.push_str(&format!("\x1b[{rows};1H{} {bottom}{}{reset}", bg(pal.bar_bg), " ".repeat(cols.saturating_sub(bw))));
        out.push_str(&format!("\x1b[2;{}H\x1b[?25h", 4 + qw));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{NarrowPosition, Size};

    #[test]
    fn listed_by_default_is_md_or_markdown_in_any_case() {
        for f in ["a.md", "a.MD", "a.markdown", "dir.d/a.Markdown"] {
            assert!(listed(Path::new(f)), "{f}");
        }
        for f in ["a.mdx", "a.mdown", "a.mkd", "a.txt", "md", "a.md.bak"] {
            assert!(!listed(Path::new(f)), "{f}");
        }
    }

    fn dark() -> Palette {
        crate::theme::builtin("github-dark").unwrap().palette
    }

    #[test]
    fn a_light_theme_paints_the_list_light() {
        let light = crate::theme::builtin("github-light").unwrap().palette;
        let mut p = picker(vec![item("a.md", "A", 60), item("b.md", "B", 120)]);
        p.done = true;
        let mut out = String::new();
        p.draw(&mut out, 60, 10, SystemTime::UNIX_EPOCH + Duration::from_secs(1_000_000), &light);
        let drawn = crate::viewer::sgr_colors(&out);
        for c in [light.bg, light.fg, light.fg_muted, light.fg_bright, light.selection_bg, light.bar_bg, light.border] {
            assert!(drawn.contains(&c), "{c:?} not drawn");
        }
        assert!(drawn.iter().all(|c| [light.bg, light.fg, light.fg_muted, light.fg_bright, light.selection_bg, light.bar_bg, light.border].contains(c)), "{drawn:?}");
        assert!(out.starts_with(&format!("\x1b[0m{}\x1b[1;1H\x1b[2K", crate::theme::bg(light.bg))));
        assert!(!out.contains("\x1b[2J"), "a full clear drops the previews in Rex");
    }

    fn item(rel: &str, title: &str, age_s: u64) -> Item {
        Item { path: PathBuf::from("/r").join(rel), rel: rel.into(), title: title.into(), mtime: SystemTime::UNIX_EPOCH + Duration::from_secs(1_000_000 - age_s) }
    }

    fn picker(items: Vec<Item>) -> Picker {
        let mut p = Picker::new(PathBuf::from("/r"));
        p.done = true;
        p.items.extend(items);
        p.resort();
        p
    }

    fn names(p: &Picker) -> Vec<&str> {
        p.rows.iter().map(|r| p.items[r.item].rel.as_str()).collect()
    }

    fn typed(p: &mut Picker, s: &str) {
        for c in s.chars() {
            p.handle(Key::Char(c), 80, 24);
        }
    }

    #[test]
    fn hits_count_chars_in_decomposed_names() {
        let mut p = picker(vec![item("cafe\u{301}.md", "", 1), item("re\u{301}sume\u{301}/x.md", "", 2)]);
        typed(&mut p, "md");
        let hits: Vec<Vec<u32>> = p.rows.iter().map(|r| r.hits.clone()).collect();
        assert_eq!(hits, [vec![6, 7], vec![11, 12]]);
    }

    #[test]
    fn p01_newest_first_before_typing() {
        let p = picker(vec![item("old.md", "", 500), item("new.md", "", 10), item("mid.md", "", 100)]);
        assert_eq!(names(&p), ["new.md", "mid.md", "old.md"]);
    }

    #[test]
    fn p02_typing_filters_by_path_and_title() {
        let mut p = picker(vec![item("docs/behavior.md", "Behavior spec", 1), item("README.md", "mddog", 2), item("notes/x.md", "Readme notes", 3)]);
        typed(&mut p, "readme");
        assert_eq!(names(&p), ["README.md", "notes/x.md"]);
        assert_eq!(p.rows[0].hits, [0, 1, 2, 3, 4, 5]);
        p.handle(Key::Ctrl('u'), 80, 24);
        assert_eq!(p.rows.len(), 3);
        typed(&mut p, "zzz");
        assert!(p.rows.is_empty());
        assert_eq!(p.handle(Key::Enter, 80, 24), Pick::Nothing);
    }

    #[test]
    fn p03_arrows_and_ctrl_n_p_move() {
        let mut p = picker(vec![item("a.md", "", 1), item("b.md", "", 2), item("c.md", "", 3)]);
        assert_eq!(p.handle(Key::Down, 80, 24), Pick::Redraw);
        p.handle(Key::Ctrl('n'), 80, 24);
        assert_eq!(p.selected(), Some(Path::new("/r/c.md")));
        assert_eq!(p.handle(Key::Down, 80, 24), Pick::Nothing);
        p.handle(Key::Ctrl('p'), 80, 24);
        p.handle(Key::Up, 80, 24);
        assert_eq!(p.selected(), Some(Path::new("/r/a.md")));
        p.handle(Key::End, 80, 24);
        assert_eq!(p.selected(), Some(Path::new("/r/c.md")));
        p.handle(Key::Home, 80, 24);
        assert_eq!(p.selected(), Some(Path::new("/r/a.md")));
    }

    #[test]
    fn p04_enter_opens_and_esc_closes() {
        let mut p = picker(vec![item("a.md", "", 1), item("b.md", "", 2)]);
        p.handle(Key::Down, 80, 24);
        assert_eq!(p.handle(Key::Enter, 80, 24), Pick::Open(PathBuf::from("/r/b.md")));
        assert_eq!(p.handle(Key::Esc, 80, 24), Pick::Close);
        assert_eq!(p.handle(Key::Ctrl('c'), 80, 24), Pick::Quit);
        assert_eq!(p.handle(Key::Char('q'), 80, 24), Pick::Redraw);
    }

    #[test]
    fn p05_click_opens_a_row() {
        let mut p = picker(vec![item("a.md", "", 1), item("b.md", "", 2)]);
        let mut click = |row| {
            p.handle(Key::Mouse(Click::Press, 5, row), 80, 24);
            p.handle(Key::Mouse(Click::Release, 5, row), 80, 24)
        };
        assert_eq!(click(5), Pick::Open(PathBuf::from("/r/b.md")));
        assert_eq!(click(9), Pick::Nothing);
        assert_eq!(click(2), Pick::Nothing);
        // The release of a drag begun on the page, before Ctrl-p opened the list.
        assert_eq!(p.handle(Key::Mouse(Click::Release, 5, 5), 80, 24), Pick::Nothing);
    }

    #[test]
    fn a_relist_after_scrolling_draws() {
        let (wake, _w) = mpsc::channel();
        let mut p = picker((0..50).map(|i| item(&format!("f{i}.md"), "", i)).collect());
        p.handle(Key::End, 80, 10);
        p.draw(&mut String::new(), 80, 10, SystemTime::UNIX_EPOCH, &dark());
        assert!(p.top > 0);
        p.root = std::env::temp_dir().join("mddog-no-such-dir");
        p.list(wake);
        p.draw(&mut String::new(), 80, 10, SystemTime::UNIX_EPOCH, &dark());
        assert_eq!(p.top, 0, "the emptied list scrolls back to its top");
    }

    #[test]
    fn ctrl_w_deletes_after_wide_space() {
        let mut p = picker(vec![]);
        typed(&mut p, "a\u{3000}b");
        p.handle(Key::Ctrl('w'), 80, 24);
        assert_eq!(p.query, "a\u{3000}");
    }

    #[test]
    fn a_note_cannot_send_escapes() {
        let mut p = picker(vec![]);
        p.note = Some("Can't open \x1b]0;x\x07a.md".into());
        let mut out = String::new();
        p.draw(&mut out, 80, 10, SystemTime::UNIX_EPOCH, &dark());
        assert!(!out.contains("\x1b]0;"));
    }

    #[test]
    fn a_new_batch_keeps_the_selection() {
        let mut p = picker(vec![item("a.md", "", 5), item("b.md", "", 6)]);
        p.handle(Key::Down, 80, 24);
        p.items.extend(vec![item("new.md", "", 1)]);
        p.resort();
        assert_eq!(p.selected(), Some(Path::new("/r/b.md")));
    }

    #[test]
    fn titles_skip_front_matter_and_code() {
        assert_eq!(title("---\ntitle: x\n# no\n---\n\n```\n# code\n```\n## Real one ##\n"), "Real one");
        assert_eq!(title("text\n#nospace\n    # indented code\n# Yes"), "Yes");
        assert_eq!(title("no heading"), "");
        assert_eq!(title("# C#\n"), "C#");
        assert_eq!(title("````\n```\n# in code\n````\n# After"), "After");
    }

    #[test]
    fn scope_is_the_git_root_or_the_folder() {
        let dir = std::env::temp_dir().join(format!("mddog-scope-{}", std::process::id()));
        std::fs::create_dir_all(dir.join("repo/.git")).unwrap();
        std::fs::create_dir_all(dir.join("repo/docs")).unwrap();
        std::fs::create_dir_all(dir.join("plain")).unwrap();
        assert_eq!(scope(&dir.join("repo/docs"), &dir), dir.join("repo"));
        assert_eq!(scope(&dir.join("plain"), &dir.join("plain")), dir.join("plain"));
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn the_list_follows_the_file_to_another_repo() {
        let dir = std::env::temp_dir().join(format!("mddog-follow-{}", std::process::id()));
        for d in ["a/.git", "a/docs", "a/sub/.git", "b/.git", "b/docs", "notes"] {
            std::fs::create_dir_all(dir.join(d)).unwrap();
        }
        let mut p = Picker::new(dir.join("a"));
        p.query = "x".into();
        assert!(!p.follow(&dir.join("a/docs"), &dir));
        assert!(!p.follow(&dir.join("a/sub"), &dir), "a repo inside the root keeps the root");
        assert_eq!(p.query, "x");
        assert!(p.follow(&dir.join("b/docs"), &dir));
        assert_eq!((p.root.as_path(), p.query.as_str()), (dir.join("b").as_path(), ""));
        // A list of a plain folder keeps it for its own files and follows a file into a repo.
        let mut p = Picker::new(dir.join("notes"));
        assert!(!p.follow(&dir.join("notes"), &dir));
        assert!(p.follow(&dir.join("b/docs"), &dir));
        assert_eq!(p.root, dir.join("b"));
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn walk_skips_hidden_ignored_and_other_files() {
        let dir = std::env::temp_dir().join(format!("mddog-walk-{}", std::process::id()));
        for f in ["a.md", "b.MD", "c.markdown", "d.txt", "e.mdx", ".hidden/e.md", "ignored/f.md", "sub/g.md"] {
            let p = dir.join(f);
            std::fs::create_dir_all(p.parent().unwrap()).unwrap();
            std::fs::write(p, "# T\n").unwrap();
        }
        std::fs::write(dir.join(".gitignore"), "ignored/\n").unwrap();
        let (wake, _w) = mpsc::channel();
        let mut p = Picker::new(dir.clone());
        p.list(wake);
        let until = Instant::now() + Duration::from_secs(5);
        while !p.done && Instant::now() < until {
            p.receive();
        }
        let mut got: Vec<&str> = p.items.iter().map(|i| i.rel.as_str()).collect();
        got.sort_unstable();
        assert_eq!(got, ["a.md", "b.MD", "c.markdown", "sub/g.md"]);
        assert!(p.items.iter().all(|i| i.title == "T"));
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn rows_fit_the_width_and_mark_matches() {
        let mut p = picker(vec![item("a-very-long-folder-name/and-a-long-file-name.md", "A title that is long", 7200)]);
        typed(&mut p, "long");
        let mut out = String::new();
        p.draw(&mut out, 40, 10, SystemTime::UNIX_EPOCH + Duration::from_secs(1_000_000), &dark());
        assert!(out.contains("2h"));
        assert!(out.contains('…'));
        assert_eq!(tail("/a/long/path", 6), ("…/path".to_string(), 6));
        assert_eq!(tail("/a", 6), ("/a".to_string(), 2));
        let (s, w) = styled("abcdef", &[1, 2], 4, "B", "H");
        assert_eq!((s.as_str(), w), ("BaHbcB…", 4));
    }

    #[test]
    fn ages_are_short() {
        assert_eq!(age(Duration::from_secs(5)), "now");
        assert_eq!(age(Duration::from_secs(3 * 3600)), "3h");
        assert_eq!(age(Duration::from_secs(3 * 86_400)), "3d");
        assert_eq!(age(Duration::from_secs(400 * 86_400)), "1y");
    }

    fn pv(f: impl FnOnce(&mut ListPreview)) -> ListPreview {
        let mut p = ListPreview::default();
        f(&mut p);
        p
    }

    #[test]
    fn the_preview_goes_right_when_wide_and_below_when_narrow() {
        let d = ListPreview::default();
        let right = split(&d, false, 150, 40);
        assert_eq!((right.list_col, right.list_row, right.list_cols, right.list_rows), (0, 3, 74, 36));
        assert_eq!(right.preview, Some(Area { col: 75, row: 3, cols: 75, rows: 36 }));
        assert_eq!(right.border(), Some((true, 74)));
        let bottom = split(&d, false, 100, 40);
        assert_eq!((bottom.list_cols, bottom.list_rows), (100, 17));
        assert_eq!(bottom.preview, Some(Area { col: 0, row: 21, cols: 100, rows: 18 }));
        assert_eq!(bottom.border(), Some((false, 20)));
        // The list keeps 10 rows, and a preview under 8 rows is hidden.
        assert_eq!(split(&d, false, 100, 24).preview.map(|a| a.rows), Some(9));
        assert_eq!(split(&d, false, 100, 20).preview, None);
        assert_eq!(split(&d, true, 200, 60).preview, None);
    }

    #[test]
    fn every_position_and_size_is_honored_within_the_minimums() {
        let left = split(&pv(|p| (p.position, p.size) = (Position::Left, Size::Cells(50))), false, 150, 40);
        assert_eq!(left.preview, Some(Area { col: 0, row: 3, cols: 50, rows: 36 }));
        assert_eq!((left.list_col, left.list_cols, left.border()), (51, 99, Some((true, 50))));
        let top = split(&pv(|p| p.position = Position::Top), false, 150, 40);
        assert_eq!(top.preview, Some(Area { col: 0, row: 3, cols: 150, rows: 18 }));
        assert_eq!((top.list_row, top.list_rows, top.border()), (22, 17, Some((false, 21))));
        // A size that would squeeze the list stops at the list's 40 columns.
        assert_eq!(split(&pv(|p| p.size = Size::Percent(95)), false, 150, 40).list_cols, 40);
        let narrow = pv(|p| (p.narrow_position, p.narrow_size) = (NarrowPosition::Top, Some(Size::Cells(12))));
        assert_eq!(split(&narrow, false, 100, 40).preview, Some(Area { col: 0, row: 3, cols: 100, rows: 12 }));
        assert_eq!(split(&pv(|p| p.narrow_position = NarrowPosition::Hidden), false, 100, 40).preview, None);
        // Too narrow for the list and a preview side by side.
        assert_eq!(split(&pv(|p| p.narrow_columns = 0), false, 60, 40).preview, None);
    }

    #[test]
    fn ctrl_t_hides_the_preview_and_a_click_on_it_opens_nothing() {
        let mut p = picker(vec![item("a.md", "A", 60), item("b.md", "B", 120)]);
        p.pv = ListPreview::default();
        p.hidden = false;
        assert!(p.split(150, 40).preview.is_some());
        assert_eq!(p.handle(Key::Ctrl('t'), 150, 40), Pick::Redraw);
        assert_eq!(p.split(150, 40).preview, None);
        p.handle(Key::Ctrl('t'), 150, 40);
        p.handle(Key::Mouse(Click::Press, 100, 4), 150, 40);
        assert_eq!(p.handle(Key::Mouse(Click::Release, 100, 4), 150, 40), Pick::Nothing);
        p.handle(Key::Mouse(Click::Press, 10, 4), 150, 40);
        assert_eq!(p.handle(Key::Mouse(Click::Release, 10, 4), 150, 40), Pick::Open(PathBuf::from("/r/a.md")));
        // With the preview on the left, the list's rows are to its right.
        p.pv.position = Position::Left;
        p.handle(Key::Mouse(Click::Press, 100, 5), 150, 40);
        assert_eq!(p.handle(Key::Mouse(Click::Release, 100, 5), 150, 40), Pick::Open(PathBuf::from("/r/b.md")));
    }

    #[test]
    fn around_is_the_selection_then_its_neighbors() {
        let mut p = picker(vec![item("a.md", "A", 60), item("b.md", "B", 120), item("c.md", "C", 180)]);
        let names = |p: &Picker| p.around().iter().map(|(f, _)| f.file_name().unwrap().to_str().unwrap().to_string()).collect::<Vec<_>>();
        assert_eq!(names(&p), ["a.md", "b.md"]);
        p.handle(Key::Down, 80, 24);
        assert_eq!(names(&p), ["b.md", "c.md", "a.md"]);
    }

    #[test]
    fn a_long_path_keeps_its_file_name() {
        let (s, w) = styled_tail("docs/guides/setup/install.md", &[], 14, "", "*");
        assert_eq!((s.as_str(), w), ("…up/install.md", 14));
        // The hits in the dropped start are dropped, and the rest move with the text.
        let (s, _) = styled_tail("abc/def.md", &[0, 4, 5], 7, "-", "*");
        assert_eq!(s, "-…-*de-f.md");
        assert_eq!(styled_tail("a.md", &[], 14, "", "*").0, "a.md");
    }

    #[test]
    fn the_label_names_the_file_the_preview_shows() {
        assert_eq!(label(Shows::File("docs/a.md"), 40), " docs/a.md ");
        assert_eq!(label(Shows::Stale("docs/a.md"), 40), " docs/a.md · loading… ");
        assert_eq!(label(Shows::Loading, 40), " loading… ");
        assert_eq!(label(Shows::Failed("b.md"), 40), " b.md · can't show ");
        let l = label(Shows::Stale("fixtures/math/transformer-blocks-readme.md"), 30);
        assert_eq!(l, " …blocks-readme.md · loading… ");
        assert_eq!(term::width(&l), 30);
        assert_eq!(label(Shows::File("a.md"), 3), "");
        let l = |sel, shown, failed| label(Shows::of(sel, shown, failed), 40);
        assert_eq!(l(Some("a.md"), Some("a.md"), false), " a.md ");
        assert_eq!(l(Some("b.md"), Some("a.md"), false), " a.md · loading… ");
        assert_eq!(l(Some("b.md"), None, false), " loading… ");
        assert_eq!(l(Some("b.md"), None, true), " b.md · can't show ");
    }

    #[test]
    fn a_click_needs_its_press_and_release_on_list_rows() {
        let mut p = picker(vec![item("a.md", "A", 60), item("b.md", "B", 120)]);
        (p.pv, p.hidden) = (ListPreview::default(), false);
        // At 150x40 the list takes columns 1 to 74, the line is column 75, and the preview starts at 76.
        let click = |p: &mut Picker, press: (u16, u16), release: (u16, u16)| {
            p.handle(Key::Mouse(Click::Press, press.0, press.1), 150, 40);
            p.handle(Key::Mouse(Click::Release, release.0, release.1), 150, 40)
        };
        assert_eq!(click(&mut p, (100, 4), (10, 4)), Pick::Nothing, "a drag from the preview opens nothing");
        assert_eq!(click(&mut p, (75, 4), (75, 4)), Pick::Nothing, "the line between them isn't a row");
        assert_eq!(click(&mut p, (74, 4), (74, 4)), Pick::Open(PathBuf::from("/r/a.md")));
    }

    #[test]
    fn a_preview_needs_both_its_minimum_width_and_height() {
        // Wide enough beside the list, but too short: 11 rows leave 7 for the preview.
        assert_eq!(split(&ListPreview::default(), false, 150, 11).preview, None);
        // Tall enough below the list, but too narrow.
        let below = pv(|p| (p.position, p.narrow_columns) = (Position::Bottom, 0));
        assert_eq!(split(&below, false, 25, 40).preview, None);
        assert!(split(&below, false, 30, 40).preview.is_some());
    }

    #[test]
    fn no_selection_means_no_label() {
        assert_eq!(label(Shows::of(None, None, false), 40), "");
    }
}
