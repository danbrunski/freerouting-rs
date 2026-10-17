//! The router flags of the command line that both front ends share (K02.5).
//!
//! The CLI (`freerouting`) and the GUI's front-end mode
//! (`freerouting-gui -de d.dsn -do o.ses`) take the same router flags, so the
//! mapping from arguments to a [`SettingsPatch`] lives here and neither
//! binary has a copy of it:
//!
//! - [`parse_cli_arguments`] (the settings half of Java's `CliSettings`):
//!   `-mp`, `-mt`, `-inc`, `-oit`, the scoring flags and
//!   `--router.<path>=` / `--optimizer.<path>=`;
//! - [`rewrite_aliases`]: the long forms of C08, which are not Java's.
//!   `--max-passes`, `--threads`, `--timeout` and `--result-json` become the
//!   `--router.*` flags that set the same fields, so an alias can never do
//!   more than the settings do. A bad value is a *usage error* (the CLI exits
//!   2 on it; the GUI refuses to start);
//! - [`apply_escaped_inc`]: `-inc 'Power\,Default'` names the class
//!   `Power,Default`, which KiCad writes (DIVERGENCES, "Command line").
//!
//! [`parse_router_flags`] runs all three on one argument list; the CLI runs
//! them as separate steps because its own flags (`--progress`, `--seed`, ...)
//! come out of the list in between.

use crate::job::{LogLevel, LogLine};

use super::cli_args::{CliParse, parse_cli_arguments};

/// The long flags [`rewrite_aliases`] takes: `(name, what it becomes)`.
pub const ALIAS_FLAGS: &[(&str, &str)] = &[
    ("--result-json", "--router.result_json"),
    ("--timeout", "--router.job_timeout"),
    ("--max-passes", "--router.autorouter.max_passes"),
    (
        "--threads",
        "--router.autorouter.max_threads and --router.optimizer.max_threads",
    ),
];

/// The single-dash router flags (`CliSettings.mapFlagToProperty`).
const SHORT_ROUTER_FLAGS: &[&str] = &[
    "mp",
    "mt",
    "inc",
    "oit",
    "scoring-version",
    "router-scoring-version",
    "optimizer-scoring-version",
];

/// How many arguments the router flag at `argv[i]` takes, itself included
/// (1 or 2), or `None` when it is not a router flag. A front end that has
/// flags of its own uses this to pass the router flags on to
/// [`parse_router_flags`] untouched. A value is the next argument when it does
/// not start with `-`, as in `CliSettings`; `--name=value` is one argument.
pub fn router_flag_span(argv: &[String], i: usize) -> Option<usize> {
    let arg = argv.get(i)?.as_str();
    let value_follows = || argv.get(i + 1).is_some_and(|next| !next.starts_with('-'));
    if let Some(long) = arg.strip_prefix("--") {
        return match long.split_once('=') {
            Some((name, _)) => (name.starts_with("router.")
                || name.starts_with("optimizer.")
                || name == "scoring-version"
                || name == "router-scoring-version"
                || name == "optimizer-scoring-version"
                || ALIAS_FLAGS.iter().any(|(alias, _)| &alias[2..] == name))
            .then_some(1),
            None => ALIAS_FLAGS
                .iter()
                .any(|(alias, _)| *alias == arg)
                .then(|| 1 + usize::from(value_follows())),
        };
    }
    let flag = arg.strip_prefix('-')?;
    SHORT_ROUTER_FLAGS
        .contains(&flag)
        .then(|| 1 + usize::from(value_follows()))
}

/// The longest job limit (`RoutingJobScheduler` caps `job_timeout`).
pub const MAX_TIMEOUT_SECONDS: u64 = 24 * 3600;

/// What [`parse_router_flags`] found.
#[derive(Debug, Default)]
pub struct RouterFlags {
    /// The router settings of the command line (priority 60).
    pub parse: CliParse,
    /// Bad values of the alias flags, each a complete message.
    pub usage_errors: Vec<String>,
    /// Warnings of the alias flags, then Java's warnings, in order.
    pub log: Vec<LogLine>,
}

