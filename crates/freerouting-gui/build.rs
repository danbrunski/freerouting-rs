//! Windows only (R01.5): embeds the icon and the version resource (file properties) into the exe.
//!
//! It runs for MSVC targets, or for any Windows target when `FREEROUTING_WINRES=1`. The
//! default skips the gnu toolchain, so the local KiCad loop (mingw, which may have no `windres`)
//! builds as before. Without the resource the exe is the same program with a generic icon. The
//! version in the properties is the Cargo package version (`winresource` reads it itself).

fn main() {
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-changed=icons/icon.ico");
    println!("cargo:rerun-if-env-changed=FREEROUTING_WINRES");
    #[cfg(windows)]
    embed();
}

#[cfg(windows)]
fn embed() {
    let target_os = std::env::var("CARGO_CFG_TARGET_OS").unwrap_or_default();
    let target_env = std::env::var("CARGO_CFG_TARGET_ENV").unwrap_or_default();
    let forced = std::env::var("FREEROUTING_WINRES").is_ok_and(|v| v == "1");
    if target_os != "windows" || !(target_env == "msvc" || forced) {
        return;
    }
    let mut res = winresource::WindowsResource::new();
    res.set_icon("icons/icon.ico");
    res.set("ProductName", "freerouting-rs");
    res.set("FileDescription", "freerouting-rs GUI");
    res.set("LegalCopyright", "GPL-3.0-only");
    if let Err(error) = res.compile() {
        // Never fail the build over a cosmetic resource.
        println!("cargo:warning=could not embed the Windows resource: {error}");
    }
}
