use anchor_lang::prelude::*;
use anchor_spl::token::{Token, TokenAccount};
use shared::{MockPoolState, TransactionRecord};

declare_id!("HfsaDERzuB1m79Z1JHcbNz2JtwVcRowBso7xb5vWVQK");

#[program]
pub mod flash_loan_program {
    use super::*;

    /// 原子性闪电贷与套利 - 单个交易完成借款、套利、还款
    /// 优化栈使用，避免栈溢出
    pub fn atomic_flash_loan_with_arbitrage(
        mut ctx: Context<AtomicFlashLoanWithArbitrage>,
        amount: u64,
        min_expected_profit: u64,
        _description: String,
    ) -> Result<()> {
        // === CHECK阶段 ===
        let fee = FlashLoanHandler::validate_and_prepare(&ctx, amount)?;
        
        // === EFFECTS阶段 ===
        FlashLoanHandler::execute_loan(&mut ctx, amount)?;
        
        // === INTERACTIONS阶段 ===
        let actual_profit = FlashLoanHandler::execute_arbitrage(&ctx, amount, min_expected_profit)?;
        
        // === 还款阶段 ===
        FlashLoanHandler::process_repayment(&mut ctx, amount, fee)?;
        
        // === 记录阶段 ===
        FlashLoanHandler::record_transaction(&mut ctx, amount, fee, actual_profit)?;
        
        Ok(())
    }

    /// 查询交易记录 - 只读函数
    pub fn get_transaction_record(ctx: Context<GetTransactionRecord>, user: Pubkey) -> Result<()> {
        let transaction_record = &ctx.accounts.transaction_record;
        
        // 验证用户匹配
        require!(
            transaction_record.user == user,
            FlashLoanError::UnauthorizedAccess
        );
        
        msg!("Transaction Record:");
        msg!("  Transaction ID: {}", transaction_record.transaction_id);
        msg!("  User: {}", transaction_record.user);
        msg!("  Loan Amount: {}", transaction_record.loan_amount);
        msg!("  Fee: {}", transaction_record.fee);
        msg!("  Profit: {}", transaction_record.profit);
        msg!("  Net Profit: {}", transaction_record.net_profit);
        msg!("  ROI (bps): {}", transaction_record.calculate_roi_bps());
        msg!("  Is Profitable: {}", transaction_record.is_profitable());
        
        Ok(())
    }
}

/// 闪电贷处理器 - 将所有辅助函数移到这里
pub struct FlashLoanHandler;

impl FlashLoanHandler {
    /// 验证和准备阶段
    pub fn validate_and_prepare(
        ctx: &Context<AtomicFlashLoanWithArbitrage>,
        amount: u64,
    ) -> Result<u64> {
        let fee = ctx.accounts.mock_pool_state.calculate_fee(amount)?;
        
        require!(
            ctx.accounts.mock_pool_state.can_lend(),
            FlashLoanError::PoolNotActive
        );
        
        require!(
            ctx.accounts.mock_pool_state.has_sufficient_funds(amount),
            FlashLoanError::InsufficientPoolBalance
        );
        
        msg!("💰 开始原子性闪电贷与套利: {} lamports", amount);
        Ok(fee)
    }

    /// 执行借款
    pub fn execute_loan(
        ctx: &mut Context<AtomicFlashLoanWithArbitrage>,
        amount: u64,
    ) -> Result<()> {
        // 先更新池子状态 - 防止重入攻击 (CEI模式)
        let mock_pool_state = &mut ctx.accounts.mock_pool_state;
        mock_pool_state.balance -= amount;
        mock_pool_state.total_borrowed += amount;
        
        msg!("🔒 已更新池子状态，防止重入攻击");
        
        // 然后进行实际SOL转账
        **ctx.accounts.mock_pool_state.to_account_info().try_borrow_mut_lamports()? -= amount;
        **ctx.accounts.borrower.to_account_info().try_borrow_mut_lamports()? += amount;
        
        msg!("✅ 已转账 {} lamports 给用户", amount);
        Ok(())
    }

