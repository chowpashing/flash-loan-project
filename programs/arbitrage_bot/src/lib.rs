use anchor_lang::prelude::*;
use anchor_spl::token::{Token, TokenAccount};

declare_id!("2bY7JFDsaAnDhHGBrei3uhT2XW3S1582HR5pxnFR2jMN");

#[program]
pub mod arbitrage_bot {
    use super::*;

    /// 原子性套利执行函数 - 通过CPI调用mock_dex
    /// 这个函数会被flash-loan合约通过CPI调用
    pub fn execute_arbitrage_atomic(
        ctx: Context<ExecuteArbitrageAtomic>,
        loan_amount: u64,
        min_expected_profit: u64,
    ) -> Result<u64> {
        let arbitrage_bot = &mut ctx.accounts.arbitrage_bot;
        
        // 防止重入
        require!(!arbitrage_bot.is_executing, ErrorCode::ReentrancyDetected);
        arbitrage_bot.is_executing = true;

        msg!("🤖 ArbitrageBot: 开始原子性套利执行");
        msg!("  借款金额: {} lamports", loan_amount);
        msg!("  最小期望利润: {} lamports", min_expected_profit);

        // 步骤1: 第一次交换 - 模拟在DEX A上用借来的资金买入某种代币
        {
            let cpi_accounts = mock_dex::cpi::accounts::Swap {
                pool: ctx.accounts.dex_pool_a.to_account_info(),
                token_in_account: ctx.accounts.token_in_account.to_account_info(),
                token_x_vault: ctx.accounts.dex_a_token_x_vault.to_account_info(),
                token_y_vault: ctx.accounts.dex_a_token_y_vault.to_account_info(),
                user_token_x: ctx.accounts.user_token_x.to_account_info(),
                user_token_y: ctx.accounts.user_token_y.to_account_info(),
                user_authority: arbitrage_bot.to_account_info(),
                token_program: ctx.accounts.token_program.to_account_info(),
            };

            let seeds = &[
                b"arbitrage_bot".as_ref(),
                &[*ctx.bumps.get("arbitrage_bot").unwrap()]
            ];
            let signer_seeds = &[&seeds[..]];

            let cpi_ctx = CpiContext::new_with_signer(
                ctx.accounts.mock_dex_program.to_account_info(),
                cpi_accounts,
                signer_seeds,
            );

            // 执行第一次交换：用借来的资金买入代币
            mock_dex::cpi::swap(cpi_ctx, loan_amount, 0)?;
        }

        msg!("  DEX A 交换完成");

        // 步骤2: 第二次交换 - 在DEX B上用获得的代币换回原始代币以获取利润
        {
            let cpi_accounts = mock_dex::cpi::accounts::Swap {
                pool: ctx.accounts.dex_pool_b.to_account_info(),
                token_in_account: ctx.accounts.user_token_y.to_account_info(), // 现在用Token Y作为输入
                token_x_vault: ctx.accounts.dex_b_token_x_vault.to_account_info(),
                token_y_vault: ctx.accounts.dex_b_token_y_vault.to_account_info(),
                user_token_x: ctx.accounts.user_token_x.to_account_info(),
                user_token_y: ctx.accounts.user_token_y.to_account_info(),
                user_authority: arbitrage_bot.to_account_info(),
                token_program: ctx.accounts.token_program.to_account_info(),
            };

            let seeds = &[
                b"arbitrage_bot".as_ref(),
                &[*ctx.bumps.get("arbitrage_bot").unwrap()]
            ];
            let signer_seeds = &[&seeds[..]];

            let cpi_ctx = CpiContext::new_with_signer(
                ctx.accounts.mock_dex_program.to_account_info(),
                cpi_accounts,
                signer_seeds,
            );

            // 获取当前Token Y余额
            let token_y_balance = ctx.accounts.user_token_y.amount;
            
            // 执行第二次交换：用Token Y换回Token X
            mock_dex::cpi::swap(cpi_ctx, token_y_balance, 0)?;
        }

        msg!("  DEX B 交换完成");

        // 计算实际利润
        let final_token_x_balance = ctx.accounts.user_token_x.amount;
        let actual_profit = final_token_x_balance.saturating_sub(loan_amount);

        msg!("  初始借款: {} lamports", loan_amount);
        msg!("  最终余额: {} lamports", final_token_x_balance);
        msg!("  实际利润: {} lamports", actual_profit);

        // 验证利润是否满足最小要求
        require!(
            actual_profit >= min_expected_profit,
            ErrorCode::InsufficientProfit
        );

        // 更新统计
        arbitrage_bot.total_trades += 1;
        arbitrage_bot.total_profit += actual_profit;
        arbitrage_bot.is_executing = false;

        msg!("✅ ArbitrageBot: 套利完成，利润: {} lamports", actual_profit);

        Ok(actual_profit)
    }

    /// 初始化套利机器人状态
    pub fn initialize_bot(ctx: Context<InitializeBot>) -> Result<()> {
        let arbitrage_bot = &mut ctx.accounts.arbitrage_bot;
        arbitrage_bot.owner = ctx.accounts.owner.key();
        arbitrage_bot.is_executing = false;
        arbitrage_bot.total_trades = 0;
        arbitrage_bot.total_profit = 0;

        msg!("🤖 ArbitrageBot 初始化完成!");
        msg!("  所有者: {}", arbitrage_bot.owner);

        Ok(())
    }
}

#[derive(Accounts)]
pub struct ExecuteArbitrageAtomic<'info> {
    #[account(
        mut,
        seeds = [b"arbitrage_bot"],
        bump,
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
    pub system_program: Program<'info, System>,
}

#[derive(Accounts)]
pub struct InitializeBot<'info> {
    #[account(
        init,
        payer = owner,
        seeds = [b"arbitrage_bot"],
        bump,
        space = ArbitrageBotState::SPACE,
    )]
    pub arbitrage_bot: Account<'info, ArbitrageBotState>,

    #[account(mut)]
    pub owner: Signer<'info>,
    
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
    #[msg("余额不足")]
    InsufficientBalance,
    #[msg("滑点过高")]
    SlippageTooHigh,
    #[msg("检测到重入攻击")]
    ReentrancyDetected,
    #[msg("计算溢出")]
    Overflow,
    #[msg("计算下溢")]
    Underflow,
} 