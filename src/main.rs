#[macro_use]
extern crate rocket;

use std::path::Path;

use anyhow::anyhow;
use std::str::FromStr;
use std::sync::Arc;
use std::thread::sleep;
use std::time::Duration;

use cli_batteries::version;

use ethers::prelude::*;
use ethers::utils::format_units;

use inquire::{Password, Select, Text};
use tracing::{info_span, Instrument};
use uniswap_rs::bindings::ierc20::ierc20;
use uniswap_rs::Dex;

use crate::args::Options;
use crate::args::*;
use crate::ethereum::{Ethereum, WEI_IN_ETHER};
use crate::maker::Maker;
use crate::taker::{CovertTransaction, Taker};
use crate::utils::{KeylessWallet, keypair_from_bip39, keypair_from_hex, keypair_gen, read_from_keystore, write_to_keystore};

mod args;
mod client;
mod ethereum;
mod maker;
mod server;
mod taker;
mod utils;

fn main() {
    cli_batteries::run(version!(), app);
}

async fn app(opts: Options) -> eyre::Result<()> {
    if let Some(command) = opts.command {
        match command {
            Command::Setup(args) => setup(args).await.map_err(|e| eyre::anyhow!(e))?,
            Command::Provide(args) => provide(args).await.map_err(|e| eyre::anyhow!(e))?,
            Command::Transfer(args) => transfer(args)
                .instrument(info_span!("transfer"))
                .await
                .map_err(|e| eyre::anyhow!(e))?,
            Command::Uniswap(args) => uniswap(args)
                .instrument(info_span!("uniswap"))
                .await
                .map_err(|e| eyre::anyhow!(e))?,
        }
    }

    Ok(())
}

async fn setup(args: SetupArgs) -> anyhow::Result<()> {
    let options = vec![
        "Generate new",
        "Recover from hex",
        "Recover from BIP39 mnemonic",
    ];
    let picked = Select::new("Wallet source?", options.clone())
        .prompt()
        .unwrap();
    let sk = match options
        .iter()
        .position(|e| *e == picked)
        .expect("unexpected option")
    {
        0 => keypair_gen().0,
        1 => keypair_from_hex(&Text::new("Paste hex here:").prompt().unwrap())?.0,
        2 => keypair_from_bip39(&Text::new("Mnemonic phrase:").prompt().unwrap())?.0,
        _ => panic!("unexpected option"),
    };

    let name = Text::new("Wallet name:").prompt().unwrap();
    let password = Password::new("Password:").prompt().unwrap();

    write_to_keystore(sk, args.keystore_dir, name, password)
}

async fn provide(args: ProvideArgs) -> anyhow::Result<()> {
    let name = args
        .wallet_name
        .unwrap_or_else(|| Text::new("Wallet name:").prompt().unwrap());
    let password = args
        .password
        .unwrap_or_else(|| Password::new("Password:").prompt().unwrap());
    let keystore = Path::new(&args.keystore_dir).join(name);
    let wallet = read_from_keystore(keystore, password)?;
    info!("Bob's address: {}", wallet.address());

    let eth_provider = Ethereum::new(&args.network).await?;

    let target_address = Address::from_str(&args.secondary_address)
        .map_err(|e| anyhow!("error parsing target address: {e}"))?;
    let (alice, to_alice) = Maker::new(
        eth_provider.clone(),
        wallet,
        target_address,
        args.time_lock_param,
    )
    .unwrap();

    tokio::spawn(async {
        alice.run().await;
    });

    server::serve(to_alice, args.server_address).await;

    Ok(())
}

async fn transfer(args: TransferArgs) -> anyhow::Result<()> {
    let name = args
        .wallet_name
        .unwrap_or_else(|| Text::new("Wallet name:").prompt().unwrap());
    let password = args
        .password
        .unwrap_or_else(|| Password::new("Password:").prompt().unwrap());
    let keystore = Path::new(&args.keystore_dir).join(name);
    let wallet = read_from_keystore(keystore, password)?;
    info!("Alice's address: {}", wallet.address());

    let eth_provider = Ethereum::new(&args.network).await?;

    let client = client::Client::new(args.relay_address)?;

    let target_address = Address::from_str(&args.target_address)
        .map_err(|e| anyhow!("error parsing target address: {e}"))?;
    let mut alice = Taker::new(
        eth_provider.clone(),
        wallet,
        args.amount,
        args.time_lock_param,
    );
    let setup_msg = info_span!("taker::setup1").in_scope(|| alice.setup1())?;

    let bob_setup_msg = client.setup(args.amount, setup_msg).await?;

    info!("setup complete: key share generated, time-locked commitments exchanged.");

    let lock_msg1 = alice
        .setup2(bob_setup_msg)
        .instrument(info_span!("taker::setup2"))
        .await?;

    let bob_lock_msg = client.lock(lock_msg1).await?;

    let lock_msg2 = alice
        .lock(bob_lock_msg, CovertTransaction::Swap(args.amount, target_address))
        .instrument(info_span!("taker::lock"))
        .await?;

    info!("lock complete: pre-signatures generated.");

    let bob_swap_msg = client.swap(lock_msg2).await?;
    let _ = alice
        .complete(bob_swap_msg)
        .instrument(info_span!("taker::swap"))
        .await?;

    info!("transfer complete!");

    let target_address = Address::from_str(&args.target_address)
        .map_err(|e| anyhow!("error parsing target address: {e}"))?;

    loop {
        let wei = eth_provider
            .provider
            .get_balance(target_address, None)
            .await
            .unwrap();
        let eth = wei.as_u64() as f64 / WEI_IN_ETHER.as_u64() as f64;
        info!("balance of {} is {} ETH", args.target_address, eth);
        sleep(Duration::from_secs(1));
        if eth == args.amount {
            break;
        }
    }

    Ok(())
}

