//! Writing files (G08): Save As, the `.rules` file, and the output of the
//! front-end mode. No window code is here; [`crate::app_files`] is `App`'s
//! side.
//!
//! **Java.** `BoardFrameFileActions.saveAs` writes a session
//! (`BoardExportActions.saveAsSpecctraSessionSes`: the design name is the
//! *input* file's name, `board.dsn`, so the session is named `board.ses` and
//! its `base_design` is `board.dsn`) or a design
//! (`saveAsSpecctraDesignDsn`), and after a session asks
//! `confirm_rules_save` and writes `<output>.rules`
//! (`RoutingJob.getRulesFile`). The front-end mode's
//! `GuiManager.exportBoardToFile` takes the design name from the *output*
//! file's stem and writes nothing when the result is empty. The `.scr` and
//! `.json` exports, and `.frb`, are not offered (G00 question 2).
//!
//! **No partial files.** Every write goes to a temporary file in the target's
//! folder and is renamed over the target (`job::outputs::write_atomic`, the
//! writer of the CLI). A failed write leaves the old file as it was, and an
//! empty result is an error, as in Java (`not overwriting`).
//!
//! **"Changed" means the content changed.** Java's `isBoardChanged` compares a
//! CRC32 of the board written as DSN with the one taken at load or at the
//! last save (`HeadlessBoardManager.calculateCrc32`). [`content_hash`] is that
//! checksum; `Session::is_changed` checks the cheap edit counter first.
//! (B01's `state_hash` is not used: it covers the undo history too, so an
//! edit followed by an undo would stay "changed".)

use std::path::{Path, PathBuf};

use freerouting::board::RoutingBoard;
use freerouting::io::specctra::dsn_writer::{write_dsn, write_dsn_to};
use freerouting::io::specctra::rules_writer::write_rules;
use freerouting::io::specctra::ses_writer::{SesStyle, write_ses_to};
use freerouting::job::outputs::{OutputFormat, write_atomic};

use crate::text;

/// The design name Java gives `calculateCrc32ForBoard`'s DSN writer.
const CHECKSUM_DESIGN_NAME: &str = "N/A";

/// `java.util.zip.CRC32` of `bytes`.
pub fn crc32(bytes: &[u8]) -> u32 {
    let mut table = [0u32; 256];
    for (i, slot) in table.iter_mut().enumerate() {
        let mut c = i as u32;
        for _ in 0..8 {
            c = if c & 1 == 1 {
                0xEDB8_8320 ^ (c >> 1)
            } else {
                c >> 1
            };
        }
        *slot = c;
    }
    !bytes.iter().fold(!0u32, |crc, &b| {
        table[((crc ^ u32::from(b)) & 0xFF) as usize] ^ (crc >> 8)
    })
}

/// The checksum of the board's content (Java: the CRC32 of the DSN text
/// written with the name `N/A`). `None` if the DSN writer fails, which the
/// caller must treat as "changed".
pub fn content_hash(board: &RoutingBoard) -> Option<u32> {
    std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        crc32(write_dsn(board, CHECKSUM_DESIGN_NAME).as_bytes())
    }))
    .ok()
}

/// What Save As can write (G00 question 2: no `.frb`, `.scr` or JSON).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SaveKind {
    /// A Specctra session file.
    Ses,
    /// A Specctra design file.
    Dsn,
}

impl SaveKind {
    /// The kind of an output file name, by its extension (ignoring case).
    pub fn of_path(path: &Path) -> Option<SaveKind> {
        match path
            .extension()?
            .to_string_lossy()
            .to_ascii_lowercase()
            .as_str()
        {
            "ses" => Some(SaveKind::Ses),
            "dsn" => Some(SaveKind::Dsn),
            _ => None,
        }
    }

    pub fn extension(self) -> &'static str {
        match self {
            SaveKind::Ses => "ses",
            SaveKind::Dsn => "dsn",
        }
    }

    /// Java's file filter text (`BoardExportActions.showSaveAsDialog`).
    pub fn filter(self) -> &'static str {
        match self {
            SaveKind::Ses => text::SAVE_FILTER_SES,
            SaveKind::Dsn => text::SAVE_FILTER_DSN,
        }
    }

    fn format(self) -> OutputFormat {
        match self {
            SaveKind::Ses => OutputFormat::Ses,
            SaveKind::Dsn => OutputFormat::Dsn,
        }
    }
}

/// The design name Save As passes to the writers: the design file's name with
/// `.dsn` (Java's `routingJob.input.getFilename()`; a KiCad board is named
/// after its stem).
pub fn save_design_name(design: &Path) -> String {
    match design
        .extension()
        .map(|e| e.to_string_lossy().to_lowercase())
    {
        Some(e) if e == "dsn" => design
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default(),
        _ => format!(
            "{}.dsn",
            design
                .file_stem()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_default()
        ),
    }
}

