//! The output files of a job: the format table and the atomic writer (C07.4).
//!
//! Port of the output side of `CliRunner` (`writeMainOutput`,
//! `writeCliOutputIfAvailable`) and `MultiOutputGenerator` (SES, DRC JSON)
//! at `a773a70ec`, with the fixes of the C series ("No 0-byte outputs"):
//!
//! - **Formats.** `.ses` ([`write_ses_to`]), `.dsn` ([`write_dsn_to`]),
//!   `.rules` ([`write_rules`]) and `.drc` / `.drc.json` (the DRC report) are
//!   written, and `.kicad_pcb` (K08: the routed copper written into a copy of
//!   the `.kicad_pcb` input, [`render_kicad_board`]). `.frb`, `.scr`, `.json`
//!   and everything else are rejected by [`classify`] **before** any work. Java's `-do x.xyz` writes
//!   the SES text into `x.xyz` and exits 0, `-do x.frb`, `.scr` and
//!   `.kicad_pcb` produce a 0-byte file or nothing; `.drc.json` is read as
//!   plain `.json` there, so Java writes a KiCad design JSON under that name.
//! - **Atomic.** Every output is written to `.<name>.<pid>.tmp` in the target
//!   directory and renamed into place, so a killed process leaves no partial
//!   file. Stale temporary files of earlier killed runs are removed at the
//!   next start ([`remove_stale_temp_files`]).
//! - **Never 0 bytes.** A serializer result of 0 bytes is an error and the
//!   temporary file is removed.

use std::fmt;
use std::fs::File;
use std::io::{self, BufWriter, Write};
use std::path::{Path, PathBuf};

use crate::board::BasicBoard;
use crate::drc::DesignRulesChecker;
use crate::io::kicad::{KicadSource, render_kicad_board};
use crate::io::specctra::dsn_writer::write_dsn_to;
use crate::io::specctra::rules_writer::write_rules;
use crate::io::specctra::ses_writer::{SesStyle, write_ses_to};
use crate::settings::RouterSettings;

/// An output format a job can write.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum OutputFormat {
    /// A Specctra session file (C05), byte-identical to Java's.
    Ses,
    /// A Specctra design file (C06).
    Dsn,
    /// A rules file (C04).
    Rules,
    /// The design rules check report (KiCad's DRC JSON).
    DrcReport,
    /// A copy of the `.kicad_pcb` input with the routed copper (K08).
    KicadPcb,
}

impl OutputFormat {
    /// The name Java's `FileFormat` has, for the log (`SES`, `DSN`,
    /// `RULES`, `DRC_JSON`).
    pub fn name(self) -> &'static str {
        match self {
            OutputFormat::Ses => "SES",
            OutputFormat::Dsn => "DSN",
            OutputFormat::Rules => "RULES",
            OutputFormat::DrcReport => "DRC_JSON",
            OutputFormat::KicadPcb => "KICAD_PCB",
        }
    }

    /// The format of a file name, by its extension (ignoring case), or why
    /// there is none. `.drc.json` and `.drc` are the DRC report wherever they
    /// appear (Java reads the former as plain JSON).
    pub fn of_path(path: &Path) -> Result<OutputFormat, OutputError> {
        let name = path
            .file_name()
            .map(|n| n.to_string_lossy().to_lowercase())
            .unwrap_or_default();
        if name.ends_with(".drc.json") || name.ends_with(".drc") {
            return Ok(OutputFormat::DrcReport);
        }
        let extension = name.rsplit_once('.').map(|(_, e)| e).unwrap_or("");
        match extension {
            "ses" => Ok(OutputFormat::Ses),
            "dsn" => Ok(OutputFormat::Dsn),
            "rules" => Ok(OutputFormat::Rules),
            "kicad_pcb" => Ok(OutputFormat::KicadPcb),
            "frb" | "scr" | "json" => Err(OutputError::Unsupported {
                path: path.to_path_buf(),
                reason: format!(
                    "the .{extension} format cannot be produced here (Java writes an empty or \
                     mislabelled file)"
                ),
            }),
            _ => Err(OutputError::Unsupported {
                path: path.to_path_buf(),
                reason: "unknown output file extension".to_string(),
            }),
        }
    }
}

/// One output of a job.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct OutputSpec {
    pub path: PathBuf,
    pub format: OutputFormat,
}

/// Why an output could not be planned or written.
#[derive(Debug)]
pub enum OutputError {
    /// The extension names a format that is not written.
    Unsupported { path: PathBuf, reason: String },
    /// The serializer produced nothing (Java: `Nothing to save to ...`).
    Empty { path: PathBuf, format: OutputFormat },
    /// The file system refused.
    Io { path: PathBuf, source: io::Error },
}

impl fmt::Display for OutputError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            OutputError::Unsupported { path, reason } => write!(
                f,
                "Unsupported output file '{}': {reason}. Supported: .ses, .dsn, .rules, .drc, \
                 .drc.json, .kicad_pcb",
                path.display()
            ),
            OutputError::Empty { path, format } => write!(
                f,
                "Nothing to save to '{}': no {} output was produced.",
                path.display(),
                format.name()
            ),
            OutputError::Io { path, source } => {
                write!(
                    f,
                    "Couldn't save the output file '{}': {source}",
                    path.display()
                )
            }
        }
    }
}