    /// 执行套利
    pub fn execute_arbitrage(
        ctx: &Context<AtomicFlashLoanWithArbitrage>,
        amount: u64,
        min_expected_profit: u64,
    ) -> Result<u64> {
        Self::call_arbitrage_bot(ctx, amount, min_expected_profit)
    }

    /// 调用套利机器人（提取CPI逻辑）
    pub fn call_arbitrage_bot(
        ctx: &Context<AtomicFlashLoanWithArbitrage>,
        amount: u64,
        min_expected_profit: u64,
    ) -> Result<u64> {
        let cpi_accounts = arbitrage_bot::cpi::accounts::ExecuteArbitrageAtomic {
            arbitrage_bot: ctx.accounts.arbitrage_bot.to_account_info(),
            mock_dex_program: ctx.accounts.mock_dex_program.to_account_info(),
            dex_pool_a: ctx.accounts.dex_pool_a.to_account_info(),
            dex_a_token_x_vault: ctx.accounts.dex_a_token_x_vault.to_account_info(),
            dex_a_token_y_vault: ctx.accounts.dex_a_token_y_vault.to_account_info(),
            dex_pool_b: ctx.accounts.dex_pool_b.to_account_info(),
            dex_b_token_x_vault: ctx.accounts.dex_b_token_x_vault.to_account_info(),
            dex_b_token_y_vault: ctx.accounts.dex_b_token_y_vault.to_account_info(),
            token_in_account: ctx.accounts.token_in_account.to_account_info(),
            user_token_x: ctx.accounts.user_token_x.to_account_info(),
            user_token_y: ctx.accounts.user_token_y.to_account_info(),
            token_program: ctx.accounts.token_program.to_account_info(),
            payer: ctx.accounts.borrower.to_account_info(),
            system_program: ctx.accounts.system_program.to_account_info(),
        };

        let cpi_ctx = CpiContext::new(
            ctx.accounts.arbitrage_bot_program.to_account_info(),
            cpi_accounts,
        );

        let result = arbitrage_bot::cpi::execute_arbitrage_atomic(cpi_ctx, amount, min_expected_profit)?.get();
        msg!("✅ 套利完成，获得利润: {} lamports", result);
        Ok(result)
    }

    /// 处理还款
    pub fn process_repayment(
        ctx: &mut Context<AtomicFlashLoanWithArbitrage>,
        amount: u64,
        fee: u64,
    ) -> Result<()> {
        let total_repayment = amount + fee;
        
        // 检查用户是否有足够资金还款
        require!(
            ctx.accounts.borrower.lamports() >= total_repayment,
            FlashLoanError::InsufficientFundsForRepayment
        );
        
        // 先更新池子状态 - 防止重入攻击
        let mock_pool_state = &mut ctx.accounts.mock_pool_state;
        mock_pool_state.balance += total_repayment;
        mock_pool_state.total_repaid += total_repayment;
        
        msg!("🔒 已更新还款状态，防止重入攻击");
        
        // 然后进行实际SOL转账
        **ctx.accounts.borrower.to_account_info().try_borrow_mut_lamports()? -= total_repayment;
        **ctx.accounts.mock_pool_state.to_account_info().try_borrow_mut_lamports()? += total_repayment;
        
        msg!("✅ 已归还 {} lamports (本金 {} + 费用 {})", total_repayment, amount, fee);
        Ok(())
    }

    /// 记录交易
    pub fn record_transaction(
        ctx: &mut Context<AtomicFlashLoanWithArbitrage>,
        amount: u64,
        fee: u64,
        actual_profit: u64,
    ) -> Result<()> {
        let transaction_record = &mut ctx.accounts.transaction_record;
        transaction_record.transaction_id = Clock::get()?.unix_timestamp as u64;
        transaction_record.user = ctx.accounts.borrower.key();
        transaction_record.loan_amount = amount;
        transaction_record.fee = fee;
        transaction_record.profit = actual_profit;
        transaction_record.net_profit = actual_profit.saturating_sub(fee);
        transaction_record.timestamp = Clock::get()?.unix_timestamp;
        transaction_record.bump = ctx.bumps.transaction_record;
        
        emit!(AtomicFlashLoanCompleted {
            user: ctx.accounts.borrower.key(),
            transaction_id: transaction_record.transaction_id,
            loan_amount: amount,
            fee,
            net_profit: transaction_record.net_profit,
            timestamp: Clock::get()?.unix_timestamp,
        });
        
        msg!("✅ 套利闪电贷完成! 净利润: {} lamports", transaction_record.net_profit);
        Ok(())
    }
}

