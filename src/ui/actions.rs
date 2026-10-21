//! Every keymap action: dialogs, file operations and external programs.
//! Titles, prompts and messages follow lfm's actions.py.

use std::ffi::OsStr;
use std::path::{Path, PathBuf};
use std::time::Duration;

use strum::IntoEnumIterator;

use crate::actions::Action;
use crate::config::history::{History, HistorySection};
use crate::config::settings::BOOKMARK_KEYS;
use crate::config::theme::ColorItem;
use crate::fs::path::get_norm_path;
use crate::jobs::{command_on_file, run_in_background};
use crate::keys::KeyCode;
use crate::ops::bulk;
use crate::ops::contents::format_os_error;
use crate::ops::simple::{self, resolve_destination};
use crate::startup::load_all;
use crate::ui::dialogs::{
    ConfirmDialog, DoubleEntryDialog, EntryDialog, ErrorDialog, GetKeyDialog, ProgressView,
    SelectDialog, ViewDialog,
};
use crate::ui::screen::{event_loop_with, redraw, run_modal, Flow, Input, Screen, UiProgress};
use crate::ui::state::{App, Enter};
use crate::ui::tools;

/// lfm's keys.dump report (preferences.dump_keys_to_file).
pub fn dump_keys(app: &App) -> String {
    let mut by_key = Vec::new();
    let mut by_action = Vec::new();
    let mut missing = Vec::new();
    for a in Action::iter() {
        let name: &str = a.into();
        let keys = app.keymap.keys_for(a);
        if keys.is_empty() {
            missing.push(name);
            continue;
        }
        let mut ks: Vec<String> = keys.iter().map(|k| k.to_string()).collect();
        ks.sort();
        for k in &ks {
            by_key.push(format!("{k:10} ->   {name}"));
        }
        by_action.push(format!("{name:24} ->  {}", ks.join(", ")));
    }
    by_key.sort();
    by_action.sort();
    missing.sort();
    let mut out = format!("{} yafm dump keys {}\n\n", "#".repeat(30), "#".repeat(30));
    out.push_str(&format!(
        "{} Key -> Action {}\n",
        "#".repeat(20),
        "#".repeat(20)
    ));
    out.push_str(&by_key.join("\n"));
    out.push_str(&format!(
        "\n\n{} Action -> Key {}\n",
        "#".repeat(20),
        "#".repeat(20)
    ));
    out.push_str(&by_action.join("\n"));
    if missing.is_empty() {
        out.push_str("\n\nAll actions have key binding\n");
    } else {
        out.push_str("\n\nActions with no keys:\n");
        for m in missing {
            out.push_str(&format!(". {m}\n"));
        }
    }
    out.push('\n');
    out.push_str(&"#".repeat(70));
    out
}

pub(crate) const CONTINUE: std::io::Result<Flow> = Ok(Flow::Continue);

pub(crate) fn error(screen: &mut dyn Screen, app: &mut App, msg: &str) -> std::io::Result<Flow> {
    run_modal(screen, app, ErrorDialog::new(msg))?;
    CONTINUE
}

pub(crate) fn ask_entry(
    screen: &mut dyn Screen,
    app: &mut App,
    title: &str,
    help: &str,
    text: &str,
    section: Option<HistorySection>,
    is_files: bool,
) -> std::io::Result<Option<String>> {
    let history = section
        .map(|s| app.history.entries(s).to_vec())
        .unwrap_or_default();
    run_modal(
        screen,
        app,
        EntryDialog::new(title, help, text, history, is_files),
    )
}

/// Runs `cmd` with `sh -c` in `cwd`, giving it the terminal.
pub(crate) fn run_in_terminal(
    screen: &mut dyn Screen,
    cmd: &OsStr,
    cwd: &Path,
) -> std::io::Result<()> {
    screen.suspend(&mut || {
        let _ = std::process::Command::new("sh")
            .arg("-c")
            .arg(cmd)
            .current_dir(cwd)
            .status();
    })
}

pub(crate) fn program(app: &App, name: &str) -> String {
    app.settings
        .programs
        .get(name)
        .cloned()
        .unwrap_or_else(|| name.to_string())
}

pub(crate) fn run_on_file(
    screen: &mut dyn Screen,
    app: &mut App,
    prog: &str,
    file: &Path,
) -> std::io::Result<Flow> {
    let dir = app.tab().dir().to_path_buf();
    run_in_terminal(screen, &command_on_file(prog, file, false), &dir)?;
    app.refresh_all();
    CONTINUE
}

pub(crate) fn os_msg(e: &std::io::Error, path: &Path) -> String {
    format_os_error(e, &path.display().to_string())
}

pub(crate) fn selected_paths(app: &App) -> Vec<PathBuf> {
    app.tab()
        .selected_or_current()
        .iter()
        .map(|e| e.path.clone())
        .collect()
}

pub(crate) fn clear_selection_and_refresh(app: &mut App) {
    app.tab_mut().selected.clear();
    app.refresh_all();
}

fn goto_or_error(screen: &mut dyn Screen, app: &mut App, path: &Path) -> std::io::Result<Flow> {
    tools::change_dir(screen, app, path)?;
    CONTINUE
}

/// ".." — at the root of a virtual folder this leaves it.
fn dir_up(screen: &mut dyn Screen, app: &mut App) -> std::io::Result<Flow> {
    let at_vfs_root = app
        .tab()
        .vfs
        .last()
        .is_some_and(|l| l.tmp == app.tab().dir());
    if at_vfs_root {
        tools::leave_vfs_root(screen, app)?;
        return CONTINUE;
    }
    match app.dir_up() {
        Ok(()) => CONTINUE,
        Err(msg) => error(screen, app, &msg),
    }
}

/// The real directory holding the outermost archive of the active tab, if in a VFS.
fn outside_vfs_dir(app: &App) -> Option<PathBuf> {
    app.tab().vfs.first().map(|l| {
        l.source
            .parent()
            .map(Path::to_path_buf)
            .unwrap_or_else(|| PathBuf::from("/"))
    })
}

#[derive(Clone, Copy)]
enum Bulk {
    Copy,
    Move,
    Move2,
}

