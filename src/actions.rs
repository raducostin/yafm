//! The public actions of lfm (`@public` functions in `actions.py`), bindable to keys,
//! plus yafm's `show_current_dir_size` and `toggle_human_sizes`.

use strum::{EnumIter, EnumString, IntoStaticStr};

#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug, EnumString, IntoStaticStr, EnumIter)]
#[strum(serialize_all = "snake_case")]
pub enum Action {
    CursorUp,
    CursorDown,
    #[strum(serialize = "cursor_up10")]
    CursorUp10,
    #[strum(serialize = "cursor_down10")]
    CursorDown10,
    CursorPageup,
    CursorPagedown,
    CursorHome,
    CursorEnd,
    CursorGotoFile,
    #[strum(serialize = "cursor_goto_file_1char")]
    CursorGotoFile1char,
    DirUp,
    DirEnter,
    Goto,
    BookmarkGoto,
    BookmarkSet,
    BookmarkSelectFromlist,
    HistorySelectFromlist,
    PaneChangeFocus,
    PaneOtherTabEqual,
    PanesSwap,
    PanesCycleView,
    Refresh,
    RedrawScreen,
    DotfilesToggle,
    /// yafm extra: human-readable sizes or exact bytes.
    ToggleHumanSizes,
    FiltersEdit,
    SortFiles,
    ShowDirsSize,
    /// yafm extra: size of the directory under the cursor only.
    ShowCurrentDirSize,
    TabNew,
    TabClose,
    TabLeft,
    TabRight,
    Select,
    SelectGlob,
    DeselectGlob,
    SelectInvert,
    RenameFile,
    CopyFile,
    MoveFile,
    #[strum(serialize = "move_file2")]
    MoveFile2,
    DeleteFile,
    ExecOnFile,
    ViewFile,
    EditFile,
    MakeDir,
    TouchFile,
    LinkCreate,
    LinkEdit,
    BackupFile,
    ChangePerms,
    ChangeOwner,
    DiffFileWithBackup,
    ShowFileInfo,
    ShowFilesystemsInfo,
    UncompressDir,
    UncompressDirOtherPane,
    CompressDir,
    CompressUncompressFile,
    FindGrep,
    ShowTree,
    MainMenu,
    FileMenu,
    HelpMenu,
    TogglePowercli,
    OpenShell,
    QuitChdir,
    QuitNochdir,
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::str::FromStr;
    use strum::IntoEnumIterator;

    #[test]
    fn there_are_66_lfm_actions_plus_two() {
        assert_eq!(Action::iter().count(), 68);
    }

    #[test]
    fn action_names_are_snake_case() {
        assert_eq!(Action::from_str("move_file2").unwrap(), Action::MoveFile2);
        assert_eq!(
            Action::from_str("cursor_goto_file_1char").unwrap(),
            Action::CursorGotoFile1char
        );
        assert_eq!(Action::from_str("cursor_up10").unwrap(), Action::CursorUp10);
        for a in Action::iter() {
            let name: &str = a.into();
            assert_eq!(Action::from_str(name).unwrap(), a);
        }
    }
}
