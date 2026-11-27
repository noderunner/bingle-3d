//! The word is spelled `color`, in code, comments, docs and data files (CLAUDE.md, Words). This
//! fails when a tracked file spells it the other way, except in the places that must: a copy of
//! the server's schema, and the old spelling of a data file's key that is still read.

use std::path::Path;
use std::process::Command;

/// Where the old spelling is kept on purpose: a file, and what a line of it says there.
const EXCEPTIONS: &[(&str, &[&str])] = &[
    // A copy of Bingle's schema: it changes when the client takes a newer API.
    ("schema/", &[""]),
    // Format 1 of the appearance file called a seat's paint this, and is still read.
    (
        "src/appearance.rs",
        &[
            "`.paint`, what the panels draw the color as",
            "a seat's `colour` is its `paint`",
            "that says `colour` is still read",
            "Format 1 called",
            "alias = \"colour\"",
            "format_1s_colour_too",
            "{seat}colour =",
        ],
    ),
    // The environment's sun and fog called their color this before 2026-11-27.
    (
        "src/environment.rs",
        &[
            "call it `colour`",
            "still says `colour`",
            "alias = \"colour\"",
            "old spelling",
            "colour = [",
        ],
    ),
    (
        "docs/plan.md",
        &[
            "`colour` until format 2",
            "format 1 called `colour`",
            "`colour` and",
        ],
    ),
];

#[test]
fn no_tracked_file_spells_color_the_other_way() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    // The word is built here, so that this file does not carry it.
    let word = ["col", "our"].concat();
    let Ok(listed) = Command::new("git")
        .args(["ls-files", "-z"])
        .current_dir(root)
        .output()
    else {
        eprintln!("git is not available: the spelling is not checked");
        return;
    };
    if !listed.status.success() {
        eprintln!("not a git checkout: the spelling is not checked");
        return;
    }
    let mut found = Vec::new();
    for name in String::from_utf8(listed.stdout).unwrap().split('\0') {
        // A file that was deleted in the work tree, a binary, or a link is not text to read.
        let Ok(text) = std::fs::read_to_string(root.join(name)) else {
            continue;
        };
        let allowed: Vec<&str> = EXCEPTIONS
            .iter()
            .filter(|(path, _)| name.starts_with(path))
            .flat_map(|(_, lines)| lines.iter().copied())
            .collect();
        for (n, line) in text.lines().enumerate() {
            if line.to_lowercase().contains(&word) && !allowed.iter().any(|a| line.contains(a)) {
                found.push(format!("{name}:{}: {}", n + 1, line.trim()));
            }
        }
    }
    assert!(
        found.is_empty(),
        "spell it `color` (CLAUDE.md, Words), or list the place in EXCEPTIONS in {}:\n{}",
        file!(),
        found.join("\n")
    );
}
