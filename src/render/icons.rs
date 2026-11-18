//! The icons the page draws: Primer Octicons (MIT, github.com/primer/octicons), 16 by 16, and Lucide icons
//! (ISC, lucide.dev), 24 by 24. The GitHub alert icons are github.com's, from @primer/octicons 19.42.0. The callout
//! icons are the ones Obsidian 1.15.1 draws, from lucide-static 0.563.0.
use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

use crate::render::doc::Alert;
use crate::render::page::{Bitmap, BtnKind};
use crate::render::svg::{render_svg, svg_tree};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Icon {
    /// Obsidian's icon for a callout kind.
    Alert(Alert),
    /// GitHub's Octicon for one of its five alert kinds.
    Octicon(Alert),
    /// A callout's fold arrow: chevron-down when open. Folded, Obsidian turns it a quarter turn to point right.
    Fold(bool),
    Copy,
    Copied,
    Expand,
    ZoomIn,
    ZoomOut,
    ZoomReset,
}

impl Icon {
    /// A button's icon: its kind's, or the check once it copied.
    pub fn of(kind: BtnKind, copied: bool) -> Icon {
        match (kind, copied) {
            (BtnKind::Expand, _) => Icon::Expand,
            (_, true) => Icon::Copied,
            (BtnKind::ZoomIn, _) => Icon::ZoomIn,
            (BtnKind::ZoomOut, _) => Icon::ZoomOut,
            (BtnKind::ZoomReset, _) => Icon::ZoomReset,
            _ => Icon::Copy,
        }
    }