/// All router flags of `argv` in one call: aliases, `CliSettings`, escaped
/// `-inc`. Arguments that are not router flags are ignored.
pub fn parse_router_flags(argv: &[String]) -> RouterFlags {
    let mut out = RouterFlags::default();
    let rewritten = rewrite_aliases(argv, &mut out.usage_errors, &mut out.log);
    out.parse = parse_router_flags_rewritten(&rewritten);
    for warning in std::mem::take(&mut out.parse.warnings) {
        out.log.push(LogLine {
            level: LogLevel::Warn,
            text: warning,
        });
    }
    out
}

/// [`parse_cli_arguments`] plus [`apply_escaped_inc`], for an argument list
/// that [`rewrite_aliases`] has already seen.
pub fn parse_router_flags_rewritten(argv: &[String]) -> CliParse {
    let mut parse = parse_cli_arguments(argv);
    apply_escaped_inc(argv, &mut parse);
    parse
}

/// Takes the alias flags (see [`ALIAS_FLAGS`]) out of `argv` and returns the
/// rest, with each alias rewritten to the flags it stands for. Each flag takes
/// its value as the next argument or after `=`.
pub fn rewrite_aliases(
    argv: &[String],
    usage_errors: &mut Vec<String>,
    log: &mut Vec<LogLine>,
) -> Vec<String> {
    let mut rest = Vec::with_capacity(argv.len());
    let mut i = 0;
    while i < argv.len() {
        let arg = argv[i].as_str();
        if arg == "--settings" {
            rest.push(argv[i].clone());
            if let Some(file) = argv.get(i + 1) {
                rest.push(file.clone());
                i += 1;
            }
            i += 1;
            continue;
        }
        let (name, inline) = match arg.split_once('=') {
            Some((name, value)) => (name, Some(value.to_string())),
            None => (arg, None),
        };
        let Some(&(name, _)) = ALIAS_FLAGS.iter().find(|(n, _)| *n == name) else {
            rest.push(argv[i].clone());
            i += 1;
            continue;
        };
        i += 1;
        let value = if inline.is_some() {
            inline
        } else if let Some(next) = argv.get(i).filter(|a| !a.starts_with('-')) {
            i += 1;
            Some(next.clone())
        } else {
            None
        };
        let Some(value) = value else {
            usage_errors.push(format!("{name} needs a value"));
            continue;
        };
        match name {
            "--result-json" => rest.push(format!("--router.result_json={value}")),
            "--max-passes" => {
                if java_decode_int(&value).is_some() {
                    rest.push(format!("--router.autorouter.max_passes={value}"));
                } else {
                    usage_errors.push(format!("--max-passes: '{value}' is not an integer"));
                }
            }
            "--threads" => match value.trim().parse::<i32>() {
                Ok(n) if n >= 1 => {
                    rest.push(format!("--router.autorouter.max_threads={n}"));
                    rest.push(format!("--router.optimizer.max_threads={n}"));
                }
                _ => usage_errors.push(format!(
                    "--threads: '{value}' is not a thread count of 1 or more"
                )),
            },
            "--timeout" => match parse_timeout_seconds(&value) {
                Some(seconds) => {
                    let capped = seconds.min(MAX_TIMEOUT_SECONDS);
                    if capped != seconds {
                        log.push(LogLine {
                            level: LogLevel::Warn,
                            text: format!(
                                "--timeout {value} is longer than 24 hours; using 24:00:00"
                            ),
                        });
                    }
                    rest.push(format!("--router.job_timeout={}", timeout_text(capped)));
                }
                None => usage_errors.push(format!(
                    "--timeout: '{value}' is not a duration (use 90s, 10m, 2h or HH:MM:SS)"
                )),
            },
            _ => unreachable!("every row of ALIAS_FLAGS is handled"),
        }
    }
    rest
}