abigen!(
    IErc20,
    r#"[
            function balanceOf(address account) external view returns (uint256)
    ]"#,
);

async fn uniswap(args: UniswapArgs) -> anyhow::Result<()> {
    let name = args
        .base_opts
        .wallet_name
        .unwrap_or_else(|| Text::new("Wallet name:").prompt().unwrap());
    let password = args
        .base_opts
        .password
        .unwrap_or_else(|| Password::new("Password:").prompt().unwrap());
    let keystore = Path::new(&args.base_opts.keystore_dir).join(name);
    let wallet = read_from_keystore(keystore, password)?;
    info!("Alice's address: {}", wallet.address());

    let eth_provider = Ethereum::new(&args.base_opts.network).await?;

    let client = client::Client::new(args.base_opts.relay_address)?;

    let target_address = Address::from_str(&args.base_opts.target_address)
        .map_err(|e| anyhow!("error parsing target address: {e}"))?;
    let mut alice = Taker::new(
        eth_provider.clone(),
        wallet.clone(),
        args.base_opts.amount,
        args.base_opts.time_lock_param,
    );
    let setup_msg = info_span!("taker::setup1").in_scope(|| alice.setup1())?;

    let bob_setup_msg = client.setup(args.base_opts.amount, setup_msg).await?;

    info!("setup complete: key share generated, time-locked commitments exchanged.");

    let lock_msg1 = alice
        .setup2(bob_setup_msg)
        .instrument(info_span!("taker::setup2"))
        .await?;

    info!("CoinSwap Address 2: {}", alice.s2_address());

    let bob_lock_msg = client.lock(lock_msg1).await?;

    let mut dex = {
        let client = Arc::new({
            SignerMiddleware::new(eth_provider.provider.clone(), KeylessWallet::new(alice.s2_address(), eth_provider.chain_id()))
        });

        Dex::new_with_chain(client, Chain::Goerli, uniswap_rs::ProtocolType::UniswapV2)
    };

    // get contract addresses from address book
    let usdc = uniswap_rs::contracts::address(&args.target_erc20, Chain::Goerli);
    // swap amount
    let raw_amount = U256::exp10(3);
    let amount = uniswap_rs::Amount::ExactIn(raw_amount);

    // construct swap path
    // specify native ETH by using NATIVE_ADDRESS or Address::repeat_byte(0xee)
    let eth = uniswap_rs::constants::NATIVE_ADDRESS;
    let path = [eth, usdc];

    // create the swap transaction
    let mut swap_call = dex.swap(amount, 0.5, &path, Some(target_address), None).await?;
    let eip1559_tx = swap_call.tx.as_eip1559_mut().unwrap();
    let _= eip1559_tx.max_fee_per_gas.insert(8.into());

    let lock_msg2 = alice
        .lock(bob_lock_msg, CovertTransaction::CustomTx(swap_call.tx))
        .instrument(info_span!("taker::lock"))
        .await?;

    info!("lock complete: pre-signatures generated.");

    let bob_swap_msg = client.swap(lock_msg2).await?;
    let _ = alice
        .complete(bob_swap_msg)
        .instrument(info_span!("taker::swap"))
        .await.unwrap();

    info!("swap complete!");

    let target_address = Address::from_str(&args.base_opts.target_address)
        .map_err(|e| anyhow!("error parsing target address: {e}"))?;

    let erc20 = IErc20::new(usdc, Arc::new(eth_provider.provider));

    loop {
        let balance = erc20.balance_of(target_address).call().await.unwrap();
        let tokens: f64 = format_units(balance, 0)?.parse()?;
        info!("balance of {} is {} {}", args.base_opts.target_address, tokens, args.target_erc20);
        sleep(Duration::from_secs(1));
        if tokens != 0.0 {
            break;
        }
    }

    Ok(())
}
