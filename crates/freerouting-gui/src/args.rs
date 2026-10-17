//! Command-line arguments:
//! `freerouting-gui [board.dsn [board.ses]] [--bench pan-zoom|drag|route] [--frames N]
//! [--screenshot out.png] [--exit] [--control-dir dir]
//! [--bench-synthetic 4x4 board.dsn]`, and Java's front-end flags (G08.4):
//! `-de board.dsn [board.ses] [board.rules]`, `-do out.ses`, `-dr board.rules`,
//! `-dct seconds` (and `-di dir`, which is read and ignored), plus the router
//! flags of the CLI (K02.5): `-mp`, `-mt`, `-inc`, `-oit`, `--router.<path>=`,
//! `--optimizer.<path>=`, the scoring flags and the aliases `--max-passes`,
//! `--threads`, `--timeout`, `--result-json`.
//!
//! The front-end flags are those of `GlobalSettings.applyCommandLineArguments`
//! (at the pin): `-de` takes the files up to the next `-...` argument and sorts
//! them by extension (`.dsn` or `.kicad_pcb` is the design, `.ses` the session,
//! `.rules` the rules); `a.dsn+b.ses` is split when no such file exists; `-do`
//! takes the output files up to the next `-...` argument, the first is the
//! output and the others are only reported; `-dct` is the auto-start
//! countdown in seconds (a value of 0 or less means no countdown). `--autoroute`
//! is `-dct 0`.
//!
//! **Router flags** are not parsed here: they are collected and handed to the
//! engine's `freerouting::settings::parse_router_flags`, the same mapping the
//! CLI uses, so `freerouting-gui -de d.dsn -do o.ses -mt 4 -oit 0.05` and the
//! CLI mean the same. The result is [`FrontEndOptions::router`], laid over the
//! settings of the job (over the saved ones from the settings window, as on
//! the CLI's command line). A bad value is an error (exit 2). `-mp` is
//! clamped to 0..=9999 as Java's GUI does. `--result-json` and `--timeout`
//! are read but the interactive job does not write a manifest or stop on a
//! timeout. `-mp -1` is not a value (a value never starts with `-`, as in the
//! CLI), so it is an unknown option.

use std::ffi::OsString;
use std::path::PathBuf;

use freerouting::settings::SettingsPatch;
use freerouting::settings::cli_flags::{parse_router_flags, router_flag_span};

/// Frames a `--bench` run counts when `--frames` is not given.
pub const DEFAULT_FRAMES: usize = 600;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BenchScript {
    /// Zoom in, pan and zoom out around the board, repeated.
    PanZoom,
    /// Drag one component back and forth with the drag tool (G09.7, FT7),
    /// the camera fixed on the fitted board.
    Drag,
    /// Starts the autorouter on the open board and runs until it ends
    /// (G07.4, FT4 to FT6). `FREEROUTING_BENCH_PASSES` limits the passes and
    /// `FREEROUTING_BENCH_CANCEL_MS` presses Cancel after that long.
    Route,
}

impl BenchScript {
    pub fn name(self) -> &'static str {
        match self {
            BenchScript::PanZoom => "pan-zoom",
            BenchScript::Drag => "drag",
            BenchScript::Route => "route",
        }
    }
}

/// Java's `GuiApplicationSettings.dialogConfirmationTimeout` default.
pub const DEFAULT_CONFIRMATION_TIMEOUT: u32 = 5;

/// The front-end flags (`-do`, `-dr`, `-dct`): what the KiCad plugin's "Route
/// in GUI" passes. `-de` fills [`Args::open`].
#[derive(Clone, Debug, PartialEq)]
pub struct FrontEndOptions {
    /// `-do`: the file the session is written to when routing ends.
    pub output: Option<PathBuf>,
    /// The other `-do` files. Java's GUI writes the first only.
    pub additional_outputs: Vec<PathBuf>,
    /// `-dr`, or the `.rules` file of `-de`: read at start without asking.
    pub rules: Option<PathBuf>,
    /// `-dct`: seconds of the auto-start countdown; 0 starts at once.
    pub confirmation_timeout: u32,
    /// The router flags of the command line (`-mp`, `-mt`, `--router.*`, ...)
    /// as a settings patch; `autorouter.max_passes` (`-mp`, 0 is no limit) is
    /// clamped to 0..=9999 as in Java.
    pub router: SettingsPatch,
    /// Warnings of reading the router flags (deprecated flags, capped
    /// values), for the log window.
    pub router_warnings: Vec<String>,
    /// True if a design file was named (by `-de` or positionally), which
    /// Java needs, with `-do`, to auto-start.
    pub design_given: bool,
}

