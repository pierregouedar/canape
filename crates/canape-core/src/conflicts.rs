//! Other tools that write the same firmware settings as Canape, known by
//! process name: the service warns while one runs, and the shell does not
//! offer them as games.

/// Process name (lowercase) → name shown to the user.
const KNOWN: &[(&str, &str)] = &[
    ("whservice.exe", "Winhanced"),
    ("armourycrate.service.exe", "Armoury Crate"),
    ("ghelper.exe", "G-Helper"),
    ("handheldcompanion.exe", "Handheld Companion"),
];

/// Display names of the conflicting programs among `processes`, deduplicated.
pub fn find<'a>(processes: impl IntoIterator<Item = &'a str>) -> Vec<String> {
    let mut found: Vec<String> = processes
        .into_iter()
        .filter_map(|process| {
            let process = process.to_ascii_lowercase();
            KNOWN
                .iter()
                .find(|(exe, _)| *exe == process)
                .map(|(_, name)| name.to_string())
        })
        .collect();
    found.sort();
    found.dedup();
    found
}

/// Whether a shortcut named `name` opens one of these tools.
pub fn is_tool_name(name: &str) -> bool {
    let name = name.trim().to_lowercase();
    KNOWN.iter().any(|(_, tool)| {
        name == tool.to_lowercase() || name.starts_with(&format!("{} ", tool.to_lowercase()))
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn finds_known_programs_once() {
        let found = find([
            "explorer.exe",
            "WHService.exe",
            "ghelper.exe",
            "whservice.exe",
        ]);
        assert_eq!(found, vec!["G-Helper", "Winhanced"]);
    }

    #[test]
    fn tool_shortcuts_by_name() {
        assert!(is_tool_name("Winhanced"));
        assert!(is_tool_name("G-Helper "));
        assert!(!is_tool_name("Heaven Benchmark 4.0"));
    }
}
