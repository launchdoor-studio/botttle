<p align="center">
  <img src="./assets/logo.png" alt="botttle" width="128" />
</p>

<h1 align="center">botttle</h1>

<p align="center">
  A GPU-accelerated terminal workspace — tabs, panes, and room for agents.
</p>

<p align="center">
  <img src="https://img.shields.io/badge/built%20with-Rust-000000?logo=rust&logoColor=white" alt="Built with Rust" />
  <img src="https://img.shields.io/badge/rendering-GPUI-7c8cf8" alt="Rendered with GPUI" />
  <img src="https://img.shields.io/badge/platform-macOS-lightgrey" alt="macOS" />
  <img src="https://img.shields.io/badge/license-MIT-blue" alt="MIT license" />
</p>

---

## About

botttle is a terminal emulator that draws on the GPU. Every pane runs your login
shell through a real PTY and a full ANSI emulator, and the entire window — grid,
chrome, tab strip — is rendered by [GPUI](https://gpui.rs), the same engine
behind [Zed](https://zed.dev).

It is also a workspace. Panes split right and down to any depth, tabs hold their
own layouts, and both are first-class rather than bolted on: a new pane starts in
the directory the current one is in, the focused pane is marked so you can see
where input lands, and the emulator's state — title, cursor, working directory —
is available to the app around it.

That structure is the point. Everything an agent does in a development loop —
running builds, reading logs, driving tools — already happens in a terminal, but
a terminal treats all of it as one undifferentiated stream of bytes. botttle
keeps the terminal's honesty — `ctrl-c`, `ctrl-d` and the rest reach the program
untouched — while giving the surrounding workspace the structure
that agent-driven work needs. The next step is a pane that is an agent session
rather than a shell, sharing the same tree, tab, and working directory.

**Status:** early, and moving. It is used daily by its author on macOS. Linux
should build — the platform-specific paths are there — but is untested.

## Features

**Terminal**

- Real PTYs, a full ANSI emulator ([`alacritty_terminal`](https://github.com/alacritty/alacritty)),
  24-bit color, text attributes, and configurable scrollback.
- Your login shell — the one set on your account, so a `chsh` to fish or
  anything else takes effect in the next pane, not the next macOS login.
- **Scrolling that reaches every program.** Wheel and trackpad move through the
  scrollback, smoothly. Programs that track the mouse get the wheel as mouse
  events, and full-screen ones like `less` and `vim` get it as arrow keys;
  `shift` always scrolls botttle's own history. `shift-pageup` / `pagedown` /
  `home` / `end` page through it, and a thumb shows where you are.
- Mouse selection — click, double-click for words, triple-click for lines.
  **Selecting copies**, no `⌘C` needed (it can be turned off).
- Every character keeps to its cell. Symbols and icons from a fallback font
  can't push the rest of a row out of line, and icons the terminal font lacks
  are drawn from an installed Nerd Font, so Starship and similar prompts work
  without switching fonts.
- Window title tracking (OSC 0/2), clipboard escapes (OSC 52), and color queries.
- Block, bar, and underline cursors; a hollow cursor in unfocused panes.

**Workspace**

- Tabs, each holding its own pane layout, in a strip across the top or a
  collapsible sidebar down the left.
- **Rename a tab** by double-clicking it, `⌘⇧R`, or File → Rename Tab… . A
  name sticks however the program retitles the pane; clear it to go back.
- Multiple windows from `⌘N`, File → New Window, or the Dock icon's menu. Each
  new one steps out from the last instead of landing on top of it.
- Splits to any depth, with gutters you drag to resize. Splitting along an axis a pane already occupies adds a
  sibling instead of nesting, so repeated splits stay flat.
- New tabs and splits open in the current pane's working directory, read from the
  PTY's foreground process group.

**Built for coding CLIs**

- **`shift-enter` is a newline**, not a send. Terminals have always sent the same
  byte for both, so botttle sends `ESC CR` — the sequence Claude Code's own
  `/terminal-setup` installs into other terminals.
- **`ctrl-v` pastes an image** by writing it to a file and typing the path, which
  is how Claude Code and Codex take images. With no image on the clipboard,
  `ctrl-v` reaches the program untouched.
- **Drop a file on a pane** and its path is typed at the prompt, quoted — the
  flow both CLIs document for attaching an image that is already on disk.
- **`⌘-click` opens a URL or a path.** Holding `⌘` underlines the target first.
  Paths resolve against the pane's working directory, and a `file.rs:42:9`
  position suffix opens the file. Links programs attach behind their text
  (OSC 8) — Claude Code, `ls --hyperlink`, `gh`, `delta` — open too.

**Appearance**

- 12 themes — six families in light and dark — swapped live.
- App icon styles: Default follows the active light or dark theme; Bright stays fixed. The choice also updates the macOS Dock icon.
- **Transparency and blur.** Dial the window's opacity down and the desktop shows
  through; turn on blur and macOS frosts what is behind it. Chrome stays more
  solid than the grid so tabs and status text hold up over anything.
- A settings screen for fonts, sizes, line height, ligatures, cursor, and
  background, persisted as plain JSON.

Not there yet: IME composition, search, the kitty keyboard protocol, and the
agent layer itself.

## Installation

### macOS

```bash
git clone https://github.com/launchdoor-studio/botttle
cd botttle
scripts/bundle-macos.sh --install
```

This builds a release `botttle.app`, gives it the icon, signs it with the best
identity in your keychain, and copies it to `/Applications`. Because the bundle
identifier and signing identity are stable, permissions you grant it survive a
rebuild.

Signing prefers a Developer ID Application certificate, then an Apple Development
certificate, then an ad-hoc signature — all three run on the machine that built
them. Pass `--sign "<identity>"` to choose one, or `--no-sign` to skip it.

To hand the app to someone else, Gatekeeper wants it notarized:

```bash
scripts/bundle-macos.sh --notarize --install
```

That submits the signed zip through [`asc`](https://github.com/rorkai/asc) using
the App Store Connect key already in your keychain, staples the ticket, and
prints the Gatekeeper verdict. The Developer ID Application certificate itself
has to be created once by hand at
<https://developer.apple.com/account/resources/certificates/add> — the App Store
Connect API refuses it with *"This operation can only be performed by the Account
Holder."*

### From source

```bash
cargo run --release
```

Needs a recent stable Rust toolchain. On Linux, GPUI also needs the usual Wayland
or X11 development packages.

## Keybindings

`⌘` on macOS; `ctrl+shift` elsewhere.

| Chord | Action |
| --- | --- |
| `⌘N` | New window |
| `⌘T` | New tab |
| `⌘W` | Close pane — closes the tab with the last pane |
| `⌘⇧W` | Close tab |
| `⌘⇧R` / double-click a tab | Rename tab — `enter` keeps it, `esc` cancels |
| `⌘D` / `⌘⇧D` | Split right / split down |
| `⌘]` / `⌘[` | Focus next / previous pane |
| `⌘⇧]` / `⌘⇧[` | Next / previous tab |
| `ctrl-tab` / `ctrl-⇧-tab` | Next / previous tab |
| `⌘C` / `⌘V` | Copy selection / paste — an image if the clipboard holds one |
| `ctrl-V` | Paste a clipboard image as a file path |
| drag & drop | Type the dropped files' paths at the prompt |
| `shift-enter` | Newline instead of send |
| `⌘-click` | Open the URL or path under the pointer |
| `⇧PageUp` / `⇧PageDown` | Scroll back / forward a page |
| `⇧Home` / `⇧End` | Top / bottom of the scrollback |
| `⇧`-scroll | Scroll the scrollback even when a program has the mouse |
| `⌘K` | Clear |
| `⌘=` / `⌘-` / `⌘0` | Font size |
| `⌘B` | Show / hide the tab sidebar |
| `⌘,` | Settings — `esc` closes |

Apart from `ctrl-tab` and the `shift`-scrolling keys outside full-screen
programs, every other key reaches the program untouched — `ctrl-c`,
`ctrl-d`, `ctrl-z` and friends included.

## Configuration

`⌘,` opens the settings screen; every change is written straight to
`~/.config/botttle/settings.json`. The file is plain JSON and can be edited by
hand — unknown and missing keys fall back to defaults.

| Key | Type | Default | Notes |
| --- | --- | --- | --- |
| `theme` | string | `"Botttle Dark"` | Any name from [Themes](#themes) |
| `icon_style` | `"default"` \| `"bright"` | `"default"` | Default follows the theme's light or dark |
| `terminal_font_family` | string \| null | `null` | `null` picks the best monospace font installed |
| `terminal_font_size` | number | `13.0` | 6–40 |
| `line_height` | number | `1.4` | Multiple of the font size, 1.0–2.5 |
| `ligatures` | bool | `true` | Programming ligatures, where the font has them |
| `ui_font_family` | string \| null | `null` | Tabs, status bar, settings |
| `ui_font_size` | number | `12.0` | 8–24 |
| `background` | string \| null | `null` | `"#rrggbb"` override for the window and terminal grounds |
| `opacity` | number | `1.0` | 0.3–1.0; below 1 the desktop shows through |
| `blur` | bool | `false` | Frost what is behind the window; needs `opacity` below 1 |
| `blur_radius` | number | `20` | 1–64 points |
| `tab_layout` | `"horizontal"` \| `"vertical"` | `"horizontal"` | Strip across the top, or sidebar |
| `show_sidebar` | bool | `true` | With the sidebar layout |
| `glass_sidebar` | bool | `false` | Let the blur show through the sidebar |
| `cursor_shape` | `"block"` \| `"bar"` \| `"underline"` | `"block"` | |
| `scrollback_lines` | number | `10000` | Applies to panes opened afterwards |
| `paste_images` | bool | `true` | `ctrl-v` writes a clipboard image to a file |
| `shift_enter_newline` | bool | `true` | Send `ESC CR` for `shift-enter` |
| `copy_on_select` | bool | `true` | Copy a selection as soon as it is made |

## Themes

Botttle, Gruvbox, One, Cursor, OpenCode, and VS Code, each in a light and a dark
variant — `"Gruvbox Dark"`, `"One Light"`, and so on. Gruvbox, One, and VS Code
use those projects' published terminal palettes; Cursor and OpenCode are
approximations matched by eye.

A background override replaces the window and terminal grounds while leaving the
chrome on the theme's own surfaces, so the tab strip stays readable.

## Architecture

The grid is drawn as styled text, row by row, with per-cell colors and
attributes applied as text highlights, which keeps the terminal inside GPUI's
normal layout and paint path rather than beside it. Each row is split into
segments sized in whole cells — runs of ASCII, and runs of anything else — so a
glyph from a fallback font can't shift the columns after it. Emulation runs on its own
thread — `alacritty_terminal` owns the PTY and the parser — and reaches the UI
thread over a channel, so a program flooding stdout can't stall the window.

```
crates/botttle/src
├── main.rs            window setup, cascading, and app wiring
├── workspace.rs       root view: titlebar, tabs and sidebar, renaming, actions
├── pane.rs            the pane tree — split, close, collapse, render
├── actions.rs         actions and their default key bindings
├── menus.rs           the macOS menu bar and Dock menu
├── glass.rs           window transparency and blur
├── settings.rs        user settings, persisted as JSON
├── settings_view.rs   the settings screen
├── assets.rs          the logo, compiled into the binary
├── theme/             palettes, fonts, sizing — resolved into one global
└── terminal/
    ├── mod.rs         PTY + emulator, and the bridge to the main thread
    ├── view.rs        grid rendering, keyboard, mouse, selection, links
    ├── keys.rs        keystrokes to terminal byte sequences
    ├── hyperlink.rs   finding the URL or path under the pointer, and OSC 8 links
    ├── shell.rs       the login shell, and quoting paths for it
    ├── image_paste.rs clipboard images to files on disk
    ├── cwd.rs         where a new pane starts
    └── color.rs       ANSI colors to GPUI colors
```

## Development

```bash
cargo run                      # debug build
cargo test                     # unit tests, plus an end-to-end pty test
cargo clippy --all-targets
scripts/bundle-macos.sh --debug   # a .app around the debug binary
```

The test suite covers key encoding, link detection, row layout, theme resolution, settings
round-trips, and working-directory lookup, and includes an end-to-end check that
a real shell's output reaches the grid we render from.

## Website

`site/index.html` is the project page — a single self-contained file with the
logo and every asset inlined, so it can be served from anywhere:

```bash
python3 -m http.server --directory site
```

## Acknowledgements

botttle stands on [GPUI](https://gpui.rs) for rendering and windowing, and on
[`alacritty_terminal`](https://github.com/alacritty/alacritty) — Alacritty's
emulator, extracted as a library — for turning bytes into a grid.

## License

MIT — see [LICENSE](./LICENSE).