fn copy_move(screen: &mut dyn Screen, app: &mut App, kind: Bulk) -> std::io::Result<Flow> {
    let act = if matches!(kind, Bulk::Copy) {
        "Copy"
    } else {
        "Move"
    };
    let files = selected_paths(app);
    if files.is_empty() {
        return CONTINUE;
    }
    let subtitle = match &files[..] {
        [one] => format!(
            "{act} '{}' to:",
            one.file_name()
                .map(|n| n.to_string_lossy())
                .unwrap_or_default()
        ),
        _ => format!("{act} {} items to:", files.len()),
    };
    let default = format!("{}/", app.other_tab().dir().display()).replace("//", "/");
    let title = format!("{act} file(s)");
    let Some(text) = ask_entry(
        screen,
        app,
        &title,
        &subtitle,
        &default,
        Some(HistorySection::Path),
        true,
    )?
    else {
        return CONTINUE;
    };
    if text.is_empty() {
        return CONTINUE;
    }
    let base = app.tab().dir().to_path_buf();
    let dest = match resolve_destination(&text, &base, files.len(), &act.to_lowercase()) {
        Ok(d) => d,
        Err(msg) => return error(screen, app, &msg),
    };
    if !app.is_vfs_temp(&dest) {
        app.history
            .append(HistorySection::Path, &dest.display().to_string());
    }
    let confirm = app.settings.confirmations.overwrite;
    {
        let mut p = UiProgress::new(screen, app, &title);
        match kind {
            Bulk::Copy => bulk::copy_files(&files, &base, &dest, confirm, &mut p),
            Bulk::Move => bulk::move_files(&files, &base, &dest, confirm, &mut p),
            Bulk::Move2 => bulk::move_files2(&files, &base, &dest, confirm, &mut p),
        };
    }
    clear_selection_and_refresh(app);
    CONTINUE
}

fn delete(screen: &mut dyn Screen, app: &mut App) -> std::io::Result<Flow> {
    let files = selected_paths(app);
    if files.is_empty() {
        return CONTINUE;
    }
    let base = app.tab().dir().to_path_buf();
    let confirm = app.settings.confirmations.delete;
    {
        let mut p = UiProgress::new(screen, app, "Delete file(s)");
        bulk::delete_files(&files, &base, confirm, &mut p);
    }
    clear_selection_and_refresh(app);
    CONTINUE
}

fn rename(screen: &mut dyn Screen, app: &mut App) -> std::io::Result<Flow> {
    let cur = app.tab().current().clone();
    if cur.name == ".." {
        return CONTINUE;
    }
    let name = cur.display_name();
    let help = format!("Rename '{name}' to");
    let Some(newname) = ask_entry(
        screen,
        app,
        "Rename",
        &help,
        &name,
        Some(HistorySection::File),
        true,
    )?
    else {
        return CONTINUE;
    };
    if newname.is_empty() {
        return CONTINUE;
    }
    let dir = app.tab().dir().to_path_buf();
    let dest = if newname.starts_with('/') {
        PathBuf::from(&newname)
    } else {
        dir.join(&newname)
    };
    if dest == cur.path {
        return error(
            screen,
            app,
            &format!("Cannot rename '{name}'\nSource and destination are the same file"),
        );
    }
    if dest.parent() != Some(dir.as_path()) {
        return error(screen, app, "Cannot rename to different directory");
    }
    if std::fs::symlink_metadata(&dest).is_ok() && app.settings.confirmations.overwrite {
        let q = format!("Overwrite '{newname}'?");
        if !run_modal(screen, app, ConfirmDialog::new("Rename file", &q, false))? {
            return CONTINUE;
        }
    }
    if let Err(e) = simple::rename_file(&cur.path, &dest) {
        return error(
            screen,
            app,
            &format!(
                "Cannot rename: '{name}' to '{newname}'\n{}",
                os_msg(&e, &dest)
            ),
        );
    }
    app.history.append(HistorySection::File, &newname);
    app.refresh_all();
    CONTINUE
}

fn create_in_dir(
    screen: &mut dyn Screen,
    app: &mut App,
    dir_not_file: bool,
) -> std::io::Result<Flow> {
    let (title, help, what) = if dir_not_file {
        ("Make directory", "Type directory name", "make directory")
    } else {
        ("Touch file", "Type file name", "touch file")
    };
    let Some(name) = ask_entry(
        screen,
        app,
        title,
        help,
        "",
        Some(HistorySection::File),
        true,
    )?
    else {
        return CONTINUE;
    };
    if name.is_empty() {
        return CONTINUE;
    }
    let path = app.tab().dir().join(&name);
    let res = if dir_not_file {
        simple::make_dir(&path)
    } else {
        simple::touch_file(&path)
    };
    if let Err(e) = res {
        return error(
            screen,
            app,
            &format!("Cannot {what}\n{}", os_msg(&e, &path)),
        );
    }
    app.history.append(HistorySection::File, &name);
    app.refresh_all();
    CONTINUE
}

fn link_create(screen: &mut dyn Screen, app: &mut App) -> std::io::Result<Flow> {
    let here = app.tab().dir().to_path_buf();
    let other = app.other_tab().dir().join(&app.other_tab().current().name);
    let target = other
        .strip_prefix(&here)
        .map(Path::to_path_buf)
        .unwrap_or(other);
    let d = DoubleEntryDialog::new(
        "Create link",
        "Link name",
        "Pointing to",
        "",
        &target.display().to_string(),
        app.history.entries(HistorySection::File).to_vec(),
        app.history.entries(HistorySection::Path).to_vec(),
        true,
    );
    let Some((name, pointto)) = run_modal(screen, app, d)? else {
        return CONTINUE;
    };
    if name.is_empty() {
        return error(
            screen,
            app,
            "Cannot create link\nYou must specify the name for the new link",
        );
    }
    if pointto.is_empty() {
        return error(
            screen,
            app,
            "Cannot create link\nYou must specify the file to link",
        );
    }
    let link = here.join(&name);
    if let Err(e) = simple::link_create(&link, Path::new(&pointto)) {
        return error(
            screen,
            app,
            &format!("Cannot create link\n{}", os_msg(&e, &link)),
        );
    }
    app.history.append(HistorySection::File, &name);
    app.refresh_all();
    CONTINUE
}

fn link_edit(screen: &mut dyn Screen, app: &mut App) -> std::io::Result<Flow> {
    let link = app.tab().current().path.clone();
    let Ok(old) = std::fs::read_link(&link) else {
        return CONTINUE;
    };
    let old = old.display().to_string();
    let Some(new) = ask_entry(
        screen,
        app,
        "Edit link",
        "Link points to",
        &old,
        Some(HistorySection::Path),
        true,
    )?
    else {
        return CONTINUE;
    };
    if new.is_empty() || new == old {
        return CONTINUE;
    }
    if let Err(e) = simple::link_edit(&link, Path::new(&new)) {
        return error(
            screen,
            app,
            &format!("Cannot edit link\n{}", os_msg(&e, &link)),
        );
    }
    app.history.append(HistorySection::Path, &new);
    app.refresh_all();
    CONTINUE
}

