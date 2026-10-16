//! Scripted runs: `--bench`, `--screenshot` and `--exit`. The screenshot is
//! taken by the app itself (`ViewportCommand::Screenshot`) and written as a
//! PNG, which avoids X server races and DPI problems of capturing the screen.

use std::fs::File;
use std::io::BufWriter;
use std::path::Path;

use eframe::egui::{self, ColorImage};

use crate::args::Args;
use crate::bench::Bench;
use crate::session::Loaded;
use crate::stats::PaintStats;
use crate::view::Camera;
use crate::viewport::ViewState;

/// Frames drawn with the board before the screenshot is requested, so the
/// canvas has its final size and the board is fitted.
const SETTLE_FRAMES: u32 = 2;

#[derive(Clone, Copy, PartialEq, Eq)]
enum Shot {
    Idle,
    Requested,
    Done,
}

pub struct Scripted {
    exit: bool,
    path: Option<std::path::PathBuf>,
    bench: Option<Bench>,
    shot: Shot,
    settled: u32,
    control: Option<std::path::PathBuf>,
}

impl Scripted {
    pub fn new(args: &Args) -> Scripted {
        Scripted {
            exit: args.exit,
            path: args.screenshot.clone(),
            bench: args.bench.map(|s| Bench::new(s, args.frames)),
            shot: if args.screenshot.is_some() {
                Shot::Idle
            } else {
                Shot::Done
            },
            settled: 0,
            control: args.control_dir.clone(),
        }
    }

    /// Before the canvas is drawn: the bench moves the camera.
    pub fn before_canvas(&mut self, view: &mut ViewState, loaded: Option<&Loaded>) {
        let (Some(bench), Some(loaded), Some(_)) = (&mut self.bench, loaded, view.camera) else {
            return;
        };
        let fit = Camera::fit(&loaded.scene, view.canvas);
        view.camera = Some(bench.camera(fit, view.canvas));
        view.auto_fit = None;
    }

    /// After the frame. `ready` is true once nothing is left to load.
    pub fn after_frame(
        &mut self,
        ctx: &egui::Context,
        ready: bool,
        ms: f64,
        paint: PaintStats,
        loaded: Option<&Loaded>,
    ) {
        if let (Some(bench), Some(loaded)) = (&mut self.bench, loaded) {
            ctx.request_repaint();
            if let Some(json) = bench.frame_done(ms, paint, loaded.tess.triangle_count()) {
                println!("{json}");
                ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                self.bench = None;
            }
        }
        if ready && self.shot == Shot::Idle {
            self.settled += 1;
            if self.settled > SETTLE_FRAMES && self.go(ctx) {
                ctx.send_viewport_cmd(egui::ViewportCommand::Screenshot(Default::default()));
                self.shot = Shot::Requested;
            }
            if self.control.is_none() {
                ctx.request_repaint();
            }
        }
        let image = ctx.input(|i| {
            i.events.iter().find_map(|e| match e {
                egui::Event::Screenshot { image, .. } => Some(image.clone()),
                _ => None,
            })
        });
        if let (Some(image), Some(path)) = (image, &self.path) {
            match save_png(path, &image) {
                Ok(()) => eprintln!("Screenshot saved to {}", path.display()),
                Err(e) => eprintln!("Screenshot failed: {e}"),
            }
            self.shot = Shot::Done;
            if self.exit {
                ctx.send_viewport_cmd(egui::ViewportCommand::Close);
            }
        } else if self.exit && self.path.is_none() && ready {
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
        }
        if self.shot == Shot::Requested {
            ctx.request_repaint();
        }
    }
}

impl Scripted {
    /// Whether the screenshot may be taken now. With a control directory,
    /// that is once `shoot` exists there; `ready` is created first.
    fn go(&self, ctx: &egui::Context) -> bool {
        let Some(dir) = &self.control else {
            return true;
        };
        // The marker is only an empty file; a failure shows as a timeout in
        // the script, which says what it waited for.
        let _ = std::fs::write(dir.join("ready"), b"");
        ctx.request_repaint_after(std::time::Duration::from_millis(50));
        dir.join("shoot").exists()
    }
}

/// Writes `image` as an 8-bit RGBA PNG.
pub fn save_png(path: &Path, image: &ColorImage) -> Result<(), String> {
    let file = File::create(path).map_err(|e| format!("{}: {e}", path.display()))?;
    let mut encoder = png::Encoder::new(
        BufWriter::new(file),
        image.width() as u32,
        image.height() as u32,
    );
    encoder.set_color(png::ColorType::Rgba);
    encoder.set_depth(png::BitDepth::Eight);
    let bytes: Vec<u8> = image.pixels.iter().flat_map(|c| c.to_array()).collect();
    encoder
        .write_header()
        .and_then(|mut w| w.write_image_data(&bytes))
        .map_err(|e| format!("{}: {e}", path.display()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use eframe::egui::Color32;

    #[test]
    fn writes_a_png() {
        let dir = std::env::temp_dir().join(format!("fr-gui-png-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("t.png");
        let image = ColorImage::new([3, 2], vec![Color32::from_rgb(1, 2, 3); 6]);
        save_png(&path, &image).unwrap();
        let bytes = std::fs::read(&path).unwrap();
        assert_eq!(&bytes[1..4], b"PNG");
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
