use agsol_gold_admin_panel::{
    parse_keypair, request_airdrop, ThawAuctionOpt, MIN_BALANCE, TEST_ADMIN_SECRET,
};

use agsol_gold_client::pad_to_32_bytes;

use agsol_gold_contract::instruction::factory::{thaw_auction, ThawAuctionArgs};
use agsol_gold_contract::pda::auction_root_state_seeds;
use agsol_gold_contract::state::AuctionRootState;
use agsol_gold_contract::ID as GOLD_ID;

use log::{error, info, warn};
use solana_client::rpc_client::RpcClient;
use solana_sdk::borsh::try_from_slice_unchecked;
use solana_sdk::commitment_config::CommitmentConfig;
use solana_sdk::pubkey::Pubkey;
use solana_sdk::signer::keypair::Keypair;
use solana_sdk::signer::Signer;
use solana_sdk::transaction::Transaction;
use structopt::StructOpt;

use anyhow::anyhow;

pub fn main() {
    env_logger::init();
    let opt = ThawAuctionOpt::from_args();

    let (connection_url, should_airdrop) = if opt.mainnet {
        ("https://api.mainnet-beta.solana.com".to_owned(), false)
    } else if opt.devnet {
        ("https://api.devnet.solana.com".to_owned(), true)
    } else if opt.localnet {
        ("http://localhost:8899".to_owned(), true)
    } else {
        ("https://api.testnet.solana.com".to_owned(), true)
    };

    let connection = RpcClient::new_with_commitment(connection_url, CommitmentConfig::confirmed());

    let admin_keypair = parse_keypair(opt.keypair, &TEST_ADMIN_SECRET);

    if let Err(e) = try_main(&connection, &admin_keypair, should_airdrop, opt.auction_id) {
        error!("{}", e);
    }
}

fn try_main(
    connection: &RpcClient,
    admin_keypair: &Keypair,
    should_airdrop: bool,
    auction_id: String,
) -> Result<(), anyhow::Error> {
    // AIRDROP IF NECESSARY
    let admin_balance = connection.get_balance(&admin_keypair.pubkey())?;
    if admin_balance < MIN_BALANCE {
        warn!(
            "admin balance ({}) is below threshold ({})",
            admin_balance, MIN_BALANCE
        );
        if should_airdrop {
            request_airdrop(connection, admin_keypair)?;
        }
    }

    let id_bytes = pad_to_32_bytes(&auction_id)?;

    if let Err(err) = check_auction_state(connection, &id_bytes) {
        error!("error while thawing auction \"{}\": {}", auction_id, err);
    }

    let thaw_args = ThawAuctionArgs {
        contract_admin_pubkey: admin_keypair.pubkey(),
        auction_id: id_bytes,
    };

    let thaw_ix = thaw_auction(&thaw_args);

    let latest_blockhash = connection.get_latest_blockhash()?;

    let transaction = Transaction::new_signed_with_payer(
        &[thaw_ix],
        Some(&admin_keypair.pubkey()),
        &[admin_keypair],
        latest_blockhash,
    );

    let signature = connection.send_and_confirm_transaction(&transaction)?;
    info!(
        "Auction \"{}\" thawed successfully    signature: {:?}",
        auction_id, signature
    );

    Ok(())
}

fn check_auction_state(connection: &RpcClient, id_bytes: &[u8]) -> Result<(), anyhow::Error> {
    let (state_pubkey, _) =
        Pubkey::find_program_address(&auction_root_state_seeds(id_bytes), &GOLD_ID);

    let auction_state_data = connection.get_account_data(&state_pubkey)?;
    let auction_state: AuctionRootState = try_from_slice_unchecked(&auction_state_data)?;

    if auction_state.status.is_finished {
        return Err(anyhow!("auction has finished"));
    }

    if !auction_state.status.is_frozen {
        return Err(anyhow!("auction is not frozen"));
    }

    Ok(())
}