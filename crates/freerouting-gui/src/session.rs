//! The open design: the engine board, the design it was built from, and the
//! files it came from. Loading reads a `.dsn` (and optionally a `.ses` on top
//! of it) and flattens the board into a [`Scene`] for drawing.
//!
//! The session keeps the board, which ratsnest, DRC, item info, editing,
//! saving and routing all need. Background jobs take an `Arc` clone of it
//! (free); an edit goes through [`Session::board_mut`], which copies the
//! board only while a job still holds the old one.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use freerouting::board::RoutingBoard;
use freerouting::dsn::{self, Pcb, Session as SesFile};
use freerouting::io::specctra::{BoardReadResult, SesImportSummary, build_board, import_session};

use crate::scene::Scene;
use crate::view::Tessellation;

/// The files a session was read from.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct SessionPaths {
    pub design: PathBuf,
    pub session: Option<PathBuf>,
    pub rules: Option<PathBuf>,
}

/// The open design and its board.
pub struct Session {
    pub board: Arc<RoutingBoard>,
    pub design: Arc<Pcb>,
    pub paths: SessionPaths,
    /// Bumped by every command that mutates the session. `board.revision()`
    /// counts inserts and removes only, so it would miss rule edits.
    pub edit_generation: u64,
    pub saved_generation: u64,
    /// What the readers skipped or did not understand, as text.
    pub warnings: Vec<String>,
    /// What importing the `.ses` did, if one was opened.
    pub ses_summary: Option<SesImportSummary>,
}

impl Session {
    pub fn is_dirty(&self) -> bool {
        self.edit_generation != self.saved_generation
    }

    /// The board for editing. Copies it first if a background job still
    /// shares it, and counts the edit.
    pub fn board_mut(&mut self) -> &mut RoutingBoard {
        self.edit_generation += 1;
        Arc::make_mut(&mut self.board)
    }
}

/// A board that has been read and tessellated.
pub struct Loaded {
    pub session: Session,
    pub scene: Scene,
    pub tess: Tessellation,
}

/// The design a session file was written for, if one is already open.
pub type CurrentDesign = (Arc<Pcb>, PathBuf);

fn read_text(path: &Path) -> Result<String, String> {
    let bytes = std::fs::read(path).map_err(|e| format!("{}: {e}", path.display()))?;
    Ok(dsn::decode(&bytes).into_owned())
}

fn read_design(path: &Path) -> Result<(Pcb, Vec<String>), String> {
    let (pcb, warnings) = Pcb::parse_with_warnings(&read_text(path)?)
        .map_err(|e| format!("{}: {e}", path.display()))?;
    Ok((pcb, warnings.iter().map(ToString::to_string).collect()))
}

/// `name` without a trailing `.dsn`: sessions written by the Java version
/// name their design with the extension.
fn design_stem(name: &str) -> &str {
    match name.len().checked_sub(4) {
        Some(at) if name.is_char_boundary(at) && name[at..].eq_ignore_ascii_case(".dsn") => {
            &name[..at]
        }
        _ => name,
    }
}

/// The design a session belongs to: `base_design`, the session's file stem
/// or its own name, with `.dsn`, next to the session.
pub fn find_design(session_path: &Path, session: &SesFile) -> Option<PathBuf> {
    let dir = session_path.parent()?;
    let mut names = Vec::new();
    if let Some(base) = &session.base_design {
        names.push(base.clone());
    }
    if let Some(stem) = session_path.file_stem() {
        names.push(stem.to_string_lossy().into_owned());
    }
    names.push(session.name.clone());
    names.iter().find_map(|n| {
        let p = dir.join(format!("{}.dsn", design_stem(n)));
        p.is_file().then_some(p)
    })
}

/// The engine board of a design. The board comes out of the read result by
/// value.
fn build(
    pcb: &Pcb,
    design_path: &Path,
    warnings: &mut Vec<String>,
) -> Result<RoutingBoard, String> {
    match build_board(pcb) {
        BoardReadResult::Success { board, warnings: w } => {
            warnings.extend(w);
            Ok(*board)
        }
        BoardReadResult::OutlineMissing {
            board: Some(board),
            warnings: w,
        } => {
            warnings.push("The design has no usable board outline.".into());
            warnings.extend(w);
            Ok(*board)
        }
        BoardReadResult::OutlineMissing { board: None, .. } => Err(format!(
            "{}: the design has no usable board outline.",
            design_path.display()
        )),
        BoardReadResult::ParseError { detail, .. } => {
            Err(format!("{}: {detail}", design_path.display()))
        }
    }
}

