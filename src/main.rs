use age::secrecy::ExposeSecret;
use bip0039::{Count, English, Mnemonic};
use clap::{Args, Parser, Subcommand};
use secrecy::{ExposeSecret as _, SecretString, SecretVec, Zeroize};
use std::sync::atomic::{AtomicUsize, Ordering};
use tokio::io::AsyncWriteExt;
use tonic::transport::Channel;
use tracing::info;
use tracing_subscriber::{fmt, prelude::*, EnvFilter};
use zcash_client_backend::{
    data_api::{AccountBirthday, WalletWrite},
    proto::service::{
        self, compact_tx_streamer_client::CompactTxStreamerClient,
    },
};
use zcash_protocol::consensus::{self, BlockHeight, Parameters};

// Understand modules a bit better and how we want
// to use them in other parts of the code base

mod config;
mod data;
mod error;
mod remote;

use crate::{
    config::WalletConfig,
    data::{init_dbs, Network},
    remote::{tor_client, Servers},
};

// Clap is super smart in the documentation it generates
// for the command-line --help specific to each command

// I'm using different names for each level to more easily distinguish
// what is what. That means hierarchy of command, flag, options.

// Understand why I dervie Debug,
// this wasn't in my original code
#[derive(Debug, Parser)]
pub(crate) struct Cli {
    #[command(subcommand)]
    pub(crate) command: Option<Command>,
}

#[derive(Debug, Args)]
pub(crate) struct Options {
    /// A name for the account
    #[arg(long)]
    name: String,

    /// Age identity file to encrypt the mnemonic
    /// phrase to (generated if it doesn't exist)
    #[arg(short, long)]
    identity: String,

    /// The wallet's birthday
    /// (default is current chain height)
    #[arg(short, long)]
    // Not required if it's an option which
    // clap is smart enough to handle
    birthday: Option<u32>,

    /// Network the wallet is used with: \"test\"
    /// or \"main\" (default is \"test\")
    #[arg(short, long)]
    #[arg(value_parser = Network::parse)]
    network: Network,

    /// The server to initialize with
    /// (default is \"ecc\")
    #[arg(short, long)]
    #[arg(default_value = "ecc", value_parser = Servers::parse)]
    server: Servers,
}