impl Default for FrontEndOptions {
    fn default() -> Self {
        FrontEndOptions {
            output: None,
            additional_outputs: Vec::new(),
            rules: None,
            confirmation_timeout: DEFAULT_CONFIRMATION_TIMEOUT,
            router: SettingsPatch::default(),
            router_warnings: Vec::new(),
            design_given: false,
        }
    }
}

impl FrontEndOptions {
    /// Java's condition for the auto-start dialog: an input and an output
    /// file were passed.
    pub fn auto_start(&self) -> bool {
        self.design_given && self.output.is_some()
    }
}

/// `DE_VALID_EXTENSIONS` of Java's `isMultiFileConcatenation`, plus the KiCad board.
const DE_EXTENSIONS: &[&str] = &[".dsn", ".ses", ".rules", ".kicad_pcb"];
/// `DO_VALID_EXTENSIONS`.
const DO_EXTENSIONS: &[&str] = &[".ses", ".dsn", ".scr", ".rules", ".json", ".drc"];

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Args {
    pub open: Vec<PathBuf>,
    pub bench: Option<BenchScript>,
    pub frames: usize,
    /// Saves the first frame after the load finishes as a PNG.
    pub screenshot: Option<PathBuf>,
    /// Quits after the screenshot.
    pub exit: bool,
    /// A directory for `tools/gui/smoke.sh`: the app creates `ready` in it
    /// once the board is drawn, and takes the screenshot only after `shoot`
    /// appears, so the script can send keys in between.
    pub control_dir: Option<PathBuf>,
    /// `--bench-synthetic 4x4 board.dsn`: tile the board's scene `nx` by
    /// `ny` times before drawing it. Implies `--bench pan-zoom`.
    pub synthetic: Option<(u32, u32)>,
    /// G08.4: `-do`, `-dr`, `-dct`.
    pub frontend: FrontEndOptions,
}

impl Args {
    /// A run that is not interactive: it must not touch the saved settings.
    pub fn scripted(&self) -> bool {
        self.bench.is_some() || self.screenshot.is_some() || self.exit || self.control_dir.is_some()
    }

