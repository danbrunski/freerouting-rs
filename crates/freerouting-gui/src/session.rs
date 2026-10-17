//! The open design: the engine board, the design it was built from, and the
//! files it came from. Loading reads a `.dsn` or a `.kicad_pcb` (K07.2; and
//! optionally a `.ses` on top
//! of it) and flattens the board into a [`Scene`] for drawing.
//!
//! The session keeps the board, which ratsnest, DRC, item info, editing,
//! saving and routing all need. Background jobs take an `Arc` clone of it
//! (free); an edit goes through [`Session::board_mut`], which copies the
//! board only while a job still holds the old one.

use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Instant;

use freerouting::board::items::ItemId;
use freerouting::board::{BoardChanges, RoutingBoard};
use freerouting::dsn::{self, Pcb, Session as SesFile};
use freerouting::io::kicad::{DesignKind, ReadOptions, design_kind, read_kicad_input};
use freerouting::io::specctra::{BoardReadResult, SesImportSummary, build_board, import_session};

use crate::scene::{Flattener, Scene};
use crate::stats::LoadTimings;
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

impl Session {
    /// Runs `f` on the board for editing and counts the edit when `f`
    /// returns true (G03). Like [`board_mut`](Self::board_mut), it copies
    /// the board first if a background job still shares it.
    pub fn edit(&mut self, f: impl FnOnce(&mut RoutingBoard) -> bool) -> bool {
        let changed = f(Arc::make_mut(&mut self.board));
        if changed {
            self.edit_generation += 1;
        }
        changed
    }
}

/// A board that has been read and tessellated.
pub struct Loaded {
    pub session: Session,
    pub scene: Scene,
    pub tess: Tessellation,
    /// How long the phases of the load took.
    pub timings: LoadTimings,
}

impl Loaded {
    /// Builds the scene and its tessellation again from the session board,
    /// in full. After an edit use [`sync_scene`](Self::sync_scene), which
    /// patches; this is the fallback for a journal that overflowed.
    pub fn rebuild_scene(&mut self) {
        self.scene = Scene::from_board(&self.session.design.name, &self.session.board);
        self.tess = Tessellation::new(&self.scene);
    }
}

/// How [`Loaded::sync_scene`] brought the drawing up to date (G05.5).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct SceneSync {
    /// Engine items the journal reported as gone or changed.
    pub removed: usize,
    /// Engine items flattened again (new or changed).
    pub added: usize,
    /// True if the journal overflowed (or was off) and the scene was built
    /// again in full.
    pub rebuilt: bool,
    /// The chunks the patch rebuilt, ascending; empty after a rebuild or
    /// when nothing changed.
    pub dirty_chunks: Vec<u32>,
}

impl Loaded {
    /// Brings the scene and the chunks up to date with the board after an
    /// edit, from the board's change journal (B01.7): the chunks that own a
    /// removed or changed item and the ones that receive an added item are
    /// rebuilt (`Tessellation::patch`, G02.4), nothing else. Only a journal
    /// that overflowed, or one that was off, makes a full rebuild.
    pub fn sync_scene(&mut self) -> SceneSync {
        let board = Arc::make_mut(&mut self.session.board);
        if !board.is_journaling() {
            board.set_journaling(true);
            self.rebuild_scene();
            return SceneSync {
                rebuilt: true,
                ..SceneSync::default()
            };
        }
        let changes = board.take_changes();
        self.sync_scene_with(changes)
    }

    /// [`sync_scene`](Self::sync_scene) for changes that the caller already
    /// took from the journal: the editor drains the journal after each
    /// command and returns the changes in its response (G09).
    pub fn sync_scene_with(&mut self, changes: BoardChanges) -> SceneSync {
        if changes.overflow {
            self.rebuild_scene();
            return SceneSync {
                rebuilt: true,
                ..SceneSync::default()
            };
        }
        // A changed item is flattened again: its shape may differ.
        let gone: Vec<ItemId> = changes
            .removed
            .iter()
            .chain(&changes.changed)
            .copied()
            .collect();
        let fresh: Vec<ItemId> = changes
            .inserted
            .iter()
            .chain(&changes.changed)
            .copied()
            .collect();
        if gone.is_empty() && fresh.is_empty() {
            return SceneSync::default();
        }
        let flattener = Flattener::new(&self.session.board);
        let mut added = Vec::new();
        for id in &fresh {
            if let Some(item) = self.session.board.basic().item(*id) {
                flattener.flatten(item, &mut added);
            }
        }
        let report = self.patch_scene(&gone, added);
        SceneSync {
            removed: gone.len(),
            added: fresh.len(),
            rebuilt: false,
            dirty_chunks: report.dirty_chunks,
        }
    }

