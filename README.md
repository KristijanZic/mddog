<h1 align="center">
  <img src="assets/logo.webp" width="160" alt="Logo"/><br/>
  mddog
</h1>

<p align="center">
  <a href="https://github.com/qu8n/mddog/releases/latest"><img src="https://img.shields.io/github/v/release/qu8n/mddog" alt="Latest release"/></a>
  <a href="LICENSE"><img src="https://img.shields.io/github/license/qu8n/mddog" alt="License"/></a>
  <img src="https://img.shields.io/badge/platform-macOS-lightgrey?logo=apple" alt="Platform: macOS"/>
  <img src="https://img.shields.io/badge/built%20with-Rust-orange?logo=rust" alt="Built with Rust"/>
</p>

<p align="center">
  Real Markdown, right in your terminal
</p>

https://github.com/user-attachments/assets/bf496fc9-b348-47e7-bbad-5c2874194c61

## Why mddog?

Other terminal viewers render Markdown with only characters. Code editors render the real page but live outside the terminal. mddog renders the real page in your terminal, using a single binary without browser nor Electron.

|                                       | mddog | Text-based terminal viewers | Code editor preview |
|---------------------------------------|:-----:|:---------------------------:|:-------------------:|
| Stays in the terminal                 | ✅    | ✅                          | 🚫                  |
| Real fonts and heading sizes          | ✅    | 🚫                          | ✅                  |
| Diagrams and math drawn as graphics   | ✅    | 🚫                          | Some                |
| Images                                | ✅    | Some                        | ✅                  |
| Live reload                           | ✅    | Some                        | ✅                  |
| Pane in Claude Code                   | ✅    | 🚫                          | 🚫                  |

<sub><em>Compared in November 2026 to terminal viewers like glow, leaf, mdcat, Frogmouth, and rich-cli, and to the built-in Markdown previews in VS Code, Cursor, Zed, and JetBrains IDEs.</em></sub>

## Install

```sh
brew install qu8n/tap/mddog
```

Or, without Homebrew:

```sh
curl -fsSL https://raw.githubusercontent.com/qu8n/mddog/main/install.sh | sh
```

Or, with Nix:

```sh
nix run github:qu8n/mddog            # run directly
nix profile add github:qu8n/mddog    # install imperatively
```

Or add it declaratively to your system `flake.nix` inputs:

```nix
inputs.mddog = {
  url = "github:qu8n/mddog";
  # Optional: reuse your own nixpkgs input (used on Apple Silicon)
  # inputs.nixpkgs.follows = "nixpkgs";
};
```

Then include `inputs.mddog.packages.${pkgs.stdenv.hostPlatform.system}.default` in your packages list.

### Requirements

- macOS
- A terminal that supports the [kitty graphics protocol](https://sw.kovidgoyal.net/kitty/graphics-protocol/): Ghostty, iTerm2, kitty, WezTerm, or Rex


## Usage

```sh
mdd             # list Markdown files in the current directory to pick from
mdd README.md   # view a file
```

## Docs

Read the full docs at **[mddog.pages.dev/docs](https://mddog.pages.dev/docs)** for key bindings, Claude Code plugin, configuration, themes, and more.

## Credits

The logo was made by Helen Nguyen ([@daymelon](https://instagram.com/daymelon)), commissioned for mddog and not covered by the MIT license.
