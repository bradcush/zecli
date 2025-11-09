use crate::data::{Network, DEFAULT_WALLET_DIR};
use anyhow::anyhow;
use bip0039::Mnemonic;
use serde::{Deserialize, Serialize};
use std::fs::{self};
use std::io::Write;
use std::path::Path;
use zcash_protocol::consensus::{self, BlockHeight};

const KEYS_FILE: &str = "keys.toml";

pub(crate) struct WalletConfig {}

impl WalletConfig {
    pub(crate) fn init_with_mnemonic<'a, P: AsRef<Path>>(
        wallet_dir: Option<P>,
        recipients: impl Iterator<Item = &'a dyn age::Recipient>,
        mnemonic: &Mnemonic,
        birthday: BlockHeight,
        network: consensus::Network,
    ) -> Result<(), anyhow::Error> {
        init_wallet_config(
            wallet_dir,
            Some(encrypt_mnemonic(recipients, mnemonic)?),
            birthday,
            network,
        )
    }
}

fn init_wallet_config<P: AsRef<Path>>(
    wallet_dir: Option<P>,
    mnemonic: Option<String>,
    birthday: BlockHeight,
    network: consensus::Network,
) -> Result<(), anyhow::Error> {
    // Create the wallet directory.
    let wallet_dir = wallet_dir
        .as_ref()
        .map(|p| p.as_ref())
        .unwrap_or(DEFAULT_WALLET_DIR.as_ref());
    fs::create_dir_all(wallet_dir)?;
    // Write the mnemonic phrase to
    // disk along with its birthday
    let mut keys_file = {
        let mut p = wallet_dir.to_owned();
        p.push(KEYS_FILE);
        fs::OpenOptions::new().create_new(true).write(true).open(p)
    }?;
    let config = ConfigEncoding {
        mnemonic,
        network: Some(Network::from(network).name().to_string()),
        birthday: Some(u32::from(birthday)),
    };
    // Seems like we're doing a custom config
    // which is just stringified from toml
    let config_str =
        toml::to_string(&config).map_err::<anyhow::Error, _>(|_| {
            anyhow!("error writing wallet config")
        })?;
    write!(&mut keys_file, "{config_str}")?;
    Ok(())
}

#[derive(Deserialize, Serialize)]
struct ConfigEncoding {
    mnemonic: Option<String>,
    network: Option<String>,
    birthday: Option<u32>,
}

fn encrypt_mnemonic<'a>(
    recipients: impl Iterator<Item = &'a dyn age::Recipient>,
    mnemonic: &Mnemonic,
) -> Result<String, anyhow::Error> {
    let encryptor = age::Encryptor::with_recipients(recipients)?;
    let mut ciphertext = vec![];
    let mut writer =
        encryptor.wrap_output(age::armor::ArmoredWriter::wrap_output(
            &mut ciphertext,
            age::armor::Format::AsciiArmor,
        )?)?;
    writer.write_all(mnemonic.phrase().as_bytes())?;
    writer.finish().and_then(|armor| armor.finish())?;
    Ok(String::from_utf8(ciphertext).expect("armor is valid UTF-8"))
}
