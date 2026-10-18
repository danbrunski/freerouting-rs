//! `--list-stages` and `--dump-settings` (N01.4): what a run would use,
//! printed instead of routing.
//!
//! - `--list-stages` prints every stage slot of the pipeline, the stage ids
//!   registered for it, the id the routing profile starts with and the
//!   options of the slot with the profile's defaults
//!   ([`freerouting::autoroute::stages::catalog`]); `--list-stages=json`
//!   prints the same as JSON.
//! - `--dump-settings` prints every setting with its effective value and
//!   the source that set it, and marks the values that came from the
//!   profile or differ from it ([`freerouting::settings::dump`]). With
//!   `-de` the design is loaded as for a run (its `autoroute_settings`, the
//!   rules file, the board tuning); without it the sources are the defaults,
//!   the profile, the settings file, the environment and the command line.
//!
//! The profile is the one the settings sources name (`--profile`,
//! `--router.profile=`, the environment, the settings file), else
//! `enhanced`.

use std::path::Path;

use freerouting::autoroute::stages::catalog::{
    EXPERIMENTAL, Experimental, ExperimentalKind, SlotArity, StageSlot, catalog,
};
use freerouting::drc::json::JsonValue;
use freerouting::job::prepare::{
    DesignKind, ReadOptions, design_kind, prepare_job, read_kicad_input,
};
use freerouting::settings::dump::{effective_json, effective_rows, effective_text, value_to_json};
use freerouting::settings::{Profile, ResolveContext, SettingsPatch, Source, defaults, resolve};

use crate::args::{Args, ListFormat};

fn arity_name(arity: SlotArity) -> &'static str {
    match arity {
        SlotArity::One => "one",
        SlotArity::Optional => "optional",
        SlotArity::List => "list",
    }
}

fn strings<'a>(items: impl IntoIterator<Item = &'a str>) -> JsonValue {
    JsonValue::Array(items.into_iter().map(JsonValue::string).collect())
}

/// The stage listing as JSON.
pub fn stages_json(profile: Profile, slots: &[StageSlot]) -> JsonValue {
    let profiles = Profile::ALL
        .iter()
        .map(|p| {
            JsonValue::Object(vec![
                ("name".to_string(), JsonValue::string(p.name())),
                ("label".to_string(), JsonValue::string(p.label())),
                (
                    "description".to_string(),
                    JsonValue::string(p.description()),
                ),
            ])
        })
        .collect();
    let slots = slots
        .iter()
        .map(|slot| {
            let options = slot
                .options
                .iter()
                .map(|o| {
                    JsonValue::Object(vec![
                        (
                            "key".to_string(),
                            JsonValue::String(format!("router.{}", o.path)),
                        ),
                        (
                            "default".to_string(),
                            o.default.clone().map_or(JsonValue::Null, value_to_json),
                        ),
                        ("doc".to_string(), JsonValue::string(o.doc)),
                        (
                            "experimental".to_string(),
                            JsonValue::Bool(o.experimental.is_some()),
                        ),
                    ])
                })
                .collect();
            JsonValue::Object(vec![
                ("kind".to_string(), JsonValue::string(slot.desc.kind)),
                ("label".to_string(), JsonValue::string(slot.desc.label)),
                (
                    "key".to_string(),
                    JsonValue::String(format!("router.{}", slot.desc.key)),
                ),
                (
                    "arity".to_string(),
                    JsonValue::string(arity_name(slot.desc.arity)),
                ),
                ("ids".to_string(), strings(slot.ids.iter().copied())),
                (
                    "default".to_string(),
                    strings(slot.default.iter().map(String::as_str)),
                ),
                ("options".to_string(), JsonValue::Array(options)),
                (
                    "experimental_ids".to_string(),
                    strings(slot.experimental_ids.iter().map(|e| e.key)),
                ),
            ])
        })
        .collect();
    JsonValue::Object(vec![
        ("profile".to_string(), JsonValue::string(profile.name())),
        ("profiles".to_string(), JsonValue::Array(profiles)),
        ("slots".to_string(), JsonValue::Array(slots)),
        (
            "experimental".to_string(),
            JsonValue::Array(EXPERIMENTAL.iter().map(experimental_json).collect()),
        ),
    ])
}

/// One experimental entry as JSON (N02.4).
fn experimental_json(e: &Experimental) -> JsonValue {
    let (kind, key) = match e.kind {
        ExperimentalKind::Option => ("option", format!("router.{}", e.key)),
        ExperimentalKind::Stage => ("stage", e.key.to_string()),
    };
    JsonValue::Object(vec![
        ("kind".to_string(), JsonValue::string(kind)),
        ("key".to_string(), JsonValue::String(key)),
        ("slot".to_string(), JsonValue::string(e.slot)),
        ("class".to_string(), JsonValue::string(e.class)),
        ("help".to_string(), JsonValue::string(e.help)),
    ])
}

