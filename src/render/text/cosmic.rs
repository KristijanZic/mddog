use super::*;
use crate::render::canvas::Canvas;
use cosmic_text::{Attrs, Buffer, CacheKeyFlags, Family, FeatureTag, FontFeatures, FontSystem, LayoutGlyph, Metrics, Shaping, Style, SwashCache, SwashContent, Weight, Wrap, fontdb};
use std::collections::HashMap;
use std::sync::OnceLock;

const SANS_FONTS: &[&str] = &["Adwaita Sans", "Inter", "Cantarell", "Noto Sans", "DejaVu Sans", "Liberation Sans", "Ubuntu"];
const MONO_FONTS: &[&str] = &["Adwaita Mono", "Noto Sans Mono", "DejaVu Sans Mono", "Liberation Mono", "Ubuntu Mono"];
const SERIF_FONTS: &[&str] = &["Noto Serif", "DejaVu Serif", "Liberation Serif"];
const MATH_FONTS: &[&str] = &["STIX Two Text", "STIXGeneral", "STIX Two Math", "Latin Modern Roman", "DejaVu Serif"];

struct System {
    locale: String,
    db: fontdb::Database,
    sans: String,
    mono: String,
    serif: String,
    math: Option<String>,
}

fn installed(db: &fontdb::Database, name: &str) -> Option<String> {
    db.faces().flat_map(|f| f.families.iter()).find(|(n, _)| n.eq_ignore_ascii_case(name)).map(|(n, _)| n.clone())
}

fn fc_match(alias: &str) -> Option<std::process::Child> {
    use std::process::{Command, Stdio};
    Command::new("fc-match").args(["-f", "%{family[0]}", alias]).stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::null()).spawn().ok()
}

fn matched(child: Option<std::process::Child>, until: std::time::Instant) -> Option<String> {
    let mut child = child?;
    loop {
        match child.try_wait() {
            Ok(Some(_)) => break,
            Ok(None) if std::time::Instant::now() < until => std::thread::sleep(std::time::Duration::from_millis(1)),
            _ => {
                let _ = child.kill();
                let _ = child.wait();
                return None;
            }
        }
    }
    let out = child.wait_with_output().ok()?;
    out.status.success().then(|| String::from_utf8_lossy(&out.stdout).trim().to_string()).filter(|s| !s.is_empty())
}

fn generic(db: &fontdb::Database, matched: Option<String>, fallbacks: &[&str]) -> String {
    matched
        .and_then(|n| installed(db, &n))
        .or_else(|| fallbacks.iter().find_map(|n| installed(db, n)))
        .or_else(|| db.faces().filter_map(|f| f.families.first()).map(|(n, _)| n).find(|n| !["emoji", "symbol", "math"].iter().any(|w| n.to_ascii_lowercase().contains(w))).cloned())
        .unwrap_or_default()
}

fn system() -> &'static System {
    static SYSTEM: OnceLock<System> = OnceLock::new();
    SYSTEM.get_or_init(|| {
        let until = std::time::Instant::now() + std::time::Duration::from_secs(1);
        let asked = ["sans-serif", "monospace", "serif"].map(fc_match);
        let (locale, mut db) = FontSystem::new().into_locale_and_db();
        let [sans, mono, serif] = asked.map(|c| matched(c, until));
        let (sans, mono, serif) = (generic(&db, sans, SANS_FONTS), generic(&db, mono, MONO_FONTS), generic(&db, serif, SERIF_FONTS));
        db.set_sans_serif_family(sans.clone());
        db.set_monospace_family(mono.clone());
        db.set_serif_family(serif.clone());
        let math = MATH_FONTS.iter().find_map(|n| installed(&db, n));
        System { locale, db, sans, mono, serif, math }
    })
}

pub fn warm() {
    system();
}

pub fn sans_family() -> &'static str {
    &system().sans
}

pub fn mono_family() -> &'static str {
    &system().mono
}

pub fn serif_family() -> &'static str {
    &system().serif
}

pub fn missing_fonts() -> Option<&'static str> {
    system()
        .sans
        .is_empty()
        .then_some("no fonts found, so mddog can't draw text. Install a font package, such as fonts-dejavu-core on Debian or Ubuntu, dejavu-sans-fonts on Fedora, or font-dejavu on Alpine.")
}