/// Reads `path`, a design or a session. A session goes on top of its own
/// design if that is found next to it, else on top of `current`.
///
/// The session's wires and vias are imported by the engine
/// (`import_session`, the port of Java's `SesReader.read`) on top of the
/// design's own wiring, as Java's GUI does.
pub fn load(path: &Path, current: Option<CurrentDesign>) -> Result<Loaded, String> {
    let is_session = path
        .extension()
        .is_some_and(|e| e.eq_ignore_ascii_case("ses"));
    let mut warnings = Vec::new();
    let mut ses = None;
    let (pcb, design_path) = if is_session {
        let (file, w) = SesFile::parse_with_warnings(&read_text(path)?)
            .map_err(|e| format!("{}: {e}", path.display()))?;
        warnings.extend(w.iter().map(ToString::to_string));
        let found = find_design(path, &file);
        ses = Some(file);
        match found {
            Some(design) => {
                let (pcb, w) = read_design(&design)?;
                warnings.extend(w);
                (Arc::new(pcb), design)
            }
            None => current.ok_or_else(|| {
                format!(
                    "No design file found for session {}. Open the .dsn first, then the .ses.",
                    path.display()
                )
            })?,
        }
    } else {
        let (pcb, w) = read_design(path)?;
        warnings.extend(w);
        (Arc::new(pcb), path.to_path_buf())
    };
    let mut board = build(&pcb, &design_path, &mut warnings)?;
    let ses_summary = ses.map(|file| import_session(&mut board, &file));
    let board = Arc::new(board);
    let scene = Scene::from_board(&pcb.name, &board);
    let tess = Tessellation::new(&scene);
    Ok(Loaded {
        session: Session {
            board,
            design: pcb,
            paths: SessionPaths {
                design: design_path,
                session: is_session.then(|| path.to_path_buf()),
                rules: None,
            },
            edit_generation: 0,
            saved_generation: 0,
            warnings,
            ses_summary,
        },
        scene,
        tess,
    })
}

/// Reads `paths` in order: a design, then optionally a session on top of it.
/// `current` is the design already open, for a session without a design file.
pub fn load_all(paths: &[PathBuf], mut current: Option<CurrentDesign>) -> Result<Loaded, String> {
    let mut last = None;
    for path in paths {
        let loaded = load(path, current.take())?;
        current = Some((
            loaded.session.design.clone(),
            loaded.session.paths.design.clone(),
        ));
        last = Some(loaded);
    }
    last.ok_or_else(|| "No file to open.".into())
}

#[cfg(test)]
mod tests {
    use super::*;
    use freerouting::io::specctra::read_session;
    use freerouting::scoring::BoardStatistics;

