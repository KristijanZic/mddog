//! A callout with a fold marker opens and closes on a click, like `<details>`. Its own test binary, because
//! it loads a config with `callouts = "obsidian"` and the config is read once per process.

use mddog::render::engine::{Engine, Press};

#[test]
fn a_folded_callout_opens_on_a_click() {
    let dir = std::env::temp_dir().join(format!("mddog-fold-{}", std::process::id()));
    std::fs::create_dir_all(dir.join("mddog")).unwrap();
    std::fs::write(dir.join("mddog/config.toml"), "callouts = \"obsidian\"\n").unwrap();
    // SAFETY: the only test in this binary, so no other thread reads the environment.
    unsafe { std::env::set_var("XDG_CONFIG_HOME", &dir) };
    mddog::config::load().unwrap();
    let e = Engine::new(640.0, 2.0, false);
    e.load("> [!faq]- Folded\n> hidden body\n", std::path::Path::new("."), false, mddog::theme::builtin("obsidian-light").unwrap());
    assert_eq!(e.search("Folded", None).len(), 1, "the title shows");
    assert!(e.search("hidden body", None).is_empty(), "a folded callout hides its body");
    assert!((0..40).step_by(2).any(|y| e.press(40.0, f64::from(y)) == Press::Other), "no title to click");
    assert_eq!(e.search("hidden body", None).len(), 1, "the click opened it");
    std::fs::remove_dir_all(&dir).unwrap();
}