/// The design name of the front-end export: the output's file name up to its
/// last dot (`exportBoardToFile`).
pub fn output_design_name(output: &Path) -> String {
    let name = output
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    match name.rfind('.') {
        Some(dot) if dot > 0 => name[..dot].to_string(),
        _ => name,
    }
}

/// `RoutingJob.getRulesFile`: the output's name with the extension changed
/// to `.rules`.
pub fn rules_file_for(output: &Path) -> PathBuf {
    output.with_extension("rules")
}

/// The default path of Save As: the last output, else `<design dir>/<design
/// stem>.ses`.
pub fn default_save_path(last_output: Option<&Path>, design: &Path) -> PathBuf {
    match last_output {
        Some(path) => path.to_path_buf(),
        None => design.with_extension("ses"),
    }
}

/// Why a write failed, as the status line says it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SaveError {
    /// Java's text for the failure (`message_specctra_ses_save_failed`,
    /// `error_design_save_failed`).
    pub message: String,
    /// What the file system or the writer said.
    pub detail: String,
}

impl std::fmt::Display for SaveError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{} ({})", self.message, self.detail)
    }
}

/// Writes `board` as `kind` to `path` (atomically). Returns the size in bytes.
pub fn save_as(
    board: &RoutingBoard,
    design_name: &str,
    path: &Path,
    kind: SaveKind,
) -> Result<u64, SaveError> {
    write_atomic(path, kind.format(), false, None, |out| match kind {
        SaveKind::Ses => write_ses_to(board, design_name, SesStyle::Java, out),
        SaveKind::Dsn => write_dsn_to(board, design_name, out),
    })
    .map_err(|e| SaveError {
        message: match kind {
            SaveKind::Ses => text::fill(
                text::MESSAGE_SPECCTRA_SES_SAVE_FAILED,
                &[&path.to_string_lossy()],
            ),
            SaveKind::Dsn => text::ERROR_DESIGN_SAVE_FAILED.to_string(),
        },
        detail: e.to_string(),
    })
}

/// Writes the board's rules to `path` (`BoardExportActions.saveRulesAs`, no
/// router settings), atomically.
pub fn save_rules(board: &RoutingBoard, design_name: &str, path: &Path) -> Result<u64, SaveError> {
    write_atomic(path, OutputFormat::Rules, false, None, |out| {
        use std::io::Write;
        out.write_all(write_rules(board, None, design_name).as_bytes())
    })
    .map_err(|e| SaveError {
        message: text::fill(text::ERROR_RULES_SAVE_FAILED, &[&path.to_string_lossy()]),
        detail: e.to_string(),
    })
}

/// The front-end mode's write (`GuiManager.exportBoardToFile`): `.ses` or
/// `.dsn` by extension, the design name from the output's stem, nothing
/// written for an empty result. The error is the log line.
pub fn export_output(board: &RoutingBoard, path: &Path) -> Result<u64, String> {
    let Some(kind) = SaveKind::of_path(path) else {
        return Err(format!(
            "Couldn't export board to '{}', unsupported extension",
            path.display()
        ));
    };
    save_as(board, &output_design_name(path), path, kind).map_err(|e| {
        format!(
            "Board export produced 0 bytes or failed; not overwriting '{}': {}",
            path.display(),
            e.detail
        )
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn crc32_is_java_crc32() {
        // The standard check value of CRC-32/ISO-HDLC, which `java.util.zip.CRC32` computes.
        assert_eq!(crc32(b"123456789"), 0xCBF4_3926);
        assert_eq!(crc32(b""), 0);
    }

    #[test]
    fn names_follow_java() {
        assert_eq!(save_design_name(Path::new("/x/board.dsn")), "board.dsn");
        assert_eq!(
            save_design_name(Path::new("/x/board.kicad_pcb")),
            "board.dsn"
        );
        assert_eq!(output_design_name(Path::new("/x/out.ses")), "out");
        assert_eq!(output_design_name(Path::new("/x/a.b.ses")), "a.b");
        assert_eq!(output_design_name(Path::new("/x/.ses")), ".ses");
        assert_eq!(
            rules_file_for(Path::new("/x/out.ses")),
            Path::new("/x/out.rules")
        );
        assert_eq!(
            default_save_path(None, Path::new("/d/board.dsn")),
            Path::new("/d/board.ses")
        );
        assert_eq!(SaveKind::of_path(Path::new("a.SES")), Some(SaveKind::Ses));
        assert_eq!(SaveKind::of_path(Path::new("a.dsn")), Some(SaveKind::Dsn));
        assert_eq!(SaveKind::of_path(Path::new("a.scr")), None);
        assert_eq!(SaveKind::of_path(Path::new("a")), None);
    }
}