/// `Integer.decode`: an optional sign, then decimal, `0x`/`0X`/`#` hexadecimal
/// or leading-`0` octal digits. `None` where Java throws.
pub fn java_decode_int(text: &str) -> Option<i32> {
    let (negative, rest) = match text.strip_prefix('-') {
        Some(rest) => (true, rest),
        None => (false, text.strip_prefix('+').unwrap_or(text)),
    };
    let (radix, digits) = if let Some(hex) = rest
        .strip_prefix("0x")
        .or_else(|| rest.strip_prefix("0X"))
        .or_else(|| rest.strip_prefix('#'))
    {
        (16, hex)
    } else if rest.len() > 1 && rest.starts_with('0') {
        (8, &rest[1..])
    } else {
        (10, rest)
    };
    if digits.is_empty() || digits.starts_with(['-', '+']) {
        return None;
    }
    let magnitude = i64::from_str_radix(digits, radix).ok()?;
    i32::try_from(if negative { -magnitude } else { magnitude }).ok()
}

/// `--timeout`: `90s`, `10m`, `2h`, `HH:MM:SS`, `MM:SS` or plain seconds, as
/// seconds. Returns `None` for anything else.
pub fn parse_timeout_seconds(text: &str) -> Option<u64> {
    fn digits(text: &str) -> Option<u64> {
        if text.is_empty() || !text.bytes().all(|b| b.is_ascii_digit()) {
            return None;
        }
        text.parse().ok()
    }
    let text = text.trim();
    if text.contains(':') {
        let parts: Vec<&str> = text.split(':').collect();
        let numbers: Option<Vec<u64>> = parts.iter().map(|p| digits(p)).collect();
        let numbers = numbers?;
        return match numbers.as_slice() {
            [h, m, s] => h
                .checked_mul(3600)?
                .checked_add(m.checked_mul(60)?)?
                .checked_add(*s),
            [m, s] => m.checked_mul(60)?.checked_add(*s),
            _ => None,
        };
    }
    let last = text.chars().last()?;
    match last.to_ascii_lowercase() {
        's' => digits(&text[..text.len() - 1]),
        'm' => digits(&text[..text.len() - 1])?.checked_mul(60),
        'h' => digits(&text[..text.len() - 1])?.checked_mul(3600),
        _ => digits(text),
    }
}

/// `HH:MM:SS` for a number of seconds (the form of `job_timeout`).
pub fn timeout_text(seconds: u64) -> String {
    format!(
        "{:02}:{:02}:{:02}",
        seconds / 3600,
        seconds / 60 % 60,
        seconds % 60
    )
}

/// The names of `-inc VALUE` with `\,` keeping a comma. Java's `split(",")`
/// removes trailing empty strings; each name is trimmed as `FIELDS` lists are.
pub fn split_net_classes(value: &str) -> Vec<String> {
    let mut names = Vec::new();
    let mut current = String::new();
    let mut chars = value.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '\\' if chars.peek() == Some(&',') => {
                chars.next();
                current.push(',');
            }
            ',' => names.push(std::mem::take(&mut current)),
            other => current.push(other),
        }
    }
    names.push(current);
    while names.last().is_some_and(String::is_empty) {
        names.pop();
    }
    names.into_iter().map(|n| n.trim().to_string()).collect()
}

