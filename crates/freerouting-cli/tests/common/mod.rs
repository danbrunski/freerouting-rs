//! Helpers shared by the CLI integration tests.
#![allow(dead_code)]

use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use freerouting::drc::json::JsonValue;

pub fn repo(path: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .join(path)
}

/// A fresh directory under the target directory.
pub fn scratch(name: &str) -> PathBuf {
    let dir = Path::new(env!("CARGO_TARGET_TMPDIR")).join(format!("cli-{name}"));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

/// Copies fixtures into `dir` under their own names.
pub fn copy_fixtures(dir: &Path, names: &[&str]) {
    for name in names {
        std::fs::copy(repo("fixtures").join(name), dir.join(name)).unwrap();
    }
}

/// Runs the `freerouting` binary in `dir`.
pub fn run_in(dir: &Path, args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_freerouting"))
        .args(args)
        .current_dir(dir)
        .env_remove("FREEROUTING_TEST_ABORT_BEFORE_RENAME")
        .env("XDG_CONFIG_HOME", dir.join("no-config"))
        .output()
        .expect("the binary runs")
}

pub fn stderr(output: &Output) -> String {
    String::from_utf8_lossy(&output.stderr).into_owned()
}

pub fn stdout(output: &Output) -> String {
    String::from_utf8_lossy(&output.stdout).into_owned()
}

pub fn code(output: &Output) -> i32 {
    output.status.code().expect("the process exited")
}

pub fn member<'a>(value: &'a JsonValue, key: &str) -> Option<&'a JsonValue> {
    match value {
        JsonValue::Object(m) => m.iter().find(|(k, _)| k == key).map(|(_, v)| v),
        _ => None,
    }
}

pub fn string_of(value: Option<&JsonValue>) -> Option<String> {
    match value {
        Some(JsonValue::String(s)) => Some(s.clone()),
        _ => None,
    }
}

pub fn strings_of(value: Option<&JsonValue>) -> Vec<String> {
    match value {
        Some(JsonValue::Array(items)) => items.iter().filter_map(|v| string_of(Some(v))).collect(),
        _ => Vec::new(),
    }
}

/// A row of `fixtures/reference/clirun/`.
pub struct ClirunRow {
    pub id: String,
    pub args: Vec<String>,
    pub exit_code: i32,
    /// `(path, size)` of the files Java left.
    pub files: Vec<(String, u64)>,
    pub row: JsonValue,
}

pub fn clirun_row(id: &str) -> ClirunRow {
    let text =
        std::fs::read_to_string(repo(&format!("fixtures/reference/clirun/{id}.json"))).unwrap();
    let row = JsonValue::parse(&text).unwrap();
    let exit_code = match member(&row, "exitCode") {
        Some(JsonValue::Number(n)) => n.parse().unwrap(),
        _ => panic!("no exitCode in {id}"),
    };
    let files = match member(&row, "files") {
        Some(JsonValue::Array(items)) => items
            .iter()
            .map(|f| {
                let size = match member(f, "size") {
                    Some(JsonValue::Number(n)) => n.parse().unwrap(),
                    _ => 0,
                };
                (string_of(member(f, "path")).unwrap(), size)
            })
            .collect(),
        _ => Vec::new(),
    };
    ClirunRow {
        id: id.to_string(),
        args: strings_of(member(&row, "args")),
        exit_code,
        files,
        row,
    }
}

impl ClirunRow {
    /// Java's text of an output file.
    pub fn text(&self, name: &str) -> Option<String> {
        member(&self.row, "texts").and_then(|t| string_of(member(t, name)))
    }

    /// Java's arguments of the row, plus `--profile legacy`: the rows compare
    /// with Java, so they pin the parity profile (N01.2).
    pub fn args(&self) -> Vec<&str> {
        let mut args: Vec<&str> = self.args.iter().map(String::as_str).collect();
        args.extend(["--profile", "legacy"]);
        args
    }
}