fn exec_on_file(screen: &mut dyn Screen, app: &mut App) -> std::io::Result<Flow> {
    let title = "Execute command on file(s)";
    let Some(cmd) = ask_entry(
        screen,
        app,
        title,
        "Enter command",
        "",
        Some(HistorySection::Exec),
        false,
    )?
    else {
        return CONTINUE;
    };
    if cmd.is_empty() {
        return CONTINUE;
    }
    let dir = app.tab().dir().to_path_buf();
    for f in selected_paths(app) {
        run_in_terminal(screen, &command_on_file(&cmd, &f, false), &dir)?;
    }
    app.history.append(HistorySection::Exec, &cmd);
    clear_selection_and_refresh(app);
    CONTINUE
}

fn dir_enter(screen: &mut dyn Screen, app: &mut App) -> std::io::Result<Flow> {
    match app.classify_current() {
        Enter::Parent => dir_up(screen, app),
        Enter::Dir => {
            let path = app.tab().current().path.clone();
            goto_or_error(screen, app, &path)
        }
        Enter::File(kind) => {
            let path = app.tab().current().path.clone();
            match kind.as_str() {
                "archive" => {
                    tools::enter_archive(screen, app, &path)?;
                    CONTINUE
                }
                "audio" | "ebook" | "graphics" | "pdf" | "video" | "web" => {
                    let dir = app.tab().dir().to_path_buf();
                    let _ = run_in_background(
                        &command_on_file(&program(app, &kind), &path, false),
                        &dir,
                    );
                    app.refresh_all();
                    CONTINUE
                }
                _ => {
                    let pager = program(app, "pager");
                    run_on_file(screen, app, &pager, &path)
                }
            }
        }
    }
}

fn ask_bookmark_key(
    screen: &mut dyn Screen,
    app: &mut App,
    title: &str,
    question: &str,
) -> std::io::Result<Option<char>> {
    loop {
        match run_modal(screen, app, GetKeyDialog::new(title, question))? {
            None => return Ok(None),
            Some(c) if BOOKMARK_KEYS.contains(c) => return Ok(Some(c)),
            Some(_) => {}
        }
    }
}

/// Sizes the directory under the cursor (yafm extra); does nothing on files and "..".
fn show_current_dir_size(screen: &mut dyn Screen, app: &mut App) -> std::io::Result<Flow> {
    let cur = app.tab().current();
    if !cur.is_dir() || cur.name == ".." {
        return CONTINUE;
    }
    let dirs = vec![(cur.name.clone(), cur.path.clone())];
    size_dirs(screen, app, &dirs)
}

fn show_dirs_size(screen: &mut dyn Screen, app: &mut App) -> std::io::Result<Flow> {
    let tab = app.tab();
    let selected = tab.selected_entries();
    let dirs: Vec<(std::ffi::OsString, PathBuf)> = if selected.is_empty() {
        tab.fs
            .dirs()
            .iter()
            .map(|e| (e.name.clone(), e.path.clone()))
            .collect()
    } else {
        selected
            .iter()
            .filter(|e| e.is_dir())
            .map(|e| (e.name.clone(), e.path.clone()))
            .collect()
    };
    app.tab_mut().selected.clear();
    size_dirs(screen, app, &dirs)
}

/// Computes each directory's size under a progress panel (Ctrl-C asks to stop).
fn size_dirs(
    screen: &mut dyn Screen,
    app: &mut App,
    dirs: &[(std::ffi::OsString, PathBuf)],
) -> std::io::Result<Flow> {
    let title = "Calculate directories size";
    let mut view = ProgressView {
        title: title.into(),
        item: String::new(),
        index: 0,
        count: dirs.len(),
        acc: 0,
        total: 1,
    };
    for (i, (name, path)) in dirs.iter().enumerate() {
        if let Some(Input::Key(k)) = screen.poll_input(Duration::ZERO)? {
            if k.ctrl && k.code == KeyCode::Char('c') {
                let q = format!("Stop \"{title}\"?");
                if run_modal(screen, app, ConfirmDialog::new("Stop process", &q, false))? {
                    break;
                }
            }
        }
        view.item = name.to_string_lossy().into_owned();
        (view.index, view.acc, view.total) = (i + 1, (i + 1) as u64, dirs.len() as u64);
        {
            let (v, a) = (&view, &*app);
            redraw(
                screen,
                a,
                Some(&|area, buf| {
                    v.render(area, buf, &a.theme);
                    None
                }),
            )?;
        }
        if let Some(size) = simple::dir_size(path) {
            if let Some(e) = app.tab_mut().fs.entry_mut(name) {
                e.update_size(size);
            }
        }
    }
    CONTINUE
}

fn backup(screen: &mut dyn Screen, app: &mut App) -> std::io::Result<Flow> {
    let ext = app.settings.misc.backup_extension.clone();
    for f in selected_paths(app) {
        if let Err(msg) = simple::backup_file(&f, &ext) {
            run_modal(screen, app, ErrorDialog::new(&msg))?;
        }
    }
    clear_selection_and_refresh(app);
    CONTINUE
}

/// Opens a config file in the editor, then reloads everything and reports problems.
fn edit_config_file(
    screen: &mut dyn Screen,
    app: &mut App,
    path: PathBuf,
) -> std::io::Result<Flow> {
    let editor = program(app, "editor");
    let dir = app.tab().dir().to_path_buf();
    run_in_terminal(screen, &command_on_file(&editor, &path, false), &dir)?;
    let loaded = load_all(&app.paths);
    (app.settings, app.keymap, app.theme) = (loaded.settings, loaded.keymap, loaded.theme);
    app.refresh_all();
    if !loaded.warnings.is_empty() {
        return error(screen, app, &loaded.warnings.join("\n"));
    }
    CONTINUE
}

fn pick(
    screen: &mut dyn Screen,
    app: &mut App,
    title: &str,
    items: &[&str],
) -> std::io::Result<Option<char>> {
    let entries: Vec<String> = items.iter().map(|s| s.to_string()).collect();
    let chosen = run_modal(screen, app, SelectDialog::new(title, entries, true, true))?;
    Ok(chosen.and_then(|i| items[i].chars().next()))
}