    pub fn parse(args: impl IntoIterator<Item = OsString>) -> Result<Args, String> {
        let mut out = Args {
            frames: DEFAULT_FRAMES,
            ..Args::default()
        };
        let argv: Vec<OsString> = args.into_iter().collect();
        let text: Vec<String> = argv
            .iter()
            .map(|a| a.to_string_lossy().into_owned())
            .collect();
        let mut router_args: Vec<String> = Vec::new();
        let mut i = 0;
        while i < argv.len() {
            if let Some(span) = router_flag_span(&text, i) {
                router_args.extend(text[i..i + span].iter().cloned());
                i += span;
                continue;
            }
            let arg = argv[i].clone();
            i += 1;
            match arg.to_string_lossy().as_ref() {
                "--bench" => {
                    let name = take(&argv, &mut i, "--bench")?;
                    out.bench = Some(match name.to_string_lossy().as_ref() {
                        "pan-zoom" => BenchScript::PanZoom,
                        "drag" => BenchScript::Drag,
                        "route" => BenchScript::Route,
                        other => return Err(format!("unknown bench script {other:?}")),
                    });
                }
                "--frames" => {
                    let n = take(&argv, &mut i, "--frames")?;
                    out.frames = n
                        .to_string_lossy()
                        .parse()
                        .ok()
                        .filter(|&n| n > 0)
                        .ok_or("--frames needs a positive number")?;
                }
                "--bench-synthetic" => {
                    let tiling = take(&argv, &mut i, "--bench-synthetic")?;
                    let tiling = tiling.to_string_lossy().into_owned();
                    out.synthetic = Some(parse_tiling(&tiling)?);
                    out.open
                        .push(PathBuf::from(take(&argv, &mut i, "--bench-synthetic")?));
                    out.bench.get_or_insert(BenchScript::PanZoom);
                }
                "--screenshot" => {
                    out.screenshot = Some(PathBuf::from(take(&argv, &mut i, "--screenshot")?))
                }
                "--exit" => out.exit = true,
                "--control-dir" => {
                    out.control_dir = Some(PathBuf::from(take(&argv, &mut i, "--control-dir")?))
                }
                "--autoroute" => out.frontend.confirmation_timeout = 0,
                "-de" => {
                    let files = files_up_to_next_flag(&argv, &mut i, DE_EXTENSIONS, true);
                    let (mut design, mut session) = (None, None);
                    for file in files {
                        match extension_of(&file).as_str() {
                            "dsn" | "kicad_pcb" => design = Some(file),
                            "ses" => session = Some(file),
                            "rules" => out.frontend.rules = Some(file),
                            _ => {
                                return Err(format!(
                                    "unknown file type in -de: {}; expected .dsn, .ses or .rules",
                                    file.display()
                                ));
                            }
                        }
                    }
                    // Only the last design and the last session count, like Java.
                    if design.is_some() || session.is_some() {
                        out.open = design.into_iter().chain(session).collect();
                    }
                }
                "-do" => {
                    let mut files = files_up_to_next_flag(&argv, &mut i, DO_EXTENSIONS, false);
                    if out.frontend.output.is_none() && !files.is_empty() {
                        out.frontend.output = Some(files.remove(0));
                    }
                    out.frontend.additional_outputs.extend(files);
                }
                "-dr" => out.frontend.rules = Some(PathBuf::from(take(&argv, &mut i, "-dr")?)),
                "-di" => {
                    take(&argv, &mut i, "-di")?;
                }
                "-dct" => {
                    let seconds = take(&argv, &mut i, "-dct")?;
                    let seconds: i64 = seconds
                        .to_string_lossy()
                        .trim()
                        .parse()
                        .map_err(|_| "-dct needs a whole number of seconds".to_string())?;
                    out.frontend.confirmation_timeout = u32::try_from(seconds.max(0)).unwrap_or(0);
                }
                flag if flag.starts_with('-') && flag.len() > 1 => {
                    return Err(format!("unknown option {flag}"));
                }
                _ => out.open.push(PathBuf::from(arg)),
            }
        }
        out.frontend.design_given = out.open.first().is_some_and(|p| is_design(p));
        let flags = parse_router_flags(&router_args);
        if let Some(error) = flags.usage_errors.first() {
            return Err(error.clone());
        }
        out.frontend.router = flags.parse.patch;
        if let Some(passes) = out.frontend.router.autorouter.max_passes.as_mut() {
            *passes = (*passes).clamp(0, 9999);
        }
        out.frontend.router_warnings = flags.log.into_iter().map(|l| l.text).collect();
        Ok(out)
    }
}

/// The argument after a flag.
fn take(argv: &[OsString], i: &mut usize, flag: &str) -> Result<OsString, String> {
    let v = argv.get(*i).cloned();
    *i += usize::from(v.is_some());
    v.ok_or_else(|| format!("{flag} needs a value"))
}

/// The lower-case extension of `path`, empty if it has none.
fn extension_of(path: &std::path::Path) -> String {
    path.extension()
        .map(|e| e.to_string_lossy().to_lowercase())
        .unwrap_or_default()
}

fn is_design(path: &std::path::Path) -> bool {
    matches!(extension_of(path).as_str(), "dsn" | "kicad_pcb")
}

/// Java's `isMultiFileConcatenation`: `a.dsn+b.ses`, every part with one of
/// the valid extensions.
fn is_concatenation(raw: &str, valid: &[&str]) -> bool {
    let parts: Vec<&str> = raw.split('+').collect();
    parts.len() > 1
        && parts.iter().all(|part| {
            let part = part.trim();
            let name_start = part.rfind(['/', '\\']).map_or(0, |at| at + 1);
            match part.rfind('.') {
                Some(dot) if dot > name_start && dot + 1 < part.len() => {
                    valid.contains(&part[dot..].to_lowercase().as_str())
                }
                _ => false,
            }
        })
}

