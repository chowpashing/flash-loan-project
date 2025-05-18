use anchor_lang::prelude::*;

declare_id!("91B4FwqgmGPDgnpyZanv2PxGt8Xkp37vy2GZVcB6jQGQ");

#[program]
pub mod flash_loan {
    use super::*;

    pub fn initialize(ctx: Context<Initialize>) -> Result<()> {
        Ok(())
    }
}

#[derive(Accounts)]
pub struct Initialize {}
