//! Command-line arguments:
//! `freerouting-gui [board.dsn [board.ses]] [--bench pan-zoom|drag] [--frames N]
//! [--screenshot out.png] [--exit] [--control-dir dir]
//! [--bench-synthetic 4x4 board.dsn]`.

use std::ffi::OsString;
use std::path::PathBuf;

/// Frames a `--bench` run counts when `--frames` is not given.
pub const DEFAULT_FRAMES: usize = 600;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BenchScript {
    /// Zoom in, pan and zoom out around the board, repeated.
    PanZoom,
    /// Drag one component back and forth with the drag tool (G09.7, FT7),
    /// the camera fixed on the fitted board.
    Drag,
}

impl BenchScript {
    pub fn name(self) -> &'static str {
        match self {
            BenchScript::PanZoom => "pan-zoom",
            BenchScript::Drag => "drag",
        }
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
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
        let mut args = args.into_iter();
        while let Some(arg) = args.next() {
            let mut value = |flag: &str| args.next().ok_or_else(|| format!("{flag} needs a value"));
            match arg.to_string_lossy().as_ref() {
                "--bench" => {
                    let name = value("--bench")?;
                    out.bench = Some(match name.to_string_lossy().as_ref() {
                        "pan-zoom" => BenchScript::PanZoom,
                        "drag" => BenchScript::Drag,
                        other => return Err(format!("unknown bench script {other:?}")),
                    });
                }
                "--frames" => {
                    let n = value("--frames")?;
                    out.frames = n
                        .to_string_lossy()
                        .parse()
                        .ok()
                        .filter(|&n| n > 0)
                        .ok_or("--frames needs a positive number")?;
                }
                "--bench-synthetic" => {
                    let tiling = value("--bench-synthetic")?;
                    let tiling = tiling.to_string_lossy().into_owned();
                    out.synthetic = Some(parse_tiling(&tiling)?);
                    out.open.push(PathBuf::from(value("--bench-synthetic")?));
                    out.bench.get_or_insert(BenchScript::PanZoom);
                }
                "--screenshot" => out.screenshot = Some(PathBuf::from(value("--screenshot")?)),
                "--exit" => out.exit = true,
                "--control-dir" => out.control_dir = Some(PathBuf::from(value("--control-dir")?)),
                flag if flag.starts_with("--") => return Err(format!("unknown option {flag}")),
                _ => out.open.push(PathBuf::from(arg)),
            }
        }
        Ok(out)
    }
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
    fn rejects_bad_flags() {
        assert!(parse(&["--bench"]).is_err());
        assert!(parse(&["--bench", "nope"]).is_err());
        assert!(parse(&["--frames", "0"]).is_err());
        assert!(parse(&["--wat"]).is_err());
    }
}
