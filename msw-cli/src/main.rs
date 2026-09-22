/* This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at http://mozilla.org/MPL/2.0/. */

//! Command line front end.
//!
//! Applying a display configuration is a stateless system call, so this talks
//! to `msw-core` directly rather than to the tray application. Profile
//! switching works whether or not the tray app happens to be running, which is
//! what makes it dependable as a Stream Deck target.

use std::collections::BTreeMap;
use std::process::ExitCode;

use anyhow::{bail, Context, Result};
use clap::{Parser, Subcommand};
use msw_core::{apply, power, Store};

#[derive(Parser)]
#[command(
    name = "msw",
    version,
    about = "Switch between saved monitor configurations",
    long_about = "Switch between saved monitor configurations.\n\n\
                  Profiles are stored as JSON under %APPDATA%\\ModernMonitorSwitcher\\profiles.\n\
                  Bind `msw apply <name>` to a Stream Deck button or a shortcut to switch instantly."
)]
struct Cli {
    /// Read profiles from this directory instead of the default.
    #[arg(long, global = true, value_name = "DIR")]
    profiles_dir: Option<std::path::PathBuf>,

    /// Print what is happening. Repeat for more detail.
    #[arg(short, long, global = true, action = clap::ArgAction::Count)]
    verbose: u8,

    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// List saved profiles.
    List {
        /// Emit JSON instead of a table.
        #[arg(long)]
        json: bool,
    },

    /// Show the display configuration currently on screen.
    Current {
        /// Emit the full configuration as JSON.
        #[arg(long)]
        json: bool,
    },

    /// Save the current configuration under a name.
    Save {
        name: String,
        /// Overwrite an existing profile of the same name.
        #[arg(long)]
        force: bool,
    },

    /// Switch to a saved profile.
    Apply {
        name: String,
        /// Report whether the profile would apply, changing nothing on screen.
        #[arg(long)]
        dry_run: bool,
    },

    /// Delete a saved profile.
    Delete {
        name: String,
        /// Do not ask for confirmation.
        #[arg(short, long)]
        yes: bool,
    },

    /// Rename a saved profile.
    Rename { from: String, to: String },

    /// Switch every monitor off. Any input wakes them.
    MonitorsOff,

    /// Ask Windows to restore its own remembered layout for the monitors that
    /// are connected now. Use this if a profile leaves you somewhere unusable.
    Reset,
}

fn main() -> ExitCode {
    let cli = Cli::parse();
    init_logging(cli.verbose);

    match run(&cli) {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("error: {e:#}");
            ExitCode::FAILURE
        }
    }
}

fn init_logging(verbose: u8) {
    let level = match verbose {
        0 => "warn",
        1 => "info",
        _ => "debug",
    };
    let filter =
        std::env::var("MSW_LOG").unwrap_or_else(|_| format!("msw_core={level},msw_cli={level}"));
    let _ = tracing_subscriber::fmt()
        .with_env_filter(filter)
        .with_target(false)
        .without_time()
        .with_writer(std::io::stderr)
        .try_init();
}

fn store(cli: &Cli) -> Result<Store> {
    match &cli.profiles_dir {
        Some(dir) => Ok(Store::at(dir)),
        None => Store::default_location().context("locating the profiles directory"),
    }
}

/// Monitor nicknames, as set in the tray application.
///
/// Read straight from the settings file rather than shared through a library,
/// because this is the only thing out here that cares about them. A missing or
/// unreadable file simply means no nicknames, which leaves the model names in
/// place: worth no more than a debug line, since the command still works.
fn nicknames() -> BTreeMap<String, String> {
    #[derive(serde::Deserialize, Default)]
    struct JustTheNames {
        #[serde(default)]
        monitor_names: BTreeMap<String, String>,
    }

    let Some(appdata) = std::env::var_os("APPDATA") else {
        return BTreeMap::new();
    };
    let path = std::path::Path::new(&appdata)
        .join("ModernMonitorSwitcher")
        .join("settings.json");

    match std::fs::read_to_string(&path) {
        Ok(text) => serde_json::from_str::<JustTheNames>(&text)
            .map(|s| s.monitor_names)
            .unwrap_or_else(|e| {
                tracing::debug!(error = %e, "could not read monitor nicknames");
                BTreeMap::new()
            }),
        Err(_) => BTreeMap::new(),
    }
}

fn run(cli: &Cli) -> Result<()> {
    let store = store(cli)?;

    match &cli.command {
        Command::List { json } => list(&store, *json),
        Command::Current { json } => current(*json),
        Command::Save { name, force } => save(&store, name, *force),
        Command::Apply { name, dry_run } => apply_profile(&store, name, *dry_run),
        Command::Delete { name, yes } => delete(&store, name, *yes),
        Command::Rename { from, to } => {
            store.rename(from, to)?;
            println!("Renamed {from:?} to {to:?}.");
            Ok(())
        }
        Command::MonitorsOff => {
            power::all_monitors_off();
            Ok(())
        }
        Command::Reset => reset(),
    }
}

