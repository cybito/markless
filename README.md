# Markless

Markless is a terminal markdown viewer and editor with image support. It is focused on fast navigation, clear rendering, and sensible defaults for long documents.

## Screenshots

![Table of Contents sidebar](<images/table of contents.png>)
![Directory browse mode](<images/browser.png>)
![Inline image support using Kitty](<images/image support.png>)
![Code syntax highlighting](<images/code highlighting.png>)
![Table Support](images/tables.png)
![Editor mode](images/editmode.png)

## Features

- Markdown rendering with headings, lists, tables, block quotes, code blocks, footnotes, task lists, and description lists
- Syntax-highlighted code blocks with lazy highlighting for performance
- Inline images (Kitty, Sixel, iTerm2, and half-block fallback) with auto-detection
- Mermaid diagram rendering (SVG rasterized inline, click to open full SVG)
- LaTeX math rendering — inline `$...$` as Unicode, display `$$...$$` and ` ```math ``` ` as images via Typst
- CSV files and ` ```csv ``` ` code blocks rendered as formatted tables
- Binary file viewing with hex dump display
- Built-in editor mode with save, undo-friendly exit, and conflict detection
- External editor support (`--editor vim`, `--editor hx`, etc.)
- Directory browse mode with file preview
- Table of contents sidebar with keyboard and mouse support
- Search with incremental matching, match count, and highlight
- Anchor and footnote navigation within documents
- File watching for live reload
- Link hover and click (including image placeholders and rendered diagrams)
- Text selection with mouse drag and copy (pbcopy on macOS, OSC 52 fallback)
- Opens image files directly (png, jpg, gif, webp, svg, avif, and more)
- Auto theme detection via OSC 11 terminal query
- Fast scrolling with stable layout and reflow on resize

## Installation

### Development (from source)

```bash
cargo install --path .
```

### Production (from crates.io)

```bash
cargo install markless
```

## Usage

```bash
markless README.md          # View a file
markless .                  # Browse current directory
markless                    # Browse current directory (default)
markless src/               # Browse a directory
```

When given a directory, markless opens in browse mode: the sidebar shows the file listing and the first markdown file (or first file) is previewed automatically. Navigate with arrow keys, press Enter to open, and Backspace to go to the parent directory.

## Command Line Options

- `--watch`  Auto-reload on file changes
- `--no-mouse-select`  Disable markless mouse selection capture
- `--mouse-select`  Re-enable markless mouse selection capture (overrides saved `--no-mouse-select`)
- `--no-toc`  Hide the table of contents sidebar
- `--toc`  Start with TOC visible
- `--no-images`  Disable inline image rendering (show placeholders only)
- `--image-mode <kitty|sixel|iterm2|halfblock>`  Force a specific image rendering protocol
- `--force-half-cell`  Force half-cell image rendering (shortcut for `--image-mode halfblock`)
- `--wrap-width <COLS>`  Maximum content width for word-wrapping
- `--theme <auto|light|dark>`  Force highlight theme background (`auto` queries the terminal via OSC 11)
- `--no-inline-math`  Render inline math as images instead of Unicode text
- `--inline-math`  Re-enable inline Unicode math (overrides saved `--no-inline-math`)
- `--perf`  Enable startup performance logging
- `--render-debug-log <PATH>`  Write render/image debug events to a file
- `--editor <cmd>`  Use an external editor instead of the built-in one (e.g. `--editor vim`, `--editor "emacsclient -t"`)
- `--no-editor`  Disable the external editor (use the built-in editor)
- `--save`  Save current flags as defaults in the global config
- `--clear`  Clear saved defaults in the global config

Config files:
- Global (macOS): `~/Library/Application Support/markless/config`
- Local override: `.marklessrc` in the current directory

## Key Bindings

Navigation
- `j` / `k` or arrows: scroll
- `Space` / `PageDown`: page down
- `b` / `PageUp`: page up
- `d` / `u`: half page
- `g` / `G`: top / bottom

Search
- `/`: start search
- `Enter`: next match
- `Esc`: clear search

TOC
- `t`: toggle TOC
- `T`: toggle + focus TOC
- `Tab`: switch focus
- `j` / `k`, arrows, `Enter` / `Space`: navigate + jump
- `h` / `Left`: collapse (parent directory in browse mode)
- `l` / `Right`: expand (enter directory in browse mode)

Browse
- `B`: enter directory browse mode
- `F`: return to file-only mode (heading TOC)
- `Backspace`: navigate to parent directory (in TOC)

Editor
- `e`: enter edit mode
- `Ctrl-e`: toggle edit mode (enter or exit)
- `Esc`: return to view mode
- `Ctrl-s`: save file
- Arrows, Home/End: navigate
- `Ctrl+Left` / `Ctrl+Right`: word movement
- `Ctrl+Home` / `Ctrl+End`: buffer start / end
- PageUp/PageDown: scroll editor

Other
- `w`: toggle watch
- `r` / `R`: reload file
- `o`: open visible links (1-9)
- `?` / `F1`: toggle help
- `q` / `Ctrl-c` / `Ctrl-q`: quit

Mouse
- Scroll wheel: scroll
- Click links or images: open
- Hover link/image: show URL
- Click + drag: select text and copy on release
- Shift + click: copy the entire displayed line
- Alt + click: copy the paragraph between displayed blank lines

文本选区按终端显示的 grapheme 边界复制；十六进制段落复制包含尚未缓存的行，不受滚动位置影响。

## ImeKit integration (custom branch)

This branch reports input intent to ImeKit through
one persistent connection. Reading, navigation, help, and link selection report
`command`; search and the built-in editor report `text`. Search stays in text
mode after Enter, until Esc closes it. Text mode releases temporary English; it
does not force a particular input method.

On supported local interactive terminals, markless automatically connects to
`~/.local/state/infra-as-code/ime-control/run/control.sock`. SSH and non-graphical
sessions bypass direct integration. With `HERDR_IME_INTENT=1`, it instead reports
to Herdr using `HERDR_ENV=1`, a trusted `HERDR_SOCKET_PATH`, and exactly one of
`HERDR_PANE_ID` or `HERDR_IME_POPUP_TERMINAL_ID`. Invalid Herdr configuration never
falls back to switching an input source on the server host.

Focus loss and exit release the declaration. Before launching an external editor,
markless suspends its declaration; after the editor returns, a real key or focus
event resumes reporting the current mode. Run this customized binary directly;
it obtains its own mode lease rather than relying on an `ime-guard` entry lease.

IME support is optional. Connection, protocol, or service failures silently disable
it for the rest of the process, while normal input and editor behavior continue.
The application never switches the system input source itself. Herdr's `recorded`
acknowledgment confirms recorded intent only, not a completed local source switch.

## License

MIT