impl Options {
    pub(crate) async fn run(
        self,
        wallet_dir: Option<String>,
    ) -> Result<(), anyhow::Error> {
        let opts = self;
        let params = consensus::Network::from(opts.network);
        let server = opts.server.pick(params)?;
        // Use Tor for all connections
        let mut client =
            server.connect(|| tor_client(wallet_dir.as_ref())).await?;
        // Get the current chain height (for the wallet's birthday and/or
        // recover-until height). Not sure what birthday and heigh refer to.
        let chain_tip: u32 = client
            .get_latest_block(service::ChainSpec::default())
            .await?
            .into_inner()
            .height
            .try_into()
            .expect("block heights must fit into u32");
        let recipients = if tokio::fs::try_exists(&opts.identity).await? {
            age::IdentityFile::from_file(opts.identity)?.to_recipients()?
        } else {
            // Better understand what an age identity is. Whe we
            // don't have one we create it for the first time. Seems
            // like it's just some basic encryption for a seed phrase.
            eprintln!(
                "Generating a new age identity to encrypt the mnemonic phrase"
            );
            let identity = age::x25519::Identity::generate();
            let recipient = identity.to_public();
            // Write it to the path so we have it for next time
            let mut f = tokio::fs::File::create_new(opts.identity).await?;
            // All writing logic for the key we want unsafely save locally
            // so we can encrypt/decrypt the seed phrase we generate
            f.write_all(
                format!(
                    "# created: {}\n",
                    chrono::Local::now()
                        .to_rfc3339_opts(chrono::SecondsFormat::Secs, true)
                )
                .as_bytes(),
            )
            .await?;
            f.write_all(format!("# public key: {recipient}\n").as_bytes())
                .await?;
            f.write_all(
                format!("{}\n", identity.to_string().expose_secret())
                    .as_bytes(),
            )
            .await?;
            f.flush().await?;
            // I think it's just a vector of recipients,
            // obviously we would only have one at this point
            vec![Box::new(recipient) as _]
        };
        // Parse or create the wallet's mnemonic phrase
        let phrase = SecretString::new(rpassword::prompt_password(
            "Enter mnemonic (or just press Enter to generate a new one):",
        )?);
        let (mnemonic, recover_until) = if !phrase.expose_secret().is_empty() {
            (
                <Mnemonic<English>>::from_phrase(phrase.expose_secret())?,
                Some(chain_tip.into()),
            )
        } else {
            (Mnemonic::generate(Count::Words24), None)
        };
        // Understand what a birthday is, seems like it
        // could be like some sort of creation time
        let birthday = Self::get_wallet_birthday(
            client,
            opts.birthday
                .unwrap_or(chain_tip.saturating_sub(100))
                .into(),
            recover_until,
        )
        .await?;
        // Save the wallet keys to disk, mnemonic is going to be encrypted in a
        // persisted config. We need to encrypt it to recover if we want
        // because we'll only use a derived seed from here on out.
        WalletConfig::init_with_mnemonic(
            wallet_dir.as_ref(),
            recipients.iter().map(|r| r.as_ref() as _),
            &mnemonic,
            birthday.height(),
            opts.network.into(),
        )?;
        let seed = {
            // Nice use of blocked scope so we get scope
            // but don't need a function for a simple
            // assignment. Defaulting to an empty passphrase.
            let mut seed = mnemonic.to_seed("");
            let secret = seed.to_vec();
            seed.zeroize();
            // Understand what exactly a SecretVec is
            SecretVec::new(secret)
        };
        Self::init_dbs(
            params,
            wallet_dir.as_ref(),
            &opts.name,
            &seed,
            birthday,
            None,
        )
    }

    pub(crate) async fn get_wallet_birthday(
        mut client: CompactTxStreamerClient<Channel>,
        birthday_height: BlockHeight,
        recover_until: Option<BlockHeight>,
    ) -> Result<AccountBirthday, anyhow::Error> {
        // Fetch the tree state corresponding to the last block
        // prior to the wallet's birthday height. NOTE: THIS
        // APPROACH LEAKS THE BIRTHDAY TO THE SERVER!
        // Think about how we should do this without leaking,
        // understand why it's important to not leak the birthday.
        let request = service::BlockId {
            height: u64::from(birthday_height).saturating_sub(1),
            ..Default::default()
        };
        let treestate = client.get_tree_state(request).await?.into_inner();
        let birthday =
            AccountBirthday::from_treestate(treestate, recover_until)
                .map_err(error::Error::from)?;
        Ok(birthday)
    }

    pub(crate) fn init_dbs(
        params: impl Parameters + 'static,
        wallet_dir: Option<&String>,
        account_name: &str,
        seed: &SecretVec<u8>,
        birthday: AccountBirthday,
        key_source: Option<&str>,
    ) -> Result<(), anyhow::Error> {
        // Initialise the block and wallet DBs. Better
        // understand how this is used and what we're storing
        // in here initially that's not in a config file.
        let mut db_data = init_dbs(params, wallet_dir)?;
        // How is the seec protected in this database?
        db_data.create_account(account_name, seed, &birthday, key_source)?;
        Ok(())
    }
}

#[derive(Debug, Subcommand)]
pub(crate) enum Flag {
    /// Initialise a new light wallet
    Init(Options),
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
    // Because it's async in a different thread, when I use println!
    // down the line it's not going to be output to the console
    runtime.block_on(async {
        let Some(cmd) = opts.command else {
            return Ok(());
        };
        match cmd {
            // I think options gets matched through the enum
            // and I can then use it in the arm of the match
            Command::Wallet(Wallet { dir, flag }) => match flag {
                Flag::Init(options) => options.run(dir).await,
            },
        }
    })
}
