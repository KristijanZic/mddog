use crate::config::Choice;
use crate::theme::Theme;

#[derive(Debug)]
pub struct List {
    pub choices: Vec<Choice>,
    pub sel: usize,
    pub offset: usize,
    pub current: usize,
    pub before: Option<&'static Theme>,
    pub shown: usize,
}

impl List {
    pub fn new(choices: Vec<Choice>, before: Option<&'static Theme>, follows: bool, now: &'static Theme, height: usize) -> List {
        let current = choices
            .iter()
            .position(|c| match c {
                Choice::Follow { .. } => follows,
                Choice::Theme { theme, .. } => !follows && std::ptr::eq(*theme, now),
            })
            .unwrap_or(0);
        let mut list = List { choices, sel: current, offset: 0, current, before, shown: current };
        list.reveal(height);
        list
    }

    pub fn chosen(&self) -> &Choice {
        &self.choices[self.sel]
    }

    pub fn preview(&mut self) -> bool {
        let changed = self.shown != self.sel;
        self.shown = self.sel;
        changed
    }

    pub fn step(&mut self, down: bool, height: usize) -> bool {
        let to = if down { self.sel + 1 } else { self.sel.wrapping_sub(1) };
        if to >= self.choices.len() {
            return false;
        }
        self.sel = to;
        self.reveal(height);
        true
    }

    pub fn point(&mut self, row: usize) -> bool {
        let i = self.offset + row;
        if row >= self.choices.len().saturating_sub(self.offset) || i == self.sel {
            return false;
        }
        self.sel = i;
        true
    }

    pub fn at(&self, row: usize) -> Option<usize> {
        Some(self.offset + row).filter(|&i| i < self.choices.len())
    }

    pub fn reveal(&mut self, height: usize) {
        let height = height.max(1);
        self.offset = self.offset.min(self.sel).max((self.sel + 1).saturating_sub(height));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::theme;

    fn choice(name: &str) -> Choice {
        Choice::Theme { value: name.into(), theme: theme::builtin(name).unwrap() }
    }

    fn choices() -> Vec<Choice> {
        let follow = Choice::Follow { value: "light:github-light,dark:github-dark".into(), light: theme::builtin("github-light").unwrap(), dark: theme::builtin("github-dark").unwrap() };
        vec![follow, choice("github-light"), choice("github-dark"), choice("obsidian-light"), choice("obsidian-dark")]
    }

    #[test]
    fn opens_on_the_theme_in_use() {
        let dark = theme::builtin("github-dark").unwrap();
        assert_eq!(List::new(choices(), None, true, dark, 10).current, 0);
        assert_eq!(List::new(choices(), Some(dark), false, dark, 10).current, 2);
        assert_eq!(List::new(choices()[1..].to_vec(), None, false, dark, 10).current, 1);
    }

    #[test]
    fn the_highlight_stays_in_view() {
        let mut l = List::new(choices(), None, true, theme::builtin("github-dark").unwrap(), 2);
        assert!(!l.step(false, 2));
        assert!(l.step(true, 2) && l.step(true, 2));
        assert_eq!((l.sel, l.offset), (2, 1));
        assert!(l.step(true, 2) && l.step(true, 2) && !l.step(true, 2));
        assert_eq!((l.sel, l.offset), (4, 3));
        assert!(l.point(0));
        assert_eq!(l.sel, 3);
        assert!(!l.point(0) && !l.point(2));
        assert_eq!((l.at(1), l.at(2)), (Some(4), None));
    }

    #[test]
    fn a_preview_shows_the_highlight_once() {
        let mut l = List::new(choices(), None, true, theme::builtin("github-dark").unwrap(), 10);
        assert!(!l.preview());
        assert!(l.step(true, 10) && l.step(true, 10));
        assert_eq!(l.shown, 0);
        assert!(l.preview() && !l.preview());
        assert_eq!(l.shown, 2);
    }
}