/// The stage listing as a table.
pub fn stages_table(profile: Profile, slots: &[StageSlot]) -> String {
    let mut out = format!(
        "Routing profile: {} ({})\n\
         Pick one with --profile legacy|enhanced; set a stage with --router.<key>=<id>.\n",
        profile.name(),
        profile.description()
    );
    for slot in slots {
        let default = if slot.default.is_empty() {
            "(none)".to_string()
        } else {
            slot.default.join(", ")
        };
        out.push_str(&format!(
            "\n{} (router.{}, {})\n  default: {default}\n  ids:     {}\n",
            slot.desc.label,
            slot.desc.key,
            arity_name(slot.desc.arity),
            if slot.ids.is_empty() {
                "(none registered)".to_string()
            } else {
                slot.ids
                    .iter()
                    .map(|id| {
                        if slot.experimental_ids.iter().any(|e| e.key == *id) {
                            format!("{id} [experimental]")
                        } else {
                            (*id).to_string()
                        }
                    })
                    .collect::<Vec<_>>()
                    .join(", ")
            }
        ));
        for option in &slot.options {
            let value = option
                .default
                .as_ref()
                .map_or_else(|| "(unset)".to_string(), ToString::to_string);
            let mark = if option.experimental.is_some() {
                "  [experimental]"
            } else {
                ""
            };
            out.push_str(&format!("  router.{} = {value}{mark}\n", option.path));
        }
    }
    out.push_str(
        "\nExperimental (off in every profile; N02's keep rule: each wins on a board class, \
         or is kept pending a measurement)\n",
    );
    for e in EXPERIMENTAL {
        let key = match e.kind {
            ExperimentalKind::Option => format!("router.{}", e.key),
            ExperimentalKind::Stage => format!("{} (stage)", e.key),
        };
        out.push_str(&format!(
            "  {key}\n    class: {}\n    {}\n",
            e.class, e.help
        ));
    }
    out
}

/// The settings sources of the command line without a design: the
/// defaults, the settings file, the environment and the flags.
fn sources(
    config: Option<SettingsPatch>,
    environment: Option<SettingsPatch>,
    parsed: &Args,
    ctx: &ResolveContext,
) -> Vec<(Source, SettingsPatch)> {
    let mut sources = vec![(Source::Defaults, defaults(ctx))];
    if let Some(patch) = config {
        sources.push((Source::ConfigFile, patch));
    }
    if let Some(patch) = environment {
        sources.push((Source::Environment, patch));
    }
    sources.push((Source::Cli, parsed.patch.clone()));
    sources
}

/// `--list-stages`: the text to print, or a settings error.
pub fn list_stages(
    parsed: &Args,
    config: Option<SettingsPatch>,
    environment: Option<SettingsPatch>,
    format: ListFormat,
) -> Result<String, String> {
    let ctx = ResolveContext::from_machine();
    let resolved = resolve(&sources(config, environment, parsed, &ctx), &ctx);
    let profile = resolved.profile.map_err(|e| e.to_string())?;
    let slots = catalog(&freerouting_stages::registry(), profile, ctx.cores);
    Ok(match format {
        ListFormat::Table => stages_table(profile, &slots),
        ListFormat::Json => format!("{}\n", stages_json(profile, &slots).to_pretty_string()),
    })
}

/// `--dump-settings`: the text to print, or an error (a settings error or a
/// design that does not load).
pub fn dump_settings(
    parsed: &Args,
    config: Option<SettingsPatch>,
    environment: Option<SettingsPatch>,
    format: ListFormat,
) -> Result<String, String> {
    let ctx = ResolveContext::from_machine();
    let (patch, provenance) = match &parsed.design_file {
        None => {
            let resolved = resolve(&sources(config, environment, parsed, &ctx), &ctx);
            resolved.profile.clone().map_err(|e| e.to_string())?;
            (resolved.patch, resolved.provenance)
        }
        Some(design) => {
            let path = Path::new(design);
            let (dsn, kicad) = if design_kind(path) == DesignKind::KicadPcb {
                let read = read_kicad_input(path, ReadOptions::default())
                    .map_err(|e| format!("{design}: {e}"))?;
                (Vec::new(), Some(read))
            } else {
                let bytes = std::fs::read(path)
                    .map_err(|e| format!("Couldn't load the input file '{design}': {e}"))?;
                (bytes, None)
            };
            let mut input = crate::cli::build_job_input(parsed, design, dsn, None);
            if let Some(read) = kicad {
                input.set_kicad(read);
            }
            input.config_file = config;
            input.environment = environment;
            let prepared = prepare_job(&input, &ctx).map_err(|e| e.to_string())?;
            (prepared.patch, prepared.provenance)
        }
    };
    let profile = Profile::of_patch(&patch).map_err(|e| e.to_string())?;
    let rows = effective_rows(&patch, &provenance, profile, ctx.cores);
    Ok(match format {
        ListFormat::Table => effective_text(profile, &rows),
        ListFormat::Json => format!("{}\n", effective_json(profile, &rows).to_pretty_string()),
    })
}
