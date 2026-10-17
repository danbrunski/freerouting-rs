//! What this build is: the version and the git commit (R01.2).
//!
//! The CLI's `--version` line and the GUI's About text print these two
//! constants, so they cannot disagree. Nothing in the engine reads them
//! (the router's output is the same with any stamp); `build.rs` sets
//! `FREEROUTING_GIT_SHA`.

/// The workspace version (`[workspace.package] version` in `Cargo.toml`).
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

/// The commit the binary was built from: 12 hex digits, or `unknown` when
/// the build had no git checkout and no `FREEROUTING_GIT_SHA`.
pub const GIT_SHA: &str = env!("FREEROUTING_GIT_SHA");

/// `<VERSION> (<GIT_SHA>)`, the text the About dialog shows.
pub fn version_with_sha() -> String {
    format!("{VERSION} ({GIT_SHA})")
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The sha check shared with the tarball test below.
    fn is_stamp(sha: &str) -> bool {
        sha == "unknown" || (sha.len() == 12 && sha.bytes().all(|b| b.is_ascii_hexdigit()))
    }

    #[test]
    fn version_is_the_package_version() {
        assert_eq!(VERSION, env!("CARGO_PKG_VERSION"));
        assert_eq!(VERSION.split('.').count(), 3, "{VERSION}");
    }

    #[test]
    fn git_sha_is_twelve_hex_digits_or_unknown() {
        assert!(is_stamp(GIT_SHA), "{GIT_SHA}");
    }

    #[test]
    fn stamp_checker_rejects_other_shapes() {
        assert!(is_stamp("0123456789ab"));
        assert!(is_stamp("unknown"));
        assert!(!is_stamp("0123456789a"));
        assert!(!is_stamp("0123456789abc"));
        assert!(!is_stamp("0123456789ag"));
        assert!(!is_stamp(""));
    }

    #[test]
    fn version_with_sha_has_both() {
        assert_eq!(version_with_sha(), format!("{VERSION} ({GIT_SHA})"));
    }
}