fn main_menu(screen: &mut dyn Screen, app: &mut App) -> std::io::Result<Flow> {
    let items = [
        "/    Find/grep file(s)",
        "#    Show directories size",
        "s    Sort files",
        "t    Tree",
        "f    Show filesystems info",
        "o    Open shell",
        "c    Edit configuration",
        "k    Edit keys",
        "e    Edit theme",
        "h    Delete history",
    ];
    match pick(screen, app, "Main Menu", &items)? {
        Some('/') => dispatch(screen, app, Action::FindGrep),
        Some('#') => dispatch(screen, app, Action::ShowDirsSize),
        Some('s') => dispatch(screen, app, Action::SortFiles),
        Some('t') => dispatch(screen, app, Action::ShowTree),
        Some('f') => dispatch(screen, app, Action::ShowFilesystemsInfo),
        Some('o') => dispatch(screen, app, Action::OpenShell),
        Some('c') => edit_config_file(screen, app, app.paths.config()),
        Some('k') => edit_config_file(screen, app, app.paths.keys()),
        Some('e') => edit_config_file(screen, app, app.paths.theme()),
        Some('h') => {
            app.history = History::new();
            if let Err(e) = app.history.save(&app.paths.history()) {
                return error(screen, app, &format!("Cannot save history file\n{e}"));
            }
            CONTINUE
        }
        _ => CONTINUE,
    }
}

fn file_menu(screen: &mut dyn Screen, app: &mut App) -> std::io::Result<Flow> {
    let items = [
        "@    Exec on file",
        "i    File info",
        "p    Change file permissions",
        "o    Change file owner and/or group",
        "a    Backup file(s)",
        "d    Diff file with backup",
        "z    Compress/uncompress file(s)…",
        "x    Uncompress file",
        "u    Uncompress file in other panel",
        "c    Compress directory to format…",
    ];
    let action = match pick(screen, app, "File Menu", &items)? {
        Some('@') => Action::ExecOnFile,
        Some('i') => Action::ShowFileInfo,
        Some('p') => Action::ChangePerms,
        Some('o') => Action::ChangeOwner,
        Some('a') => Action::BackupFile,
        Some('d') => Action::DiffFileWithBackup,
        Some('z') => Action::CompressUncompressFile,
        Some('x') => Action::UncompressDir,
        Some('u') => Action::UncompressDirOtherPane,
        Some('c') => Action::CompressDir,
        _ => return CONTINUE,
    };
    dispatch(screen, app, action)
}

const LICENSE: &str = include_str!("../../COPYING");
const README: &str = include_str!("../../README.md");
const NEWS: &str = include_str!("../../CHANGELOG.md");
const TODO: &str = include_str!("../../TODO.md");

fn help_menu(screen: &mut dyn Screen, app: &mut App) -> std::io::Result<Flow> {
    let items = [
        "r    Readme",
        "n    News",
        "t    Todo",
        "l    License",
        "k    Key bindings",
    ];
    match pick(screen, app, "Help Menu", &items)? {
        Some('k') => {
            let file = app.paths.dir.join("keys.dump");
            if let Err(e) = std::fs::write(&file, dump_keys(app)) {
                return error(
                    screen,
                    app,
                    &format!("Cannot write {}\n{e}", file.display()),
                );
            }
            let pager = program(app, "pager");
            run_on_file(screen, app, &pager, &file)
        }
        Some(c @ ('r' | 'n' | 't' | 'l')) => {
            let (title, text) = match c {
                'r' => ("README", README),
                'n' => ("News", NEWS),
                't' => ("To do", TODO),
                _ => ("License", LICENSE),
            };
            let lines = text
                .lines()
                .map(|l| (l.to_string(), ColorItem::ViewWhiteOnBlack))
                .collect();
            run_modal(screen, app, ViewDialog::new(title, lines))?;
            CONTINUE
        }
        Some(_) => CONTINUE,
        None => CONTINUE,
    }
}

fn quit(screen: &mut dyn Screen, app: &mut App, chdir: bool) -> std::io::Result<Flow> {
    if app.settings.confirmations.quit
        && !run_modal(screen, app, ConfirmDialog::new("yafm", "Quit yafm?", true))?
    {
        return CONTINUE;
    }
    let path = outside_vfs_dir(app).unwrap_or_else(|| app.tab().dir().to_path_buf());
    for p in 0..2 {
        for k in 0..app.panes[p].tabs.len() {
            tools::close_all_vfs(screen, app, (p, k))?;
        }
    }
    Ok(Flow::Quit(chdir.then_some(path)))
}

fn result(screen: &mut dyn Screen, app: &mut App, r: Result<(), String>) -> std::io::Result<Flow> {
    match r {
        Ok(()) => CONTINUE,
        Err(msg) => error(screen, app, &msg),
    }
}

