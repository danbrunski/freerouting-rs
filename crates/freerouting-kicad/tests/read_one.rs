//! `FR_KICAD_BOARD=path cargo test --release --test read_one -- --ignored --nocapture`: reads one board and prints the time.
#[test]
#[ignore = "needs FR_KICAD_BOARD"]
fn read_one() {
    let Some(path) = std::env::var_os("FR_KICAD_BOARD") else {
        return;
    };
    let path = std::path::PathBuf::from(path);
    let text = String::from_utf8_lossy(&std::fs::read(&path).unwrap()).into_owned();
    let pro = std::fs::read_to_string(path.with_extension("kicad_pro")).ok();
    let t = std::time::Instant::now();
    let r = freerouting_kicad::read(&text, pro.as_deref()).unwrap();
    println!(
        "{}: {} bytes, {} ms, {} nets, {} images, {} warnings",
        path.display(),
        text.len(),
        t.elapsed().as_millis(),
        r.pcb.network.as_ref().unwrap().nets.len(),
        r.pcb.library.as_ref().unwrap().images.len(),
        r.warnings.len()
    );
}
