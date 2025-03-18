use anchor_lang::prelude::*;
use anchor_spl::{
    token,
    token::{Mint, Token, TokenAccount},
};
declare_id!("7mT9LzDLhZQHfXTamhKMNuhsxCJyFcRWkbq3Ed4EKFh4");

#[program]
pub mod vesting_contract {
    use anchor_lang::solana_program::system_instruction::{self, transfer};
    use anchor_spl::token::Transfer;

    use super::*;

    pub fn initialize(
        ctx: Context<VestingSetup>,
        amount: u64,
        vesting_start: u64,
        vesting_end: u64,
        vesting_ticks: u64,
        price_per_sol: u64,
    ) -> Result<()> {
        // move the token to the vesting account
        msg!("Moving token to vesting account");
        msg!("Token mint: {}", ctx.accounts.token_mint.key());
        msg!("Vesting account: {}", ctx.accounts.vesting_account.key());
        msg!("Token program: {}", ctx.accounts.token_program.key());
        msg!("Token account: {}", ctx.accounts.vesting_token.key());
        // move tokens
        token::transfer(
            CpiContext::new(
                ctx.accounts.token_program.to_account_info(),
                Transfer {
                    from: ctx.accounts.user.to_account_info(),
                    to: ctx.accounts.vesting_token.to_account_info(),
                    authority: ctx.accounts.token_mint.to_account_info(),
                },
            ),
            amount,
        )?;
        // initialize vesting account
        let vesting_account = &mut ctx.accounts.vesting_account;
        vesting_account.authority = ctx.accounts.user.key();
        vesting_account.token_mint = ctx.accounts.token_mint.key();
        vesting_account.total_amount = amount;
        vesting_account.claimed_amount = 0;
        vesting_account.vesting_start = vesting_start;
        vesting_account.vesting_end = vesting_end;
        vesting_account.vesting_ticks = vesting_ticks;
        vesting_account.price_per_sol = price_per_sol;
        Ok(())
    }

    pub fn purchase_vesting(ctx: Context<PurchaseVesting>, amount_lamports: u64) -> Result<()> {
        // transfer sol to the vesting account
        msg!("Transferring SOL to vesting account");
        msg!("Vesting account: {}", ctx.accounts.vesting_account.key());
        msg!("User: {}", ctx.accounts.user.key());
        msg!("Amount: {}", amount_lamports);
        // initialize vesting account if it doesn't exist
        if ctx.accounts.vesting_account.total_amount == 0 {
            ctx.accounts.vesting_account.authority = ctx.accounts.user.key();
            ctx.accounts.vesting_account.token_mint = ctx.accounts.vesting_pool.token_mint;
            ctx.accounts.vesting_account.total_amount = 0;
            ctx.accounts.vesting_account.claimed_amount = 0;
            ctx.accounts.vesting_account.vesting_start = ctx.accounts.vesting_pool.vesting_start;
            ctx.accounts.vesting_account.vesting_end = ctx.accounts.vesting_pool.vesting_end;
            ctx.accounts.vesting_account.vesting_ticks = ctx.accounts.vesting_pool.vesting_ticks;
            ctx.accounts.vesting_account.used_ticks = 0;
            ctx.accounts.vesting_account.last_claim = Clock::get()?.unix_timestamp as u64;
        }

        // assign tokens to vesting account
        let allowed_amount = (amount_lamports * ctx.accounts.vesting_pool.price_per_sol) / 1_000_000_000;
        ctx.accounts.vesting_account.total_amount += allowed_amount;
        ctx.accounts.vesting_pool.total_amount -= allowed_amount;

        // transfer sol via system program
        let transfer_instruction = system_instruction::transfer(
            &ctx.accounts.user.key(),
            &ctx.accounts.vesting_account.key(),
            amount_lamports
        );

        // Invoke the transfer instruction
        anchor_lang::solana_program::program::invoke_signed(
            &transfer_instruction,
            &[
                ctx.accounts.user.to_account_info(),
                ctx.accounts.vesting_account.to_account_info(),
                ctx.accounts.system_program.to_account_info(),
            ],
            &[],
        )?;
        Ok(())
    }

    pub fn claim_vesting_sol(ctx: Context<ClaimVesting>) -> Result<()> {
        let vesting_pool = &mut ctx.accounts.vesting_pool;
        // get the sol minus required rent
        let required_rent = Rent::get()?.minimum_balance(std::mem::size_of::<VestingAccount>() + 8) as u64;
        let current_balance = vesting_pool.to_account_info().lamports();
        let amount_to_claim = current_balance - required_rent;
        if amount_to_claim > 0 {
            msg!("Claiming {} SOL", amount_to_claim);
            // transfer sol to the user by subtracting lamports
            vesting_pool.to_account_info().sub_lamports(amount_to_claim)?;
            ctx.accounts.user.to_account_info().add_lamports(amount_to_claim)?;
        }
        Ok(())
    }

