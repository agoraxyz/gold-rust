//! Smart contract built for the [Solana](https://solana.com/) blockchain that
//! enables users to create their own fundraiser.
mod entrypoint;
mod error;
#[cfg(feature = "client")]
pub mod frontend;
/// Smart contract instructions that can be called externally.
pub mod instruction;
/// Program Derived Addresses that can be modified only by this contract.
pub mod pda;
/// Processor that processes the incoming instructions, thus modifying data on
/// the blockchain.
pub mod processor;
/// Data structures that describe the application's current state.
pub mod state;
// Utilities for the instruction processing
pub mod utils;

pub use error::AuctionContractError;
pub use solana_program;

use metaplex_token_metadata::state::Data as MetadataStateData;

solana_program::declare_id!("go1dcKcvafq8SDwmBKo6t2NVzyhvTEZJkMwnnfae99U");

/// Maximum number of auctions the [`AuctionPool`](state::AuctionPool) may
/// hold.
const MAX_AUCTION_NUM: usize = 100;
/// Maxumum number of [`Bids`](state::BidData) in the
/// [`BidHistory`](state::BidHistory).
const MAX_BID_HISTORY_LENGTH: usize = 10;
/// Maximum number of characters allowed in an auction description.
const MAX_DESCRIPTION_LEN: usize = 200;
/// Maximum number of characters in each social url.
const MAX_SOCIALS_LEN: usize = 100;
/// Maximum number of socials the [`AuctionState`](state::AuctionRootState) may
/// hold.
const MAX_SOCIALS_NUM: usize = 5;
/// Additional bytes allocated to the
/// [`AuctionRootState`](state::AuctionRootState) account for future
/// development.
pub const EXTRA_ROOT_STATE_BYTES: usize = 32;

/// The recommended number of state accounts that can be safely wiped via a
/// `DeleteAuction` contract call without exceeding the allotted compute units.
pub const RECOMMENDED_CYCLE_STATES_DELETED_PER_CALL: u64 = 30;

pub fn unpuff_metadata(metadata_state_data: &mut MetadataStateData) {
    metadata_state_data.name.retain(|c| c != '\u{0}');
    metadata_state_data.uri.retain(|c| c != '\u{0}');
    metadata_state_data.symbol.retain(|c| c != '\u{0}');
}
