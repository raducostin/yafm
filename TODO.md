# To do

Deferred minor issues found in reviews. None loses data.

## Keys and configuration
- A binding like `S-b` or `S-tab` is accepted but never fires (lfm rejected `S-<char>`).
- `copy_file = "F5"` (string instead of list) reports a misleading error.
- Duplicate lines in imported lfm files are dropped without a warning.
- More than 10 PowerCLI favourites are cut off without a warning.
- An unknown section in history.json discards the whole history.
- Terminals that send `^H` for Backspace get C-h (dotfiles toggle).
- The start directories are checked only for being directories, not for being enterable.

## File operations
- A move whose copy skipped or failed a child reports "Directory not empty" for each parent.
- A read error part-way through a directory can drop entries from a copy without an error.
- Children of a directory that could not be created each report their own error.
- Sorting by date uses whole seconds.

## Screen
- `@` on `..` opens the command dialog and saves the command although nothing runs.
- A file name containing a control character (tab) shifts its row by one column.
- Two places ignore a terminal resize until the next one (go-to-first-letter, directory sizes).
- Bookmarks and history store paths as UTF-8; non-UTF-8 directories cannot be returned to.

## PowerCLI
- `re_sub` group references (`$1`) cannot be written: inside `{...}` every `$` names a variable (lfm's rule).
- `$f`, `$s`… carry file names as UTF-8 text; non-UTF-8 names reach commands with replacement characters.

## Archives and search
- If an archive's temp folder disappears, the tab leaves it but still thinks it is inside.
- Find results with trailing spaces in names are trimmed (as in lfm).
- A grep result whose file name contains `:12:` picks the first match.
- Archive rebuild keeps permissions but not timestamps or owner.
- Rebuilding a panelized folder rewrites every file (their times change).

Deferred items from reviews are tracked here so they are not lost.
