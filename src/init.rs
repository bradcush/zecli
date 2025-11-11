use age::secrecy::ExposeSecret;
use bip0039::{Count, English, Mnemonic};
use clap::Args;
use secrecy::{ExposeSecret as _, SecretString, SecretVec, Zeroize};
use tokio::io::AsyncWriteExt;
use tonic::transport::Channel;
use zcash_client_backend::{
    data_api::{AccountBirthday, WalletWrite},
    proto::service::{
        self, compact_tx_streamer_client::CompactTxStreamerClient,
    },
};
use zcash_protocol::consensus::{self, BlockHeight, Parameters};

use crate::{
    config::WalletConfig,
    data::{init_dbs, Network},
    error,
    remote::{tor_client, Servers},
};

#[derive(Debug, Args)]
pub(crate) struct InitOptions {
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

impl InitOptions {
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
        // recover-until height). Not sure what birthday and height refer to.
        // This is the height of the blockchain when the wallet is created.
        let chain_tip: u32 = client
            .get_latest_block(service::ChainSpec::default())
            .await?
            .into_inner()
            .height
            .try_into()
            .expect("block heights must fit into u32");
        let recipients = if tokio::fs::try_exists(&opts.identity).await? {
            // Seems like age encryption library used is < 1 which is cautioned
            // to be used for testing purposes only and not necessarily secure
            age::IdentityFile::from_file(opts.identity)?.to_recipients()?
        } else {
            // Better understand what an age identity is. When we don't have
            // one we create it for the first time. Seems like it's just some
            // basic encryption for a seed phrase. Uses the age encryption
            // library (https://github.com/FiloSottile/age).
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
            // SecretVec allows for protected-access memory for
            // cryptographic secrets. Convenient allocation/access
            // for more protected memory. Notions of guard pages around
            // memory, read/written only in limited scopes.
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
        // Fetch the tree state corresponding to the last block prior
        // to the wallet's birthday height. NOTE: THIS APPROACH LEAKS
        // THE BIRTHDAY TO THE SERVER! Leaking a birthday allows the
        // server to fingerprint transactions. Need to hide the
        // exact birthday if we want to preserve privacy.
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
        // Initialize the block and wallet DBs. Better
        // understand how this is used and what we're storing
        // in here initially that's not in a config file.
        let mut db_data = init_dbs(params, wallet_dir)?;
        // How is the seed protected in this database? It doesn't seem
        // like it would be but maybe I'm missing something.
        db_data.create_account(account_name, seed, &birthday, key_source)?;
        Ok(())
    }
}
