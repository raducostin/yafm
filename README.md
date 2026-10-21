# yafm — Yet Another File Manager

yafm is a two-pane terminal file manager for Linux, a Rust port of
**lfm — Last File Manager 3.1** by Iñigo Serna. It keeps lfm's screens,
dialogs and default hotkeys, and ships as one binary with no Python needed.

- 1-pane or 2-pane view, up to 4 tabs per pane, function-key bar
- copy / move / delete / rename / links / permissions / owner, with
  "overwrite? Yes / All / No / NOne / Stop" prompts and Ctrl-C to stop
- browse archives as folders (`.tar.gz .tgz .tar.bz2 .tar.xz .tar.lz .tar.lz4 .tar .zip .rar .7z`),
  and optionally rebuild them when you leave
- find / grep with Go, Panelize, View, Edit, Do
- directory tree, file info, filesystems info, diff with backup
- compress / uncompress, filters, sorting, bookmarks, history
- PowerCLI: a command line that runs a command for every selected file,
  with `$f`-style variables and `{...}` expressions
- colour themes and key bindings in TOML; wide (CJK) and non-UTF-8 names work

See **INSTALL.md** for requirements and installation.

## Starting

```
yafm [left-dir] [right-dir]
```

Options: `--check-config` (load the config files, print problems),
`--import-legacy [--force]` (import lfm's `~/.config/lfm` files),
`--restore-config`, `--restore-keys`, `--restore-theme`, `--delete-history`,
`--cwd-file FILE` (see *Quit and cd*), `-d` (debug log), `--help`, `--version`.

The terminal must be at least 66 columns wide.

## Keys

Default bindings (`~/.config/yafm/keys.toml`; `C-` = Ctrl, `A-` = Alt, `S-` = Shift).
The bottom bar always shows what F1–F10 (and F11/F12 when bound) do.

| Keys | Action | Name in keys.toml |
|---|---|---|
| `up`, `k` | Move cursor up | `cursor_up` |
| `down`, `j` | Move cursor down | `cursor_down` |
| `pageup`, `backspace`, `C-p` | Page up | `cursor_pageup` |
| `pagedown`, `C-n` | Page down | `cursor_pagedown` |
| `C-up` | Move up 10 | `cursor_up10` |
| `C-down` | Move down 10 | `cursor_down10` |
| `home`, `C-a` | First entry | `cursor_home` |
| `end`, `C-e` | Last entry | `cursor_end` |
| `C-s` | Go to file whose name contains… | `cursor_goto_file` |
| `A-s` | Go to next file starting with the next key | `cursor_goto_file_1char` |
| `left` | Parent directory (leaves an archive at its root) | `dir_up` |
| `right`, `enter` | Enter dir / archive, or open the file | `dir_enter` |
| `g` | Go to directory… | `goto` |
| `b` | Go to bookmark 0-9 a-z | `bookmark_goto` |
| `B` | Set bookmark 0-9 a-z | `bookmark_set` |
| `C-d` | Choose bookmark from a list | `bookmark_select_fromlist` |
| `C-y` | Return to a previous directory | `history_select_fromlist` |
| `tab` | Other pane | `pane_change_focus` |
| `=` | Show this directory in the other pane | `pane_other_tab_equal` |
| `C-u`, `,` | Swap panes | `panes_swap` |
| `.` | Toggle 1 or 2 panes | `panes_cycle_view` |
| `C-r` | Reload | `refresh` |
| `A-r` | Redraw screen | `redraw_screen` |
| `C-h` | Show/hide dotfiles | `dotfiles_toggle` |
| `C-f` | Edit filter (globs to hide, !glob to keep) | `filters_edit` |
| `s` | Sort the active pane: (o)none (n)ame (e)xt (s)ize (d)ate, uppercase reverses | `sort_files` |
| `#` | Show size of selected/all directories | `show_dirs_size` |
| `spc` | Show size of the directory under the cursor | `show_current_dir_size` |
| `H` | Sizes: human-readable (1.5K, 760K, 11G) or exact bytes | `toggle_human_sizes` |
| `:` | New tab | `tab_new` |
| `!` | Close tab | `tab_close` |
| `<` | Tab to the left | `tab_left` |
| `>` | Tab to the right | `tab_right` |
| `ins` | Select item, cursor down | `select` |
| `+` | Select group… | `select_glob` |
| `-` | Deselect group… | `deselect_glob` |
| `*` | Invert selection | `select_invert` |
| `F2` | Rename | `rename_file` |
| `F3` | View (pager) | `view_file` |
| `F4` | Edit (editor) | `edit_file` |
| `F5` | Copy | `copy_file` |
| `F6` | Move | `move_file2` |
| `F7` | Make directory | `make_dir` |
| `F8` | Delete | `delete_file` |
| `@` | Run a command on file(s) | `exec_on_file` |
| `t` | Touch file | `touch_file` |
| `l` | Create symlink | `link_create` |
| `L` | Edit symlink | `link_edit` |
| `i` | File information | `show_file_info` |
| `/` | Find / grep | `find_grep` |
| `C-t` | Directory tree | `show_tree` |
| `F9` | Main menu | `main_menu` |
| `F12` | File menu | `file_menu` |
| `F1`, `h` | Help menu | `help_menu` |
| `C-o` | Open a shell (exit to return) | `open_shell` |
| `C-x` | PowerCLI command line | `toggle_powercli` |
| `q`, `F10` | Quit (and cd there, see below) | `quit_chdir` |
| `C-q` | Quit without cd | `quit_nochdir` |

Menus: **F9** main menu (find/grep, directory sizes, sort, tree, filesystems
info, shell, edit configuration / keys / theme, delete history), **F12** file
menu (exec, info, permissions, owner/group, backup, diff with backup,
compress/uncompress, uncompress here / in other pane, compress directory),
**h** help (this README, news, to-do list, licence, key bindings dump).

### In text fields

Enter accepts, Esc or C-c cancels, Tab moves between fields and buttons.
Editing: C-a / C-e start / end, C-b / C-f, C-p / C-n word left / right,
C-q / C-r delete word left / right, C-w clear, C-h delete to start, C-k delete
to end, C-z undo all, Ins overwrite mode, Up / Down history, C-g history list.
For file names: C-v current file, C-s current dir, C-o other pane dir,
C-d bookmarks, C-y previous dirs, C-t complete.

## PowerCLI

Ctrl-X opens a command line on the status row; Enter runs it, Ctrl-X hides it
(keeping the text), Ctrl-C clears it. The command runs once for each selected
file (or the file under the cursor), in the current directory.

- `&` at the end: run in the background (no output, returns at once)
- `$` at the end: run with the terminal (for `less`, `vim`…)
- otherwise output is captured and offered in a viewer
- Tab / C-t complete program names and paths; C-g lists stored commands
  (`[powercli] favs` in config.toml) and history

Variables: `$f` file name, `$F` full path, `$E` name without extension,
`$e` extension, `$p` current dir, `$o` other pane dir, `$s` selected
names (quoted), `$a` all names (quoted), `$i` index in the selection,
`$tm $ta $tc $tn` modification / access / change time / now,
`$dm $da $dc $dn` the same as dates, `$b0`…`$bz` bookmarks.
Quote file names yourself: `cp "$f" "$o"`.

`{...}` is an expression (Rhai, a small scripting language) evaluated per file;
variables are available inside. Helpers modelled on Python:
`lower upper title capitalize strip replace(a, b) startswith endswith
split(sep) slice(a, b) zfill(n) re_sub(regex, repl)`, `n.pad(width)` for
numbers, `$tm.strftime("%Y%m%d")`, and `fmt("%02d %s", [a, b])`.
Single-quoted strings work too. Examples:

```
mv "$f" "{$f.lower().replace('.bak', '.orig')}"
cp "$f" "$o/{fmt("%2.2d. %s - %s", [$i, $tm.strftime("%Y%m%d"), $E.upper()])}.jpg"
mv "$f" "{if $e == ".jpeg" { $E + ".jpg" } else { $f }}"
```

## Configuration

Files live in `~/.config/yafm/` (or `$XDG_CONFIG_HOME/yafm/`):

| File | Contents |
|---|---|
| `config.toml` | options, confirmations, programs (pager, editor, shell, viewers), file-type colours, bookmarks, PowerCLI favourites |
| `keys.toml` | key bindings, `action = ["key", ...]` |
| `theme.toml` | colours, `item = "fg bg"` or `"=other_item"`; `*` makes a colour bright |
| `history.json` | dialog histories |
| `yafm.log` | log of the last run |

Edit them from the main menu (F9 → c / k / e); yafm reloads them and reports
problems. Notable options: `show_dotfiles`, `sort_type`, `rebuild_vfs`,
`ask_rebuild_vfs`, `show_output_after_exec`, `show_function_keys`,
`human_readable_sizes`. `[panes.left]` and `[panes.right]` keep each pane's sort
(`sort_type`, `sort_reverse`); `options.sort_type` is only the starting sort.

**Coming from lfm:** on the first run yafm imports `~/.config/lfm/lfm.ini`,
`lfm.keys` and `lfm.theme` (they are never changed). lfm's history file
cannot be read and is not imported. Run `yafm --import-legacy --force` to
import again.

## Quit and cd

`q` quits and can leave your shell in yafm's current directory. Add to
`~/.bashrc` or `~/.zshrc`:

```sh
yafm() {
    local f d
    f="$(mktemp)" || return
    command yafm --cwd-file "$f" "$@"
    d="$(cat "$f")"; rm -f "$f"
    if [ -n "$d" ] && [ -d "$d" ]; then cd "$d"; fi
}
```

`C-q` quits without changing directory.

## Differences from lfm

- Configuration is TOML in `~/.config/yafm` (lfm's INI files are imported once).
- PowerCLI `{...}` is Rhai instead of Python; nested `{ }` blocks are allowed.
- Quit-and-cd uses `--cwd-file` instead of `/tmp/lfm-PID.path`.
- Sort key `e` sorts by extension (lfm sorted by name); Esc cancels text fields.
- In 1-pane view Tab switches the visible pane, so keys never act on a hidden pane.
- Copy / move never delete or overwrite a file that was not copied successfully;
  copying a file onto itself and moving a directory into itself are refused.
- Archive rebuild writes a new archive first and keeps your changes if it fails;
  leaving nested archives with "rebuild" rebuilds the whole chain.
- `=` is refused inside an archive; a new tab opened in an archive starts
  next to the archive.
- Directory sizes match `du -sb` of current coreutils (directories count 0).
- A function-key bar (`show_function_keys = false` hides it).
- `F1` also opens the Help menu (lfm showed "F1=Help" but bound only `h`).
- `Space` sizes just the directory under the cursor (`#` sizes all or the selected ones);
  in lfm Space was a third Page down key (`pagedown` and `C-n` still are).
- Sizes are human-readable by default (`1.5K`, `760K`, `11G`); `H` switches to exact
  bytes and the column widens to fit. lfm showed bytes, then K/M for big files.
- Each pane has its own sort (`s` sorts the active pane), kept across restarts and
  shown on the pane's bottom border (`Sort: date ↓`, ↓ = reversed; in the status line
  in the one-pane view). lfm had one sort for both panes.

## Licence

yafm © 2026 Radu Petean <radu@petean.ro>.
GPL-3.0-or-later, like lfm. See COPYING. lfm © 2001-17 Iñigo Serna.
