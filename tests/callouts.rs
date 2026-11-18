#![allow(clippy::unwrap_used, reason = "a failed step should fail the test")]

//! GitHub alerts: each case in `fixtures/callouts/` must have an alert exactly where GitHub's HTML in
//! `fixtures/callouts/gh/` has one, and a plain blockquote that starts with the literal `[!KIND]` marker
//! everywhere else. Only the structure is compared, because `gh api /markdown` turns every soft break into
//! `<br>`. GitHub's icons are stripped from the HTML.

use mddog::render::doc::{self, Block, Inline};
use std::path::Path;

/// For a quote whose first paragraph starts with `[!`: that paragraph's first line, and whether more lines follow.
fn marker(blocks: &[Block]) -> String {
    let Some(Block::Para(v)) = blocks.first() else { return String::new() };
    let end = v.iter().position(|i| matches!(i, Inline::Soft | Inline::Hard));
    let line: String = v[..end.unwrap_or(v.len())].iter().map(|i| if let Inline::Text(t, _) = i { t.as_str() } else { "" }).collect();
    if !line.starts_with("[!") {
        return String::new();
    }
    format!(" {line} {}", if end.is_some() { "joined" } else { "alone" })
}

/// Every alert and quote in document order, a parent before its children.
fn ours(blocks: &[Block], out: &mut Vec<String>) {
    for b in blocks {
        match b {
            Block::Alert { kind: k, body: bs, .. } => {
                out.push(format!("alert {}", k.title().to_lowercase()));
                ours(bs, out);
            }
            Block::Quote(bs) => {
                out.push(format!("quote{}", marker(bs)));
                ours(bs, out);
            }
            Block::Align(_, bs) | Block::Details { body: bs, .. } => ours(bs, out),
            Block::List { items, .. } => items.iter().for_each(|i| ours(&i.blocks, out)),
            Block::Footnotes(notes) => notes.iter().for_each(|n| ours(&n.2, out)),
            _ => {}
        }
    }
}

/// The same list read from GitHub's HTML.
fn github(html: &str) -> Vec<String> {
    const ALERT: &str = "<div class=\"markdown-alert markdown-alert-";
    let mut out = vec![];
    let mut rest = html;
    loop {
        let q = rest.find("<blockquote>");
        let a = rest.find(ALERT);
        match (q, a) {
            (Some(q), a) if a.is_none_or(|a| q < a) => {
                rest = &rest[q + "<blockquote>".len()..];
                let body = rest.trim_start().strip_prefix("<p>").unwrap_or("");
                let mut item = "quote".to_string();
                if body.starts_with("[!") {
                    let end = body.find("</p>").unwrap();
                    let (line, joined) = match body[..end].find("<br>") {
                        Some(br) => (&body[..br], true),
                        None => (&body[..end], false),
                    };
                    item += &format!(" {} {}", strip_tags(line), if joined { "joined" } else { "alone" });
                }
                out.push(item);
            }
            (_, Some(a)) => {
                rest = &rest[a + ALERT.len()..];
                out.push(format!("alert {}", &rest[..rest.find('"').unwrap()]));
            }
            _ => return out,
        }
    }
}

/// Text with its HTML tags removed, as `<strong>bold</strong>` becomes `bold`.
fn strip_tags(s: &str) -> String {
    let mut out = String::new();
    let mut tag = false;
    for c in s.chars() {
        match c {
            '<' => tag = true,
            '>' => tag = false,
            c if !tag => out.push(c),
            _ => {}
        }
    }
    out
}

#[test]
fn alerts_match_github() {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("fixtures/callouts");
    let mut cases: Vec<_> = std::fs::read_dir(&dir).unwrap().map(|e| e.unwrap().path()).filter(|p| p.extension().is_some_and(|e| e == "md")).collect();
    cases.sort();
    assert!(cases.len() >= 20, "the cases are missing from {}", dir.display());
    let mut wrong = vec![];
    for md in &cases {
        let name = md.file_stem().unwrap().to_str().unwrap();
        let html = std::fs::read_to_string(dir.join("gh").join(format!("{name}.html"))).unwrap();
        let mut got = vec![];
        ours(&doc::parse(&std::fs::read_to_string(md).unwrap()).blocks, &mut got);
        let want = github(&html);
        if got != want {
            wrong.push(format!("{name}:\n  github: {want:?}\n  mddog:  {got:?}"));
        }
    }
    assert!(wrong.is_empty(), "{}", wrong.join("\n"));
}

#[test]
fn a_demoted_marker_keeps_a_hard_break() {
    let d = doc::parse("- > [!Note]  \n  > Hard break.\n\n- > [!tip]\n  > Soft break.\n");
    let Some(Block::List { items, .. }) = d.blocks.first() else { panic!("not a list") };
    let first = |i: usize| match &items[i].blocks[..] {
        [Block::Quote(q)] => match &q[..] {
            [Block::Para(v), ..] => v[..2].to_vec(),
            _ => panic!("item {i}'s quote has no paragraph"),
        },
        _ => panic!("item {i} is not a quote"),
    };
    assert!(matches!(&first(0)[..], [Inline::Text(t, _), Inline::Hard] if t == "[!Note]"));
    assert!(matches!(&first(1)[..], [Inline::Text(t, _), Inline::Soft] if t == "[!tip]"));
}

/// The kind of each top-level alert, or `quote` for a plain quote.
fn kinds(src: &str) -> Vec<String> {
    let mut out = vec![];
    ours(&doc::parse(src).blocks, &mut out);
    out
}

/// Outputs from `gh api /markdown`: anything after the marker is a body, even when it draws nothing, except HTML
/// that is only a comment.
#[test]
fn any_body_but_a_comment_keeps_an_alert() {
    assert_eq!(kinds("> [!NOTE]\n> <span></span>\n"), ["alert note"]);
    assert_eq!(kinds("> [!NOTE]\n> <div></div>\n"), ["alert note"]);
    assert_eq!(kinds("> [!NOTE]\n> ```\n> ```\n"), ["alert note"]);
    assert_eq!(kinds("> [!NOTE]\n> <!-- c -->\n> text\n"), ["alert note"]);
    assert_eq!(kinds("> [!NOTE]\n> <!-- hidden -->\n"), ["quote [!NOTE] alone"]);
}

#[test]
fn a_lone_cr_ends_the_marker_line() {
    assert_eq!(kinds("- x\n\n  > [!NOTE]\r  > body\r"), ["quote [!NOTE] joined"]);
    assert_eq!(kinds("- x\r\r  > [!NOTE]\r  >\r  > body\r"), ["quote [!NOTE] alone"]);
    assert_eq!(kinds("> [!NOTE]\r> body\r"), ["alert note"]);
}