#[derive(Accounts)]
#[instruction(amount: u64, min_expected_profit: u64, description: String)]
pub struct AtomicFlashLoanWithArbitrage<'info> {
    #[account(
        mut,
        seeds = [b"mock_pool_state"],
        bump = mock_pool_state.bump,
    )]
    pub mock_pool_state: Account<'info, MockPoolState>,

    #[account(
        init,
        payer = borrower,
        seeds = [b"transaction_record", borrower.key().as_ref(), &Clock::get().unwrap_or_default().unix_timestamp.to_le_bytes()],
        bump,
        space = TransactionRecord::SPACE,
    )]
    pub transaction_record: Account<'info, TransactionRecord>,

    #[account(mut)]
    pub borrower: Signer<'info>,

    // 套利机器人相关账户
    /// CHECK: arbitrage_bot程序
    pub arbitrage_bot_program: AccountInfo<'info>,

    #[account(mut)]
    pub arbitrage_bot: Account<'info, arbitrage_bot::ArbitrageBotState>,

    // DEX和代币相关账户
    /// CHECK: mock_dex程序
    pub mock_dex_program: AccountInfo<'info>,
    
    /// CHECK: DEX A的池子
    #[account(mut)]
    pub dex_pool_a: AccountInfo<'info>,

    /// CHECK: DEX A的Token X vault
    #[account(mut)]
    pub dex_a_token_x_vault: Account<'info, TokenAccount>,

    /// CHECK: DEX A的Token Y vault  
    #[account(mut)]
    pub dex_a_token_y_vault: Account<'info, TokenAccount>,

    /// CHECK: DEX B的池子
    #[account(mut)]
    pub dex_pool_b: AccountInfo<'info>,

    /// CHECK: DEX B的Token X vault
    #[account(mut)]
    pub dex_b_token_x_vault: Account<'info, TokenAccount>,

    /// CHECK: DEX B的Token Y vault
    #[account(mut)]
    pub dex_b_token_y_vault: Account<'info, TokenAccount>,

    /// CHECK: 输入代币账户
    #[account(mut)]
    pub token_in_account: Account<'info, TokenAccount>,

    /// CHECK: 用户Token X账户
    #[account(mut)]
    pub user_token_x: Account<'info, TokenAccount>,

    /// CHECK: 用户Token Y账户
    #[account(mut)]
    pub user_token_y: Account<'info, TokenAccount>,

    pub token_program: Program<'info, Token>,
    pub system_program: Program<'info, System>,
}

#[derive(Accounts)]
#[instruction(user: Pubkey)]
pub struct GetTransactionRecord<'info> {
    #[account(
        seeds = [b"transaction_record", user.as_ref()],
        bump,
    )]
    pub transaction_record: Account<'info, TransactionRecord>,
}

#[event]
pub struct AtomicFlashLoanCompleted {
    pub user: Pubkey,
    pub transaction_id: u64,
    pub loan_amount: u64,
    pub fee: u64,
    pub net_profit: u64,
    pub timestamp: i64,
}

#[error_code]
pub enum FlashLoanError {
    #[msg("Insufficient funds for repayment")]
    InsufficientFundsForRepayment,
    #[msg("Invalid flash loan amount")]
    InvalidAmount,
    #[msg("Insufficient pool balance")]
    InsufficientPoolBalance,
    #[msg("Pool is not active")]
    PoolNotActive,
    #[msg("Calculation overflow")]
    Overflow,
    #[msg("Calculation underflow")]
    Underflow,
    #[msg("Insufficient profit")]
    InsufficientProfit,
    #[msg("Unauthorized access")]
    UnauthorizedAccess,
}