use anchor_lang::prelude::*;

declare_id!("7mT9LzDLhZQHfXTamhKMNuhsxCJyFcRWkbq3Ed4EKFh4");

#[program]
pub mod vesting_contract {
    use super::*;

    pub fn initialize(ctx: Context<Initialize>) -> Result<()> {
        Ok(())
    }
}

#[derive(Accounts)]
pub struct Initialize {}
