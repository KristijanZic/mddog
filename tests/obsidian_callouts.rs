//! `callouts = "obsidian"` reads `> [!kind]` blocks as Obsidian 1.15.1 does. Each expectation comes from
//! rendering the same Markdown in Obsidian's reading view and reading the callout's `data-callout`, title and
//! fold state.

use mddog::config::Callouts;
use mddog::render::doc::{Alert, Block, Inline, parse_with};

/// The one block `src` parses to, read in Obsidian mode.
fn one(src: &str) -> Block {
    let mut d = parse_with(src, Callouts::Obsidian);
    assert_eq!(d.blocks.len(), 1, "{src:?} -> {:?}", d.blocks);
    d.blocks.remove(0)
}

fn text(v: &[Inline]) -> String {
    v.iter()
        .map(|i| match i {
            Inline::Text(t, _) => t.as_str(),
            _ => "/",
        })
        .collect()
}

/// The kind, title and fold of a callout.
fn head(b: &Block) -> (Alert, String, Option<bool>) {
    match b {
        Block::Alert { kind, title, fold, .. } => (*kind, text(title.as_deref().expect("Obsidian mode always titles")), fold.map(|f| f.0)),
        b => panic!("not a callout: {b:?}"),
    }
}

fn body(b: &Block) -> &[Block] {
    match b {
        Block::Alert { body, .. } => body,
        b => panic!("not a callout: {b:?}"),
    }
}

#[test]
fn every_word_obsidian_knows() {
    use Alert::*;
    // `important` and `caution` look like tip and warning in Obsidian, but keep their own word as the title.
    let table = [
        ("note", Note),
        ("abstract", Abstract),
        ("summary", Abstract),
        ("tldr", Abstract),
        ("info", Info),
        ("todo", Todo),
        ("tip", Tip),
        ("hint", Tip),
        ("important", Tip),
        ("success", Success),
        ("check", Success),
        ("done", Success),
        ("question", Question),
        ("help", Question),
        ("faq", Question),
        ("warning", Warning),
        ("caution", Warning),
        ("attention", Warning),
        ("failure", Failure),
        ("fail", Failure),
        ("missing", Failure),
        ("danger", Danger),
        ("error", Danger),
        ("bug", Bug),
        ("example", Example),
        ("quote", Quote),
        ("cite", Quote),
        ("foo", Note),
    ];
    for (word, kind) in table {
        let b = one(&format!("> [!{word}]\n> body\n"));
        let mut title = word.to_string();
        title[..1].make_ascii_uppercase();
        assert_eq!(head(&b), (kind, title, None), "{word}");
        assert!(matches!(body(&b), [Block::Para(p)] if text(p) == "body"), "{word}: {:?}", body(&b));
    }
}

#[test]
fn the_kind_ignores_case_and_the_title_is_the_word() {
    assert_eq!(head(&one("> [!NOTE]\n> upper\n")), (Alert::Note, "Note".into(), None));
    assert_eq!(head(&one("> [!WaRnInG]\n> x\n")), (Alert::Warning, "Warning".into(), None));
    // Spaces in the kind become dashes, and the default title turns them back into spaces.
    assert_eq!(head(&one("> [!foo bar]\n> x\n")), (Alert::Note, "Foo bar".into(), None));
    // Text after `|` is metadata for CSS, not part of the kind.
    assert_eq!(head(&one("> [!tip|wide] T\n> x\n")), (Alert::Tip, "T".into(), None));
}

#[test]
fn the_first_line_is_the_title() {
    let b = one("> [!important] My title\n> with title\n");
    assert_eq!(head(&b), (Alert::Tip, "My title".into(), None));
    assert!(matches!(body(&b), [Block::Para(p)] if text(p) == "with title"));
    let b = one("> [!note] only title\n");
    assert_eq!(head(&b), (Alert::Note, "only title".into(), None));
    assert!(body(&b).is_empty());
    let b = one("> [!TIP] **bold** title\n> body\n");
    assert_eq!(head(&b).1, "bold title");
    let Block::Alert { title: Some(t), .. } = &b else { unreachable!() };
    assert!(matches!(&t[0], Inline::Text(s, m) if s == "bold" && m.bold), "{t:?}");
    // Two trailing spaces make a hard break, which ends the title the same way.
    let b = one("> [!note] T  \n> x\n");
    assert_eq!(head(&b).1, "T");
    assert!(matches!(body(&b), [Block::Para(p)] if text(p) == "x"));
}

