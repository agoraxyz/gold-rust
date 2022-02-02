use super::*;

use metaplex_token_metadata::state::Data as MetadataStateData;
use metaplex_token_metadata::state::MasterEditionV2;
use solana_program::clock::UnixTimestamp;
use solana_program::sysvar::rent::Rent;

use std::str::FromStr;

const METADATA_DATA_START_POS: usize = 65;

// NOTE: The user can be made to pay for this account's creation by locking its fee besides their bid at the time of bidding
//   and using this locked fee now.
// NOTE: With the current calculation method we may scam the auction owner with at most 19 lamports due to rounding.
//   This may be improved.

// NOTE: We might introduce a "grace period" in which the user can not bid before initiating a new auction
//   in case they wanted to bid in the last second, so that they do not bid on the next auctioned asset accidentally
/// Closes auction cycle
///
/// Creates holding account for the won asset for the user with the highest bid.
/// The cost of this account's creation is deducted from the highest bid.
///
/// Then, distributes the deducted highest bid in the following fashion:
///
/// - 95% to the auction owner
///
/// - 5% to the contract admin
pub fn close_auction_cycle(
    program_id: &Pubkey,
    accounts: &[AccountInfo],
    auction_id: AuctionId,
) -> ProgramResult {
    let account_info_iter = &mut accounts.iter();
    // misc
    let payer_account = next_account_info(account_info_iter)?;

    // contract state accounts
    let auction_bank_account = next_account_info(account_info_iter)?;
    let auction_owner_account = next_account_info(account_info_iter)?;
    let auction_root_state_account = next_account_info(account_info_iter)?;
    let current_auction_cycle_state_account = next_account_info(account_info_iter)?;
    let next_auction_cycle_state_account = next_account_info(account_info_iter)?;

    // user accounts
    let top_bidder_account = next_account_info(account_info_iter)?;

    // contract signer pda
    let contract_pda = next_account_info(account_info_iter)?;

    // external programs
    let rent_program = next_account_info(account_info_iter)?;
    let system_program = next_account_info(account_info_iter)?;
    let token_program = next_account_info(account_info_iter)?;

    if !payer_account.is_signer {
        msg!("admin signature is missing");
        return Err(ProgramError::MissingRequiredSignature);
    }

    // Check cross-program invocation addresses
    assert_rent_program(rent_program.key)?;
    assert_system_program(system_program.key)?;
    assert_token_program(token_program.key)?;

    // Check account ownership
    // User accounts:
    //   payer_account
    //   top_bidder_account
    // Pda accounts:
    //   contract_pda
    // Accounts created in this instruction:
    //   next_auction_cycle_state_account

    // check root and cycle states
    SignerPda::check_owner(
        &auction_root_state_seeds(&auction_id),
        program_id,
        program_id,
        auction_root_state_account,
    )?;

    let mut auction_root_state = AuctionRootState::read(auction_root_state_account)?;

    let cycle_num_bytes = auction_root_state
        .status
        .current_auction_cycle
        .to_le_bytes();

    SignerPda::check_owner(
        &auction_cycle_state_seeds(auction_root_state_account.key, &cycle_num_bytes),
        program_id,
        program_id,
        current_auction_cycle_state_account,
    )?;

    let mut current_auction_cycle_state =
        AuctionCycleState::read(current_auction_cycle_state_account)?;

    if auction_owner_account.key != &auction_root_state.auction_owner {
        return Err(AuctionContractError::AuctionOwnerMismatch.into());
    }

    // Check auction status (freeze, active, able to end cycle)
    let clock = Clock::get()?;
    let current_timestamp = clock.unix_timestamp;
    check_status(
        &auction_root_state,
        &current_auction_cycle_state,
        current_timestamp,
        AuctionInteraction::CloseCycle,
    )?;
    // check auction_bank
    SignerPda::check_owner(
        &auction_bank_seeds(&auction_id),
        program_id,
        program_id,
        auction_bank_account,
    )?;

    // If there were no bids, just reset auction cycle
    let most_recent_bid_option = current_auction_cycle_state.bid_history.get_last_element();
    if let Some(most_recent_bid) = most_recent_bid_option {
        if top_bidder_account.key != &most_recent_bid.bidder_pubkey {
            return Err(AuctionContractError::TopBidderAccountMismatch.into());
        }
        auction_root_state.available_funds = auction_root_state
            .available_funds
            .checked_add(most_recent_bid.bid_amount)
            .ok_or(AuctionContractError::ArithmeticError)?;
    } else {
        increment_idle_streak(
            &mut current_auction_cycle_state,
            &mut auction_root_state,
            current_auction_cycle_state_account,
            auction_root_state_account,
        )?;
        return Ok(());
    }

    let next_cycle_num_bytes = (auction_root_state
        .status
        .current_auction_cycle
        .checked_add(1)
        .ok_or(AuctionContractError::ArithmeticError)?)
    .to_le_bytes();
    let next_auction_cycle_state_seeds =
        auction_cycle_state_seeds(auction_root_state_account.key, &next_cycle_num_bytes);
    let next_cycle_state_pda = SignerPda::new_checked(
        &next_auction_cycle_state_seeds,
        program_id,
        next_auction_cycle_state_account,
    )?;

    let contract_pda_seeds = contract_pda_seeds();
    let contract_signer_pda =
        SignerPda::new_checked(&contract_pda_seeds, program_id, contract_pda)?;

    match auction_root_state.token_config {
        TokenConfig::Nft(ref nft_data) => {
            let metadata_program = next_account_info(account_info_iter)?;
            // nft child accounts
            let child_edition_account = next_account_info(account_info_iter)?;
            let child_edition_marker_account = next_account_info(account_info_iter)?;
            let child_metadata_account = next_account_info(account_info_iter)?;
            let child_mint_account = next_account_info(account_info_iter)?;
            let child_holding_account = next_account_info(account_info_iter)?;
            // master accounts
            let master_edition_account = next_account_info(account_info_iter)?;
            let master_metadata_account = next_account_info(account_info_iter)?;
            let master_mint_account = next_account_info(account_info_iter)?;
            let master_holding_account = next_account_info(account_info_iter)?;

            // Check account ownership
            // Accounts created in this instruction:
            //   child_edition_account
            //   child_metadata_account
            //   child_mint_account
            //   child_holding_account

            if !child_edition_marker_account.data_is_empty()
                && *child_edition_marker_account.owner != META_ID
            {
                return Err(AuctionContractError::InvalidAccountOwner.into());
            }
            if *master_edition_account.owner != META_ID {
                return Err(AuctionContractError::InvalidAccountOwner.into());
            }
            assert_token_account_owner(master_holding_account, contract_pda.key)?;
            assert_mint_authority(master_mint_account, master_edition_account.key)?;

            // Check cross-program invocation addresses
            assert_metaplex_program(metadata_program.key)?;

            // Check pda addresses
            // Not checking the following pdas since these are checked (and owned) by metaplex
            // child_edition_account
            // child_metadata_account
            // child_edition_marker_account
            // master_edition_account
            // master_metadata_account
            let next_edition = auction_root_state.status.current_auction_cycle;
            let next_edition_bytes = next_edition.to_le_bytes();

            let child_mint_seeds = child_mint_seeds(&next_edition_bytes, &auction_id);
            let child_mint_pda =
                SignerPda::new_checked(&child_mint_seeds, program_id, child_mint_account)?;

            let child_holding_seeds = child_holding_seeds(&next_edition_bytes, &auction_id);
            let child_holding_pda =
                SignerPda::new_checked(&child_holding_seeds, program_id, child_holding_account)?;

            SignerPda::check_owner(
                &master_mint_seeds(&auction_id),
                program_id,
                &TOKEN_ID,
                master_mint_account,
            )?;

            SignerPda::check_owner(
                &master_holding_seeds(&auction_id),
                program_id,
                &TOKEN_ID,
                master_holding_account,
            )?;

            SignerPda::check_owner(
                &metadata_seeds(master_mint_account.key),
                &META_ID,
                &META_ID,
                master_metadata_account,
            )?;

            // check nft validity
            if &nft_data.master_edition != master_edition_account.key {
                return Err(AuctionContractError::MasterEditionMismatch.into());
            } else {
                let master_edition_data: MasterEditionV2 =
                    try_from_slice_unchecked(&master_edition_account.data.borrow()[..])?;

                let current_edition = next_edition
                    .checked_sub(1)
                    .ok_or(AuctionContractError::ArithmeticError)?;
                if master_edition_data.supply != current_edition {
                    return Err(AuctionContractError::ChildEditionNumberMismatch.into());
                }
            }

            if !child_metadata_account.data_is_empty() {
                return Err(AuctionContractError::NftAlreadyExists.into());
            }

            // Mint child nft to highest bidder
            // create child nft mint account
            msg!("Mint account creation");
            create_mint_account(
                payer_account,
                child_mint_account,
                contract_pda,
                child_mint_pda.signer_seeds(),
                rent_program,
                system_program,
                token_program,
                0,
            )?;

            msg!("Holding account creation");
            // create child nft holding account
            create_token_holding_account(
                payer_account,
                top_bidder_account,
                child_holding_account,
                child_mint_account,
                child_holding_pda.signer_seeds(),
                system_program,
                token_program,
                rent_program,
            )?;

            let mint_ix = spl_token::instruction::mint_to(
                token_program.key,
                child_mint_account.key,
                child_holding_account.key,
                contract_pda.key,
                &[contract_pda.key],
                1,
            )?;

            invoke_signed(
                &mint_ix,
                &[
                    contract_pda.to_owned(),
                    token_program.to_owned(),
                    child_holding_account.to_owned(),
                    child_mint_account.to_owned(),
                ],
                &[&contract_signer_pda.signer_seeds()],
            )?;

            // turn single child token into nft
            let mint_child_ix = meta_instruction::mint_new_edition_from_master_edition_via_token(
                *metadata_program.key,
                *child_metadata_account.key,
                *child_edition_account.key,
                *master_edition_account.key,
                *child_mint_account.key,
                *contract_pda.key,
                *payer_account.key,
                *contract_pda.key,
                *master_holding_account.key,
                *contract_pda.key,
                *master_metadata_account.key,
                *master_mint_account.key,
                next_edition,
            );

            invoke_signed(
                &mint_child_ix,
                &[
                    master_edition_account.clone(),
                    master_holding_account.clone(),
                    master_metadata_account.clone(),
                    child_edition_account.clone(),
                    child_edition_marker_account.clone(),
                    child_holding_account.clone(),
                    child_metadata_account.clone(),
                    child_mint_account.clone(),
                    payer_account.clone(),
                    contract_pda.clone(),
                    rent_program.clone(),
                    system_program.clone(),
                    token_program.clone(),
                ],
                &[&contract_signer_pda.signer_seeds()],
            )?;

            // change master metadata so that child can inherit it
            // if last cycle is being closed, set increments to 0 (#0 and 0.jpg)
            if !nft_data.is_repeating {
                let mut new_master_metadata = try_from_slice_unchecked::<MetadataStateData>(
                    &master_metadata_account.data.borrow_mut()[METADATA_DATA_START_POS..],
                )
                .unwrap();

                increment_uri(
                    &mut new_master_metadata.uri,
                    is_last_auction_cycle(&auction_root_state),
                )?;

                let change_master_metadata_ix = meta_instruction::update_metadata_accounts(
                    *metadata_program.key,
                    *master_metadata_account.key,
                    *contract_pda.key,
                    None,
                    Some(new_master_metadata),
                    None,
                );

                invoke_signed(
                    &change_master_metadata_ix,
                    &[master_metadata_account.clone(), contract_pda.clone()],
                    &[&contract_signer_pda.signer_seeds()],
                )?;
            }
        }
        TokenConfig::Token(ref token_data) => {
            // Token mint account
            let token_mint_account = next_account_info(account_info_iter)?;
            // User's token holding account
            let token_holding_account = next_account_info(account_info_iter)?;

            // Check account ownership
            // Accounts created in this instruction:
            //   token_holding_account
            assert_mint_authority(token_mint_account, contract_pda.key)?;

            // Check pda addresses
            SignerPda::check_owner(
                &token_mint_seeds(&auction_id),
                program_id,
                &TOKEN_ID,
                token_mint_account,
            )?;

            if token_mint_account.key != &token_data.mint {
                return Err(AuctionContractError::InvalidSeeds.into());
            }

            let token_holding_seeds =
                token_holding_seeds(token_mint_account.key, top_bidder_account.key);
            let token_holding_pda =
                SignerPda::new_checked(&token_holding_seeds, program_id, token_holding_account)?;

            // create token holding account (if needed)
            if token_holding_account.data_is_empty() {
                create_token_holding_account(
                    payer_account,
                    top_bidder_account,
                    token_holding_account,
                    token_mint_account,
                    token_holding_pda.signer_seeds(),
                    system_program,
                    token_program,
                    rent_program,
                )?;
            }

            // mint tokens to the highest bidder
            let mint_ix = spl_token::instruction::mint_to(
                token_program.key,
                token_mint_account.key,
                token_holding_account.key,
                contract_pda.key,
                &[contract_pda.key],
                token_data.per_cycle_amount,
            )?;

            invoke_signed(
                &mint_ix,
                &[
                    contract_pda.to_owned(),
                    token_program.to_owned(),
                    token_holding_account.to_owned(),
                    token_mint_account.to_owned(),
                ],
                &[&contract_signer_pda.signer_seeds()],
            )?;
        }
    }

    // Reset auction cycle
    if is_last_auction_cycle(&auction_root_state) {
        auction_root_state.status.is_finished = true;
        auction_root_state.available_funds = auction_root_state
            .available_funds
            .checked_add(Rent::get()?.minimum_balance(0))
            .ok_or(AuctionContractError::ArithmeticError)?;
    } else {
        create_state_account(
            payer_account,
            next_auction_cycle_state_account,
            next_cycle_state_pda.signer_seeds(),
            program_id,
            system_program,
            AuctionCycleState::MAX_SERIALIZED_LEN,
        )?;

        let end_time = clock
            .unix_timestamp
            .checked_add(auction_root_state.auction_config.cycle_period)
            .ok_or(AuctionContractError::ArithmeticError)?;

        let next_auction_cycle_state = AuctionCycleState {
            bid_history: BidHistory::new(),
            end_time,
        };

        next_auction_cycle_state.write(next_auction_cycle_state_account)?;
        auction_root_state.status.current_auction_cycle = auction_root_state
            .status
            .current_auction_cycle
            .checked_add(1)
            .ok_or(AuctionContractError::ArithmeticError)?;
    }

    auction_root_state.status.current_idle_cycle_streak = 0;
    auction_root_state.write(auction_root_state_account)?;

    Ok(())
}


