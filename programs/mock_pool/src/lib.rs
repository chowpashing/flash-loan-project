use anchor_lang::prelude::*;
use anchor_lang::system_program;
use shared::{MockPoolState, PoolStatus};

declare_id!("BtJ6VkrNWjgfPVH63LevLiZYSoKGKfueS1d54i6jWfzq");

#[program]
pub mod mock_pool {
    use super::*;

    /// 初始化池，创建共享状态
    pub fn initialize(ctx: Context<Initialize>, initial_balance: u64, fee_bps: u16) -> Result<()> {
        // 验证费用率
        require!(fee_bps <= 1000, PoolError::InvalidFeeRate); // 最大 10%
        
        // 验证初始余额
        require!(initial_balance > 0, PoolError::InvalidInitialBalance);

        let pool_state = &mut ctx.accounts.pool_state;
        pool_state.pool_id = Clock::get()?.unix_timestamp as u64;
        pool_state.balance = initial_balance;
        pool_state.fee_bps = fee_bps;
        pool_state.authority = ctx.accounts.authority.key();
        pool_state.total_borrowed = 0;
        pool_state.total_repaid = 0;
        pool_state.active_loans = 0;
        pool_state.created_at = Clock::get()?.unix_timestamp;
        pool_state.last_updated = Clock::get()?.unix_timestamp;
        pool_state.status = PoolStatus::Active;
        pool_state.bump = ctx.bumps.pool_state;

        // 将 initial_balance 的 SOL 转移到池子账户
        if initial_balance > 0 {
            system_program::transfer(
                CpiContext::new(
                    ctx.accounts.system_program.to_account_info(),
                    system_program::Transfer {
                        from: ctx.accounts.authority.to_account_info(),
                        to: pool_state.to_account_info(),
                    },
                ),
                initial_balance,
            )?;
        }

        // 发送事件
        emit!(PoolInitialized {
            pool_id: pool_state.pool_id,
            initial_balance,
            fee_bps,
            authority: ctx.accounts.authority.key(),
            timestamp: Clock::get()?.unix_timestamp,
        });

        msg!("Pool Initialized with {} SOL and {} BPS fee", initial_balance, fee_bps);
        Ok(())
    }

    /// 查询池子状态
    pub fn get_pool_info(ctx: Context<GetPoolInfo>) -> Result<()> {
        let pool_state = &ctx.accounts.pool_state;
        
        msg!("Pool Information:");
        msg!("  Pool ID: {}", pool_state.pool_id);
        msg!("  Balance: {} lamports", pool_state.balance);
        msg!("  Fee BPS: {}", pool_state.fee_bps);
        msg!("  Authority: {}", pool_state.authority);
        msg!("  Total Borrowed: {}", pool_state.total_borrowed);
        msg!("  Total Repaid: {}", pool_state.total_repaid);
        msg!("  Active Loans: {}", pool_state.active_loans);
        msg!("  Status: {:?}", pool_state.status);
        msg!("  Utilization Rate: {} BPS", pool_state.get_utilization_rate());
        msg!("  Can Lend: {}", pool_state.can_lend());
        
        Ok(())
    }

    /// 紧急暂停池子
    pub fn emergency_pause(ctx: Context<EmergencyPause>) -> Result<()> {
        let pool_state = &mut ctx.accounts.pool_state;
        
        // 验证权限
        require!(
            pool_state.authority == ctx.accounts.authority.key(),
            PoolError::InvalidAuthority
        );
        
        pool_state.status = PoolStatus::Emergency;
        pool_state.update_timestamp()?;
        
        emit!(PoolStatusChanged {
            pool_id: pool_state.pool_id,
            old_status: PoolStatus::Active,
            new_status: PoolStatus::Emergency,
            authority: ctx.accounts.authority.key(),
            timestamp: Clock::get()?.unix_timestamp,
        });
        
        msg!("Pool {} set to emergency status", pool_state.pool_id);
        Ok(())
    }

    /// 恢复池子正常状态
    pub fn resume_pool(ctx: Context<ResumePool>) -> Result<()> {
        let pool_state = &mut ctx.accounts.pool_state;
        
        // 验证权限
        require!(
            pool_state.authority == ctx.accounts.authority.key(),
            PoolError::InvalidAuthority
        );
        
        let old_status = pool_state.status.clone();
        pool_state.status = PoolStatus::Active;
        pool_state.update_timestamp()?;
        
        emit!(PoolStatusChanged {
            pool_id: pool_state.pool_id,
            old_status,
            new_status: PoolStatus::Active,
            authority: ctx.accounts.authority.key(),
            timestamp: Clock::get()?.unix_timestamp,
        });
        
        msg!("Pool {} resumed to active status", pool_state.pool_id);
        Ok(())
    }
}

#[derive(Accounts)]
pub struct Initialize<'info> {
    #[account(
        init,
        payer = authority,
        seeds = [b"mock_pool_state"],
        bump,
        space = MockPoolState::SPACE,
    )]
    pub pool_state: Account<'info, MockPoolState>,
    
    #[account(mut)]
    pub authority: Signer<'info>,
    
    pub system_program: Program<'info, System>,
}

#[derive(Accounts)]
pub struct GetPoolInfo<'info> {
    #[account(
        seeds = [b"mock_pool_state"],
        bump = pool_state.bump,
    )]
    pub pool_state: Account<'info, MockPoolState>,
}

#[derive(Accounts)]
pub struct EmergencyPause<'info> {
    #[account(
        mut,
        seeds = [b"mock_pool_state"],
        bump = pool_state.bump,
    )]
    pub pool_state: Account<'info, MockPoolState>,
    
    pub authority: Signer<'info>,
}

#[derive(Accounts)]
pub struct ResumePool<'info> {
    #[account(
        mut,
        seeds = [b"mock_pool_state"],
        bump = pool_state.bump,
    )]
    pub pool_state: Account<'info, MockPoolState>,
    
    pub authority: Signer<'info>,
}

#[event]
pub struct PoolInitialized {
    pub pool_id: u64,
    pub initial_balance: u64,
    pub fee_bps: u16,
    pub authority: Pubkey,
    pub timestamp: i64,
}

#[event]
pub struct PoolStatusChanged {
    pub pool_id: u64,
    pub old_status: PoolStatus,
    pub new_status: PoolStatus,
    pub authority: Pubkey,
    pub timestamp: i64,
}

#[error_code]
pub enum PoolError {
    #[msg("Insufficient funds in pool")]
    InsufficientFunds,
    #[msg("Invalid fee rate")]
    InvalidFeeRate,
    #[msg("Invalid initial balance")]
    InvalidInitialBalance,
    #[msg("Invalid authority")]
    InvalidAuthority,
    #[msg("Calculation overflow")]
    Overflow,
    #[msg("Calculation underflow")]
    Underflow,
}