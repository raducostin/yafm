//! Minimal reader for the INI files written by lfm 3.1 (Python `ConfigParser`).

pub type IniSection = (String, Vec<(String, String)>);

/// `[Section]` names verbatim; each line split at the first `:` or `=`; keys
/// trimmed and lowercased, values trimmed; `#`/`;` comment lines and blank lines skipped.
pub fn parse_ini(src: &str) -> Vec<IniSection> {
    let mut sections: Vec<IniSection> = Vec::new();
    for line in src.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() || trimmed.starts_with('#') || trimmed.starts_with(';') {
            continue;
        }
        if let Some(name) = trimmed.strip_prefix('[').and_then(|r| r.strip_suffix(']')) {
            sections.push((name.to_string(), Vec::new()));
            continue;
        }
        let (Some(pos), Some(section)) = (trimmed.find([':', '=']), sections.last_mut()) else {
            continue;
        };
        let key = trimmed[..pos].trim().to_lowercase();
        let value = trimmed[pos + 1..].trim().to_string();
        section.1.push((key, value));
    }
    sections
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ini_splits_on_first_separator_and_lowercases() {
        let s = parse_ini("[Main]\npane_other_tab_equal:\t       =\nQuit_NoChdir:  C-q \n# c\n");
        assert_eq!(s[0].0, "Main");
        assert_eq!(
            s[0].1,
            vec![
                ("pane_other_tab_equal".to_string(), "=".to_string()),
                ("quit_nochdir".to_string(), "C-q".to_string())
            ]
        );
    }

    #[test]
    fn equals_separator_and_empty_values() {
        let s = parse_ini("; top\n[A]\nx = 1 : 2\n5: \n\n[B]\n");
        assert_eq!(s.len(), 2);
        assert_eq!(
            s[0].1,
            vec![
                ("x".to_string(), "1 : 2".to_string()),
                ("5".to_string(), String::new())
            ]
        );
        assert!(s[1].1.is_empty());
    }
}
