//! N05.1: the registry the binaries route with.

use std::path::{Path, PathBuf};

use freerouting::autoroute::stages::StageRegistry;
use freerouting::autoroute::stages::catalog::registered_ids;
use freerouting::job::outputs::{OutputFormat, OutputSpec};
use freerouting::job::prepare::{JobInput, RulesChoice};
use freerouting::job::run::{JobContext, run_job};
use freerouting::settings::{ResolveContext, parse_cli_arguments};

const FIXTURE: &str = "Issue026-J2_reference.dsn";

fn fixtures() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures")
}

#[test]
fn the_registry_is_the_builtin_one_plus_the_krt_stages() {
    let full = registered_ids(&freerouting_stages::registry());
    let builtin = registered_ids(&StageRegistry::builtin());
    let mut krt_only = 0;
    for ((kind, all), (_, own)) in full.iter().zip(&builtin) {
        for id in own {
            assert!(all.contains(id), "{kind}: builtin {id} is registered");
        }
        krt_only += all.iter().filter(|id| id.starts_with("krt-")).count();
    }
    assert!(krt_only > 0, "the KRT stages are registered");
    for id in ["krt-grid", "krt-meander", "krt-diffpair"] {
        assert!(
            full.iter().any(|(_, ids)| ids.contains(&id)),
            "{id} is registered"
        );
    }
}

/// Routes `FIXTURE` with `-mp 3` plus `extra` flags and the context's
/// registry; returns the exit code and the SES bytes.
fn route(
    dir: &Path,
    name: &str,
    extra: &[&str],
    registry: Option<std::sync::Arc<StageRegistry>>,
) -> (i32, Vec<u8>) {
    let path = fixtures().join(FIXTURE);
    let mut input = JobInput::new(
        std::fs::read(&path).unwrap(),
        JobInput::design_name_of(FIXTURE),
    );
    input.rules = RulesChoice::Adjacent;
    input.dsn_dir = path.parent().map(Path::to_path_buf);
    let mut args = vec!["-mp".to_string(), "3".to_string()];
    args.extend(extra.iter().map(|a| a.to_string()));
    input.cli = Some(parse_cli_arguments(&args).patch);
    let out = dir.join(format!("{name}.ses"));
    let outputs = [OutputSpec {
        path: out.clone(),
        format: OutputFormat::Ses,
    }];
    let mut ctx = JobContext::new(ResolveContext { cores: 1 });
    ctx.registry = registry;
    let outcome = run_job(input, &outputs, &ctx);
    (outcome.exit_code(), std::fs::read(out).unwrap_or_default())
}

#[test]
fn registering_the_krt_stages_changes_no_session_while_none_is_selected() {
    let dir = std::env::temp_dir().join(format!("n051-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let (code_a, builtin) = route(&dir, "builtin", &[], None);
    let (code_b, full) = route(
        &dir,
        "full",
        &[],
        Some(freerouting_stages::shared_registry()),
    );
    assert_eq!((code_a, code_b), (0, 0));
    assert!(!builtin.is_empty());
    assert!(builtin == full, "the sessions differ");
}

#[test]
fn a_krt_stage_runs_with_the_full_registry_and_is_unknown_without() {
    let dir = std::env::temp_dir().join(format!("n051-sel-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let key = "--router.autorouter.connection_router=krt-grid";
    let (code, ses) = route(
        &dir,
        "krt",
        &[key],
        Some(freerouting_stages::shared_registry()),
    );
    assert_eq!(code, 0);
    assert!(!ses.is_empty());
    let (code, _) = route(&dir, "unknown", &[key], None);
    assert_ne!(code, 0, "krt-grid is unknown to the built-in registry");
}
