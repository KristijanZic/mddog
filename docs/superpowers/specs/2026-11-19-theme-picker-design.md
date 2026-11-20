# Theme picker

Issue #12 asks for a quick way to switch mddog between light and dark, whatever macOS and the terminal use. This builds a theme list instead of a toggle.

## Behavior

- Ctrl-t opens the theme list in the sidebar, in place of the contents panel. Ctrl-t or Esc closes it and restores the theme shown before.
- Entries: "Follow macOS" (only when mddog started with a light and dark pair), the built-ins, the theme files that `theme` names, and every `*.css` in the config folder and its `themes/` folder that reads as a theme. A file that fails to read as a theme is left out.
- j/k, the arrows, and the mouse pointer move the highlight. Space, or a click on a row, previews that theme on the page, shown in bold in the list. Previewing on every move was tried and dropped: a page redraw blocks keys while the terminal takes in a screen of new images (160 to 270 ms in Rex, slow in WezTerm), and the waits needed to hide that took more code than the feature was worth.
- The page stays usable while the list is open, as with the contents panel: clicks, selection, the wheel and page keys work and keep the preview, and following a link keeps the list. `t` and Ctrl-p cancel the preview first. A centered popup was rejected: it would hide the middle of the page, the part being judged, and need holes cut in the page's images.
- Enter saves the highlighted theme to `config.toml` in the config folder that mddog read at startup (`$XDG_CONFIG_HOME/mddog`, else `~/.config/mddog`). A symlinked file is written through the link.
  - The pick replaces the whole `theme` value, so a pair is dropped. What shows now is what shows on the next launch.
  - "Follow macOS" saves the pair mddog started with.
  - Saving the value already there leaves the file untouched.
- The edit keeps the rest of the file: only the `theme` value changes, or one `theme = ...` line is added at the top.

## Approaches

- Toggle key (advisor's pick): one key, session only. Rejected: the user wants to choose among all themes and change the default.
- Session only, or a separate state file: rejected. Enter always saves (user's call).
- Pair handling: replace one slot of the pair (VS Code, Zed), split the pair into `light_theme` and `dark_theme` keys (advisor), or overwrite `theme` with the one theme. A slot can leave the next launch on a different theme than the one saved, which is #12's case. User chose the plain overwrite.

## Calls made by the builder

- The list shares the sidebar with the contents panel, so the page keeps the width it has with the panel open.
- If the save fails, the theme stays for the session and the status row says why.