fn increment_idle_streak(
    current_auction_cycle_state: &mut AuctionCycleState,
    auction_root_state: &mut AuctionRootState,
    current_auction_cycle_state_account: &AccountInfo,
    auction_root_state_account: &AccountInfo,
) -> Result<(), ProgramError> {
    current_auction_cycle_state.end_time = current_auction_cycle_state
        .end_time
        .checked_add(auction_root_state.auction_config.cycle_period)
        .ok_or(AuctionContractError::ArithmeticError)?;

    auction_root_state.status.current_idle_cycle_streak = auction_root_state
        .status
        .current_idle_cycle_streak
        .checked_add(1)
        .ok_or(AuctionContractError::ArithmeticError)?;

    // If the auction was idle for at least a week then filter it automatically
    if auction_root_state.auction_config.cycle_period
        * UnixTimestamp::from(auction_root_state.status.current_idle_cycle_streak)
        > crate::ALLOWED_AUCTION_IDLE_PERIOD
        || auction_root_state.status.current_idle_cycle_streak
            > crate::ALLOWED_CONSECUTIVE_IDLE_CYCLES
    {
        auction_root_state.status.is_filtered = true;
    }

    current_auction_cycle_state.write(current_auction_cycle_state_account)?;
    auction_root_state.write(auction_root_state_account)?;
    Ok(())
}