impl std::error::Error for OutputError {}

/// Plans the outputs of a command line: the format of each path, the first
/// error if any path has none. Call this before reading the input.
pub fn classify(paths: &[PathBuf]) -> Result<Vec<OutputSpec>, OutputError> {
    paths
        .iter()
        .map(|path| {
            Ok(OutputSpec {
                path: path.clone(),
                format: OutputFormat::of_path(path)?,
            })
        })
        .collect()
}

/// What the serializers of a job need.
pub struct OutputSource<'a> {
    pub board: &'a BasicBoard,
    /// The design name: the DSN file name without its extension.
    pub design_name: &'a str,
    /// The final settings, written into a rules file's `autoroute_settings`.
    pub settings: &'a RouterSettings,
    /// The `.kicad_pcb` the board was read from, for a `.kicad_pcb` output.
    pub kicad_source: Option<&'a KicadSource>,
}

/// Writes `format` of the source to `out` (not flushed past the format's own
/// flush; wrap files in a `BufWriter`).
pub fn serialize<W: Write>(
    format: OutputFormat,
    source: &OutputSource<'_>,
    mut out: W,
) -> io::Result<()> {
    match format {
        OutputFormat::Ses => write_ses_to(source.board, source.design_name, SesStyle::Java, out),
        OutputFormat::Dsn => write_dsn_to(source.board, source.design_name, out),
        OutputFormat::Rules => {
            let text = write_rules(source.board, Some(source.settings), source.design_name);
            out.write_all(text.as_bytes())?;
            out.flush()
        }
        OutputFormat::DrcReport => {
            // `MultiOutputGenerator`: the report is named after the design
            // and its coordinates are in millimeters.
            let mut checker = DesignRulesChecker::new(source.board);
            let report = checker.generate_report(source.design_name, "mm");
            out.write_all(report.to_json().as_bytes())?;
            out.flush()
        }
        OutputFormat::KicadPcb => {
            let Some(src) = source.kicad_source else {
                return Err(io::Error::other(
                    "a .kicad_pcb output needs a .kicad_pcb input (-de board.kicad_pcb)",
                ));
            };
            let (text, _) = render_kicad_board(&src.path, source.board, Some(src.hash))
                .map_err(io::Error::other)?;
            out.write_all(text.as_bytes())?;
            out.flush()
        }
    }
}

/// The temporary file of `path` for this process: `.<name>.<pid>.tmp` in the
/// same directory (so the rename stays on one file system).
pub fn temp_path(path: &Path) -> PathBuf {
    let name = path
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| "output".to_string());
    path.with_file_name(format!(".{name}.{}.tmp", std::process::id()))
}

/// Removes the temporary files that killed runs left next to `path`
/// (`.<name>.<digits>.tmp`). Returns how many were removed.
pub fn remove_stale_temp_files(path: &Path) -> usize {
    let Some(name) = path.file_name().map(|n| n.to_string_lossy().into_owned()) else {
        return 0;
    };
    let dir = match path.parent() {
        Some(parent) if !parent.as_os_str().is_empty() => parent,
        _ => Path::new("."),
    };
    let prefix = format!(".{name}.");
    let Ok(entries) = std::fs::read_dir(dir) else {
        return 0;
    };
    let mut removed = 0;
    for entry in entries.flatten() {
        let file_name = entry.file_name().to_string_lossy().into_owned();
        let stale = file_name
            .strip_prefix(&prefix)
            .and_then(|rest| rest.strip_suffix(".tmp"))
            .is_some_and(|pid| !pid.is_empty() && pid.bytes().all(|b| b.is_ascii_digit()));
        if stale && std::fs::remove_file(entry.path()).is_ok() {
            removed += 1;
        }
    }
    removed
}

/// A hook that runs between the temporary write and the rename. Tests use it
/// to stop the process there.
pub type BeforeRename = fn(&Path);

