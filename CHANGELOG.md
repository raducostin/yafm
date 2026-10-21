# News

## 0.1.3 — 2026-10-21

- Each pane has its own sort: `s` sorts the active pane, the sort is kept across restarts (`[panes.left]`, `[panes.right]` in `config.toml`) and shown on the pane's bottom border, e.g. `Sort: date ↓`.

## 0.1.2 — 2026-10-21

- Sizes are human-readable by default (`1.5K`, `760K`, `11G`); `H` toggles exact bytes (option `human_readable_sizes`, saved at exit).

## 0.1.1 — 2026-10-21

- `F1` opens the Help menu as well as `h`; the status line shows `F1=Help`.
- `Space` shows the size of the directory under the cursor (`show_current_dir_size`); it no longer pages down (`pagedown`, `C-n` still do).

## 0.1.0 — 2026-10-20

First release of yafm, a Rust port of lfm 3.1.

- Phase 1: key bindings, TOML configuration, theme, history, import of lfm's files, command-line options.
- Phase 2: listing, sorting, filters, file operations with confirmations, external commands.
- Phase 3: the two-pane screen, tabs, every lfm key, dialogs, bookmarks, function-key bar.
- Phase 4: archives as folders, compress/uncompress, find/grep with panelize, tree, file info, permissions/owner, diff with backup.
- Phase 5: PowerCLI with Rhai expressions, quit-and-cd, built-in documentation, release packages.