pub fn increment_name(
    string: &mut String,
    is_last_cycle: bool,
) -> Result<(), AuctionContractError> {
    let mut last_pos = 32;
    let mut first_pos = 32;
    let str_bytes = string.as_bytes();
    for i in (0..32).rev() {
        if str_bytes[i] == 0 {
            last_pos = i;
        }

        // "#".as_bytes() == [35]
        if str_bytes[i] == 35 {
            first_pos = i + 1;
            break;
        }
    }

    if last_pos == 0 || last_pos < first_pos || first_pos == 0 {
        return Err(AuctionContractError::MetadataManipulationError);
    }

    let integer = u64::from_str(&string[first_pos..last_pos]).unwrap();

    string.truncate(last_pos);

    if is_last_cycle {
        string.replace_range(first_pos..last_pos, &0.to_string());
    } else {
        let incremented_integer = integer
            .checked_add(1)
            .ok_or(AuctionContractError::ArithmeticError)?;
        string.replace_range(first_pos..last_pos, &(incremented_integer).to_string());
    };

    Ok(())
}

pub fn increment_uri(uri: &mut String, is_last_cycle: bool) -> Result<(), AuctionContractError> {
    let uri_len = uri.len();
    let mut last_pos = uri_len;
    let mut dot_pos = uri_len;
    let mut slash_pos = uri_len;

    let str_bytes = uri.as_bytes();
    for i in (0..uri_len).rev() {
        if str_bytes[i] == 0 {
            last_pos = i;
        }

        // ".".as_bytes() == [46]
        if str_bytes[i] == 46 {
            dot_pos = i;
        }

        // "/".as_bytes() == [47]
        if str_bytes[i] == 47 {
            slash_pos = i + 1;
            break;
        }
    }

    if last_pos == 0 || dot_pos == 0 || slash_pos == 0 || dot_pos < slash_pos {
        return Err(AuctionContractError::MetadataManipulationError);
    }

    let integer = u64::from_str(&uri[slash_pos..dot_pos]).unwrap();
    uri.truncate(last_pos);
    if is_last_cycle {
        uri.replace_range(slash_pos..dot_pos, &0.to_string());
    } else {
        let incremented_integer = integer
            .checked_add(1)
            .ok_or(AuctionContractError::ArithmeticError)?;
        uri.replace_range(slash_pos..dot_pos, &(incremented_integer).to_string());
    };

    Ok(())
}