/// Writes a file atomically: `write` fills `.<name>.<pid>.tmp` through a
/// `BufWriter`, then the file is renamed to `path`. A result of 0 bytes is
/// [`OutputError::Empty`] and leaves nothing behind. Returns the size.
pub fn write_atomic<F>(
    path: &Path,
    format: OutputFormat,
    create_dirs: bool,
    before_rename: Option<BeforeRename>,
    write: F,
) -> Result<u64, OutputError>
where
    F: FnOnce(&mut BufWriter<File>) -> io::Result<()>,
{
    let io_error = |source: io::Error| OutputError::Io {
        path: path.to_path_buf(),
        source,
    };
    if create_dirs && let Some(parent) = path.parent().filter(|p| !p.as_os_str().is_empty()) {
        std::fs::create_dir_all(parent).map_err(io_error)?;
    }
    let temp = temp_path(path);
    let result = (|| -> io::Result<u64> {
        let mut writer = BufWriter::new(File::create(&temp)?);
        write(&mut writer)?;
        writer.flush()?;
        let size = writer.get_ref().metadata()?.len();
        drop(writer);
        Ok(size)
    })();
    let size = match result {
        Ok(size) => size,
        Err(e) => {
            let _ = std::fs::remove_file(&temp);
            return Err(io_error(e));
        }
    };
    if size == 0 {
        let _ = std::fs::remove_file(&temp);
        return Err(OutputError::Empty {
            path: path.to_path_buf(),
            format,
        });
    }
    if let Some(hook) = before_rename {
        hook(&temp);
    }
    if let Err(e) = std::fs::rename(&temp, path) {
        let _ = std::fs::remove_file(&temp);
        return Err(io_error(e));
    }
    Ok(size)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scratch(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("fr-outputs-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn the_format_table() {
        let of = |p: &str| OutputFormat::of_path(Path::new(p));
        assert_eq!(of("a.ses").unwrap(), OutputFormat::Ses);
        assert_eq!(of("dir/A.SES").unwrap(), OutputFormat::Ses);
        assert_eq!(of("a.dsn").unwrap(), OutputFormat::Dsn);
        assert_eq!(of("a.rules").unwrap(), OutputFormat::Rules);
        assert_eq!(of("a.drc").unwrap(), OutputFormat::DrcReport);
        assert_eq!(of("a.drc.json").unwrap(), OutputFormat::DrcReport);
        assert_eq!(of("A.DRC.JSON").unwrap(), OutputFormat::DrcReport);
        assert_eq!(of("a.kicad_pcb").unwrap(), OutputFormat::KicadPcb);
        assert_eq!(of("A.KICAD_PCB").unwrap(), OutputFormat::KicadPcb);
        for rejected in ["a.frb", "a.scr", "a.json", "a.xyz", "noext", ".ses.x"] {
            assert!(
                matches!(of(rejected), Err(OutputError::Unsupported { .. })),
                "{rejected}"
            );
        }
    }

    #[test]
    fn classify_reports_the_first_bad_path() {
        let paths = [
            PathBuf::from("a.ses"),
            PathBuf::from("b.frb"),
            PathBuf::from("c.xyz"),
        ];
        let error = classify(&paths).unwrap_err();
        assert!(error.to_string().contains("b.frb"), "{error}");
        assert_eq!(classify(&paths[..1]).unwrap().len(), 1);
    }

    #[test]
    fn atomic_write_renames_the_temp_file() {
        let dir = scratch("atomic");
        let path = dir.join("out.ses");
        let size = write_atomic(&path, OutputFormat::Ses, false, None, |w| {
            w.write_all(b"(session)")
        })
        .unwrap();
        assert_eq!(size, 9);
        assert_eq!(std::fs::read(&path).unwrap(), b"(session)");
        assert!(!temp_path(&path).exists());
    }

    #[test]
    fn zero_bytes_are_an_error_and_leave_nothing() {
        let dir = scratch("empty");
        let path = dir.join("out.ses");
        let error = write_atomic(&path, OutputFormat::Ses, false, None, |_| Ok(())).unwrap_err();
        assert!(matches!(error, OutputError::Empty { .. }));
        assert!(error.to_string().starts_with("Nothing to save to"));
        assert_eq!(std::fs::read_dir(&dir).unwrap().count(), 0);
    }

    #[test]
    fn a_failing_serializer_leaves_nothing_and_keeps_an_old_file() {
        let dir = scratch("failing");
        let path = dir.join("out.ses");
        std::fs::write(&path, b"old").unwrap();
        let error = write_atomic(&path, OutputFormat::Ses, false, None, |w| {
            w.write_all(b"partial")?;
            Err(io::Error::other("boom"))
        })
        .unwrap_err();
        assert!(matches!(error, OutputError::Io { .. }));
        assert_eq!(std::fs::read(&path).unwrap(), b"old");
        assert_eq!(std::fs::read_dir(&dir).unwrap().count(), 1);
    }

    #[test]
    fn stale_temp_files_are_removed() {
        let dir = scratch("stale");
        let path = dir.join("out.ses");
        std::fs::write(dir.join(".out.ses.4242.tmp"), b"half").unwrap();
        std::fs::write(dir.join(".out.ses.notapid.tmp"), b"keep").unwrap();
        std::fs::write(dir.join(".other.ses.4242.tmp"), b"keep").unwrap();
        assert_eq!(remove_stale_temp_files(&path), 1);
        assert!(!dir.join(".out.ses.4242.tmp").exists());
        assert!(dir.join(".out.ses.notapid.tmp").exists());
        assert!(dir.join(".other.ses.4242.tmp").exists());
    }

    #[test]
    fn create_dirs_makes_the_parent() {
        let dir = scratch("dirs");
        let path = dir.join("a/b/out.drc.json");
        write_atomic(&path, OutputFormat::DrcReport, true, None, |w| {
            w.write_all(b"{}")
        })
        .unwrap();
        assert!(path.exists());
    }
}