fn family(names: &[String], mono: bool) -> String {
    let sys = system();
    for n in names {
        match n.to_ascii_lowercase().as_str() {
            n if system_ui(n) => return sys.sans.clone(),
            "ui-monospace" | "monospace" => return sys.mono.clone(),
            "ui-serif" | "serif" => return sys.serif.clone(),
            n => {
                if let Some(f) = installed(&sys.db, n) {
                    return f;
                }
            }
        }
    }
    if mono { sys.mono.clone() } else { sys.sans.clone() }
}

const LIFT: f32 = 65536.0;

struct Line {
    glyphs: Vec<LayoutGlyph>,
    w: f32,
    rtl: bool,
}

pub struct Fonts {
    fs: FontSystem,
    cache: SwashCache,
    lines: HashMap<(FontKey, String), Line>,
    metrics: HashMap<FontKey, (f32, f32)>,
    body: String,
    code: String,
    scale: f32,
}

impl Fonts {
    pub fn new() -> Fonts {
        let sys = system();
        let fs = FontSystem::new_with_locale_and_db(sys.locale.clone(), sys.db.clone());
        Fonts { fs, cache: SwashCache::new(), lines: HashMap::new(), metrics: HashMap::new(), body: sys.sans.clone(), code: sys.mono.clone(), scale: 1.0 }
    }

    pub fn set_families(&mut self, body: Vec<String>, code: Vec<String>) {
        let (body, code) = (family(&body, false), family(&code, true));
        if (&body, &code) != (&self.body, &self.code) {
            (self.body, self.code) = (body, code);
            self.lines.clear();
            self.metrics.clear();
        }
    }

    pub fn set_scale(&mut self, scale: f32) {
        if scale != self.scale {
            self.scale = scale;
            self.cache = SwashCache::new();
        }
    }

    fn name(&self, k: FontKey) -> String {
        match system().math.as_deref() {
            Some(m) if k.style & MATH != 0 => m.to_string(),
            _ if k.style & MONO != 0 => self.code.clone(),
            _ => self.body.clone(),
        }
    }
}

fn attrs(name: &str, k: FontKey) -> Attrs<'_> {
    let w = weight_of(k.style);
    let weight = if w != 0 {
        Weight(w)
    } else if k.style & BOLD != 0 {
        Weight::BOLD
    } else {
        Weight::NORMAL
    };
    let mut a = Attrs::new().family(Family::Name(name)).weight(weight).cache_key_flags(CacheKeyFlags::DISABLE_HINTING);
    if k.style & ITALIC != 0 {
        a = a.style(Style::Italic);
    }
    if k.style & TABULAR != 0 {
        let mut f = FontFeatures::new();
        f.enable(FeatureTag::new(b"tnum"));
        a = a.font_features(f);
    }
    if k.track != 0 {
        a = a.letter_spacing(f32::from(k.track) / 1000.0);
    }
    a
}

impl Fonts {
    pub fn ascent_descent(&mut self, k: FontKey) -> (f32, f32) {
        let k = FontKey { track: 0, ..k };
        if let Some(m) = self.metrics.get(&k) {
            return *m;
        }
        let px = k.px();
        let first = self.line(k, "H").glyphs.first().map(|g| (g.font_id, g.font_weight));
        let m = first
            .and_then(|(id, w)| self.fs.get_font(id, w))
            .map(|f| {
                let m = f.as_swash().metrics(&[]);
                let em = f32::from(m.units_per_em.max(1));
                (m.ascent / em * px, m.descent.abs() / em * px)
            })
            .unwrap_or((px * 0.95, px * 0.25));
        self.metrics.insert(k, m);
        m
    }