#[cfg(test)]
mod cycle_increment_tests {
    use super::{increment_name, increment_uri};

    const MAX_NAME_LENGTH: usize = 32;
    //const MAX_SYMBOL_LENGTH: usize = 10;
    const MAX_URI_LENGTH: usize = 200;

    fn puff_string(string: &mut String, length: usize) -> String {
        let mut array_of_zeroes = vec![];
        while array_of_zeroes.len() < length - string.len() {
            array_of_zeroes.push(0u8);
        }
        string.clone() + std::str::from_utf8(&array_of_zeroes).unwrap()
    }

    #[test]
    fn test_name_increments() {
        // name increments
        let mut puffed_name = puff_string(&mut "puffed name #123".to_string(), MAX_NAME_LENGTH);
        assert_eq!(puffed_name.len(), MAX_NAME_LENGTH);
        increment_name(&mut puffed_name, false).unwrap();
        assert_eq!(puffed_name, "puffed name #124".to_string());

        let mut long_name = "aaaa bbbb cccc dddd eeee fff #14".to_string();
        assert_eq!(long_name.len(), MAX_NAME_LENGTH);
        increment_name(&mut long_name, false).unwrap();
        assert_eq!(long_name, "aaaa bbbb cccc dddd eeee fff #15".to_string());
    }

