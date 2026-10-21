//! Program names in `$PATH`, for PowerCLI completion (lfm's get_binary_programs).

/// Sorted, unique names of the regular files in the `$PATH` directories.
pub fn binary_programs() -> Vec<String> {
    let path = std::env::var_os("PATH").unwrap_or_default();
    let mut names: Vec<String> = std::env::split_paths(&path)
        .filter_map(|dir| std::fs::read_dir(dir).ok())
        .flat_map(|rd| rd.filter_map(Result::ok))
        .filter(|de| de.path().is_file())
        .map(|de| de.file_name().to_string_lossy().into_owned())
        .collect();
    names.sort();
    names.dedup();
    names
}

#[cfg(test)]
mod tests {
    #[test]
    fn finds_sh() {
        assert!(super::binary_programs().contains(&"sh".to_string()));
    }
}
