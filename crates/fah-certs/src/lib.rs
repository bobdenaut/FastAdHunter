mod api;
mod ca;
mod error;
mod import;
mod leaf;
mod store;

pub use api::load_or_generate;
pub use ca::{CaParams, CaSummary, DEFAULT_CA_COMMON_NAME, DEFAULT_CA_VALIDITY_DAYS};
pub use error::CertError;
pub use import::{validate_ca_pair, validate_server_pair, ValidatedCaPair, ValidatedServerPair};
pub use leaf::{LeafCacheStats, MintingResolver, LEAF_CACHE_CAPACITY, LEAF_VALIDITY_DAYS};
pub use store::{ApiPairSource, CertStatus, CertStore};

pub fn install_crypto_provider() {
    let _ = rustls::crypto::aws_lc_rs::default_provider().install_default();
}
