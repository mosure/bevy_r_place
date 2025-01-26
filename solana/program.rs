use borsh::{BorshDeserialize, BorshSerialize};
use solana_program::{
    account_info::{next_account_info, AccountInfo},
    clock::Clock,
    entrypoint,
    entrypoint::ProgramResult,
    program::{invoke, invoke_signed},
    program_error::ProgramError,
    pubkey::Pubkey,
    system_instruction,
    sysvar::{Sysvar, SysvarId},
    msg,
};
use thiserror::Error;

// ----------------------------------------------
// Constants
// ----------------------------------------------
pub const BOARD_WIDTH: u16 = 4096;
pub const BOARD_HEIGHT: u16 = 4096;
pub const CHUNK_SIZE: u16 = 256;   // => 16×16 chunk PDAs

pub const COOLDOWN_SECONDS: i64 = 60;  // one pixel per user per minute
pub const PIXEL_FEE_LAMPORTS: u64 = 1_000_000; // e.g. 0.001 SOL if 1 SOL = 1e9 lamports

// Seeds
pub const CHUNK_SEED_PREFIX: &[u8] = b"rplace_chunk";
pub const VAULT_SEED_PREFIX: &[u8] = b"aggregator_vault";

// ----------------------------------------------
// Errors
// ----------------------------------------------
#[derive(Error, Debug, Copy, Clone)]
pub enum RPlaceError {
    #[error("Coordinates out of range")]
    OutOfRange,
    #[error("User still in cooldown")]
    CooldownActive,
    #[error("Chunk data is corrupt or missing")]
    ChunkDataCorrupt,
    #[error("DiffBuffer not owned by program")]
    InvalidDiffBufferOwner,
    #[error("UserState not owned by program")]
    InvalidUserOwner,
    #[error("AggregatorVault not owned by program")]
    InvalidVaultOwner,
    #[error("Chunk seeds mismatch")]
    InvalidChunkSeeds,
    #[error("Vault seeds mismatch")]
    InvalidVaultSeeds,
    #[error("Missing user signature")]
    UserMustSign,
    #[error("DiffBuffer is not initialized or empty")]
    DiffBufferCorrupt,
    #[error("No diffs to merge")]
    NoDiffsToMerge,
    #[error("Partial merges needed")]
    PartialMerge,
}
impl From<RPlaceError> for ProgramError {
    fn from(e: RPlaceError) -> Self {
        ProgramError::Custom(e as u32)
    }
}

// ----------------------------------------------
// Data structures
// ----------------------------------------------
#[derive(BorshSerialize, BorshDeserialize, Debug)]
pub struct ChunkState {
    // Must store chunk_x, chunk_y for verification
    pub chunk_x: u16,
    pub chunk_y: u16,
    // 256x256 = 65536 pixel array
    pub pixels: Vec<u8>,
}

#[derive(BorshSerialize, BorshDeserialize, Debug)]
pub struct UserState {
    pub is_initialized: bool,
    pub last_place_time: i64,
}

#[derive(BorshSerialize, BorshDeserialize, Debug, Clone)]
pub struct PixelDiff {
    pub x: u16,
    pub y: u16,
    pub color: u8,
    pub user: Pubkey,
}

#[derive(BorshSerialize, BorshDeserialize, Debug)]
pub struct DiffBuffer {
    pub is_initialized: bool,
    pub diffs: Vec<PixelDiff>,
}

/// The aggregator vault data: who is the authority to withdraw
#[derive(BorshSerialize, BorshDeserialize, Debug)]
pub struct AggregatorVaultData {
    pub is_initialized: bool,
    pub authority: Pubkey,
}

// ----------------------------------------------
// Instructions
// ----------------------------------------------
#[derive(BorshSerialize, BorshDeserialize, Debug)]
pub enum RPlaceInstruction {
    /// Place a pixel with x,y,color. Checks user cooldown, appends a PixelDiff to DiffBuffer,
    /// collects a pixel fee -> aggregator vault.
    PlacePixel { x: u16, y: u16, color: u8 },

