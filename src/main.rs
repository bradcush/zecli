use clap::{Args, ColorChoice, Parser, Subcommand};
use std::sync::atomic::{AtomicUsize, Ordering};
use tracing::info;
use tracing_subscriber::{fmt, prelude::*, EnvFilter};

// Understand modules a bit better and how we want
// to use them in other parts of the code base

mod balance;
mod config;
mod data;
mod error;
mod init;
mod remote;
mod ui;

use crate::{balance::BalanceOptions, init::InitOptions};

// Clap is super smart in the documentation it generates
// for the command-line --help specific to each command

// I'm using different names for each level to more easily distinguish
// what is what. That means hierarchy of command, flag, options.

#[derive(Debug, Subcommand)]
pub(crate) enum Flag {
    /// Initialize a new light wallet
    Init(InitOptions),

    /// Get the balance in the wallet
    Balance(BalanceOptions),
}

#[derive(Debug, Args)]
pub(crate) struct Wallet {
    /// Path to the wallet directory
    #[arg(short, long)]
    pub(crate) dir: Option<String>,

    #[command(subcommand)]
    pub(crate) flag: Flag,
}

#[derive(Debug, Subcommand)]
pub(crate) enum Command {
    /// Local wallet interaction
    Wallet(Wallet),
}

#[derive(Debug, Parser)]
#[command(color = ColorChoice::Always)]
pub(crate) struct Cli {
    #[command(subcommand)]
    pub(crate) command: Option<Command>,
}

// Returning a generic error w/ explicit typings
fn main() -> Result<(), anyhow::Error> {
    // Initialize a logger we can
    // use with different levels
    tracing_subscriber::registry()
        .with(fmt::layer())
        .with(EnvFilter::from_default_env())
        .init();
    info!("starting");
    let opts = Cli::parse();
    // Better understand what's happening here, what would the result
    // be if I didn't use await, maybe I'm forced to at some point because
    // we're calling async functions from other libraries inside
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .thread_name_fn(|| {
            static ATOMIC_ID: AtomicUsize = AtomicUsize::new(0);
            let id = ATOMIC_ID.fetch_add(1, Ordering::SeqCst);
            format!("zec-tokio-{id}")
        })
        .build()?;
    // We need this to run a future, using the current
    // thread for async tasks which we call for commands
    runtime.block_on(async {
        let Some(cmd) = opts.command else {
            return Ok(());
        };
        match cmd {
            // I think options gets matched through the enum
            // and I can then use it in the arm of the match
            Command::Wallet(Wallet { dir, flag }) => match flag {
                Flag::Init(options) => options.run(dir).await,
                Flag::Balance(options) => options.run(dir).await,
            },
        }
    })
}