    fn fixtures() -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures")
    }

    fn small(path: &Path) -> bool {
        std::fs::metadata(path).is_ok_and(|m| m.len() < 50_000)
    }

    #[test]
    fn keeps_board_issue754() {
        let loaded = load(&fixtures().join("Issue754-avionics_hub.dsn"), None).unwrap();
        let stats = BoardStatistics::new(&loaded.session.board);
        // fixtures/reference/load/Issue754-avionics_hub.json, statistics.
        assert_eq!(stats.items.total_count, 2384);
        assert_eq!(stats.connections.incomplete_count, 221);
        assert!(!loaded.session.is_dirty());
    }

    #[test]
    fn make_mut_copies_only_when_shared() {
        // Issue026's session puts traces and vias on the board.
        let mut loaded = load(&fixtures().join("Issue026-J2_reference.ses"), None).unwrap();
        let session = &mut loaded.session;
        let count = session.board.item_count();

        // A job holds the board: the edit copies it, the job's board stays.
        let job = Arc::clone(&session.board);
        session.board_mut().delete_all_traces_and_vias();
        assert!(!Arc::ptr_eq(&job, &session.board));
        assert_eq!(job.item_count(), count, "the shared board is untouched");
        let after = session.board.item_count();
        assert!(after < count);
        assert_eq!(session.edit_generation, 1);
        assert!(session.is_dirty());

        // Nobody else holds it: the edit is in place.
        drop(job);
        let before = Arc::as_ptr(&session.board);
        session.board_mut().delete_all_traces_and_vias();
        assert_eq!(Arc::as_ptr(&session.board), before, "no copy when unshared");
        assert_eq!(session.board.item_count(), after);
        assert_eq!(session.edit_generation, 2);
    }

    fn counts(board: &RoutingBoard) -> (usize, usize, usize) {
        let s = BoardStatistics::new(board);
        (
            s.items.trace_count,
            s.items.via_count,
            s.connections.incomplete_count,
        )
    }

    /// The statistics of the path that `tests/reference_drc.rs` compares
    /// with Java: `build_board`, then `read_session`.
    fn engine_counts(dsn: &Path, ses: &Path) -> (usize, usize, usize) {
        let result = dsn_board(dsn);
        let mut board = *result;
        read_session(&mut board, &read_text(ses).unwrap()).unwrap();
        counts(&board)
    }

    fn dsn_board(dsn: &Path) -> Box<RoutingBoard> {
        match build_board(&read_design(dsn).unwrap().0) {
            BoardReadResult::Success { board, .. } => board,
            other => panic!("{}: {other:?}", dsn.display()),
        }
    }

    fn compare_sessions(only_small: bool) -> usize {
        let routes = fixtures().join("reference/route");
        let mut compared = 0;
        let mut files: Vec<_> = std::fs::read_dir(&routes)
            .unwrap()
            .map(|e| e.unwrap().path())
            .filter(|p| p.extension().is_some_and(|e| e == "ses"))
            .collect();
        files.sort();
        for ses in files {
            let dsn = fixtures()
                .join(ses.file_stem().unwrap())
                .with_extension("dsn");
            if !dsn.is_file() || (only_small && !small(&dsn)) {
                continue;
            }
            // A few fixtures are not DSN text at all (Issue006's is an OLE2
            // document); the engine path cannot load those either.
            let Ok(design) = load(&dsn, None) else {
                assert!(read_design(&dsn).is_err(), "{}", dsn.display());
                continue;
            };
            let on_top = load(&ses, Some((design.session.design.clone(), dsn.clone())))
                .unwrap_or_else(|e| panic!("{}: {e}", ses.display()));
            assert_eq!(
                counts(&on_top.session.board),
                engine_counts(&dsn, &ses),
                "{}",
                ses.display()
            );
            compared += 1;
        }
        compared
    }

    #[test]
    fn sessions_match_engine_path() {
        assert!(compare_sessions(true) >= 5);
    }

    #[test]
    #[ignore = "release only: builds every routed board"]
    fn sessions_match_engine_path_all() {
        assert!(compare_sessions(false) >= 50);
    }

    #[test]
    fn finds_design_by_base_design() {
        // Session file -> the design it must find (`None`: no design file
        // of that name exists in `fixtures/`).
        let expected: [(&str, Option<&str>); 14] = [
            ("Issue026-J2_reference", Some("Issue026-J2_reference")),
            ("Issue313-FastTest", Some("Issue313-FastTest")),
            // base_design names the design with its extension.
            (
                "Issue368-CorneyIslandWireless_output_session",
                Some("Issue368-CorneyIslandWireless_input_design"),
            ),
            ("Issue508-DAC2020_bm05", Some("Issue508-DAC2020_bm05")),
            // base_design Issue508-DAC2020_bm07.
            (
                "Issue508-DAC2020_bm07-current",
                Some("Issue508-DAC2020_bm07"),
            ),
            ("Issue508-DAC2020_bm07-v190", None),
            ("Issue508-DAC2020_bm08-routed", None),
            // base_design Issue508-DAC2020_bm01.
            (
                "Issue555-BBD_Mars-64-current",
                Some("Issue508-DAC2020_bm01"),
            ),
            ("Issue555-BBD_Mars-64-v190", None),
            // base_design with extension; the stem names it too.
            ("Issue593-BBD_Mars-64", Some("Issue593-BBD_Mars-64")),
            ("Issue690-ecc83", Some("Issue690-ecc83")),
            // base_design names ..._input_board.dsn, which does not exist.
            ("Issue733-kicad_complex_hierarchy_output_session", None),
            ("Issue742-tastexx-pcb", Some("Issue742-tastexx-pcb")),
            // base_design corney_island_wireless; the stem names it.
            (
                "Issue951-corney_island_wireless",
                Some("Issue951-corney_island_wireless"),
            ),
        ];
        let on_disk = std::fs::read_dir(fixtures())
            .unwrap()
            .filter(|e| {
                e.as_ref()
                    .unwrap()
                    .path()
                    .extension()
                    .is_some_and(|x| x == "ses")
            })
            .count();
        assert_eq!(on_disk, expected.len());
        for (ses, design) in expected {
            let path = fixtures().join(format!("{ses}.ses"));
            let file = SesFile::parse(&read_text(&path).unwrap()).unwrap();
            let found = find_design(&path, &file)
                .map(|p| p.file_stem().unwrap().to_string_lossy().into_owned());
            assert_eq!(found.as_deref(), design, "{ses}");
        }
    }

    #[test]
    fn opening_a_session_alone_uses_its_design() {
        let loaded = load(&fixtures().join("Issue026-J2_reference.ses"), None).unwrap();
        assert!(
            loaded
                .session
                .paths
                .design
                .ends_with("Issue026-J2_reference.dsn")
        );
        let summary = loaded.session.ses_summary.unwrap();
        assert!(summary.wires_imported > 0 && summary.errors_encountered == 0);
        assert!(
            load(
                &fixtures().join("Issue733-kicad_complex_hierarchy_output_session.ses"),
                None
            )
            .is_err()
        );
    }
}
