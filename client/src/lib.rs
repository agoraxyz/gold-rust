// This is necessary because clippy throws 'unneeded unit expression' error
// on the wasm_bindgen expressions
#![allow(clippy::unused_unit)]

mod auction_exists;
mod get_auction;
mod types;
mod utils;

use agsol_gold_contract::instruction::factory::*;
use agsol_gold_contract::solana_program::pubkey::Pubkey;
use agsol_gold_contract::utils::pad_to_32_bytes;
use agsol_wasm_client::rpc_config::{CommitmentLevel, Encoding, RpcConfig};
use agsol_wasm_client::{Net, RpcClient};
use wasm_bindgen::prelude::*;

const LAMPORTS: f32 = 1e9;
#[cfg(not(feature = "mainnet"))]
const NET: Net = Net::Devnet;
#[cfg(feature = "mainnet")]
const NET: Net = Net::Mainnet;

const RPC_CONFIG: RpcConfig = RpcConfig {
    encoding: Some(Encoding::JsonParsed),
    commitment: Some(CommitmentLevel::Processed),
};

#[cfg(test)]
const TEST_AUCTION_ID: &str = "teletubbies";

#[wasm_bindgen(js_name = "getAuctionWasm")]
pub async fn get_auction_wasm(auction_id: String) -> Result<JsValue, JsValue> {
    let id = pad_to_32_bytes(&auction_id).map_err(|e| JsValue::from(e.to_string()))?;
    let mut client = RpcClient::new_with_config(NET, RPC_CONFIG);
    let auction = get_auction::get_auction(&mut client, &id)
        .await
        .map_err(|e| JsValue::from(e.to_string()))?;

    JsValue::from_serde(&auction).map_err(|e| JsValue::from(e.to_string()))
}

#[wasm_bindgen(js_name = "getAuctionsWasm")]
pub async fn get_auctions_wasm(secondary: bool) -> Result<JsValue, JsValue> {
    let mut client = RpcClient::new_with_config(NET, RPC_CONFIG);
    let auctions = get_auction::get_auctions(&mut client, secondary)
        .await
        .map_err(|e| JsValue::from(e.to_string()))?;

    JsValue::from_serde(&auctions).map_err(|e| JsValue::from(e.to_string()))
}

#[wasm_bindgen(js_name = "getAuctionCycleWasm")]
pub async fn get_auction_cycle_wasm(
    root_state_pubkey: Pubkey,
    cycle_num: u64,
) -> Result<JsValue, JsValue> {
    let mut client = RpcClient::new_with_config(NET, RPC_CONFIG);
    let auction_cycle =
        get_auction::get_auction_cycle_state(&mut client, &root_state_pubkey, cycle_num)
            .await
            .map_err(|e| JsValue::from(e.to_string()))?;

    JsValue::from_serde(&auction_cycle).map_err(|e| JsValue::from(e.to_string()))
}

#[wasm_bindgen(js_name = "auctionExistsWasm")]
pub async fn auction_exists_wasm(auction_id: String) -> Result<bool, JsValue> {
    let mut client = RpcClient::new_with_config(NET, RPC_CONFIG);
    let auction_id = pad_to_32_bytes(&auction_id).map_err(|e| JsValue::from(e.to_string()))?;
    auction_exists::auction_exists(&mut client, &auction_id)
        .await
        .map_err(|e| JsValue::from(e.to_string()))
}

#[wasm_bindgen(js_name = "claimFundsWasm")]
pub async fn claim_funds_wasm(
    auction_id: String,
    payer_pubkey: Pubkey,
    auction_owner_pubkey: Pubkey,
    cycle_number: u64,
    amount: f32,
) -> Result<JsValue, JsValue> {
    let args = ClaimFundsArgs {
        payer_pubkey,
        auction_owner_pubkey,
        auction_id: pad_to_32_bytes(&auction_id)?,
        cycle_number,
        amount: (amount * LAMPORTS) as u64,
    };
    let instruction = claim_funds(&args);
    JsValue::from_serde(&instruction).map_err(|e| JsValue::from(e.to_string()))
}

#[wasm_bindgen(js_name = "initializeAuctionWasm")]
pub async fn initialize_auction_wasm(config: JsValue) -> Result<Instruction, JsValue> {
    let config: FrontendAuctionConfig = config
        .into_serde()
        .map_err(|e| JsValue::from(e.to_string()))?;

    let args = config.into_initialize_auction_args();
    let instruction = initialize_auction(&args);
    JsValue::from_serde(&instruction).map_err(|e| JsValue::from(e.to_string()))
}
// TODO
//wasm_instruction!(claim_rewards);
//wasm_instruction!(delete_auction);
//wasm_instruction!(modify_auction);
//wasm_instruction!(place_bid);
