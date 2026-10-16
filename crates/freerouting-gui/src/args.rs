//! Command-line arguments:
//! `freerouting-gui [board.dsn [board.ses]] [--bench pan-zoom] [--frames N]
//! [--screenshot out.png] [--exit] [--control-dir dir]`.

use std::ffi::OsString;
use std::path::PathBuf;

/// Frames a `--bench` run counts when `--frames` is not given.
pub const DEFAULT_FRAMES: usize = 600;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BenchScript {
    /// Zoom in, pan and zoom out around the board, repeated.
    PanZoom,
}

impl BenchScript {
    pub fn name(self) -> &'static str {
        match self {
            BenchScript::PanZoom => "pan-zoom",
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
    fn rejects_bad_flags() {
        assert!(parse(&["--bench"]).is_err());
        assert!(parse(&["--bench", "nope"]).is_err());
        assert!(parse(&["--frames", "0"]).is_err());
        assert!(parse(&["--wat"]).is_err());
    }
}
