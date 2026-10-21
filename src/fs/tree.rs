//! The directory tree shown by the tree view: the path from "/" down to the
//! current directory, every ancestor expanded. Port of lfm's `DirsTree`.

use std::path::{Path, PathBuf};

/// Sorted names of the subdirectories of `path` (links to dirs count).
pub fn get_dirs(path: &Path, dotfiles: bool) -> Vec<String> {
    let Ok(rd) = std::fs::read_dir(path) else {
        return Vec::new();
    };
    let mut dirs: Vec<String> = rd
        .filter_map(Result::ok)
        .filter(|de| de.path().is_dir())
        .map(|de| de.file_name().to_string_lossy().into_owned())
        .filter(|n| dotfiles || !n.starts_with('.'))
        .collect();
    dirs.sort();
    dirs
}

pub struct DirsTree {
    pub path: PathBuf,
    /// (name, depth with "/" at -1, full path)
    pub nodes: Vec<(String, isize, PathBuf)>,
    dotfiles: bool,
}

impl DirsTree {
    pub fn new(path: &Path, dotfiles: bool) -> DirsTree {
        let mut t = DirsTree {
            path: PathBuf::new(),
            nodes: Vec::new(),
            dotfiles,
        };
        t.build(path);
        t
    }

    /// "/" plus, for each ancestor from "/" down, its subdirectories, with the
    /// one on the way to `path` expanded in place.
    fn build(&mut self, path: &Path) {
        let ancestors: Vec<&Path> = {
            let mut v: Vec<&Path> = path.ancestors().collect();
            v.reverse();
            v
        };
        self.nodes = vec![("/".to_string(), -1, PathBuf::from("/"))];
        let mut insert_at = 1;
        for (depth, anc) in ancestors.iter().enumerate() {
            let Some(next) = ancestors.get(depth + 1) else {
                break;
            };
            let expanded = next
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_default();
            let children: Vec<(String, isize, PathBuf)> = get_dirs(anc, self.dotfiles)
                .into_iter()
                .map(|d| (d.clone(), depth as isize, anc.join(&d)))
                .collect();
            let pos_expanded = children.iter().position(|c| c.0 == expanded);
            let count = children.len();
            self.nodes.splice(insert_at..insert_at, children);
            insert_at = match pos_expanded {
                Some(p) => insert_at + p + 1,
                None => insert_at + count,
            };
        }
        self.path = path.to_path_buf();
    }

    fn regenerate(&mut self, pos: usize) {
        if let Some(n) = self.nodes.get(pos) {
            let p = n.2.clone();
            self.build(&p);
        }
    }

    pub fn pos(&self) -> usize {
        self.nodes
            .iter()
            .position(|n| n.2 == self.path)
            .unwrap_or(0)
    }

    pub fn depth(&self, i: usize) -> isize {
        self.nodes.get(i).map_or(-2, |n| n.1)
    }

    fn first_sibling_pos(&self) -> usize {
        let (mut p, d) = (self.pos(), self.depth(self.pos()));
        while p > 0 && self.depth(p - 1) == d {
            p -= 1;
        }
        p
    }

    fn last_sibling_pos(&self) -> usize {
        let (mut p, d) = (self.pos(), self.depth(self.pos()));
        while p + 1 < self.nodes.len() && self.depth(p + 1) == d {
            p += 1;
        }
        p
    }

    /// Previous sibling (never leaves the current level).
    pub fn up(&mut self) {
        let p = self.pos();
        if p > 0 && self.depth(p - 1) == self.depth(p) {
            self.regenerate(p - 1);
        }
    }

    /// Next sibling (never leaves the current level).
    pub fn down(&mut self) {
        let p = self.pos();
        if p + 1 < self.nodes.len() && self.depth(p + 1) == self.depth(p) {
            self.regenerate(p + 1);
        }
    }

    /// A page of siblings, stopping at the first/last one (TreeView.run).
    pub fn page(&mut self, rows: usize, down: bool) {
        let (p, d) = (self.pos(), self.depth(self.pos()));
        let target = if down {
            match p + rows {
                t if t < self.nodes.len() && self.depth(t) == d => t,
                _ => self.last_sibling_pos(),
            }
        } else {
            match p.checked_sub(rows) {
                Some(t) if self.depth(t) == d => t,
                _ => self.first_sibling_pos(),
            }
        };
        self.regenerate(target);
    }

    pub fn home(&mut self) {
        if self.nodes.len() > 1 {
            self.regenerate(1);
        }
    }

    pub fn end(&mut self) {
        self.regenerate(self.nodes.len() - 1);
    }

    pub fn parent(&mut self) {
        let p = self.pos();
        if p == 0 {
            return;
        }
        let d = self.depth(p);
        if let Some(i) = (0..p).rev().find(|&i| self.depth(i) == d - 1) {
            self.regenerate(i);
        }
    }

    pub fn to_child(&mut self) -> bool {
        match get_dirs(&self.path, self.dotfiles).first() {
            Some(child) => {
                let p = self.path.join(child);
                self.build(&p);
                true
            }
            None => false,
        }
    }

    pub fn has_children(&self) -> bool {
        !get_dirs(&self.path, self.dotfiles).is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn mk() -> tempfile::TempDir {
        let t = tempfile::tempdir().unwrap();
        for p in ["a/a1", "a/a2", "b", ".h"] {
            std::fs::create_dir_all(t.path().join(p)).unwrap();
        }
        std::fs::write(t.path().join("file"), b"").unwrap();
        t
    }

    #[test]
    fn dirs_listing() {
        let t = mk();
        assert_eq!(get_dirs(t.path(), false), ["a", "b"]);
        assert_eq!(get_dirs(t.path(), true), [".h", "a", "b"]);
        assert!(get_dirs(&t.path().join("nope"), true).is_empty());
    }

    #[test]
    fn builds_expanded_path() {
        let t = mk();
        let tr = DirsTree::new(&t.path().join("a/a2"), false);
        let names: Vec<&str> = tr.nodes.iter().map(|n| n.0.as_str()).collect();
        assert_eq!(names[0], "/");
        assert_eq!(tr.depth(0), -1);
        assert!(names.contains(&"a1") && names.contains(&"b") && !names.contains(&".h"));
        assert_eq!(tr.nodes[tr.pos()].2, t.path().join("a/a2"));
        let root = DirsTree::new(Path::new("/"), false);
        assert_eq!((root.nodes.len(), root.pos()), (1, 0));
    }

    #[test]
    fn sibling_moves_and_parent_child() {
        let t = mk();
        let mut tr = DirsTree::new(&t.path().join("a/a1"), false);
        tr.up();
        assert_eq!(tr.path, t.path().join("a/a1")); // first sibling: stays
        tr.down();
        assert_eq!(tr.path, t.path().join("a/a2"));
        tr.down();
        assert_eq!(tr.path, t.path().join("a/a2")); // last sibling: stays
        tr.parent();
        assert_eq!(tr.path, t.path().join("a"));
        assert!(tr.has_children());
        assert!(tr.to_child());
        assert_eq!(tr.path, t.path().join("a/a1"));
        assert!(!tr.to_child());
        tr.home();
        assert_eq!(tr.nodes[tr.pos()].1, 0);
    }
}
