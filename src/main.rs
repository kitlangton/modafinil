mod system;

use clap::{Parser, Subcommand, ValueEnum};
use modafinil_cli::{Mode, Settings};
use std::{error::Error, path::PathBuf};

type Result<T = ()> = std::result::Result<T, Box<dyn Error>>;

#[derive(Parser)]
#[command(
    name = "modafinil",
    version,
    about = "Keep your Mac awake, with power-aware settings"
)]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Clone, Copy, ValueEnum)]
enum Toggle {
    On,
    Off,
}

#[derive(Subcommand)]
enum Commands {
    /// Run in this terminal. Ctrl-C releases this process's keep-awake assertion.
    Run {
        /// Override the saved mode for this run only.
        #[arg(long)]
        mode: Option<Mode>,
        /// Stop after this many seconds (useful for testing).
        #[arg(long, value_parser = clap::value_parser!(u64).range(1..))]
        seconds: Option<u64>,
        #[arg(long, hide = true)]
        config: Option<PathBuf>,
    },
    /// Show settings, power source and service state. Does not change sleep settings.
    Status {
        #[arg(long)]
        json: bool,
    },
    /// Save settings. Closed-lid changes require a one-time admin approval.
    Set {
        #[arg(long)]
        mode: Option<Mode>,
        /// Closed-lid support is always restricted to AC power, even in Always mode.
        #[arg(long)]
        closed_lid: Option<Toggle>,
    },
    /// Start at login and start now. Updates the installed binary on repeat installs.
    Install,
    /// Stop the login service without changing your saved settings.
    Stop,
    /// Start the installed login service.
    Start,
    /// Remove services and installed copies. Leaves your settings and logs.
    Uninstall,
    /// Evaluate the power policy without touching system settings.
    Check {
        #[arg(long, value_enum)]
        power: CheckPower,
        #[arg(long, default_value = "ac")]
        mode: Mode,
        #[arg(long)]
        closed_lid: bool,
    },
    #[command(hide = true)]
    LidDaemon {
        #[arg(long)]
        mode: Mode,
    },
    #[command(hide = true)]
    HelperInstall {
        #[arg(long)]
        mode: Mode,
    },
    #[command(hide = true)]
    HelperRemove,
}

#[derive(Clone, Copy, ValueEnum)]
enum CheckPower {
    Ac,
    Battery,
    Unknown,
}

fn main() {
    if let Err(error) = execute(Cli::parse()) {
        eprintln!("modafinil: {error}");
        std::process::exit(1);
    }
}

fn execute(cli: Cli) -> Result {
    if let Commands::Check {
        power,
        mode,
        closed_lid,
    } = cli.command
    {
        let power = match power {
            CheckPower::Ac => modafinil_cli::Power::Ac,
            CheckPower::Battery => modafinil_cli::Power::Battery,
            CheckPower::Unknown => modafinil_cli::Power::Unknown,
        };
        let result = modafinil_cli::policy(Settings { mode, closed_lid }, power);
        println!("awake={} closed_lid={}", result.awake, result.closed_lid);
        return Ok(());
    }
    if !cfg!(target_os = "macos") {
        return Err("this command requires macOS; `check` works on any platform".into());
    }
    match cli.command {
        Commands::Run {
            mode,
            seconds,
            config,
        } => system::run(mode, seconds, config),
        Commands::Status { json } => system::status(json),
        Commands::Set { mode, closed_lid } => {
            system::settings(mode, closed_lid.map(|v| matches!(v, Toggle::On)))
        }
        Commands::Install => system::install(),
        Commands::Stop => system::stop(),
        Commands::Start => system::start(),
        Commands::Uninstall => system::uninstall(),
        Commands::LidDaemon { mode } => system::lid_daemon(mode),
        Commands::HelperInstall { mode } => system::helper_install(mode),
        Commands::HelperRemove => system::helper_remove(),
        Commands::Check { .. } => unreachable!(),
    }
}
