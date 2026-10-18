//! N05.1 on the built binary: the CLI reaches the KRT stages.

mod common;

use common::*;
use freerouting::autoroute::stages::catalog::registered_ids;
use freerouting::drc::json::JsonValue;

const FIXTURE: &str = "Issue026-J2_reference.dsn";

fn route(name: &str, extra: &[&str]) -> (std::process::Output, std::path::PathBuf) {
    let dir = scratch(name);
    copy_fixtures(&dir, &[FIXTURE]);
    let mut args = vec!["-de", FIXTURE, "-do", "out.ses", "-mp", "3"];
    args.extend_from_slice(extra);
    (run_in(&dir, &args), dir)
}

/// `--capabilities` lists every id of the registry the binary routes with,
/// the KRT stages among them.
#[test]
fn capabilities_lists_the_registered_stage_ids() {
    let dir = scratch("krt-capabilities");
    let out = run_in(&dir, &["--capabilities"]);
    assert_eq!(code(&out), 0, "{}", stderr(&out));
    let doc = JsonValue::parse(&stdout(&out)).expect("JSON");
    let stages = member(&doc, "stages").expect("stages");
    let kinds = [
        ("fanout", "fanout"),
        ("connection_router", "connection_router"),
        ("board_stages", "board_stages"),
    ];
    for (kind, ids) in registered_ids(&freerouting_stages::registry()) {
        let key = kinds.iter().find(|(k, _)| *k == kind).map(|(_, k)| *k);
        let key = key.unwrap_or(kind);
        let Some(listed) = member(stages, key) else {
            continue;
        };
        let listed = strings_of(Some(listed));
        for id in ids {
            assert!(listed.iter().any(|l| l == id), "{kind}: {id} is not listed");
        }
    }
    let routers = strings_of(member(stages, "connection_router"));
    assert!(routers.iter().any(|id| id == "krt-grid"), "{routers:?}");
    let fanouts = strings_of(member(stages, "fanout"));
    assert!(
        fanouts.iter().any(|id| id == "krt-qfn-escape"),
        "{fanouts:?}"
    );
    let board = strings_of(member(stages, "board_stages"));
    assert!(board.iter().any(|id| id == "krt-meander"), "{board:?}");
}

#[test]
fn list_stages_lists_the_krt_ids() {
    let dir = scratch("krt-list-stages");
    let out = run_in(&dir, &["--list-stages"]);
    assert_eq!(code(&out), 0, "{}", stderr(&out));
    let text = stdout(&out);
    for (kind, ids) in registered_ids(&freerouting_stages::registry()) {
        for id in ids {
            assert!(text.contains(id), "{kind}: {id} is not in --list-stages");
        }
    }
}

/// Every KRT connection router id is accepted as a value of the key, and an
/// unknown one is still refused.
#[test]
fn a_krt_connection_router_is_accepted() {
    let (out, dir) = route(
        "krt-select",
        &["--router.autorouter.connection_router=krt-grid"],
    );
    assert_eq!(code(&out), 0, "{}", stderr(&out));
    assert!(dir.join("out.ses").exists());

    let (out, _) = route(
        "krt-select-unknown",
        &["--router.autorouter.connection_router=krt-nonesuch"],
    );
    assert_ne!(code(&out), 0);
    assert!(stderr(&out).contains("krt-nonesuch"), "{}", stderr(&out));
}

/// With no KRT key set the session is the built-in registry's (the
/// engine-level proof is `freerouting-stages/tests/registry.rs`; this
/// pins the binary against the in-process job without the KRT stages).
#[test]
fn the_default_session_is_unchanged_by_the_registration() {
    use freerouting::job::outputs::{OutputFormat, OutputSpec};
    use freerouting::job::prepare::{JobInput, RulesChoice};
    use freerouting::job::run::{JobContext, run_job};
    use freerouting::settings::{ResolveContext, parse_cli_arguments};

    let (out, dir) = route("krt-default", &[]);
    assert_eq!(code(&out), 0, "{}", stderr(&out));
    let cli_ses = std::fs::read(dir.join("out.ses")).unwrap();

    let path = repo("fixtures").join(FIXTURE);
    let mut input = JobInput::new(
        std::fs::read(&path).unwrap(),
        JobInput::design_name_of(FIXTURE),
    );
    input.rules = RulesChoice::Adjacent;
    input.dsn_dir = path.parent().map(std::path::Path::to_path_buf);
    input.cli = Some(parse_cli_arguments(&["-mp".to_string(), "3".to_string()]).patch);
    let reference = dir.join("builtin.ses");
    let outputs = [OutputSpec {
        path: reference.clone(),
        format: OutputFormat::Ses,
    }];
    let outcome = run_job(
        input,
        &outputs,
        &JobContext::new(ResolveContext::from_machine()),
    );
    assert_eq!(outcome.exit_code(), 0);
    assert!(
        std::fs::read(reference).unwrap() == cli_ses,
        "the CLI's session differs from the built-in registry's"
    );
}