#[test]
fn a_callout_with_no_body_keeps_its_title() {
    let b = one("> [!NOTE]\n");
    assert_eq!(head(&b), (Alert::Note, "Note".into(), None));
    assert!(body(&b).is_empty());
}

#[test]
fn fold_markers() {
    assert_eq!(head(&one("> [!faq]- Folded\n> hidden\n")), (Alert::Question, "Folded".into(), Some(false)));
    assert_eq!(head(&one("> [!tip]+ Open\n> shown\n")), (Alert::Tip, "Open".into(), Some(true)));
    assert_eq!(head(&one("> [!note]-\n> x\n")), (Alert::Note, "Note".into(), Some(false)));
}

#[test]
fn callouts_nest_in_obsidian() {
    let b = one("> [!note]\n> outer\n>\n> > [!warning]\n> > nested\n");
    assert_eq!(head(&b).0, Alert::Note);
    assert!(matches!(body(&b), [Block::Para(_), w] if head(w) == (Alert::Warning, "Warning".into(), None)), "{:?}", body(&b));
    let d = parse_with("- item\n\n  > [!tip]\n  > in list\n", Callouts::Obsidian);
    let Block::List { items, .. } = &d.blocks[0] else { panic!("{:?}", d.blocks) };
    assert_eq!(head(&items[0].blocks[1]).0, Alert::Tip);
}

#[test]
fn not_a_callout() {
    for src in [
        // The marker must end the word: Obsidian wants a space or the line's end after it.
        "> [!NOTE]x\n> y\n",
        "> [!]\n> y\n",
        "> plain [!NOTE]\n",
        // Obsidian matches the raw line, so an escape or an entity keeps the marker literal.
        "> \\[!note]\n> y\n",
        "> &#91;!note]\n> y\n",
        // The marker must be on the quote's first line, after `>` and at most one space.
        ">\n> [!note]\n> y\n",
        ">  [!note]\n> y\n",
        ">\t[!note]\n> y\n",
    ] {
        let d = parse_with(src, Callouts::Obsidian);
        assert!(matches!(d.blocks[0], Block::Quote(_)), "{src:?} -> {:?}", d.blocks);
    }
}

#[test]
fn github_mode_is_unchanged() {
    let d = parse_with("> [!NOTE] title\n> x\n", Callouts::Github);
    assert!(matches!(d.blocks[0], Block::Quote(_)), "{:?}", d.blocks);
    let d = parse_with("> [!faq]\n> x\n", Callouts::Github);
    assert!(matches!(d.blocks[0], Block::Quote(_)), "{:?}", d.blocks);
    let d = parse_with("> [!caution]\n> x\n", Callouts::Github);
    assert!(matches!(d.blocks[0], Block::Alert { kind: Alert::Caution, title: None, fold: None, .. }), "{:?}", d.blocks);
}

#[test]
fn the_kind_is_the_raw_text() {
    // Obsidian reads `*tip*` as the kind, not as italics, and draws an unknown kind as a note.
    let b = one("> [!*tip*] starred\n> body\n");
    assert_eq!(head(&b), (Alert::Note, "starred".into(), None));
    let b = one("> [!*tip*]\n> body\n");
    assert_eq!(head(&b), (Alert::Note, "*tip*".into(), None));
    // Obsidian finds the marker before it reads links, so a reference definition doesn't stop it.
    let d = parse_with("> [!foo]\n> y\n\n[!foo]: https://example.com\n", Callouts::Obsidian);
    assert_eq!(head(&d.blocks[0]), (Alert::Note, "Foo".into(), None));
    // An indented quote is still a callout.
    assert_eq!(head(&one("   > [!note]\n   > x\n")).0, Alert::Note);
}
