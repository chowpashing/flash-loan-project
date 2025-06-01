use anchor_lang::prelude::*;
use anchor_spl::token::{Token, TokenAccount};

declare_id!("138D5SkLsTLz8GmEMEYAntRPyvZXmiyR8Mb2rooDjx2A");

#[program]
pub mod arbitrage_bot {
    use super::*;

    /// 原子性套利执行函数 - 通过CPI调用mock_dex
    /// 遵循CEI模式：Check-Effects-Interactions
    /// 优化栈使用，避免栈溢出
    pub fn execute_arbitrage_atomic(
        ctx: Context<ExecuteArbitrageAtomic>,
        loan_amount: u64,
        min_expected_profit: u64,
    ) -> Result<u64> {
        // === CHECK 阶段：所有验证和检查 ===
        ArbitrageHandler::validate_inputs(&ctx.accounts.arbitrage_bot, loan_amount, min_expected_profit)?;

        // === EFFECTS 阶段：更新所有状态 ===
        {
            let arbitrage_bot = &mut ctx.accounts.arbitrage_bot;
            arbitrage_bot.is_executing = true;
            arbitrage_bot.total_trades += 1;
        }

        // === INTERACTIONS 阶段：外部调用 ===
        
        // 执行第一次交换
        let first_result = ArbitrageHandler::execute_first_swap(&ctx, loan_amount)?;
        
        // 执行第二次交换
        let second_result = ArbitrageHandler::execute_second_swap(&ctx, first_result)?;

        // === 最终检查和状态更新 ===
        let actual_profit = second_result.saturating_sub(loan_amount);
        
        require!(actual_profit >= min_expected_profit, ErrorCode::InsufficientProfit);

        // 更新最终状态
        {
            let arbitrage_bot = &mut ctx.accounts.arbitrage_bot;
            arbitrage_bot.total_profit += actual_profit;
            arbitrage_bot.is_executing = false;
        }

        msg!("✅ ArbitrageBot: 套利完成，利润: {} lamports", actual_profit);
        Ok(actual_profit)
    }
}

/// 套利处理器 - 将所有辅助函数移到这里
pub struct ArbitrageHandler;

impl ArbitrageHandler {
    /// 验证输入参数
    pub fn validate_inputs(
        arbitrage_bot: &ArbitrageBotState,
        loan_amount: u64,
        min_expected_profit: u64,
    ) -> Result<()> {
        // 如果是新创建的账户，已由init_if_needed处理
        require!(!arbitrage_bot.is_executing, ErrorCode::ReentrancyDetected);
        require!(loan_amount > 0, ErrorCode::InvalidLoanAmount);
        require!(min_expected_profit > 0, ErrorCode::InvalidProfitRequirement);

        msg!("🤖 ArbitrageBot: 开始原子性套利执行");
        msg!("  借款金额: {} lamports", loan_amount);
        msg!("  最小期望利润: {} lamports", min_expected_profit);
        
        Ok(())
    }

    /// 执行第一次交换 - DEX A
    pub fn execute_first_swap(
        ctx: &Context<ExecuteArbitrageAtomic>,
        loan_amount: u64,
    ) -> Result<u64> {
        let min_amount_out = Self::calculate_min_amount_out(loan_amount)?;
        
        Self::perform_swap(
            &ctx.accounts.mock_dex_program,
            &ctx.accounts.dex_pool_a,
            &ctx.accounts.token_in_account,
            &ctx.accounts.dex_a_token_x_vault,
            &ctx.accounts.dex_a_token_y_vault,
            &ctx.accounts.user_token_x,
            &ctx.accounts.user_token_y,
            &ctx.accounts.arbitrage_bot,
            &ctx.accounts.token_program,
            &ctx.bumps.arbitrage_bot,
            loan_amount,
            min_amount_out,
        )?;

        let result = ctx.accounts.user_token_y.amount;
        msg!("  DEX A 交换完成，获得Token Y: {}", result);
        Ok(result)
    }

    /// 执行第二次交换 - DEX B
    pub fn execute_second_swap(
        ctx: &Context<ExecuteArbitrageAtomic>,
        token_y_amount: u64,
    ) -> Result<u64> {
        let min_amount_out = Self::calculate_min_amount_out(token_y_amount)?;
        
        Self::perform_swap(
            &ctx.accounts.mock_dex_program,
            &ctx.accounts.dex_pool_b,
            &ctx.accounts.user_token_y,
            &ctx.accounts.dex_b_token_x_vault,
            &ctx.accounts.dex_b_token_y_vault,
            &ctx.accounts.user_token_x,
            &ctx.accounts.user_token_y,
            &ctx.accounts.arbitrage_bot,
            &ctx.accounts.token_program,
            &ctx.bumps.arbitrage_bot,
            token_y_amount,
            min_amount_out,
        )?;

        let result = ctx.accounts.user_token_x.amount;
        msg!("  DEX B 交换完成，最终Token X: {}", result);
        Ok(result)
    }