    #[test]
    fn test_uri_increments() {
        // uri increments
        let mut puffed_uri = puff_string(
            &mut "puffed/uri/some.path/123.jpg".to_string(),
            MAX_URI_LENGTH,
        );
        assert_eq!(puffed_uri.len(), MAX_URI_LENGTH);
        increment_uri(&mut puffed_uri, false).unwrap();
        assert_eq!(puffed_uri, "puffed/uri/some.path/124.jpg".to_string());

        let mut long_uri = String::new();
        for _ in 0..19 {
            // 10 long slice
            long_uri.push_str("asdf.qwer/");
        }
        let mut long_uri_expected = long_uri.clone();
        long_uri.push_str("123456.jpg");
        assert_eq!(long_uri.len(), MAX_URI_LENGTH);
        increment_uri(&mut long_uri, false).unwrap();
        long_uri_expected.push_str("123457.jpg");
        assert_eq!(long_uri, long_uri_expected);
    }

    #[test]
    fn test_last_cycle_increments() {
        // last cycle increments
        let mut long_name = "aaaa bbbb cccc dddd eeee fff #14".to_string();
        assert_eq!(long_name.len(), MAX_NAME_LENGTH);
        increment_name(&mut long_name, true).unwrap();
        assert_eq!(long_name, "aaaa bbbb cccc dddd eeee fff #0".to_string());

        let mut long_uri = String::new();
        for _ in 0..19 {
            // 10 long slice
            long_uri.push_str("asdf.qwer/");
        }
        let mut long_uri_expected = long_uri.clone();
        long_uri.push_str("123456.jpg");
        assert_eq!(long_uri.len(), MAX_URI_LENGTH);
        increment_uri(&mut long_uri, true).unwrap();
        long_uri_expected.push_str("0.jpg");
        assert_eq!(long_uri, long_uri_expected);
    }
}

