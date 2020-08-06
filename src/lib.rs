#![warn(
    unused_extern_crates,
    missing_debug_implementations,
    missing_copy_implementations,
    rust_2018_idioms,
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    clippy::fallible_impl_from,
    clippy::cast_precision_loss,
    clippy::cast_possible_wrap,
    clippy::dbg_macro
)]
#![cfg_attr(not(test), warn(clippy::unwrap_used))]
#![forbid(unsafe_code)]

//! # bitcoin-harness
//! A simple lib to start a bitcoind container, generate blocks and funds addresses.
//! Note: It uses tokio.
//!
//! # Examples
//! ```rust
//! #[tokio::main]
//! async fn main() {
//!     let tc_client = testcontainers::clients::Cli::default();
//!     let bitcoind = bitcoin_harness::Bitcoind::new(&tc_client, "0.20.0").unwrap();
//!     let client = bitcoin_harness::bitcoind_rpc::Client::new(bitcoind.node_url);
//!     let network = client.network().await.unwrap();
//!
//!     assert_eq!(network, bitcoin::Network::Regtest)
//! }
//! ```
//!

pub mod bitcoind_rpc;
pub mod json_rpc;

use crate::bitcoind_rpc::Client;
use reqwest::Url;
use std::time::Duration;
use testcontainers::{clients, images::coblox_bitcoincore::BitcoinCore, Container, Docker};

pub type Result<T> = std::result::Result<T, Error>;

const BITCOIND_RPC_PORT: u16 = 18443;

#[derive(Debug)]
pub struct Bitcoind<'c> {
    pub container: Container<'c, clients::Cli, BitcoinCore>,
    pub node_url: Url,
    pub wallet_name: String,
}

impl<'c> Bitcoind<'c> {
    /// Starts a new regtest bitcoind container
    pub fn new(client: &'c clients::Cli, tag: &str) -> Result<Self> {
        let container = client.run(BitcoinCore::default().with_tag(tag));
        let port = container
            .get_host_port(BITCOIND_RPC_PORT)
            .ok_or(Error::PortNotExposed(BITCOIND_RPC_PORT))?;

        let auth = container.image().auth();
        let url = format!(
            "http://{}:{}@localhost:{}",
            &auth.username, &auth.password, port
        );
        let url = Url::parse(&url)?;

        let wallet_name = String::from("testwallet");

        Ok(Self {
            container,
            node_url: url,
            wallet_name,
        })
    }

    /// Create a test wallet, generate enough block to fund it and activate segwit.
    /// Generate enough blocks to make the passed `spendable_quantity` spendable.
    /// Spawn a tokio thread to mine a new block every second.
    pub async fn init(&self, spendable_quantity: u32) -> Result<()> {
        let bitcoind_client = Client::new(self.node_url.clone());

        bitcoind_client
            .create_wallet(&self.wallet_name, None, None, None, None)
            .await?;

        let reward_address = bitcoind_client
            .get_new_address(&self.wallet_name, None, None)
            .await?;

        bitcoind_client
            .generate_to_address(101 + spendable_quantity, reward_address.clone(), None)
            .await?;
        let _ = tokio::spawn(mine(bitcoind_client, reward_address));

        Ok(())
    }

    /// Send Bitcoin to the specified address, limited to the spendable bitcoin quantity.
    pub async fn mint(&self, address: bitcoin::Address, amount: bitcoin::Amount) -> Result<()> {
        let bitcoind_client = Client::new(self.node_url.clone());

        bitcoind_client
            .send_to_address(&self.wallet_name, address.clone(), amount)
            .await?;

        Ok(())
    }

    pub fn container_id(&self) -> &str {
        self.container.id()
    }
}

async fn mine(bitcoind_client: Client, reward_address: bitcoin::Address) -> Result<()> {
    loop {
        tokio::time::delay_for(Duration::from_secs(1)).await;
        bitcoind_client
            .generate_to_address(1, reward_address.clone(), None)
            .await?;
    }
}

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("Bitcoin Rpc: ")]
    BitcoindRpc(#[from] bitcoind_rpc::Error),
    #[error("Url Parsing: ")]
    UrlParseError(#[from] url::ParseError),
    #[error("Docker port not exposed: ")]
    PortNotExposed(u16),
}