/// Re-reads the last `-inc VALUE` of the command line when its value has a
/// `\,` in it, and replaces the patch's `ignore_net_classes` by the names
/// [`split_net_classes`] gives. The walk is `CliSettings`': a value is taken
/// only when the next argument does not start with `-`.
pub fn apply_escaped_inc(argv: &[String], parse: &mut CliParse) {
    let mut last: Option<&str> = None;
    let mut i = 0;
    while i < argv.len() {
        let arg = argv[i].as_str();
        if arg.starts_with("--") {
            i += 1;
            continue;
        }
        if let Some(flag) = arg.strip_prefix('-') {
            let value = if i + 1 < argv.len() && !argv[i + 1].starts_with('-') {
                i += 1;
                argv[i].as_str()
            } else {
                ""
            };
            if flag == "inc" {
                last = Some(value);
            }
        }
        i += 1;
    }
    if let Some(value) = last
        && value.contains("\\,")
    {
        parse.patch.autorouter.ignore_net_classes = Some(split_net_classes(value));
        parse.parsed_arguments.insert(
            "router.autorouter.ignore_net_classes".to_string(),
            value.to_string(),
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn flags(args: &[&str]) -> RouterFlags {
        parse_router_flags(&args.iter().map(|s| s.to_string()).collect::<Vec<_>>())
    }

    #[test]
    fn the_aliases_set_the_fields_of_the_long_flags() {
        let f = flags(&[
            "--max-passes",
            "7",
            "--threads=3",
            "--timeout",
            "10m",
            "--result-json",
            "r.json",
        ]);
        assert!(f.usage_errors.is_empty(), "{:?}", f.usage_errors);
        let a = &f.parse.patch.autorouter;
        assert_eq!(a.max_passes, Some(7));
        assert_eq!(a.max_threads, Some(3));
        assert_eq!(f.parse.patch.optimizer.max_threads, Some(3));
        assert_eq!(
            f.parse
                .parsed_arguments
                .get("router.job_timeout")
                .map(String::as_str),
            Some("00:10:00")
        );
        assert_eq!(
            f.parse.patch.result_json.as_deref(),
            Some("r.json"),
            "{:?}",
            f.parse.parsed_arguments
        );
    }

    #[test]
    fn bad_alias_values_are_usage_errors() {
        for bad in [
            &["--max-passes", "many"][..],
            &["--threads", "0"],
            &["--timeout", "soon"],
            &["--timeout"],
        ] {
            assert_eq!(flags(bad).usage_errors.len(), 1, "{bad:?}");
        }
    }

    #[test]
    fn a_long_timeout_is_capped_with_a_warning() {
        let f = flags(&["--timeout", "48h"]);
        assert_eq!(
            f.parse
                .parsed_arguments
                .get("router.job_timeout")
                .map(String::as_str),
            Some("24:00:00")
        );
        assert!(
            f.log
                .iter()
                .any(|l| l.text.contains("longer than 24 hours"))
        );
    }

    #[test]
    fn the_short_flags_and_an_escaped_inc_reach_the_patch() {
        let f = flags(&[
            "-mp",
            "9",
            "-mt",
            "2",
            "-oit",
            "0.05",
            "-inc",
            "Power\\,Default,GND",
        ]);
        assert_eq!(f.parse.patch.autorouter.max_passes, Some(9));
        assert_eq!(
            f.parse.patch.autorouter.ignore_net_classes,
            Some(vec!["Power,Default".to_string(), "GND".to_string()])
        );
        assert!(f.parse.patch.optimizer.improvement_threshold.is_some());
    }

    #[test]
    fn router_flags_are_told_from_the_rest() {
        let argv: Vec<String> = [
            "-de",
            "a.dsn",
            "-mp",
            "3",
            "-mt",
            "-inc",
            "GND",
            "--router.optimizer.enabled=false",
            "--timeout",
            "5m",
            "--timeout=5m",
            "--threads",
            "--bogus",
        ]
        .iter()
        .map(|s| s.to_string())
        .collect();
        let spans: Vec<Option<usize>> = (0..argv.len())
            .map(|i| router_flag_span(&argv, i))
            .collect();
        assert_eq!(
            spans,
            [
                None,
                None,
                Some(2),
                None,
                Some(1),
                Some(2),
                None,
                Some(1),
                Some(2),
                None,
                Some(1),
                Some(1),
                None
            ]
        );
    }

    #[test]
    fn timeouts_round_trip() {
        assert_eq!(parse_timeout_seconds("90s"), Some(90));
        assert_eq!(parse_timeout_seconds("1:30"), Some(90));
        assert_eq!(timeout_text(86_400), "24:00:00");
        assert_eq!(java_decode_int("0x10"), Some(16));
        assert_eq!(java_decode_int("-"), None);
    }
}