    fn line(&mut self, k: FontKey, s: &str) -> &Line {
        if self.lines.len() > 20000 {
            self.lines.clear();
        }
        let key = (k, s.to_string());
        if !self.lines.contains_key(&key) {
            let name = self.name(k);
            let attrs = attrs(&name, k);
            let px = k.px().max(0.1);
            let mut b = Buffer::new(&mut self.fs, Metrics::new(px, px * 1.25));
            b.set_wrap(Wrap::None);
            b.set_size(None, None);
            b.set_text(s, &attrs, Shaping::Advanced, None);
            b.shape_until_scroll(&mut self.fs, false);
            let mut glyphs = vec![];
            let (mut w, mut rtl): (f32, bool) = (0.0, false);
            for run in b.layout_runs() {
                glyphs.extend_from_slice(run.glyphs);
                w = w.max(run.line_w);
                rtl |= run.rtl;
            }
            self.lines.insert(key.clone(), Line { glyphs, w, rtl });
        }
        &self.lines[&key]
    }

    pub fn measure(&mut self, k: FontKey, s: &str) -> f32 {
        self.line(k, s).w
    }

    pub fn char_x(&mut self, k: FontKey, s: &str, idx: usize) -> f32 {
        let b = s.char_indices().nth(idx).map_or(s.len(), |(b, _)| b);
        let line = self.line(k, s);
        let (start, end) = if line.rtl { (line.w, 0.0) } else { (0.0, line.w) };
        if b == 0 {
            return start;
        }
        if b == s.len() {
            return end;
        }
        line.glyphs.iter().find(|g| g.start <= b && b < g.end).map_or(end, |g| {
            let chars = s[g.start..g.end].chars().count().max(1) as f32;
            let part = s[g.start..b].chars().count() as f32 / chars;
            if g.level.is_rtl() { g.x + g.w * (1.0 - part) } else { g.x + g.w * part }
        })
    }

    pub fn draw(&mut self, c: &mut Canvas, k: FontKey, s: &str, x: f32, y: f32) {
        self.line(k, s);
        let Fonts { fs, cache, lines, .. } = self;
        let Some(line) = lines.get(&(k, s.to_string())) else { return };
        let (z, ox, oy) = c.place(x, y);
        for g in &line.glyphs {
            let pg = g.physical((ox + LIFT, oy + LIFT), z);
            let Some(img) = cache.get_image(fs, pg.cache_key) else { continue };
            let (w, h) = (img.placement.width, img.placement.height);
            if w == 0 || h == 0 {
                continue;
            }
            let rgba = match img.content {
                SwashContent::Mask => false,
                SwashContent::Color => true,
                SwashContent::SubpixelMask => continue,
            };
            c.glyph(pg.x - LIFT as i32 + img.placement.left, pg.y - LIFT as i32 - img.placement.top, w, h, &img.data, rgba);
        }
    }
}

impl Default for Fonts {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn caret_offsets_follow_the_text_direction() {
        let mut f = Fonts::new();
        let k = FontKey::new(16.0, 0);
        let ltr: Vec<f32> = (0..=3).map(|i| f.char_x(k, "abc", i)).collect();
        assert!(ltr[0].abs() < 0.01 && ltr.windows(2).all(|p| p[0] < p[1]), "{ltr:?}");
        assert!((ltr[3] - f.measure(k, "abc")).abs() < 0.01, "{ltr:?}");
        let s = "\u{5e9}\u{5dc}\u{5d5}\u{5dd}";
        let w = f.measure(k, s);
        let rtl: Vec<f32> = (0..=4).map(|i| f.char_x(k, s, i)).collect();
        assert!((rtl[0] - w).abs() < 0.01 && rtl[4].abs() < 0.01, "a right-to-left run starts at its right edge: {rtl:?} in {w}");
        assert!(rtl.windows(2).all(|p| p[0] > p[1]), "{rtl:?}");
        let mixed = "abc\u{5e9}\u{5dc}\u{5d5}\u{5dd}";
        let w = f.measure(k, mixed);
        assert!((f.char_x(k, mixed, 7) - w).abs() < 0.01, "a left-to-right line ends at its right edge, whatever its last run");
    }

    #[test]
    fn letter_spacing_is_in_em() {
        let mut f = Fonts::new();
        for px in [16.0, 32.0] {
            let plain = f.measure(FontKey::new(px, 0), "abcd");
            let tracked = f.measure(FontKey::new(px, 0).tracked(-0.05), "abcd");
            assert!((plain - tracked - 4.0 * 0.05 * px).abs() < 0.01, "{px} px: {plain} untracked, {tracked} at -0.05 em");
        }
    }
}
