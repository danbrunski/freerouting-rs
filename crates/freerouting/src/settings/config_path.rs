//! Where the settings file (`freerouting.json`) lives.
//!
//! The precedence, highest first:
//! 1. `--settings FILE` ([`ConfigChoice::Explicit`]), which replaces the
//!    default config-file source and does not stack with it;
//! 2. `--user_data_path=DIR` ([`ConfigChoice::UserDataPath`]);
//! 3. `FREEROUTING__USER_DATA_PATH`;
//! 4. `FREEROUTING__LOGGING__FILE__LOCATION`, the deprecated fallback of
//!    `UserDataPathResolver`;
//! 5. the OS default, `<config dir>/freerouting-rs/freerouting.json`:
//!    - Windows: `%APPDATA%\freerouting-rs\`;
//!    - macOS: `~/Library/Application Support/freerouting-rs/`;
//!    - Linux: `$XDG_CONFIG_HOME/freerouting-rs/`, defaulting to
//!      `~/.config/`.
//!
//! Java's own file (`%APPDATA%\freerouting\`, `~/.config/freerouting/`) is
//! read only through `--settings`.

use std::collections::HashMap;
use std::path::PathBuf;

/// The file name inside a user-data directory.
pub const CONFIG_FILE_NAME: &str = "freerouting.json";
const APP_DIR: &str = "freerouting-rs";

/// Reads environment variables, so tests can inject them.
pub trait EnvLookup {
    fn var(&self, name: &str) -> Option<String>;
}

impl EnvLookup for HashMap<String, String> {
    fn var(&self, name: &str) -> Option<String> {
        self.get(name).cloned()
    }
}

/// The process environment.
#[derive(Clone, Copy, Debug, Default)]
pub struct ProcessEnv;

impl EnvLookup for ProcessEnv {
    fn var(&self, name: &str) -> Option<String> {
        std::env::var(name).ok()
    }
}

/// The operating system families with different config directories.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Os {
    Windows,
    MacOs,
    Linux,
}

impl Os {
    /// The OS this binary runs on (anything but Windows and macOS counts as
    /// Linux).
    pub fn current() -> Os {
        if cfg!(windows) {
            Os::Windows
        } else if cfg!(target_os = "macos") {
            Os::MacOs
        } else {
            Os::Linux
        }
    }
}

/// How the settings file was chosen on the command line.
#[derive(Clone, Debug, PartialEq, Eq, Default)]
pub enum ConfigChoice {
    /// Neither `--settings` nor `--user_data_path=` was given.
    #[default]
    Default,
    /// `--user_data_path=DIR`.
    UserDataPath(PathBuf),
    /// `--settings FILE`: a missing or malformed file is an error.
    Explicit(PathBuf),
}

impl ConfigChoice {
    /// Reads `--settings FILE`, `--settings=FILE` and `--user_data_path=DIR`
    /// from a command line. `--settings` wins over `--user_data_path=`.
    pub fn from_args(args: &[String]) -> ConfigChoice {
        let mut choice = ConfigChoice::Default;
        let mut i = 0;
        while i < args.len() {
            let arg = args[i].as_str();
            if let Some(dir) = arg.strip_prefix("--user_data_path=") {
                if !matches!(choice, ConfigChoice::Explicit(_)) {
                    choice = ConfigChoice::UserDataPath(PathBuf::from(dir));
                }
            } else if let Some(file) = arg.strip_prefix("--settings=") {
                choice = ConfigChoice::Explicit(PathBuf::from(file));
            } else if arg == "--settings" && i + 1 < args.len() {
                i += 1;
                choice = ConfigChoice::Explicit(PathBuf::from(&args[i]));
            }
            i += 1;
        }
        choice
    }
}

fn non_empty(env: &dyn EnvLookup, name: &str) -> Option<String> {
    env.var(name).filter(|v| !v.is_empty())
}

/// The OS default config file, `None` when the environment does not say
/// where the user's home or config directory is.
pub fn default_config_file(env: &dyn EnvLookup, os: Os) -> Option<PathBuf> {
    let base = match os {
        Os::Windows => PathBuf::from(non_empty(env, "APPDATA")?),
        Os::MacOs => PathBuf::from(non_empty(env, "HOME")?)
            .join("Library")
            .join("Application Support"),
        Os::Linux => match non_empty(env, "XDG_CONFIG_HOME").filter(|d| d.starts_with('/')) {
            Some(dir) => PathBuf::from(dir),
            None => PathBuf::from(non_empty(env, "HOME")?).join(".config"),
        },
    };
    Some(base.join(APP_DIR).join(CONFIG_FILE_NAME))
}

/// The settings file for a choice. `None` only when the choice is
/// [`ConfigChoice::Default`] and the OS default cannot be found.
pub fn config_file(choice: &ConfigChoice, env: &dyn EnvLookup, os: Os) -> Option<PathBuf> {
    match choice {
        ConfigChoice::Explicit(file) => Some(file.clone()),
        ConfigChoice::UserDataPath(dir) => Some(dir.join(CONFIG_FILE_NAME)),
        ConfigChoice::Default => {
            if let Some(dir) = env.var("FREEROUTING__USER_DATA_PATH") {
                Some(PathBuf::from(dir).join(CONFIG_FILE_NAME))
            } else if let Some(dir) = env.var("FREEROUTING__LOGGING__FILE__LOCATION") {
                Some(PathBuf::from(dir).join(CONFIG_FILE_NAME))
            } else {
                default_config_file(env, os)
            }
        }
    }
}