pub fn dispatch(screen: &mut dyn Screen, app: &mut App, action: Action) -> std::io::Result<Flow> {
    use Action as A;
    match action {
        A::CursorUp => app.cursor_by(-1),
        A::CursorDown => app.cursor_by(1),
        A::CursorUp10 => app.cursor_by(-10),
        A::CursorDown10 => app.cursor_by(10),
        A::CursorPageup => app.cursor_page(false),
        A::CursorPagedown => app.cursor_page(true),
        A::CursorHome => app.cursor_home(),
        A::CursorEnd => app.cursor_end(),
        A::CursorGotoFile => {
            if let Some(text) = ask_entry(
                screen,
                app,
                "Go to file",
                "Type part of the file name",
                "",
                None,
                false,
            )? {
                if !text.is_empty() {
                    app.goto_name_containing(&text);
                }
            }
        }
        A::CursorGotoFile1char => {
            if let Input::Key(k) = screen.next_input()? {
                if let (KeyCode::Char(c), false) = (k.code, k.ctrl || k.alt) {
                    app.goto_name_starting(c);
                }
            }
        }
        A::DirUp => return dir_up(screen, app),
        A::DirEnter => return dir_enter(screen, app),
        A::Goto => {
            let title = "Change directory";
            let path = Some(HistorySection::Path);
            if let Some(text) =
                ask_entry(screen, app, title, "Type directory name", "", path, true)?
            {
                if !text.is_empty() {
                    let dir = get_norm_path(&text, app.tab().dir());
                    if !app.is_vfs_temp(&dir) {
                        app.history
                            .append(HistorySection::Path, &dir.display().to_string());
                    }
                    return goto_or_error(screen, app, &dir);
                }
            }
        }
        A::BookmarkGoto => {
            let q = "Press 0-9 a-z to select the bookmark, Ctrl-C to quit";
            if let Some(c) = ask_bookmark_key(screen, app, "Goto bookmark", q)? {
                if let Some(p) = app.bookmark(c) {
                    return goto_or_error(screen, app, &p);
                }
            }
        }
        A::BookmarkSet if app.tab().in_vfs() => {
            return error(screen, app, "Cannot save a bookmark to a VFS")
        }
        A::BookmarkSet => {
            let q = "Press 0-9 a-z to save bookmark, Ctrl-C to quit";
            if let Some(c) = ask_bookmark_key(screen, app, "Set bookmark", q)? {
                app.set_bookmark(c);
            }
        }
        A::BookmarkSelectFromlist => {
            let entries: Vec<String> = app
                .settings
                .bookmarks
                .iter()
                .map(|(k, p)| format!("{k}  {p}"))
                .collect();
            if let Some(i) = run_modal(
                screen,
                app,
                SelectDialog::new("Select Bookmark", entries.clone(), true, false),
            )? {
                let path = PathBuf::from(&entries[i][3..]);
                return goto_or_error(screen, app, &path);
            }
        }
        A::HistorySelectFromlist => {
            if app.tab().history.is_empty() {
                return error(screen, app, "No entries in history");
            }
            let entries: Vec<String> = app
                .tab()
                .history
                .iter()
                .rev()
                .map(|p| p.display().to_string())
                .collect();
            if let Some(i) = run_modal(
                screen,
                app,
                SelectDialog::new("Return to", entries.clone(), false, false),
            )? {
                return goto_or_error(screen, app, Path::new(&entries[i]));
            }
        }
        A::PaneChangeFocus => app.pane_change_focus(),
        A::PaneOtherTabEqual if app.tab().in_vfs() => {
            return error(screen, app, "Cannot show a VFS in the other pane")
        }
        A::PaneOtherTabEqual => {
            let other = (1 - app.active, app.other_pane().active);
            tools::close_all_vfs(screen, app, other)?;
            let r = app.pane_other_tab_equal();
            return result(screen, app, r);
        }
        A::PanesSwap => app.panes_swap(),
        A::PanesCycleView => app.panes_cycle_view(),
        A::Refresh => app.refresh_all(),
        A::RedrawScreen => screen.clear()?,
        A::DotfilesToggle => app.toggle_dotfiles(),
        A::ToggleHumanSizes => app.toggle_human_sizes(),
        A::FiltersEdit => {
            let current = app.tab().fs.cfg.filters.clone();
            let help = "Type globs, separated by commas, for the files you want to hide";
            if let Some(text) = ask_entry(
                screen,
                app,
                "Edit filter",
                help,
                &current,
                Some(HistorySection::Glob),
                false,
            )? {
                app.set_filters(&text);
                if !text.is_empty() {
                    app.history.append(HistorySection::Glob, &text);
                }
            }
        }
        A::SortFiles => {
            let q = "N(o)ne, by (n)ame, by (e)xtension, by (s)ize, by (d)ate,\nuppercase to reverse order, Ctrl-C to quit";
            while let Some(c) = run_modal(screen, app, GetKeyDialog::new("Sorting mode", q))? {
                if app.set_sort(c) {
                    break;
                }
            }
        }
        A::ShowDirsSize => return show_dirs_size(screen, app),
        A::ShowCurrentDirSize => return show_current_dir_size(screen, app),
        A::TabNew => {
            // A new tab never shares a virtual folder: it opens where the archive is.
            let dir = outside_vfs_dir(app).unwrap_or_else(|| app.tab().dir().to_path_buf());
            let r = app.tab_new_at(&dir);
            return result(screen, app, r);
        }
        A::TabClose => {
            if app.pane().tabs.len() > 1 {
                let me = (app.active, app.pane().active);
                tools::close_all_vfs(screen, app, me)?;
            }
            let r = app.tab_close();
            return result(screen, app, r);
        }
        A::TabLeft => app.tab_left(),
        A::TabRight => app.tab_right(),
        A::Select => app.select_current(),
        A::SelectGlob | A::DeselectGlob => {
            let select = action == A::SelectGlob;
            let title = if select {
                "Select group"
            } else {
                "Deselect group"
            };
            if let Some(text) = ask_entry(
                screen,
                app,
                title,
                "Type pattern",
                "*",
                Some(HistorySection::Glob),
                false,
            )? {
                if !text.is_empty() {
                    app.select_glob(&text, select);
                    if text != "*" {
                        app.history.append(HistorySection::Glob, &text);
                    }
                }
            }
        }
        A::SelectInvert => app.select_invert(),
        A::RenameFile => return rename(screen, app),
        A::CopyFile => return copy_move(screen, app, Bulk::Copy),
        A::MoveFile => return copy_move(screen, app, Bulk::Move),
        A::MoveFile2 => return copy_move(screen, app, Bulk::Move2),
        A::DeleteFile => return delete(screen, app),
        A::ExecOnFile => return exec_on_file(screen, app),
        A::ViewFile | A::EditFile => {
            if app.tab().current().name == ".." {
                return CONTINUE;
            }
            let prog = program(
                app,
                if action == A::ViewFile {
                    "pager"
                } else {
                    "editor"
                },
            );
            let path = app.tab().current().path.clone();
            return run_on_file(screen, app, &prog, &path);
        }
        A::MakeDir => return create_in_dir(screen, app, true),
        A::TouchFile => return create_in_dir(screen, app, false),
        A::LinkCreate => return link_create(screen, app),
        A::LinkEdit => return link_edit(screen, app),
        A::BackupFile => return backup(screen, app),
        A::MainMenu => return main_menu(screen, app),
        A::FileMenu => return file_menu(screen, app),
        A::HelpMenu => return help_menu(screen, app),
        A::OpenShell => {
            let (shell, dir) = (program(app, "shell"), app.tab().dir().to_path_buf());
            run_in_terminal(screen, OsStr::new(&shell), &dir)?;
            app.refresh_all();
        }
        A::QuitChdir => return quit(screen, app, true),
        A::QuitNochdir => return quit(screen, app, false),
        A::FindGrep => return tools::find_grep(screen, app),
        A::ShowTree => return tools::show_tree(screen, app),
        A::ShowFileInfo => return tools::show_file_info(screen, app),
        A::ShowFilesystemsInfo => return tools::show_filesystems_info(screen, app),
        A::ChangePerms => return tools::change_perms(screen, app),
        A::ChangeOwner => return tools::change_owner(screen, app),
        A::DiffFileWithBackup => return tools::diff_file_with_backup(screen, app),
        A::CompressUncompressFile => return tools::compress_uncompress_file(screen, app),
        A::CompressDir => return tools::compress_dir(screen, app),
        A::UncompressDir => {
            let dest = app.tab().dir().to_path_buf();
            return tools::uncompress(screen, app, &dest);
        }
        A::UncompressDirOtherPane => {
            let dest = app.other_tab().dir().to_path_buf();
            return tools::uncompress(screen, app, &dest);
        }
        A::TogglePowercli => return crate::ui::powercli::run(screen, app),
    }
    CONTINUE
}

