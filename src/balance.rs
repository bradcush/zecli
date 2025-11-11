use anyhow::anyhow;
use clap::Args;
use colored::Colorize;
use iso_currency::Currency;
use rust_decimal::{prelude::FromPrimitive, Decimal};
use textwrap::{fill, Options};
use tracing::{info, warn};
use uuid::Uuid;
use zcash_client_backend::{
    data_api::{wallet::ConfirmationsPolicy, Account as _, WalletRead},
    tor,
};
use zcash_client_sqlite::AccountUuid;
use zcash_client_sqlite::WalletDb;
use zcash_keys::keys::UnifiedAddressRequest;
use zcash_protocol::value::{Zatoshis, COIN};

use crate::{
    // Internal dependencies, some of
    // which might not need to be shared
    config::get_wallet_network,
    data::get_db_paths,
    error,
    remote::tor_client,
    ui::{format_zec, TEXT_WIDTH},
};

pub(crate) fn select_account<DbT: WalletRead<AccountId = AccountUuid>>(
    db_data: &DbT,
    account_uuid: Option<Uuid>,
) -> Result<DbT::Account, anyhow::Error>
where
    DbT::Error: std::error::Error + Sync + Send + 'static,
{
    let account_id = match account_uuid {
        Some(uuid) => Ok(AccountUuid::from_uuid(uuid)),
        None => {
            let account_ids = db_data.get_account_ids()?;
            match &account_ids[..] {
                [] => Err(anyhow!("Wallet contains no accounts.")),
                [account_id] => Ok(*account_id),
                _ => Err(anyhow!(
                    "More than one account is available; please specify the account UUID."
                )),
            }
        }
    }?;
    db_data
        .get_account(account_id)?
        .ok_or(anyhow!("Account missing: {:?}", account_id))
}

fn parse_currency(data: &str) -> Result<Currency, String> {
    Currency::from_code(data)
        .ok_or_else(|| format!("Invalid currency '{data}'"))
}

// Options accepted for the `balance` command
#[derive(Debug, Args)]
pub(crate) struct BalanceOptions {
    /// The UUID of the account if multiple exist
    account_id: Option<Uuid>,

    /// Convert ZEC values into some currency
    #[arg(long)]
    #[arg(value_parser = parse_currency)]
    convert: Option<Currency>,
}

impl BalanceOptions {
    pub(crate) async fn run(
        self,
        wallet_dir: Option<String>,
    ) -> Result<(), anyhow::Error> {
        let params = get_wallet_network(wallet_dir.as_ref())?;
        let (_, db_data) = get_db_paths(wallet_dir.as_ref());
        let db_data = WalletDb::for_path(db_data, params, (), ())?;
        let account = select_account(&db_data, self.account_id)?;
        let address = db_data
            .get_last_generated_address_matching(
                account.id(),
                UnifiedAddressRequest::AllAvailableKeys,
            )?
            .ok_or(error::Error::InvalidRecipient)?;
        // Retrieve the exchange rate if we need to
        let printer = if let Some(currency) = self.convert {
            let tor = tor_client(wallet_dir.as_ref()).await?;
            ValuePrinter::with_exchange_rate(&tor, currency).await?
        } else {
            ValuePrinter::ZecOnly
        };
        if let Some(wallet_summary) =
            db_data.get_wallet_summary(ConfirmationsPolicy::default())?
        {
            let balance = wallet_summary
                .account_balances()
                .get(&account.id())
                .ok_or_else(|| anyhow!("Missing account 0"))?;
            let address = format!("Address: {}", address.encode(&params));
            // Replace wtih a non-breaking space otherwise
            // the first line is broken after the colon
            let address = address.replace(" ", "\u{a0}");
            let address_options = Options::new(TEXT_WIDTH);
            println!("{}\n", fill(&address[..], &address_options));
            // Rest of the details for the wallet address
            let chain_height = wallet_summary.chain_tip_height();
            let height = format!("Height: {}", chain_height);
            let detail_options = Options::new(TEXT_WIDTH)
                .initial_indent("    ")
                .subsequent_indent("    ");
            println!("{}", fill(&height[..], &detail_options));
            let scan_progress = wallet_summary.progress().scan();
            let synced_percent = (*scan_progress.numerator() as f64) * 100f64
                / (*scan_progress.denominator() as f64);
            let synced = format!("Synced: {:0.3}%", synced_percent);
            println!("{}", fill(&synced[..], &detail_options));
            // We might not have progress to compute recovered
            if let Some(progress) = wallet_summary.progress().recovery() {
                let recovered_percent = (*progress.numerator() as f64) * 100f64
                    / (*progress.denominator() as f64);
                // Seems like we shouldn't show this value computed as
                // NaN%. See why we compute Nan% = 0/0, if this is what
                // we expect, and if this can/should be hidden.
                let recovered_frac = format!(
                    "{}/{}",
                    *progress.numerator(),
                    *progress.denominator()
                );
                let recovered = format!(
                    "Recovered: {:0.3}% = {}",
                    recovered_percent, recovered_frac
                );
                println!("{}", fill(&recovered[..], &detail_options))
            }
            let balance_total = printer.format(balance.total());
            let balance_total = format!("Balance: {}", balance_total);
            println!("{}", fill(&balance_total[..], &detail_options).green());
            let sapling_spendable_value =
                balance.sapling_balance().spendable_value();
            let sapling = printer.format(sapling_spendable_value);
            let sapling = format!("Sapling Spendable: {}", sapling);
            println!("{}", fill(&sapling[..], &detail_options));
            let orchard_spendable_value =
                balance.orchard_balance().spendable_value();
            let orchard = printer.format(orchard_spendable_value);
            let orchard = format!("Orchard Spendable: {}", orchard);
            println!("{}", fill(&orchard[..], &detail_options));
            // Better understand use of cfg w/ features
            #[cfg(feature = "transparent-inputs")]
            let unshielded_spendable_value =
                balance.unshielded_balance().spendable_value();
            let unshielded = printer.format(unshielded_spendable_value);
            let unshielded = format!("Unshielded Spendable: {}", unshielded);
            println!("{}", fill(&unshielded[..], &detail_options));
        } else {
            // In the case that we haven't sycned the wallet
            println!("Insufficient information to build a wallet summary.");
        }
        Ok(())
    }
}

enum ValuePrinter {
    WithConversion { currency: Currency, rate: Decimal },
    ZecOnly,
}

impl ValuePrinter {
    async fn with_exchange_rate(
        tor: &tor::Client,
        currency: Currency,
    ) -> anyhow::Result<Self> {
        info!("Fetching {:?}/ZEC exchange rate", currency);
        let exchanges = tor::http::cryptex::Exchanges::unauthenticated_known_with_gemini_trusted();
        let usd_zec = tor.get_latest_zec_to_usd_rate(&exchanges).await?;
        if currency == Currency::USD {
            let rate = usd_zec;
            info!("Current {:?}/ZEC exchange rate: {}", currency, rate);
            Ok(Self::WithConversion { currency, rate })
        } else {
            warn!("{:?}/ZEC exchange rate is unsupported", currency);
            Ok(Self::ZecOnly)
        }
    }

    fn format(&self, value: Zatoshis) -> String {
        match self {
            ValuePrinter::WithConversion { currency, rate } => {
                format!(
                    "{} ({}{:.2})",
                    format_zec(value),
                    currency.symbol(),
                    rate * Decimal::from_u64(value.into_u64()).unwrap()
                        / Decimal::from_u64(COIN).unwrap(),
                )
            }
            ValuePrinter::ZecOnly => format_zec(value),
        }
    }
}