    /// Merge up to `max_diffs` from DiffBuffer for the chunk (chunk_x, chunk_y).
    AggregatorMerge {
        chunk_x: u16,
        chunk_y: u16,
        max_diffs: u32,
    },

    /// Initialize aggregator vault with a given authority (only done once).
    InitAggregatorVault {
        authority: Pubkey,
    },

    /// aggregator withdraw fees to their own pay account (only authority can do it)
    AggregatorWithdrawFees {
        amount: u64,
    },
}

// ----------------------------------------------
// Entrypoint
// ----------------------------------------------
entrypoint!(process_instruction);

pub fn process_instruction(
    program_id: &Pubkey,
    accounts: &[AccountInfo],
    data: &[u8],
) -> ProgramResult {
    let instruction = RPlaceInstruction::try_from_slice(data)
        .map_err(|_| ProgramError::InvalidInstructionData)?;

    match instruction {
        RPlaceInstruction::PlacePixel { x, y, color } => {
            place_pixel(program_id, accounts, x, y, color)?;
        }
        RPlaceInstruction::AggregatorMerge {
            chunk_x,
            chunk_y,
            max_diffs,
        } => {
            aggregator_merge(program_id, accounts, chunk_x, chunk_y, max_diffs)?;
        }
        RPlaceInstruction::InitAggregatorVault { authority } => {
            init_aggregator_vault(program_id, accounts, authority)?;
        }
        RPlaceInstruction::AggregatorWithdrawFees { amount } => {
            aggregator_withdraw_fees(program_id, accounts, amount)?;
        }
    }

    Ok(())
}

// ----------------------------------------------
// 1) PlacePixel
// ----------------------------------------------
fn place_pixel(
    program_id: &Pubkey,
    accounts: &[AccountInfo],
    x: u16,
    y: u16,
    color: u8,
) -> ProgramResult {
    // Expected:
    //  0: DiffBuffer (writable, owned by program)
    //  1: UserState  (writable, owned by program)
    //  2: AggregatorVault (writable, owned by program)
    //  3: user/payer (signer)
    //  4: system program
    //  5: clock sysvar

    let acct_iter = &mut accounts.iter();
    let diff_buf_acct = next_account_info(acct_iter)?;
    let user_acct = next_account_info(acct_iter)?;
    let vault_acct = next_account_info(acct_iter)?;
    let payer_acct = next_account_info(acct_iter)?;
    let system_acct = next_account_info(acct_iter)?;
    let clock_acct = next_account_info(acct_iter)?;

    if diff_buf_acct.owner != program_id {
        return Err(RPlaceError::InvalidDiffBufferOwner.into());
    }
    if user_acct.owner != program_id {
        return Err(RPlaceError::InvalidUserOwner.into());
    }
    if vault_acct.owner != program_id {
        return Err(RPlaceError::InvalidVaultOwner.into());
    }
    if !payer_acct.is_signer {
        return Err(RPlaceError::UserMustSign.into());
    }

    // check x,y
    if x >= BOARD_WIDTH || y >= BOARD_HEIGHT {
        return Err(RPlaceError::OutOfRange.into());
    }

    // read clock
    if *clock_acct.key != solana_program::sysvar::clock::id() {
        return Err(ProgramError::InvalidAccountData);
    }
    let clock = solana_program::clock::Clock::from_account_info(clock_acct)?;
    let now = clock.unix_timestamp;

    // read user
    let mut user_data = UserState::try_from_slice(&user_acct.data.borrow())?;
    if user_data.is_initialized && (now < user_data.last_place_time + COOLDOWN_SECONDS) {
        return Err(RPlaceError::CooldownActive.into());
    }
    // update user
    user_data.is_initialized = true;
    user_data.last_place_time = now;
    user_data.serialize(&mut &mut user_acct.data.borrow_mut()[..])?;

    // collect pixel fee from user => aggregator vault
    let ix = system_instruction::transfer(payer_acct.key, vault_acct.key, PIXEL_FEE_LAMPORTS);
    invoke(
        &ix,
        &[
            payer_acct.clone(),
            vault_acct.clone(),
            system_acct.clone(),
        ],
    )?;

    // append diff to diff buffer
    let mut diff_buf_data = DiffBuffer::try_from_slice(&diff_buf_acct.data.borrow())?;
    if !diff_buf_data.is_initialized {
        diff_buf_data.is_initialized = true;
        diff_buf_data.diffs = vec![];
    }
    let new_diff = PixelDiff {
        x,
        y,
        color,
        user: *payer_acct.key,
    };
    diff_buf_data.diffs.push(new_diff);
    diff_buf_data.serialize(&mut &mut diff_buf_acct.data.borrow_mut()[..])?;

    msg!("PlacePixel: appended diff (x={}, y={}, color={})", x, y, color);
    Ok(())
}

