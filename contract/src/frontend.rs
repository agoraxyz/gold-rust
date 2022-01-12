use crate::state::{AuctionCycleState, AuctionRootState};
use agsol_borsh_schema::BorshSchema;
use agsol_common::{MaxLenString, MaxSerializedLen};
use borsh::{BorshDeserialize, BorshSerialize};
use metaplex_token_metadata::state::{MAX_NAME_LENGTH, MAX_SYMBOL_LENGTH, MAX_URI_LENGTH};
use solana_program::pubkey::Pubkey;

#[derive(BorshSchema, MaxSerializedLen, BorshSerialize, BorshDeserialize, Clone, Debug)]
pub enum FrontendTokenConfig {
    Nft {
        #[alias(String)]
        name: MaxLenString<MAX_NAME_LENGTH>,
        #[alias(String)]
        symbol: MaxLenString<MAX_SYMBOL_LENGTH>,
        #[alias(String)]
        uri: MaxLenString<MAX_URI_LENGTH>,
        is_repeating: bool,
    },
    Token {
        mint: Pubkey,
        decimals: u8,
        per_cycle_amount: u64,
    },
}

#[derive(BorshSchema, MaxSerializedLen, BorshSerialize, BorshDeserialize, Clone, Debug)]
pub struct FrontendAuction {
    pub root_state: AuctionRootState,
    pub cycle_state: AuctionCycleState,
    pub token_config: FrontendTokenConfig,
}
