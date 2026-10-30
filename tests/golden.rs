//! Golden-output tests: each `tests/NAME.sql` runs on a fresh copy of `tests/data/`, and its output
//! must match `tests/NAME.expected` exactly. These are the same scripts and the same expected files
//! as in the C++ version, so both programs must print exactly the same thing.
//!
//! `cargo test` runs them (each in its own temporary copy, in parallel). If you change the output on
//! purpose, run `UPDATE_EXPECTED=1 cargo test --test golden` and check the diff of the `.expected` files.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

/// Copies the files of a directory (not its subdirectories) into a new directory.
fn copy_dir(from: &Path, to: &Path) {
    fs::create_dir_all(to).unwrap();
    for entry in fs::read_dir(from).unwrap() {
        let entry = entry.unwrap();
        if entry.file_type().unwrap().is_file() {
            fs::copy(entry.path(), to.join(entry.file_name())).unwrap();
        }
    }
}

/// Runs `tests/<name>.sql` on a fresh copy of the tables and compares the output with `tests/<name>.expected`.
fn check(name: &str) {
    let tests = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests");
    let work: PathBuf = std::env::temp_dir().join(format!("mytinysql-{}-{name}", std::process::id()));
    let _ = fs::remove_dir_all(&work);
    copy_dir(&tests.join("data"), &work.join("data"));

    // Cargo builds the binary before the tests, and tells them where it is.
    let output = Command::new(env!("CARGO_BIN_EXE_myTinySql"))
        .arg(format!("--data={}", work.join("data").display()))
        .arg(format!("--sqlfile={}", tests.join(format!("{name}.sql")).display()))
        .output()
        .unwrap();
    let _ = fs::remove_dir_all(&work);

    let got = String::from_utf8(output.stdout).unwrap();
    let expected_file = tests.join(format!("{name}.expected"));
    if std::env::var_os("UPDATE_EXPECTED").is_some() {
        fs::write(&expected_file, &got).unwrap();
        return;
    }
    let expected = fs::read_to_string(&expected_file).unwrap();
    // On a failure, show the first line that differs: easier to read than two whole outputs.
    if let Some((line, (want, have))) =
        expected.lines().zip(got.lines()).enumerate().find(|(_, (want, have))| want != have)
    {
        panic!("{name}: line {} differs\n  expected: {want}\n       got: {have}", line + 1);
    }
    assert_eq!(expected, got, "{name}: the outputs have a different number of lines");
}

/// Makes one `#[test]` function per script, so `cargo test` lists each one by name.
macro_rules! golden_tests {
    ($($name:ident),* $(,)?) => {
        $(
            #[test]
            fn $name() {
                check(stringify!($name));
            }
        )*
    };
}

golden_tests!(apostrophes, create, delete, describe, errors, insert, select, update);