// ----------------------------------------------
// 2) AggregatorMerge (partial merges for one chunk)
// ----------------------------------------------
fn aggregator_merge(
    program_id: &Pubkey,
    accounts: &[AccountInfo],
    chunk_x: u16,
    chunk_y: u16,
    max_diffs: u32,
) -> ProgramResult {
    // Expected:
    //  0: DiffBuffer (writable, owned by program)
    //  1: chunk account pda (writable, owned by program)
    // ( aggregator could pass more if needed, e.g. aggregator vault, signers, etc.)
    let acct_iter = &mut accounts.iter();
    let diff_buf_acct = next_account_info(acct_iter)?;
    let chunk_acct = next_account_info(acct_iter)?;

    if diff_buf_acct.owner != program_id {
        return Err(RPlaceError::InvalidDiffBufferOwner.into());
    }
    if chunk_acct.owner != program_id {
        return Err(ProgramError::IncorrectProgramId);
    }

    // verify chunk seeds
    let (expected_chunk_pda, bump) = Pubkey::find_program_address(
        &[
            CHUNK_SEED_PREFIX,
            &chunk_x.to_le_bytes(),
            &chunk_y.to_le_bytes(),
        ],
        program_id,
    );
    if expected_chunk_pda != *chunk_acct.key {
        return Err(RPlaceError::InvalidChunkSeeds.into());
    }

    // read chunk
    let mut chunk_data = ChunkState::try_from_slice(&chunk_acct.data.borrow())?;
    let expected_len = (CHUNK_SIZE as usize) * (CHUNK_SIZE as usize);
    if chunk_data.pixels.len() != expected_len {
        return Err(RPlaceError::ChunkDataCorrupt.into());
    }
    if chunk_data.chunk_x != chunk_x || chunk_data.chunk_y != chunk_y {
        msg!("chunk coords mismatch in chunk account data");
        return Err(RPlaceError::InvalidChunkSeeds.into());
    }

    // read diff buffer
    let mut diff_buf_data = DiffBuffer::try_from_slice(&diff_buf_acct.data.borrow())?;
    if !diff_buf_data.is_initialized {
        return Err(RPlaceError::DiffBufferCorrupt.into());
    }
    if diff_buf_data.diffs.is_empty() {
        return Err(RPlaceError::NoDiffsToMerge.into());
    }

    msg!(
        "Aggregator merging up to {} diffs for chunk=({},{})",
        max_diffs,
        chunk_x,
        chunk_y
    );
    let mut merges_done = 0u32;
    let mut new_diffs: Vec<PixelDiff> = vec![];

    for diff in diff_buf_data.diffs.iter() {
        if merges_done >= max_diffs {
            // done for this aggregator call
            new_diffs.push(diff.clone());
            continue;
        }
        let c_x = diff.x / CHUNK_SIZE;
        let c_y = diff.y / CHUNK_SIZE;
        if c_x == chunk_x && c_y == chunk_y {
            // merge it
            let local_x = diff.x % CHUNK_SIZE;
            let local_y = diff.y % CHUNK_SIZE;
            let idx = (local_y as usize) * (CHUNK_SIZE as usize) + (local_x as usize);
            chunk_data.pixels[idx] = diff.color;
            merges_done += 1;
        } else {
            // belongs to another chunk
            new_diffs.push(diff.clone());
        }
    }

    // rewrite chunk
    chunk_data.serialize(&mut &mut chunk_acct.data.borrow_mut()[..])?;

    // rewrite diff buffer
    diff_buf_data.diffs = new_diffs;
    diff_buf_data.serialize(&mut &mut diff_buf_acct.data.borrow_mut()[..])?;

    msg!(
        "AggregatorMerge chunk=({},{}) => merges_done={}, leftover_diffs={}",
        chunk_x,
        chunk_y,
        merges_done,
        diff_buf_data.diffs.len()
    );

    if merges_done == max_diffs && diff_buf_data.diffs.len() != 0 {
        // more diffs for that chunk remain, aggregator must call again
        return Err(RPlaceError::PartialMerge.into());
    }

    Ok(())
}