    /// Takes the scene items of the engine items `gone` out of the scene and
    /// the chunks and puts `added` in (G07.4 patches the router's deltas
    /// through this too). `added` holds all the scene items of an engine
    /// item together, in flatten order.
    pub fn patch_scene(
        &mut self,
        gone: &[ItemId],
        added: Vec<crate::scene::Item>,
    ) -> crate::canvas::chunks::PatchReport {
        let report = self.tess.patch(gone, &added);
        let gone_set: HashSet<ItemId> = gone.iter().copied().collect();
        self.scene.items.retain(|i| !gone_set.contains(&i.id));
        self.scene.items.extend(added);
        self.scene.build_index();
        report
    }
}

/// The design a session file was written for, if one is already open.
pub type CurrentDesign = (Arc<Pcb>, PathBuf);

fn read_text(path: &Path) -> Result<String, String> {
    let bytes = std::fs::read(path).map_err(|e| format!("{}: {e}", path.display()))?;
    Ok(dsn::decode(&bytes).into_owned())
}

#[cfg(test)]
fn read_design(path: &Path) -> Result<(Pcb, Vec<String>), String> {
    read_design_timed(path, &mut LoadTimings::default())
}

/// Reads a design file into its syntax tree: a `.kicad_pcb` through the
/// engine's direct reader (K07.2; its warnings and its read report, rule areas
/// and `.kicad_dru` rules included, come back as warnings), anything else as
/// a DSN.
fn read_design_timed(path: &Path, timings: &mut LoadTimings) -> Result<(Pcb, Vec<String>), String> {
    if design_kind(path) == DesignKind::KicadPcb {
        let started = Instant::now();
        let input = read_kicad_input(path, ReadOptions::default())
            .map_err(|e| format!("{}: {e}", path.display()))?;
        timings.parse_ms += started.elapsed().as_secs_f64() * 1000.0;
        let mut warnings: Vec<String> = input
            .warnings
            .iter()
            .map(|w| format!("KiCad reader: {w}"))
            .collect();
        warnings.extend(
            input
                .report_lines()
                .into_iter()
                .map(|l| format!("KiCad read report: {l}")),
        );
        return Ok((input.pcb, warnings));
    }
    let started = Instant::now();
    let text = read_text(path)?;
    timings.read_ms += started.elapsed().as_secs_f64() * 1000.0;
    let started = Instant::now();
    let (pcb, warnings) =
        Pcb::parse_with_warnings(&text).map_err(|e| format!("{}: {e}", path.display()))?;
    timings.parse_ms += started.elapsed().as_secs_f64() * 1000.0;
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
    // a DSN first, then the KiCad board a session of the direct reader names
    names.iter().find_map(|n| {
        ["dsn", "kicad_pcb"].iter().find_map(|ext| {
            let p = dir.join(format!("{}.{ext}", design_stem(n)));
            p.is_file().then_some(p)
        })
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
    let started = Instant::now();
    let mut timings = LoadTimings::default();
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
                let (pcb, w) = read_design_timed(&design, &mut timings)?;
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
        let (pcb, w) = read_design_timed(path, &mut timings)?;
        warnings.extend(w);
        (Arc::new(pcb), path.to_path_buf())
    };
    let phase = Instant::now();
    let mut board = build(&pcb, &design_path, &mut warnings)?;
    let ses_summary = ses.map(|file| import_session(&mut board, &file));
    // From here on the board records what changes, so an edit patches the
    // drawing instead of rebuilding it (G05.5).
    board.set_journaling(true);
    timings.board_ms = phase.elapsed().as_secs_f64() * 1000.0;
    let board = Arc::new(board);
    let phase = Instant::now();
    let scene = Scene::from_board(&pcb.name, &board);
    timings.scene_ms = phase.elapsed().as_secs_f64() * 1000.0;
    let tess = Tessellation::new(&scene);
    let built = tess.build_times();
    timings.tessellation_ms = built.tessellation_ms;
    timings.chunking_ms = built.chunking_ms;
    timings.total_ms = started.elapsed().as_secs_f64() * 1000.0;
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
        timings,
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
