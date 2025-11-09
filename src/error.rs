use std::fmt;
use zcash_client_backend::data_api::{
    error::Error as WalletError,
    wallet::input_selection::GreedyInputSelectorError, BirthdayError,
};
use zcash_client_sqlite::{
    error::SqliteClientError, wallet::commitment_tree, FsBlockDbError,
    ReceivedNoteId,
};
use zcash_keys::keys::DerivationError;
use zcash_primitives::transaction::fees::zip317;
use zip321::Zip321Error;

pub(crate) type WalletErrorT = WalletError<
    SqliteClientError,
    commitment_tree::Error,
    GreedyInputSelectorError,
    zip317::FeeError,
    zip317::FeeError,
    ReceivedNoteId,
>;

#[derive(Debug)]
pub enum Error {
    Cache(FsBlockDbError),
    Derivation(DerivationError),
    InvalidTreeState,
    Wallet(WalletErrorT),
    Zip321(Zip321Error),
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Error::Cache(e) => write!(f, "{e:?}"),
            Error::Derivation(e) => write!(f, "{e:?}"),
            Error::InvalidTreeState => {
                write!(f, "Invalid TreeState received from server")
            }
            Error::Wallet(e) => e.fmt(f),
            Error::Zip321(e) => write!(f, "{e:?}"),
        }
    }
}

impl std::error::Error for Error {}

impl From<BirthdayError> for Error {
    fn from(_: BirthdayError) -> Self {
        Error::InvalidTreeState
    }
}

impl From<DerivationError> for Error {
    fn from(e: DerivationError) -> Self {
        Error::Derivation(e)
    }
}

impl From<FsBlockDbError> for Error {
    fn from(e: FsBlockDbError) -> Self {
        Error::Cache(e)
    }
}

impl From<WalletErrorT> for Error {
    fn from(e: WalletErrorT) -> Self {
        Error::Wallet(e)
    }
}

impl From<Zip321Error> for Error {
    fn from(e: Zip321Error) -> Self {
        Error::Zip321(e)
    }
}