/// The main loop with the real action dispatcher.
pub fn event_loop(screen: &mut dyn Screen, app: &mut App) -> std::io::Result<Option<PathBuf>> {
    event_loop_with(screen, app, &mut dispatch)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ui::screen::TestScreen;
    use crate::ui::testutil::test_app;
    use std::ffi::OsStr;
    use std::path::PathBuf;

    fn ui(d: &Path, keys: &str) -> (App, TestScreen) {
        let mut app = test_app(d, 100, 30);
        for p in ["pager", "editor", "shell"] {
            app.settings.programs.insert(p.into(), "true".into());
        }
        (app, TestScreen::new(100, 30, keys))
    }

    fn keys(s: &mut TestScreen, k: &str) {
        s.keys = TestScreen::new(1, 1, k).keys;
    }

    fn run(app: &mut App, s: &mut TestScreen) {
        let _ = event_loop(s, app); // ends when the scripted keys run out
    }

    #[test]
    fn make_dir_and_touch() {
        let t = tempfile::tempdir().unwrap();
        let (mut app, mut s) = ui(t.path(), "F7 n e w enter t f enter");
        run(&mut app, &mut s);
        assert!(t.path().join("new").is_dir() && t.path().join("f").is_file());
        assert_eq!(app.history.entries(HistorySection::File), ["new", "f"]);
    }

    #[test]
    fn copy_and_delete_with_confirmation() {
        let t = tempfile::tempdir().unwrap();
        let d = t.path();
        std::fs::create_dir(d.join("dst")).unwrap();
        std::fs::write(d.join("x"), b"1").unwrap();
        let (mut app, mut s) = ui(d, "");
        app.refresh_all();
        app.panes[1].tabs[0]
            .goto_folder(&d.join("dst"), 25)
            .unwrap();
        app.tab_mut().focus_file(OsStr::new("x"), 25);
        keys(&mut s, "F5 enter F8 y");
        run(&mut app, &mut s);
        assert!(d.join("dst/x").is_file() && !d.join("x").exists());
        assert_eq!(
            app.history.entries(HistorySection::Path),
            [d.join("dst").display().to_string()]
        );
    }

    #[test]
    fn move_and_overwrite_prompt() {
        let t = tempfile::tempdir().unwrap();
        let d = t.path();
        std::fs::create_dir(d.join("dst")).unwrap();
        std::fs::write(d.join("x"), b"new").unwrap();
        std::fs::write(d.join("dst/x"), b"old").unwrap();
        let (mut app, mut s) = ui(d, "");
        app.refresh_all();
        app.panes[1].tabs[0]
            .goto_folder(&d.join("dst"), 25)
            .unwrap();
        app.tab_mut().focus_file(OsStr::new("x"), 25);
        keys(&mut s, "F6 enter y");
        run(&mut app, &mut s);
        assert_eq!(std::fs::read(d.join("dst/x")).unwrap(), b"new");
        assert!(!d.join("x").exists());
    }

    #[test]
    fn nothing_happens_on_parent_entry() {
        let t = tempfile::tempdir().unwrap();
        let d = t.path();
        std::fs::write(d.join("keep"), b"").unwrap();
        let (mut app, mut s) = ui(d, "home F8 F5 esc F6 esc F2 esc @ esc");
        let before = std::fs::read_dir(d).unwrap().count();
        run(&mut app, &mut s);
        assert!(d.join("keep").exists() && d.parent().unwrap().exists());
        assert_eq!(std::fs::read_dir(d).unwrap().count(), before);
        assert_eq!(app.tab().dir(), d);
    }

    #[test]
    fn rename_rules() {
        let t = tempfile::tempdir().unwrap();
        let d = t.path();
        std::fs::write(d.join("a"), b"").unwrap();
        let (mut app, mut s) = ui(d, "");
        app.tab_mut().focus_file(OsStr::new("a"), 25);
        keys(&mut s, "F2 C-w b enter");
        run(&mut app, &mut s);
        assert!(d.join("b").exists() && !d.join("a").exists());
        app.tab_mut().focus_file(OsStr::new("b"), 25);
        keys(&mut s, "F2 enter");
        run(&mut app, &mut s);
        assert!(s
            .screen_text()
            .contains("Source and destination are the same file"));
    }

    #[test]
    fn bookmarks_goto_and_filters() {
        let t = tempfile::tempdir().unwrap();
        let d = t.path();
        std::fs::write(d.join("z.o"), b"").unwrap();
        let (mut app, mut s) = ui(d, "g s u b enter B 5 left b 5 C-f * . o enter");
        run(&mut app, &mut s);
        assert_eq!(
            app.settings.bookmarks[&'5'],
            d.join("sub").display().to_string()
        );
        assert_eq!(app.tab().dir(), d.join("sub"));
        assert_eq!(app.tab().fs.cfg.filters, "*.o");
        assert_eq!(app.history.entries(HistorySection::Glob), ["*.o"]);
    }

    #[test]
    fn links_and_select_glob() {
        let t = tempfile::tempdir().unwrap();
        let d = t.path();
        let (mut app, mut s) = ui(d, "l l n k tab C-w a . t x t enter + . r s enter");
        run(&mut app, &mut s);
        assert_eq!(
            std::fs::read_link(d.join("lnk")).unwrap(),
            PathBuf::from("a.txt")
        );
        assert!(
            app.tab().is_selected(OsStr::new("b.rs"))
                && !app.tab().is_selected(OsStr::new("a.txt"))
        );
    }

    #[test]
    fn help_and_menus() {
        let t = tempfile::tempdir().unwrap();
        let (mut app, mut s) = ui(t.path(), "h r");
        run(&mut app, &mut s);
        assert!(
            s.screen_text().contains("Yet Another File Manager"),
            "{}",
            s.screen_text()
        );
        let t = tempfile::tempdir().unwrap();
        let (mut app, mut s) = ui(t.path(), "h t");
        run(&mut app, &mut s);
        assert!(s.screen_text().contains("Deferred"));
        let t = tempfile::tempdir().unwrap();
        let (mut app, mut s) = ui(t.path(), "h n");
        run(&mut app, &mut s);
        assert!(s.screen_text().contains(env!("CARGO_PKG_VERSION")));
        let t = tempfile::tempdir().unwrap();
        let (mut app, mut s) = ui(t.path(), "h k");
        run(&mut app, &mut s);
        let dump = std::fs::read_to_string(app.paths.dir.join("keys.dump")).unwrap();
        assert!(dump.contains("copy_file") && dump.contains("F5"));
        assert_eq!(s.suspended, 1);
        let t = tempfile::tempdir().unwrap();
        let (mut app, mut s) = ui(t.path(), "F9 s S");
        run(&mut app, &mut s);
        assert!(app.settings.pane_sort[0].sort_reverse);
    }

    #[test]
    fn view_edit_shell_suspend_the_screen() {
        let t = tempfile::tempdir().unwrap();
        let (mut app, mut s) = ui(t.path(), "end F3 F4 C-o");
        run(&mut app, &mut s);
        assert_eq!(s.suspended, 3);
    }

    #[test]
    fn quit_needs_confirmation() {
        let t = tempfile::tempdir().unwrap();
        let (mut app, mut s) = ui(t.path(), "q n q y");
        assert_eq!(
            event_loop(&mut s, &mut app).unwrap(),
            Some(t.path().to_path_buf())
        );
        let t = tempfile::tempdir().unwrap();
        let (mut app, mut s) = ui(t.path(), "C-q y");
        assert_eq!(event_loop(&mut s, &mut app).unwrap(), None);
    }

    #[test]
    fn sort_key_e_sorts_by_extension() {
        let t = tempfile::tempdir().unwrap();
        let (mut app, mut s) = ui(t.path(), "s e");
        run(&mut app, &mut s);
        assert_eq!(
            app.settings.pane_sort[0].sort_type,
            crate::config::settings::SortType::ByExt
        );
    }

    #[test]
    fn tabs_errors_show_dialogs() {
        let t = tempfile::tempdir().unwrap();
        let (mut app, mut s) = ui(t.path(), "!");
        run(&mut app, &mut s);
        assert!(s.screen_text().contains("Cannot close last tab"));
    }

    #[test]
    fn dump_keys_format() {
        let t = tempfile::tempdir().unwrap();
        let app = test_app(t.path(), 100, 30);
        let d = dump_keys(&app);
        assert!(d.starts_with(&"#".repeat(30)));
        assert!(d.contains("yafm dump keys"));
        assert!(d.contains("F5         ->   copy_file"));
        assert!(d.contains("Actions with no keys:"));
    }

    fn tgz_in(d: &Path) -> PathBuf {
        crate::ui::tools::tests_support::make_tgz(d)
    }

    #[test]
    fn enter_archive_with_enter_and_leave_with_left() {
        let t = tempfile::tempdir().unwrap();
        tgz_in(t.path());
        let (mut app, mut s) = ui(t.path(), "");
        app.refresh_all();
        app.tab_mut().focus_file(OsStr::new("a.tar.gz"), 24);
        keys(&mut s, "enter");
        run(&mut app, &mut s);
        assert!(app.tab().in_vfs());
        let tmp = app.tab().vfs[0].tmp.clone();
        keys(&mut s, "left n");
        run(&mut app, &mut s);
        assert!(!app.tab().in_vfs() && !tmp.exists());
        assert_eq!(app.tab().current().display_name(), "a.tar.gz");
    }

    #[test]
    fn quit_inside_vfs_cleans_and_returns_archive_dir() {
        let t = tempfile::tempdir().unwrap();
        tgz_in(t.path());
        let (mut app, mut s) = ui(t.path(), "");
        app.refresh_all();
        app.tab_mut().focus_file(OsStr::new("a.tar.gz"), 24);
        keys(&mut s, "enter");
        run(&mut app, &mut s);
        let tmp = app.tab().vfs[0].tmp.clone();
        keys(&mut s, "q y n");
        assert_eq!(
            event_loop(&mut s, &mut app).unwrap(),
            Some(t.path().to_path_buf())
        );
        assert!(!tmp.exists());
    }

    #[test]
    fn find_then_go() {
        let t = tempfile::tempdir().unwrap();
        std::fs::create_dir(t.path().join("deep")).unwrap();
        std::fs::write(t.path().join("deep/needle.txt"), b"").unwrap();
        let (mut app, mut s) = ui(t.path(), "/ C-w n e e d l e * enter enter");
        run(&mut app, &mut s);
        assert_eq!(app.tab().dir(), t.path().join("deep"));
        assert_eq!(app.tab().current().display_name(), "needle.txt");
        assert_eq!(app.history.entries(HistorySection::Find), ["needle*"]);
    }

    #[test]
    fn grep_panelize_shows_matches() {
        let t = tempfile::tempdir().unwrap();
        std::fs::write(t.path().join("m1.txt"), "hit\n").unwrap();
        std::fs::write(t.path().join("m2.txt"), "miss\n").unwrap();
        let (mut app, mut s) = ui(t.path(), "/ C-w * . t x t tab h i t enter a");
        run(&mut app, &mut s);
        assert!(app.tab().in_vfs());
        assert!(app.tab().fs.lookup(OsStr::new("m1.txt")).is_some());
        assert!(app.tab().fs.lookup(OsStr::new("m2.txt")).is_none());
        let pos = (app.active, app.pane().active);
        crate::ui::tools::close_all_vfs(&mut TestScreen::new(100, 30, "n"), &mut app, pos).unwrap();
    }

    #[test]
    fn perms_and_compress_and_diff() {
        use std::os::unix::fs::PermissionsExt;
        let t = tempfile::tempdir().unwrap();
        let d = t.path();
        std::fs::write(d.join("f"), "a\n").unwrap();
        std::fs::write(d.join("f.bak"), "b\n").unwrap();
        let (mut app, mut s) = ui(d, "");
        app.refresh_all();
        app.tab_mut().focus_file(OsStr::new("f"), 24);
        keys(&mut s, "F12 p x enter F12 d F12 z g");
        run(&mut app, &mut s);
        assert!(d.join("f.gz").is_file(), "{}", s.screen_text());
        assert_eq!(s.suspended, 1);
        let mode = std::fs::metadata(d.join("f.gz"))
            .unwrap()
            .permissions()
            .mode();
        assert_eq!(mode & 0o100, 0o100);
    }

    #[test]
    fn tree_and_infos_open() {
        let t = tempfile::tempdir().unwrap();
        let (mut app, mut s) = ui(t.path(), "C-t right enter");
        run(&mut app, &mut s);
        assert_ne!(app.tab().dir(), t.path());
        let t2 = tempfile::tempdir().unwrap();
        let (mut app, mut s) = ui(t2.path(), "F12 i");
        run(&mut app, &mut s);
        assert!(s.screen_text().contains("Information about"));
        let t3 = tempfile::tempdir().unwrap();
        let (mut app, mut s) = ui(t3.path(), "F9 f");
        run(&mut app, &mut s);
        assert!(s.screen_text().contains("Filesystem"));
    }

    #[test]
    fn bookmark_in_vfs_refused() {
        let t = tempfile::tempdir().unwrap();
        tgz_in(t.path());
        let (mut app, mut s) = ui(t.path(), "");
        app.refresh_all();
        app.tab_mut().focus_file(OsStr::new("a.tar.gz"), 24);
        keys(&mut s, "enter B");
        run(&mut app, &mut s);
        assert!(s.screen_text().contains("Cannot save a bookmark to a VFS"));
        let pos = (app.active, app.pane().active);
        crate::ui::tools::close_all_vfs(&mut TestScreen::new(100, 30, "n"), &mut app, pos).unwrap();
    }

    #[test]
    fn uncompress_into_other_pane() {
        let t = tempfile::tempdir().unwrap();
        tgz_in(t.path());
        let (mut app, mut s) = ui(t.path(), "");
        app.refresh_all();
        app.panes[1].tabs[0]
            .goto_folder(&t.path().join("sub"), 24)
            .unwrap();
        app.tab_mut().focus_file(OsStr::new("a.tar.gz"), 24);
        keys(&mut s, "F12 u");
        run(&mut app, &mut s);
        assert!(t.path().join("sub/top").is_file() && t.path().join("sub/in/f").is_file());
    }

    #[test]
    fn vfs_temp_dirs_stay_out_of_histories() {
        let t = tempfile::tempdir().unwrap();
        tgz_in(t.path());
        let (mut app, mut s) = ui(t.path(), "");
        app.refresh_all();
        app.tab_mut().focus_file(OsStr::new("a.tar.gz"), 24);
        keys(&mut s, "enter g i n enter :");
        run(&mut app, &mut s);
        let tmp = app.panes[0].tabs[0].vfs[0].tmp.clone();
        assert_eq!(
            app.tab().dir(),
            t.path(),
            "new tab opens outside the archive"
        );
        assert!(!app.tab().history.iter().any(|p| p.starts_with(&tmp)));
        let tmp_s = tmp.display().to_string();
        assert!(!app
            .history
            .entries(HistorySection::Path)
            .iter()
            .any(|p| p.starts_with(&tmp_s)));
        crate::ui::tools::close_all_vfs(&mut TestScreen::new(100, 30, "n"), &mut app, (0, 0))
            .unwrap();
    }

    #[test]
    fn shift_h_toggles_human_readable_sizes() {
        let t = tempfile::tempdir().unwrap();
        let (mut app, mut s) = ui(t.path(), "H");
        assert!(app.settings.options.human_readable_sizes);
        run(&mut app, &mut s);
        assert!(!app.settings.options.human_readable_sizes);
        keys(&mut s, "H");
        run(&mut app, &mut s);
        assert!(app.settings.options.human_readable_sizes);
    }

    #[test]
    fn space_sizes_only_the_dir_under_cursor() {
        let t = tempfile::tempdir().unwrap();
        let d = t.path();
        for (dir, n) in [("a", 3000), ("b", 50_000)] {
            std::fs::create_dir(d.join(dir)).unwrap();
            std::fs::write(d.join(dir).join("f"), vec![0u8; n]).unwrap();
        }
        std::fs::write(d.join("file"), b"x").unwrap();
        let (mut app, mut s) = ui(d, "spc");
        app.refresh_all();
        app.tab_mut().focus_file(OsStr::new("a"), 24);
        run(&mut app, &mut s);
        let size = |app: &App, n: &str| app.tab().fs.lookup(OsStr::new(n)).unwrap().size;
        assert!(size(&app, "a") >= 3000, "{}", size(&app, "a"));
        assert!(size(&app, "b") < 50_000, "b must not be sized");
        // on a plain file it does nothing (no page down either)
        app.tab_mut().focus_file(OsStr::new("file"), 24);
        keys(&mut s, "spc");
        run(&mut app, &mut s);
        assert_eq!(size(&app, "file"), 1);
        assert_eq!(app.tab().current().name, "file");
    }

    #[test]
    fn parity_gaps_lists_sizes_backup_compress_history() {
        let t = tempfile::tempdir().unwrap();
        let d = t.path();
        // history list (C-y) and bookmark list (C-d) return to dirs
        let (mut app, mut s) = ui(d, "g s u b enter C-y enter");
        run(&mut app, &mut s);
        assert_eq!(app.tab().dir(), d);
        app.settings
            .bookmarks
            .insert('7', d.join("sub").display().to_string());
        keys(&mut s, "C-d 7");
        run(&mut app, &mut s);
        assert_eq!(app.tab().dir(), d.join("sub"));
        // directory sizes (#) fill in the size of "sub"
        std::fs::write(d.join("sub/big"), vec![0u8; 3000]).unwrap();
        keys(&mut s, "left #");
        run(&mut app, &mut s);
        assert!(app.tab().fs.lookup(OsStr::new("sub")).unwrap().size >= 3000);
        // backup (F12 a), compress dir (F12 c g)
        app.refresh_all();
        app.tab_mut().focus_file(OsStr::new("a.txt"), 24);
        keys(&mut s, "F12 a");
        run(&mut app, &mut s);
        assert!(d.join("a.txt.bak").is_file());
        app.refresh_all();
        app.tab_mut().focus_file(OsStr::new("sub"), 24);
        keys(&mut s, "F12 c g");
        run(&mut app, &mut s);
        assert!(d.join("sub.tar.gz").is_file(), "{}", s.screen_text());
        // delete history (F9 h)
        app.history.append(HistorySection::Path, "/x");
        keys(&mut s, "F9 h");
        run(&mut app, &mut s);
        assert!(app.history.entries(HistorySection::Path).is_empty());
        // page / 10-line moves and go-to-first-letter (A-s)
        for n in 0..30 {
            std::fs::write(d.join(format!("z{n:02}")), b"").unwrap();
        }
        app.refresh_all();
        keys(&mut s, "home C-down");
        run(&mut app, &mut s);
        assert_eq!(app.tab().i, 10);
        keys(&mut s, "pagedown");
        run(&mut app, &mut s);
        assert_eq!(app.tab().i, 10 + app.fh());
        keys(&mut s, "home A-s b");
        run(&mut app, &mut s);
        assert_eq!(app.tab().current().display_name(), "b.rs");
    }
}