fn list(store: &Store, json: bool) -> Result<()> {
    let profiles = store.list()?;

    if json {
        println!("{}", serde_json::to_string_pretty(&profiles)?);
        return Ok(());
    }

    if profiles.is_empty() {
        println!("No profiles yet.");
        println!();
        println!("Arrange your monitors the way you want them, then run:");
        println!("    msw save \"Play\"");
        return Ok(());
    }

    let nicknames = nicknames();
    let width = profiles.iter().map(|p| p.name.len()).max().unwrap_or(0);
    for profile in &profiles {
        let labels = profile.config.active_monitor_labels_with(&nicknames);
        let summary = match labels.len() {
            0 => "no active monitors".to_string(),
            1 => labels[0].clone(),
            n => format!("{n} monitors: {}", labels.join(", ")),
        };
        println!("{:width$}  {}", profile.name, summary);
    }
    Ok(())
}

fn current(json: bool) -> Result<()> {
    let config = msw_core::current_config()?;

    if json {
        println!("{}", serde_json::to_string_pretty(&config)?);
        return Ok(());
    }

    let nicknames = nicknames();
    let active = config.active_monitor_labels_with(&nicknames);
    println!(
        "{} active of {} known display path(s):",
        active.len(),
        config.paths.len()
    );
    for label in &active {
        println!("  * {label}");
    }

    let inactive: Vec<_> = config
        .monitors
        .iter()
        .filter(|m| {
            !config.paths.iter().any(|p| {
                p.is_active() && p.target.id == m.id && p.target.adapter_id == m.adapter_id
            })
        })
        .collect();

    if !inactive.is_empty() {
        println!();
        println!("Connected but not in use:");
        for monitor in inactive {
            println!("  - {}", monitor.label_with(&nicknames));
        }
    }

    Ok(())
}

fn save(store: &Store, name: &str, force: bool) -> Result<()> {
    if store.exists(name) && !force {
        bail!("a profile named {name:?} already exists; pass --force to replace it");
    }

    let profile = store.capture(name)?;
    println!("Saved {:?}: {}", profile.name, profile.summary());
    Ok(())
}

fn apply_profile(store: &Store, name: &str, dry_run: bool) -> Result<()> {
    let profile = store.load(name)?;

    if dry_run {
        let options = msw_core::preflight_profile(&profile)?;
        if options.is_empty() {
            println!("{name:?} would NOT apply cleanly.");
            println!();
            println!("Windows rejected every way of matching this profile to the monitors");
            println!("connected right now. Applying it anyway may still work, but check that");
            println!("the monitors this profile expects are actually plugged in.");
            return Ok(());
        }

        let (strategy, lenient) = options[0];
        println!("{name:?} would apply: {}.", strategy.describe());
        if lenient {
            println!();
            println!("Note: only with SDC_ALLOW_CHANGES, so Windows would be free to adjust");
            println!("resolution or refresh rate to make it fit.");
        }
        if options.len() > 1 {
            println!();
            println!("Fallbacks available if that fails:");
            for (strategy, lenient) in &options[1..] {
                let suffix = if *lenient { " (with adjustments)" } else { "" };
                println!("  - {}{suffix}", strategy.describe());
            }
        }
        return Ok(());
    }

    let outcome = msw_core::apply_profile(&profile)?;

    print!("Switched to {:?}", profile.name);
    if outcome.strategy != apply::Strategy::Verbatim {
        print!(" ({})", outcome.strategy.describe());
    }
    println!(".");

    if outcome.lenient {
        println!();
        println!("Windows adjusted the configuration to make it fit, so resolution or refresh");
        println!("rate may differ from what you saved. Re-save the profile to capture what you");
        println!("actually ended up with.");
    }

    Ok(())
}

fn delete(store: &Store, name: &str, yes: bool) -> Result<()> {
    if !store.exists(name) {
        bail!("no profile named {name:?}");
    }

    if !yes {
        eprint!("Delete profile {name:?}? [y/N] ");
        use std::io::{BufRead, Write};
        std::io::stderr().flush().ok();

        let mut answer = String::new();
        std::io::stdin().lock().read_line(&mut answer)?;
        if !matches!(answer.trim().to_lowercase().as_str(), "y" | "yes") {
            println!("Left {name:?} alone.");
            return Ok(());
        }
    }

    store.delete(name)?;
    println!("Deleted {name:?}.");
    Ok(())
}

fn reset() -> Result<()> {
    let status = msw_core::ccd::reset_to_database_current();
    if !msw_core::ccd::is_success(status) {
        bail!("Windows refused to restore its remembered layout (error {status})");
    }
    println!("Restored the layout Windows has remembered for the monitors connected now.");
    Ok(())
}
