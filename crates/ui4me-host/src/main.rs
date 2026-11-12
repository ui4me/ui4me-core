use anyhow::Result;
use clap::{Parser, Subcommand};
use std::path::PathBuf;
use ui4me_host::{config::AppConfig, host, platform};

#[derive(Parser)]
#[command(name = "ui4me-host", version, about = "UI4ME native application host")]
struct Cli {
    #[command(subcommand)]
    command: Command,
}
#[derive(Subcommand)]
enum Command {
    /// Launch the backend described by a TOML application manifest.
    Run {
        #[arg(long)]
        app: PathBuf,
        /// Exercise protocol/lifecycle without claiming desktop integration.
        #[arg(long)]
        headless: bool,
        /// Explicit Linux diagnostic mode without systemd user containment.
        #[arg(long)]
        allow_unsupervised: bool,
    },
    /// Native input, clipboard, dialogs and accessibility probe without Python.
    Probe {
        #[arg(long)]
        allow_unsupervised: bool,
    },
    /// Print environment facts, not a claim of native qualification.
    Doctor,
}

fn execute() -> Result<i32> {
    let cli = Cli::parse();
    // Set only before starting threads. These are explicit launcher choices,
    // never inferred silently from a missing desktop/service manager.
    let headless = matches!(cli.command, Command::Run { headless: true, .. });
    let unsupervised = matches!(
        cli.command,
        Command::Run {
            allow_unsupervised: true,
            ..
        } | Command::Probe {
            allow_unsupervised: true
        }
    );
    if headless {
        std::env::set_var("UI4ME_TEST_MODE", "1");
    }
    if unsupervised {
        std::env::set_var("UI4ME_ALLOW_UNSUPERVISED", "1");
    }
    let _logging = ui4me_host::logging::init()?;
    if matches!(cli.command, Command::Doctor) {
        println!("{}", serde_json::to_string_pretty(&host::doctor())?);
        return Ok(0);
    }
    if unsupervised {
        tracing::warn!("explicit diagnostic containment selected; this does not qualify hard-host-death cleanup on Linux");
    }
    if let Some(code) =
        platform::bootstrap_supervised(&std::env::args_os().skip(1).collect::<Vec<_>>())?
    {
        return Ok(code);
    }
    match cli.command {
        Command::Run { app, headless, .. } => host::run(AppConfig::load(&app)?, headless),
        Command::Probe { .. } => host::run_probe(),
        Command::Doctor => unreachable!(),
    }
}

fn main() {
    let code = match execute() {
        Ok(code) => code,
        Err(error) => {
            eprintln!("ui4me-host: {error:#}");
            1
        }
    };
    std::process::exit(code);
}