// ----------------------------------------------
// 3) InitAggregatorVault
// ----------------------------------------------
fn init_aggregator_vault(
    program_id: &Pubkey,
    accounts: &[AccountInfo],
    authority: Pubkey,
) -> ProgramResult {
    // Expected:
    //  0: aggregator vault pda (writable, owned by program)
    // This instruction sets aggregatorVaultData { is_initialized=true, authority=... }

    let vault_acct = next_account_info(&mut accounts.iter())?;

    if vault_acct.owner != program_id {
        msg!("Vault not owned by program");
        return Err(RPlaceError::InvalidVaultOwner.into());
    }

    // read or create aggregator vault data
    let mut vault_data = AggregatorVaultData::try_from_slice(&vault_acct.data.borrow()).unwrap_or_else(|_| {
        AggregatorVaultData {
            is_initialized: false,
            authority: Pubkey::default(),
        }
    });

    if vault_data.is_initialized {
        msg!("Vault already initialized");
        // optional: return error or just override
    }

    vault_data.is_initialized = true;
    vault_data.authority = authority;
    vault_data.serialize(&mut &mut vault_acct.data.borrow_mut()[..])?;

    msg!("InitAggregatorVault done. authority={}", authority);
    Ok(())
}

// ----------------------------------------------
// 4) aggregator_withdraw_fees
// ----------------------------------------------
fn aggregator_withdraw_fees(
    program_id: &Pubkey,
    accounts: &[AccountInfo],
    amount: u64,
) -> ProgramResult {
    // Expected:
    //  0: aggregator vault pda (writable, owned by program)
    //  1: aggregator pay account (system account, writable)
    //  2: authority (signer) => must match vaultData.authority
    //  3: system program

    let acc_iter = &mut accounts.iter();
    let vault_acct = next_account_info(acc_iter)?;
    let pay_acct = next_account_info(acc_iter)?;
    let authority_acct = next_account_info(acc_iter)?;
    let system_acct = next_account_info(acc_iter)?;

    if vault_acct.owner != program_id {
        return Err(RPlaceError::InvalidVaultOwner.into());
    }
    if !authority_acct.is_signer {
        return Err(RPlaceError::UserMustSign.into());
    }

    // read aggregator vault data
    let mut vault_data = AggregatorVaultData::try_from_slice(&vault_acct.data.borrow())?;
    if !vault_data.is_initialized {
        msg!("Vault not init");
        return Err(ProgramError::UninitializedAccount);
    }

    // check authority
    if *authority_acct.key != vault_data.authority {
        msg!("Invalid aggregator authority");
        return Err(ProgramError::MissingRequiredSignature);
    }

    // check seeds
    let (vault_pda, bump) = Pubkey::find_program_address(&[VAULT_SEED_PREFIX], program_id);
    if vault_pda != *vault_acct.key {
        return Err(RPlaceError::InvalidVaultSeeds.into());
    }

    // do system transfer from vault to pay account
    let ix = system_instruction::transfer(vault_acct.key, pay_acct.key, amount);
    let seeds: &[&[u8]] = &[
        VAULT_SEED_PREFIX,
        &[bump],
    ];

    invoke_signed(
        &ix,
        &[
            vault_acct.clone(),
            pay_acct.clone(),
            system_acct.clone(),
        ],
        &[&seeds],
    )?;

    msg!(
        "Aggregator withdrew {} lamports from vault => {}",
        amount,
        pay_acct.key
    );
    Ok(())
}