    pub fn claim_vesting_tokens(ctx: Context<ClaimTokens>) -> Result<()> {
        let vesting_pool = &mut ctx.accounts.vesting_pool;
        let vesting_account = &mut ctx.accounts.vesting_account;
        let vesting_token = &mut ctx.accounts.vesting_token;
        let token_account = &mut ctx.accounts.token_account;
        let token_program = &mut ctx.accounts.token_program;
        let system_program = &mut ctx.accounts.system_program;
        let user = &mut ctx.accounts.user;
        
        // validations
        if vesting_token.amount < vesting_account.total_amount {
            return Err(VestingError::InsufficientVestingTokens.into());
        }
        if vesting_account.used_ticks >= vesting_account.vesting_ticks {
            return Err(VestingError::VestingEnded.into());
        }
        // check if we can claim any tokens
        let current_time = Clock::get()?.unix_timestamp as u64;
        let time_since_last_claim: u64 = current_time.saturating_sub(vesting_account.last_claim);
        let tick_time: u64 = vesting_account.vesting_end - vesting_account.vesting_start;
        let can_claim = time_since_last_claim >= tick_time && current_time >= vesting_account.vesting_start;
        if !can_claim {
            return Err(VestingError::NotTimeToClaim.into());
        }
        // calculate the number of ticks we can claim
        // MAKE SURE THIS ROUDNS DOWN
        let ticks_to_claim = time_since_last_claim / tick_time;
        let amount_to_claim = (vesting_account.total_amount * ticks_to_claim) / vesting_account.vesting_ticks;
        // update the vesting account
        vesting_account.claimed_amount += amount_to_claim;
        vesting_account.used_ticks += ticks_to_claim;
        vesting_account.last_claim = current_time;
        // transfer tokens to the user
        token::transfer(
            CpiContext::new(
                token_program.to_account_info(),
                Transfer {
                    from: vesting_token.to_account_info(),
                    to: token_account.to_account_info(),
                    authority: vesting_account.to_account_info(),
                },
            ),
            amount_to_claim,
        )?;
        Ok(())
        
        
    }
}

#[account]
pub struct VestingPool {
    pub authority: Pubkey,
    pub token_mint: Pubkey,
    pub price_per_sol: u64,
    pub total_amount: u64,
    pub claimed_amount: u64,
    pub vesting_start: u64,
    pub vesting_end: u64,
    pub vesting_ticks: u64,
}

#[account]
pub struct VestingAccount {
    pub authority: Pubkey,
    pub token_mint: Pubkey,
    pub total_amount: u64,
    pub claimed_amount: u64,
    pub vesting_start: u64,
    pub vesting_end: u64,
    pub vesting_ticks: u64,
    pub used_ticks: u64,
    pub last_claim: u64,
}

#[derive(Accounts)]
pub struct VestingSetup<'info> {
    #[account(mut)]
    pub user: Signer<'info>,
    #[account(init, payer = user, space = 8 + std::mem::size_of::<VestingPool>())]
    pub vesting_account: Account<'info, VestingPool>,
    #[account(
        init,
        payer = user,
        token::mint = token_mint,
        token::authority = vesting_account,
    )]
    pub vesting_token: Account<'info, TokenAccount>,
    pub token_mint: Account<'info, Mint>,
    pub system_program: Program<'info, System>,
    pub token_program: Program<'info, Token>,
}

// just for claiming the sol
#[derive(Accounts)]
pub struct ClaimVesting<'info> {
    #[account(mut)]
    pub user: Signer<'info>,
    #[account(mut, constraint = vesting_pool.authority == user.key())]
    pub vesting_pool: Account<'info, VestingPool>,
    pub system_program: Program<'info, System>,
    pub rent: Sysvar<'info, Rent>,
}

#[derive(Accounts)]
pub struct PurchaseVesting<'info> {
    #[account(mut)]
    pub user: Signer<'info>,
    #[account(mut)]
    pub vesting_pool: Account<'info, VestingPool>,
    #[account(
        init_if_needed,
        payer = user,
        space = 8 + std::mem::size_of::<VestingAccount>()
    )]
    pub vesting_account: Account<'info, VestingAccount>,
    pub system_program: Program<'info, System>,
}

#[derive(Accounts)]
pub struct ClaimTokens<'info> {
    #[account(mut)]
    pub user: Signer<'info>,
    #[account(mut, constraint = vesting_pool.authority == user.key())]
    pub vesting_pool: Account<'info, VestingPool>,
    #[account(
        mut,
        constraint = vesting_account.authority == user.key()
    )]
    pub vesting_account: Account<'info, VestingAccount>,
    pub token_program: Program<'info, Token>,
    #[account(
        mut,
        constraint = vesting_token.mint == vesting_pool.token_mint && vesting_token.owner == vesting_pool.key()
    )]
    pub vesting_token: Account<'info, TokenAccount>,
    #[account(
        mut,
        constraint = token_account.mint == vesting_pool.token_mint && token_account.owner == vesting_pool.authority,
        token::mint = vesting_pool.token_mint,
        token::authority = user
    )]
    pub token_account: Account<'info, TokenAccount>,
    pub system_program: Program<'info, System>,
}

// errors
#[error_code]
pub enum VestingError {
    InsufficientVestingTokens,
    VestingEnded,
    NotTimeToClaim,
}