/// The arguments after a `-de` or `-do` up to the next one that starts with
/// `-`. A `+` joined argument is split unless a file of that name exists
/// (`check_exists`, as Java does for `-de` only).
fn files_up_to_next_flag(
    argv: &[OsString],
    i: &mut usize,
    valid: &[&str],
    check_exists: bool,
) -> Vec<PathBuf> {
    let mut files = Vec::new();
    while let Some(raw) = argv.get(*i) {
        let text = raw.to_string_lossy();
        if text.starts_with('-') {
            break;
        }
        *i += 1;
        let text = text.trim();
        let exists = check_exists && std::path::Path::new(text).exists();
        if !exists && is_concatenation(text, valid) {
            files.extend(
                text.split('+')
                    .map(str::trim)
                    .filter(|p| !p.is_empty())
                    .map(PathBuf::from),
            );
        } else if !text.is_empty() {
            files.push(PathBuf::from(text));
        }
    }
    files
}

/// `4x4` as `(4, 4)`: columns by rows, each at least 1.
fn parse_tiling(text: &str) -> Result<(u32, u32), String> {
    let bad = || format!("--bench-synthetic needs a tiling like 4x4, not {text:?}");
    let (nx, ny) = text.split_once(['x', 'X']).ok_or_else(bad)?;
    let n = |t: &str| t.parse::<u32>().ok().filter(|&n| (1..=16).contains(&n));
    Ok((n(nx).ok_or_else(bad)?, n(ny).ok_or_else(bad)?))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(args: &[&str]) -> Result<Args, String> {
        Args::parse(args.iter().map(OsString::from))
    }

    #[test]
    fn parses_files_and_flags() {
        let a = parse(&[
            "a.dsn", "--bench", "pan-zoom", "--frames", "30", "--exit", "b.ses",
        ])
        .unwrap();
        assert_eq!(a.open, [PathBuf::from("a.dsn"), PathBuf::from("b.ses")]);
        assert_eq!(a.bench, Some(BenchScript::PanZoom));
        assert_eq!(a.frames, 30);
        assert!(a.exit && a.scripted());
        assert!(!parse(&["a.dsn"]).unwrap().scripted());
        assert_eq!(parse(&[]).unwrap().frames, DEFAULT_FRAMES);
    }

    #[test]
    fn parses_the_synthetic_tiling() {
        let a = parse(&["--bench-synthetic", "4x3", "a.dsn", "--frames", "9"]).unwrap();
        assert_eq!(a.synthetic, Some((4, 3)));
        assert_eq!(a.open, [PathBuf::from("a.dsn")]);
        assert_eq!(a.bench, Some(BenchScript::PanZoom));
        assert!(parse(&["--bench-synthetic", "4", "a.dsn"]).is_err());
        assert!(parse(&["--bench-synthetic", "0x4", "a.dsn"]).is_err());
        assert!(parse(&["--bench-synthetic", "4x4"]).is_err());
    }

    #[test]
    fn parses_the_route_script() {
        let a = parse(&["a.dsn", "--bench", "route"]).unwrap();
        assert_eq!(a.bench, Some(BenchScript::Route));
        assert_eq!(BenchScript::Route.name(), "route");
    }

    #[test]
    fn parses_the_front_end_flags() {
        let a = parse(&[
            "-de", "a.dsn", "b.ses", "c.rules", "-do", "out.ses", "x.dsn", "-dct", "3", "-mp", "2",
            "-di", "dir",
        ])
        .unwrap();
        assert_eq!(a.open, [PathBuf::from("a.dsn"), PathBuf::from("b.ses")]);
        let f = &a.frontend;
        assert_eq!(f.rules, Some(PathBuf::from("c.rules")));
        assert_eq!(f.output, Some(PathBuf::from("out.ses")));
        assert_eq!(f.additional_outputs, [PathBuf::from("x.dsn")]);
        assert_eq!(f.confirmation_timeout, 3);
        assert_eq!(f.router.autorouter.max_passes, Some(2));
        assert!(f.auto_start());
        // The defaults, Java's 5 seconds; `-dct` of 0 or less, and the alias.
        assert_eq!(parse(&["a.dsn"]).unwrap().frontend.confirmation_timeout, 5);
        assert!(!parse(&["a.dsn"]).unwrap().frontend.auto_start());
        assert_eq!(
            parse(&["-dct", "-4"])
                .unwrap()
                .frontend
                .confirmation_timeout,
            0
        );
        assert_eq!(
            parse(&["--autoroute"])
                .unwrap()
                .frontend
                .confirmation_timeout,
            0
        );
        let passes = |args: &[&str]| parse(args).unwrap().frontend.router.autorouter.max_passes;
        assert_eq!(passes(&["-mp", "70000"]), Some(9999));
        assert_eq!(passes(&["-mp", "0"]), Some(0));
        assert_eq!(passes(&["a.dsn"]), None);
        // Concatenation with `+` splits when no such file exists.
        let a = parse(&["-de", "a.dsn+b.ses+c.rules", "-do", "o.ses+o.rules"]).unwrap();
        assert_eq!(a.open, [PathBuf::from("a.dsn"), PathBuf::from("b.ses")]);
        assert_eq!(a.frontend.rules, Some(PathBuf::from("c.rules")));
        assert_eq!(a.frontend.output, Some(PathBuf::from("o.ses")));
        assert_eq!(a.frontend.additional_outputs, [PathBuf::from("o.rules")]);
        // `-dr` and positional boards; a design alone does not auto-start.
        let a = parse(&["a.dsn", "-dr", "r.rules"]).unwrap();
        assert_eq!(a.open, [PathBuf::from("a.dsn")]);
        assert_eq!(a.frontend.rules, Some(PathBuf::from("r.rules")));
        assert!(a.frontend.design_given && !a.frontend.auto_start());
        // The last design and session of `-de` count.
        let a = parse(&["-de", "a.dsn", "b.dsn", "x.ses", "y.ses"]).unwrap();
        assert_eq!(a.open, [PathBuf::from("b.dsn"), PathBuf::from("y.ses")]);
    }

    #[test]
    fn router_flags_reach_the_patch_as_on_the_cli() {
        let a = parse(&[
            "-de",
            "a.dsn",
            "-do",
            "o.ses",
            "-mt",
            "4",
            "-oit",
            "0.05",
            "-inc",
            "Power\\,Default,GND",
            "--router.optimizer.enabled=false",
            "--max-passes=7",
            "--threads",
            "2",
            "--timeout",
            "10m",
            "-dct",
            "0",
        ])
        .unwrap();
        let r = &a.frontend.router;
        assert_eq!(r.autorouter.max_passes, Some(7));
        assert_eq!(r.optimizer.enabled, Some(false));
        assert_eq!(
            r.autorouter.ignore_net_classes,
            Some(vec!["Power,Default".to_string(), "GND".to_string()])
        );
        assert!(r.optimizer.improvement_threshold.is_some());
        assert_eq!(a.frontend.confirmation_timeout, 0);
        assert_eq!(a.open, [PathBuf::from("a.dsn")]);
        // `-oit` and `-inc` are deprecated flags: the CLI's warning is kept.
        assert!(
            a.frontend
                .router_warnings
                .iter()
                .any(|w| w.contains("-oit") && w.contains("deprecated")),
            "{:?}",
            a.frontend.router_warnings
        );
        // The same arguments through the engine give the CLI's patch.
        let router: Vec<String> = [
            "-mt",
            "4",
            "-oit",
            "0.05",
            "-inc",
            "Power\\,Default,GND",
            "--router.optimizer.enabled=false",
            "--max-passes=7",
            "--threads",
            "2",
            "--timeout",
            "10m",
        ]
        .iter()
        .map(|s| s.to_string())
        .collect();
        assert_eq!(parse_router_flags(&router).parse.patch, *r);
    }

    #[test]
    fn rejects_bad_front_end_flags() {
        assert!(parse(&["-de", "a.txt"]).is_err());
        assert!(parse(&["-dct", "soon"]).is_err());
        assert!(parse(&["-dct"]).is_err());
        // A value never starts with `-` (as in the CLI), so `-1` is unknown.
        assert!(parse(&["-mp", "-1"]).is_err());
        assert!(parse(&["--timeout", "soon"]).is_err());
        assert!(parse(&["--threads", "0"]).is_err());
    }

    #[test]
    fn rejects_bad_flags() {
        assert!(parse(&["--bench"]).is_err());
        assert!(parse(&["--bench", "nope"]).is_err());
        assert!(parse(&["--frames", "0"]).is_err());
        assert!(parse(&["--wat"]).is_err());
    }
}