    /// 计算最小输出金额（考虑手续费和滑点）
    pub fn calculate_min_amount_out(amount_in: u64) -> Result<u64> {
        let estimated_out = amount_in
            .checked_mul(9970) // 99.7% (扣除0.3%手续费)
            .ok_or(ErrorCode::CalculationOverflow)?
            .checked_div(10000)
            .ok_or(ErrorCode::CalculationOverflow)?;
            
        let result = estimated_out
            .checked_mul(9000) // 90%滑点容忍度
            .ok_or(ErrorCode::CalculationOverflow)?
            .checked_div(10000)
            .ok_or(ErrorCode::CalculationOverflow)?;
        
        Ok(result)
    }

    /// 执行单个交换操作（提取通用逻辑）
    pub fn perform_swap<'info>(
        mock_dex_program: &AccountInfo<'info>,
        pool: &AccountInfo<'info>,
        token_in_account: &Account<'info, TokenAccount>,
        token_x_vault: &Account<'info, TokenAccount>,
        token_y_vault: &Account<'info, TokenAccount>,
        user_token_x: &Account<'info, TokenAccount>,
        user_token_y: &Account<'info, TokenAccount>,
        user_authority: &Account<'info, ArbitrageBotState>,
        token_program: &Program<'info, Token>,
        bump: &u8,
        amount_in: u64,
        min_amount_out: u64,
    ) -> Result<()> {
        let cpi_accounts = mock_dex::cpi::accounts::Swap {
            pool: pool.to_account_info(),
            token_in_account: token_in_account.to_account_info(),
            token_x_vault: token_x_vault.to_account_info(),
            token_y_vault: token_y_vault.to_account_info(),
            user_token_x: user_token_x.to_account_info(),
            user_token_y: user_token_y.to_account_info(),
            user_authority: user_authority.to_account_info(),
            token_program: token_program.to_account_info(),
        };

        let seeds = &[b"arbitrage_bot".as_ref(), &[*bump]];
        let signer_seeds = &[&seeds[..]];

        let cpi_ctx = CpiContext::new_with_signer(
            mock_dex_program.to_account_info(),
            cpi_accounts,
            signer_seeds,
        );

        mock_dex::cpi::swap(cpi_ctx, amount_in, min_amount_out, "test-pool".to_string())
    }
}

#[derive(Accounts)]
pub struct ExecuteArbitrageAtomic<'info> {
    #[account(
        init_if_needed,
        payer = payer,
        seeds = [b"arbitrage_bot"],
        bump,
        space = ArbitrageBotState::SPACE,
    )]
    pub arbitrage_bot: Account<'info, ArbitrageBotState>,

    /// CHECK: mock_dex程序
    pub mock_dex_program: AccountInfo<'info>,

    // DEX A相关账户
    /// CHECK: DEX A的池子
    #[account(mut)]
    pub dex_pool_a: AccountInfo<'info>,

    /// CHECK: DEX A的Token X vault
    #[account(mut)]
    pub dex_a_token_x_vault: Account<'info, TokenAccount>,

    /// CHECK: DEX A的Token Y vault
    #[account(mut)]
    pub dex_a_token_y_vault: Account<'info, TokenAccount>,

    // DEX B相关账户
    /// CHECK: DEX B的池子
    #[account(mut)]
    pub dex_pool_b: AccountInfo<'info>,

    /// CHECK: DEX B的Token X vault
    #[account(mut)]
    pub dex_b_token_x_vault: Account<'info, TokenAccount>,

    /// CHECK: DEX B的Token Y vault
    #[account(mut)]
    pub dex_b_token_y_vault: Account<'info, TokenAccount>,

    // 用户代币账户
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
    
    #[account(mut)]
    pub payer: Signer<'info>,
    
    pub system_program: Program<'info, System>,
}

#[account]
pub struct ArbitrageBotState {
    pub owner: Pubkey,
    pub is_executing: bool,
    pub total_trades: u64,
    pub total_profit: u64,
}

impl ArbitrageBotState {
    pub const SPACE: usize = 8 + // discriminator
        32 + // owner
        1 + // is_executing
        8 + // total_trades
        8; // total_profit
}

#[error_code]
pub enum ErrorCode {
    #[msg("利润不足以偿还闪电贷")]
    InsufficientProfit,
    #[msg("检测到重入攻击")]
    ReentrancyDetected,
    #[msg("无效的借款金额")]
    InvalidLoanAmount,
    #[msg("无效的利润要求")]
    InvalidProfitRequirement,
    #[msg("计算溢出")]
    CalculationOverflow,
} 