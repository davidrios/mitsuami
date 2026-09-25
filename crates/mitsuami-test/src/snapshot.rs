//! File snapshots under `<crate>/tests/snapshots/`.
//!
//! - A missing snapshot is written and the assertion passes, except on CI
//!   (`CI` set), where it fails.
//! - A mismatch fails with a diff and writes `<file>.new` next to the
//!   snapshot for review.
//! - `MITSUAMI_UPDATE_SNAPSHOTS=1` accepts every mismatch instead.

use std::path::PathBuf;

use similar::TextDiff;

use crate::app::TestContext;

pub(crate) fn sanitize(s: &str) -> String {
    s.chars().map(|c| if c.is_ascii_alphanumeric() || c == '-' || c == '_' { c } else { '_' }).collect()
}

#[track_caller]
pub(crate) fn assert(context: &TestContext, backend: Option<&str>, name: &str, extension: &str, actual: &str) {
    let dir = PathBuf::from(context.manifest_dir).join("tests").join("snapshots");
    // Backend-specific snapshots (frames, command logs) get the backend in
    // their name; headless keeps the plain name.
    let backend = backend.filter(|b| *b != "headless").map(|b| format!(".{b}")).unwrap_or_default();
    let file = dir.join(format!("{}@{}{backend}.{extension}", context.file_prefix, sanitize(name)));
    let mut pending = file.clone().into_os_string();
    pending.push(".new");
    let pending = PathBuf::from(pending);
    let update = std::env::var("MITSUAMI_UPDATE_SNAPSHOTS").is_ok_and(|v| v == "1" || v == "true");
    let ci = std::env::var_os("CI").is_some();

    let write = |path: &PathBuf| {
        std::fs::create_dir_all(&dir).expect("create snapshot directory");
        std::fs::write(path, actual).expect("write snapshot");
    };

    let expected = match std::fs::read_to_string(&file) {
        Ok(expected) => expected,
        Err(_) if ci && !update => panic!("snapshot {} is missing (not created on CI)", file.display()),
        Err(_) => {
            write(&file);
            eprintln!("mitsuami-test: created snapshot {}", file.display());
            return;
        }
    };
    if expected.replace("\r\n", "\n") == actual {
        let _ = std::fs::remove_file(&pending);
        return;
    }
    if update {
        write(&file);
        let _ = std::fs::remove_file(&pending);
        return;
    }
    if !ci {
        write(&pending);
    }
    let diff = TextDiff::from_lines(expected.as_str(), actual).unified_diff().header("expected", "actual").to_string();
    panic!(
        "snapshot {} does not match\n\n{diff}\nreview {} and rename it over the snapshot to accept, \
         or run with MITSUAMI_UPDATE_SNAPSHOTS=1",
        file.display(),
        pending.display()
    );
}