    fn svg(self, color: u32) -> String {
        const OCTICON: &str = r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 16 16" fill="#{C}" fill-rule="evenodd">{P}</svg>"##;
        const LUCIDE: &str = r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" fill="none" stroke="#{C}" stroke-width="2" stroke-linecap="round" stroke-linejoin="round">{P}</svg>"##;
        let path = |d: &str| format!(r#"<path d="{d}"/>"#);
        let (frame, paths) = match self {
            Icon::Alert(k) => (LUCIDE, callout(k).to_string()),
            // github.com draws these with the default nonzero fill rule, which their paths are made for.
            Icon::Octicon(k) => (r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 16 16" fill="#{C}">{P}</svg>"##, path(octicon(k))),
            Icon::Fold(true) => (LUCIDE, path("m6 9 6 6 6-6")),
            Icon::Fold(false) => (LUCIDE, path("m9 18 6-6-6-6")),

            Icon::Expand => (
                OCTICON,
                path(
                    "M3.72 3.72a.75.75 0 011.06 1.06L2.56 7h10.88l-2.22-2.22a.75.75 0 011.06-1.06l3.5 3.5a.75.75 0 010 1.06l-3.5 3.5a.75.75 0 11-1.06-1.06l2.22-2.22H2.56l2.22 2.22a.75.75 0 11-1.06 1.06l-3.5-3.5a.75.75 0 010-1.06l3.5-3.5z",
                ),
            ),
            Icon::Copied => (OCTICON, path("M13.78 4.22a.75.75 0 010 1.06l-7.25 7.25a.75.75 0 01-1.06 0L2.22 9.28a.75.75 0 011.06-1.06L6 10.94l6.72-6.72a.75.75 0 011.06 0z")),
            Icon::ZoomIn => (LUCIDE, path("M5 12h14M12 5v14")),
            Icon::ZoomOut => (LUCIDE, path("M5 12h14")),
            Icon::ZoomReset => (LUCIDE, path("M3 12a9 9 0 1 0 9-9 9.75 9.75 0 0 0-6.74 2.74L3 8M3 3v5h5")),
            Icon::Copy => (
                OCTICON,
                path(
                    "M0 6.75C0 5.784.784 5 1.75 5h1.5a.75.75 0 010 1.5h-1.5a.25.25 0 00-.25.25v7.5c0 .138.112.25.25.25h7.5a.25.25 0 00.25-.25v-1.5a.75.75 0 011.5 0v1.5A1.75 1.75 0 019.25 16h-7.5A1.75 1.75 0 010 14.25v-7.5z",
                ) + &path(
                    "M5 1.75C5 .784 5.784 0 6.75 0h7.5C15.216 0 16 .784 16 1.75v7.5A1.75 1.75 0 0114.25 11h-7.5A1.75 1.75 0 015 9.25v-7.5zm1.75-.25a.25.25 0 00-.25.25v7.5c0 .138.112.25.25.25h7.5a.25.25 0 00.25-.25v-7.5a.25.25 0 00-.25-.25h-7.5z",
                ),
            ),
        };
        frame.replace("{C}", &format!("{color:06x}")).replace("{P}", &paths)
    }
}

thread_local! {
    static CACHE: RefCell<HashMap<(Icon, u32, u32), Rc<Bitmap>>> = RefCell::default();
}

/// `icon` in `color`, `px` pixels square.
pub fn bitmap(icon: Icon, color: u32, px: u32) -> Option<Rc<Bitmap>> {
    CACHE.with_borrow_mut(|m| {
        if let Some(b) = m.get(&(icon, color, px)) {
            return Some(b.clone());
        }
        let b = Rc::new(render_svg(&svg_tree(icon.svg(color).as_bytes())?, px, px)?);
        m.insert((icon, color, px), b.clone());
        Some(b)
    })
}

/// The Octicon github.com draws for a GitHub alert kind. Other kinds aren't GitHub's, so they get the note's.
fn octicon(k: Alert) -> &'static str {
    match k {
        // light-bulb
        Alert::Tip => {
            "M8 1.5c-2.363 0-4 1.69-4 3.75 0 .984.424 1.625.984 2.304l.214.253c.223.264.47.556.673.848.284.411.537.896.621 1.49a.75.75 0 0 1-1.484.211c-.04-.282-.163-.547-.37-.847a8.456 8.456 0 0 0-.542-.68c-.084-.1-.173-.205-.268-.32C3.201 7.75 2.5 6.766 2.5 5.25 2.5 2.31 4.863 0 8 0s5.5 2.31 5.5 5.25c0 1.516-.701 2.5-1.328 3.259-.095.115-.184.22-.268.319-.207.245-.383.453-.541.681-.208.3-.33.565-.37.847a.751.751 0 0 1-1.485-.212c.084-.593.337-1.078.621-1.489.203-.292.45-.584.673-.848.075-.088.147-.173.213-.253.561-.679.985-1.32.985-2.304 0-2.06-1.637-3.75-4-3.75ZM5.75 12h4.5a.75.75 0 0 1 0 1.5h-4.5a.75.75 0 0 1 0-1.5ZM6 15.25a.75.75 0 0 1 .75-.75h2.5a.75.75 0 0 1 0 1.5h-2.5a.75.75 0 0 1-.75-.75Z"
        }
        // report
        Alert::Important => {
            "M0 1.75C0 .784.784 0 1.75 0h12.5C15.216 0 16 .784 16 1.75v9.5A1.75 1.75 0 0 1 14.25 13H8.06l-2.573 2.573A1.458 1.458 0 0 1 3 14.543V13H1.75A1.75 1.75 0 0 1 0 11.25Zm1.75-.25a.25.25 0 0 0-.25.25v9.5c0 .138.112.25.25.25h2a.75.75 0 0 1 .75.75v2.19l2.72-2.72a.749.749 0 0 1 .53-.22h6.5a.25.25 0 0 0 .25-.25v-9.5a.25.25 0 0 0-.25-.25Zm7 2.25v2.5a.75.75 0 0 1-1.5 0v-2.5a.75.75 0 0 1 1.5 0ZM9 9a1 1 0 1 1-2 0 1 1 0 0 1 2 0Z"
        }
        // alert
        Alert::Warning => {
            "M6.457 1.047c.659-1.234 2.427-1.234 3.086 0l6.082 11.378A1.75 1.75 0 0 1 14.082 15H1.918a1.75 1.75 0 0 1-1.543-2.575Zm1.763.707a.25.25 0 0 0-.44 0L1.698 13.132a.25.25 0 0 0 .22.368h12.164a.25.25 0 0 0 .22-.368Zm.53 3.996v2.5a.75.75 0 0 1-1.5 0v-2.5a.75.75 0 0 1 1.5 0ZM9 11a1 1 0 1 1-2 0 1 1 0 0 1 2 0Z"
        }
        // stop
        Alert::Caution => {
            "M4.47.22A.749.749 0 0 1 5 0h6c.199 0 .389.079.53.22l4.25 4.25c.141.14.22.331.22.53v6a.749.749 0 0 1-.22.53l-4.25 4.25A.749.749 0 0 1 11 16H5a.749.749 0 0 1-.53-.22L.22 11.53A.749.749 0 0 1 0 11V5c0-.199.079-.389.22-.53Zm.84 1.28L1.5 5.31v5.38l3.81 3.81h5.38l3.81-3.81V5.31L10.69 1.5ZM8 4a.75.75 0 0 1 .75.75v3.5a.75.75 0 0 1-1.5 0v-3.5A.75.75 0 0 1 8 4Zm0 8a1 1 0 1 1 0-2 1 1 0 0 1 0 2Z"
        }
        // info
        _ => {
            "M0 8a8 8 0 1 1 16 0A8 8 0 0 1 0 8Zm8-6.5a6.5 6.5 0 1 0 0 13 6.5 6.5 0 0 0 0-13ZM6.5 7.75A.75.75 0 0 1 7.25 7h1a.75.75 0 0 1 .75.75v2.75h.25a.75.75 0 0 1 0 1.5h-2a.75.75 0 0 1 0-1.5h.25v-2h-.25a.75.75 0 0 1-.75-.75ZM8 6a1 1 0 1 1 0-2 1 1 0 0 1 0 2Z"
        }
    }
}

/// The Lucide icon Obsidian gives each callout kind in its `app.css`.
fn callout(k: Alert) -> &'static str {
    match k {
        // pencil
        Alert::Note => r#"<path d="M21.174 6.812a1 1 0 0 0-3.986-3.987L3.842 16.174a2 2 0 0 0-.5.83l-1.321 4.352a.5.5 0 0 0 .623.622l4.353-1.32a2 2 0 0 0 .83-.497z"/><path d="m15 5 4 4"/>"#,
        // clipboard-list
        Alert::Abstract => {
            r#"<rect width="8" height="4" x="8" y="2" rx="1" ry="1"/><path d="M16 4h2a2 2 0 0 1 2 2v14a2 2 0 0 1-2 2H6a2 2 0 0 1-2-2V6a2 2 0 0 1 2-2h2"/><path d="M12 11h4"/><path d="M12 16h4"/><path d="M8 11h.01"/><path d="M8 16h.01"/>"#
        }
        // info
        Alert::Info => r#"<circle cx="12" cy="12" r="10"/><path d="M12 16v-4"/><path d="M12 8h.01"/>"#,
        // circle-check
        Alert::Todo => r#"<circle cx="12" cy="12" r="10"/><path d="m9 12 2 2 4-4"/>"#,
        // flame
        Alert::Tip | Alert::Important => r#"<path d="M12 3q1 4 4 6.5t3 5.5a1 1 0 0 1-14 0 5 5 0 0 1 1-3 1 1 0 0 0 5 0c0-2-1.5-3-1.5-5q0-2 2.5-4"/>"#,
        // check
        Alert::Success => r#"<path d="M20 6 9 17l-5-5"/>"#,
        // circle-help
        Alert::Question => r#"<circle cx="12" cy="12" r="10"/><path d="M9.09 9a3 3 0 0 1 5.83 1c0 2-3 3-3 3"/><path d="M12 17h.01"/>"#,
        // triangle-alert
        Alert::Warning | Alert::Caution => r#"<path d="m21.73 18-8-14a2 2 0 0 0-3.48 0l-8 14A2 2 0 0 0 4 21h16a2 2 0 0 0 1.73-3"/><path d="M12 9v4"/><path d="M12 17h.01"/>"#,
        // x
        Alert::Failure => r#"<path d="M18 6 6 18"/><path d="m6 6 12 12"/>"#,
        // zap
        Alert::Danger => r#"<path d="M4 14a1 1 0 0 1-.78-1.63l9.9-10.2a.5.5 0 0 1 .86.46l-1.92 6.02A1 1 0 0 0 13 10h7a1 1 0 0 1 .78 1.63l-9.9 10.2a.5.5 0 0 1-.86-.46l1.92-6.02A1 1 0 0 0 11 14z"/>"#,
        // bug
        Alert::Bug => {
            r#"<path d="M12 20v-9"/><path d="M14 7a4 4 0 0 1 4 4v3a6 6 0 0 1-12 0v-3a4 4 0 0 1 4-4z"/><path d="M14.12 3.88 16 2"/><path d="M21 21a4 4 0 0 0-3.81-4"/><path d="M21 5a4 4 0 0 1-3.55 3.97"/><path d="M22 13h-4"/><path d="M3 21a4 4 0 0 1 3.81-4"/><path d="M3 5a4 4 0 0 0 3.55 3.97"/><path d="M6 13H2"/><path d="m8 2 1.88 1.88"/><path d="M9 7.13V6a3 3 0 1 1 6 0v1.13"/>"#
        }
        // list
        Alert::Example => r#"<path d="M3 5h.01"/><path d="M3 12h.01"/><path d="M3 19h.01"/><path d="M8 5h13"/><path d="M8 12h13"/><path d="M8 19h13"/>"#,
        // quote
        Alert::Quote => {
            r#"<path d="M16 3a2 2 0 0 0-2 2v6a2 2 0 0 0 2 2 1 1 0 0 1 1 1v1a2 2 0 0 1-2 2 1 1 0 0 0-1 1v2a1 1 0 0 0 1 1 6 6 0 0 0 6-6V5a2 2 0 0 0-2-2z"/><path d="M5 3a2 2 0 0 0-2 2v6a2 2 0 0 0 2 2 1 1 0 0 1 1 1v1a2 2 0 0 1-2 2 1 1 0 0 0-1 1v2a1 1 0 0 0 1 1 6 6 0 0 0 6-6V5a2 2 0 0 0-2-2z"/>"#
        }
    }
}
