# Parity with lfm 3.1

Every lfm action (the 66 `@public` functions of `actions.py`) and every
feature of lfm's README, with the yafm evidence: a test name, or a step of
`tests/pty_smoke.py` (run against the real binary in a pseudo-terminal).

## Actions (66)

| Action | Default keys | Status | Evidence |
|---|---|---|---|
| `cursor_up` | `up`, `k` | done | cursor_moves_and_clamps |
| `cursor_down` | `down`, `j` | done | cursor_moves_and_clamps; event_loop_quits_with_path |
| `cursor_pageup` | `pageup`, `backspace`, `C-p` | done | parity_gaps_… (pagedown); App::cursor_page |
| `cursor_pagedown` | `pagedown`, `C-n` (Space moved to `show_current_dir_size`) | done | parity_gaps_… |
| `cursor_up10` | `C-up` | done | parity_gaps_lists_sizes_backup_compress_history |
| `cursor_down10` | `C-down` | done | parity_gaps_… |
| `cursor_home` | `home`, `C-a` | done | cursor_moves_and_clamps |
| `cursor_end` | `end`, `C-e` | done | cursor_moves_and_clamps |
| `cursor_goto_file` | `C-s` | done | cursor_moves_and_clamps (goto_name_containing) |
| `cursor_goto_file_1char` | `A-s` | done | parity_gaps_… (A-s b) |
| `dir_up` | `left` | done | enter_and_leave_dirs_with_history; enter_archive_with_enter_and_leave_with_left |
| `dir_enter` | `right`, `enter` | done | enter_archive_with_enter_and_leave_with_left; pty smoke |
| `goto` | `g` | done | bookmarks_goto_and_filters |
| `bookmark_goto` | `b` | done | bookmarks_goto_and_filters |
| `bookmark_set` | `B` | done | bookmarks_goto_and_filters; bookmark_in_vfs_refused |
| `bookmark_select_fromlist` | `C-d` | done | parity_gaps_… (C-d 7) |
| `history_select_fromlist` | `C-y` | done | parity_gaps_… (C-y) |
| `pane_change_focus` | `tab` | done | panes_and_tabs; tab_in_full_view_moves_the_full_view_with_the_focus |
| `pane_other_tab_equal` | `=` | done | panes_and_tabs |
| `panes_swap` | `C-u`, `,` | done | panes_and_tabs |
| `panes_cycle_view` | `.` | done | panes_and_tabs; full_mode_shows_perms_and_owner |
| `refresh` | `C-r` | done | refresh_keeps_cursor_and_selection_by_name |
| `redraw_screen` | `A-r` | done | dispatch → Screen::clear (pty smoke) |
| `dotfiles_toggle` | `C-h` | done | sort_dotfiles_filters_bookmarks |
| `filters_edit` | `C-f` | done | bookmarks_goto_and_filters |
| `sort_files` | `s` | done (per pane: yafm extra) | sort_key_e_sorts_by_extension; help_and_menus (F9 s S); sort_changes_only_the_active_pane, pane_sorts_survive_save_and_restart |
| `show_dirs_size` | `#` | done | parity_gaps_… (#) |
| `show_current_dir_size` | `spc` | yafm extra | space_sizes_only_the_dir_under_cursor |
| `toggle_human_sizes` | `H` | yafm extra | shift_h_toggles_human_readable_sizes, size_column_human_by_default_bytes_when_toggled |
| `tab_new` | `:` | done | panes_and_tabs; vfs_temp_dirs_stay_out_of_histories |
| `tab_close` | `!` | done | panes_and_tabs; tabs_errors_show_dialogs |
| `tab_left` | `<` | done | panes_and_tabs |
| `tab_right` | `>` | done | panes_and_tabs |
| `select` | `ins` | done | selection_rules |
| `select_glob` | `+` | done | links_and_select_glob |
| `deselect_glob` | `-` | done | selection_rules (select_glob false) |
| `select_invert` | `*` | done | selection_rules |
| `rename_file` | `F2` | done | rename_rules |
| `view_file` | `F3` | done | view_edit_shell_suspend_the_screen |
| `edit_file` | `F4` | done | view_edit_shell_suspend_the_screen |
| `copy_file` | `F5` | done | copy_and_delete_with_confirmation |
| `move_file2` | `F6` | done | move_and_overwrite_prompt |
| `make_dir` | `F7` | done | make_dir_and_touch; pty smoke |
| `delete_file` | `F8` | done | copy_and_delete_with_confirmation; pty smoke |
| `exec_on_file` | `@` | done | nothing_happens_on_parent_entry (@ on ..) |
| `touch_file` | `t` | done | make_dir_and_touch |
| `link_create` | `l` | done | links_and_select_glob |
| `link_edit` | `L` | done | simple_ops (link_edit); dispatch |
| `show_file_info` | `i` | done | tree_and_infos_open; pty smoke |
| `find_grep` | `/` | done | find_then_go; grep_panelize_shows_matches; pty smoke |
| `show_tree` | `C-t` | done | tree_and_infos_open; pty smoke |
| `main_menu` | `F9` | done | help_and_menus; parity_gaps_… (F9 h) |
| `file_menu` | `F12` | done | perms_and_compress_and_diff |
| `help_menu` | `F1`, `h` | done | help_and_menus (F1 is a yafm extra) |
| `open_shell` | `C-o` | done | view_edit_shell_suspend_the_screen |
| `toggle_powercli` | `C-x` | done | echo_current_file_shows_output … (6 PowerCLI tests); pty smoke |
| `quit_chdir` | `q`, `F10` | done | quit_needs_confirmation; pty smoke (--cwd-file) |
| `quit_nochdir` | `C-q` | done | quit_needs_confirmation |
| `move_file` | menu | done | Copy-then-delete move; no default key in lfm either; bulk::move_files tests (move_keeps_sources_that_failed_or_were_skipped) |
| `backup_file` | menu | done | F12 a — parity_gaps_… |
| `change_perms` | menu | done | F12 p — perms_and_compress_and_diff, perms_dialog_toggles_and_applies |
| `change_owner` | menu | done | F12 o — owner_dialog_selects_from_lists (dialog); chown/chgrp command builders in system_lists_and_commands |
| `diff_file_with_backup` | menu | done | F12 d — perms_and_compress_and_diff, diffs |
| `show_filesystems_info` | menu | done | F9 f — tree_and_infos_open |
| `uncompress_dir` | menu | done | F12 x — uncompress via tools::uncompress (same code as u) |
| `uncompress_dir_other_pane` | menu | done | F12 u — uncompress_into_other_pane |
| `compress_dir` | menu | done | F12 c — parity_gaps_… |
| `compress_uncompress_file` | menu | done | F12 z — perms_and_compress_and_diff |

## README features

| lfm feature | Status | Evidence / note |
|---|---|---|
| 1-pane or 2-pane view | done | panes_and_tabs, full_mode_shows_perms_and_owner |
| Tabs (4 per pane) | done | panes_and_tabs |
| File filters | done | dotfiles_filters_and_reverse_size, bookmarks_goto_and_filters |
| Bookmarks 0-9 a-z | done | bookmarks_goto_and_filters, parity_gaps_… |
| History (per tab, per dialog) | done | enter_and_leave_dirs_with_history, history_navigation, save_load_round_trip_and_bad_json |
| VFS for compressed files (nested) | done | enter_and_leave_archive_by_parent, leaving_nested_archives_with_rebuild_packs_the_whole_chain, nested_layers_display_innermost |
| Rebuild VFS on exit | done (safer) | rebuild_packs_changes, failed_rebuild_keeps_archive, failed_rebuild_keeps_the_edits |
| Tree view | done | tree_dialog_moves_and_returns_path, tree_and_infos_open |
| Dialogs with entry completion | done | bookmark_select_and_completion, cli_completes_programs_and_paths |
| PowerCLI: loops, variables, expressions, & and $ modes, history, stored commands | done (Rhai instead of Python) | loops_over_the_selection, variables_in_lfm_order, readme_examples, expression_renames_like_lfm, background_returns_at_once_and_parent_entry_works, cli_history_list_with_favs |
| Fast access to the shell (C-o) | done | view_edit_shell_suspend_the_screen |
| find / grep with Go, Panelize, View, Edit, Do | done | find_then_go, grep_panelize_shows_matches, results_dialog_keys |
| df (filesystems info) | done | tree_and_infos_open |
| Colour files by extension | done | file_kind_uses_categories, cursor_row_uses_cursor_style |
| Customizable themes | done | references_resolve, default_theme_import_equals_builtin |
| Customizable key bindings | done | duplicate_key_first_wins, function_keys_bar_follows_the_keymap |
| Wide chars (East Asian) | done (always on) | wrap_wide_chars, wide_names_keep_columns |
| Configuration auto-discovery of programs on first run | not ported | lfm's README mentions it but lfm 3.1's code has no such step (defaults are fixed in preferences.py); yafm uses the same defaults |
| Quit and cd (shell function) | done (--cwd-file) | writes_the_dir_without_newline, pty smoke |
| `--restore-*`, `--delete-history`, `-d`, `-w` | done | restore_keys_deletes_file, help_lists_cwd_file |
| Help: README, News, Todo, License, Key bindings | done | help_and_menus |
| Edit configuration / keys / theme from the menu | done (reloads + reports problems) | main_menu → edit_config_file |

## Not ported, by design

| lfm | Why |
|---|---|
| Python `{...}` in PowerCLI | yafm has no Python; Rhai with Python-like helpers (README "PowerCLI") |
| `/tmp/lfm-PID.path` | predictable path in a shared directory; `--cwd-file` instead |
| Reading lfm's pickled history | no safe pickle reader; history starts empty |
