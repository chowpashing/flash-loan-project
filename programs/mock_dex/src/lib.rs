use anchor_lang::prelude::*;
use anchor_spl::token::{self, Mint, Token, TokenAccount, Transfer};
use shared::{DexTradeState, TradeStatus};

// 确保这里的 Program ID 与你 build 后生成的实际 ID 匹配
declare_id!("D5CLaTtb5iGTUC7JfaSK9tUVCbjQsEmWvyqmNCPjWQJu");

#[program]
pub mod mock_dex {
    use super::*;

    /// 初始化一个模拟的流动性池 (DEX Instance)
    /// 每个池子由一个唯一的 `pool_name` 字符串区分
    pub fn initialize_pool(
        ctx: Context<InitializePool>,
        pool_name: String, // 用于区分不同池子的唯一名称
        initial_x_amount: u64,
        initial_y_amount: u64,
    ) -> Result<()> {
        // 验证 pool_name 长度
        require!(!pool_name.is_empty() && pool_name.len() <= 32, ErrorCode::InvalidPoolName);

        // 验证初始金额
        require!(initial_x_amount > 0 && initial_y_amount > 0, ErrorCode::InvalidPoolName);

        // 验证初始化者的代币余额
        require!(
            ctx.accounts.initializer_token_x_account.amount >= initial_x_amount,
            ErrorCode::InsufficientLiquidity
        );
        require!(
            ctx.accounts.initializer_token_y_account.amount >= initial_y_amount,
            ErrorCode::InsufficientLiquidity
        );

        let pool = &mut ctx.accounts.pool;
        
        // 验证代币账户所有者
        require!(
            ctx.accounts.token_x_vault.owner == ctx.accounts.token_program.key(),
            ErrorCode::InvalidTokenAccountOwner
        );
        require!(
            ctx.accounts.token_y_vault.owner == ctx.accounts.token_program.key(),
            ErrorCode::InvalidTokenAccountOwner
        );

        // 将初始流动性从 initializer 转移到 DEX 的 Vaults
        // 转移 Token X
        token::transfer(
            CpiContext::new(
                ctx.accounts.token_program.to_account_info(),
                Transfer {
                    from: ctx.accounts.initializer_token_x_account.to_account_info(),
                    to: ctx.accounts.token_x_vault.to_account_info(),
                    authority: ctx.accounts.initializer.to_account_info(),
                },
            ),
            initial_x_amount,
        )?;

        // 转移 Token Y
        token::transfer(
            CpiContext::new(
                ctx.accounts.token_program.to_account_info(),
                Transfer {
                    from: ctx.accounts.initializer_token_y_account.to_account_info(),
                    to: ctx.accounts.token_y_vault.to_account_info(),
                    authority: ctx.accounts.initializer.to_account_info(),
                },
            ),
            initial_y_amount,
        )?;

        // 设置池子余额（在实际转移成功后）
        pool.x_balance = initial_x_amount;
        pool.y_balance = initial_y_amount;
        pool.name = pool_name.clone();

        // 发送事件
        emit!(PoolInitialized {
            pool_name: pool_name.clone(),
            initial_x_amount,
            initial_y_amount,
            initializer: ctx.accounts.initializer.key(),
            timestamp: Clock::get()?.unix_timestamp,
        });

        msg!("Initialized DEX pool '{}' with X: {} and Y: {}", pool_name, initial_x_amount, initial_y_amount);
        Ok(())
    }

    /// 处理来自共享状态的交易请求 - 独立于套利机器人
    pub fn execute_trade_from_state(
        ctx: Context<ExecuteTradeFromState>,
        trade_id: u64,
    ) -> Result<()> {
        let dex_trade_state = &mut ctx.accounts.dex_trade_state;
        let pool = &mut ctx.accounts.pool;
        
        // 验证交易状态
        require!(
            dex_trade_state.can_execute(),
            ErrorCode::InvalidTradeStatus
        );
        
        require!(
            dex_trade_state.trade_id == trade_id,
            ErrorCode::InvalidTradeId
        );
        
        // 更新状态为执行中
        dex_trade_state.status = TradeStatus::Executing;
        
        let amount_in = dex_trade_state.amount_in;
        
        // 简单价格计算：1:1兑换比率，减去0.3%的手续费
        let fee_bps = 30; // 0.3% 手续费
        let amount_out = amount_in
            .checked_mul(10000 - fee_bps)
            .ok_or(ErrorCode::Overflow)?
            .checked_div(10000)
            .ok_or(ErrorCode::Underflow)?;

        // 确保池子有足够的资金转出
        require!(pool.y_balance >= amount_out, ErrorCode::InsufficientLiquidity);

        // 模拟交易执行 - 在实际应用中这里会进行真实的代币转移
        // 这里我们只是更新状态
        pool.x_balance = pool.x_balance.checked_add(amount_in).ok_or(ErrorCode::Overflow)?;
        pool.y_balance = pool.y_balance.checked_sub(amount_out).ok_or(ErrorCode::Underflow)?;
        
        // 更新交易状态
        dex_trade_state.actual_amount_out = amount_out;
        dex_trade_state.status = TradeStatus::Completed;

        // 发送事件
        emit!(TradeExecutedFromState {
            trade_id,
            amount_in,
            amount_out,
            trader: dex_trade_state.trader,
            pool_name: pool.name.clone(),
            timestamp: Clock::get()?.unix_timestamp,
        });

        msg!("Executed trade {} with amount_in: {} and amount_out: {}", trade_id, amount_in, amount_out);
        Ok(())
    }

    /// 模拟兑换功能 - 保持向后兼容
    /// 简化：这里使用一个简化的固定费率模型，不模拟真实AMM曲线
    /// 允许在 Token X 和 Token Y 之间互换
    pub fn swap(
        ctx: Context<Swap>,
        amount_in: u64, // 卖出多少
        min_amount_out: u64, // 至少得到多少 (用于滑点保护，但这里简化实现)
    ) -> Result<()> {
        let pool = &mut ctx.accounts.pool;
        let token_program = &ctx.accounts.token_program;

        // 复制 pool 的名称和 bump，避免借用冲突
        let pool_name = pool.name.clone();
        let pool_bump = *ctx.bumps.get("pool").unwrap();

        require!(!pool_name.is_empty(), ErrorCode::InvalidPoolName);

        // 检查 token_in_account 是 X 还是 Y
        let from_token_account = &ctx.accounts.token_in_account;
        let input_is_x = from_token_account.mint == ctx.accounts.user_token_x.mint;
        let input_is_y = from_token_account.mint == ctx.accounts.user_token_y.mint;

        require!(input_is_x || input_is_y, ErrorCode::InvalidTokenInAccount);

        let to_token_account = if input_is_x {
            &ctx.accounts.user_token_y // 卖出 X 得到 Y
        } else {
            &ctx.accounts.user_token_x // 卖出 Y 得到 X
        };

        // 简单价格计算：1:1兑换比率，减去0.3%的手续费
        let fee_bps = 30; // 0.3% 手续费
        let amount_out = amount_in
            .checked_mul(10000 - fee_bps)
            .ok_or(ErrorCode::Overflow)?
            .checked_div(10000)
            .ok_or(ErrorCode::Underflow)?;

        require!(amount_out >= min_amount_out, ErrorCode::SlippageTooHigh);

        // 确保池子有足够的资金转出
        if input_is_x { // 卖出 X 换 Y
            require!(pool.y_balance >= amount_out, ErrorCode::InsufficientLiquidity);
        } else { // 卖出 Y 换 X
            require!(pool.x_balance >= amount_out, ErrorCode::InsufficientLiquidity);
        }

        // 1. 从用户账户转入到 DEX Vault
        token::transfer(
            CpiContext::new(
                token_program.to_account_info(),
                Transfer {
                    from: from_token_account.to_account_info(), // 用户卖出的Token
                    to: if input_is_x { ctx.accounts.token_x_vault.to_account_info() } else { ctx.accounts.token_y_vault.to_account_info() }, // 对应DEX Vault
                    authority: ctx.accounts.user_authority.to_account_info(), // 用户的签名 authority
                },
            ),
            amount_in,
        )?;

        // 更新池子内部余额
        if input_is_x { // 卖出 X 换 Y
            pool.x_balance = pool.x_balance.checked_add(amount_in).ok_or(ErrorCode::Overflow)?;
            pool.y_balance = pool.y_balance.checked_sub(amount_out).ok_or(ErrorCode::Underflow)?;
        } else { // 卖出 Y 换 X
            pool.y_balance = pool.y_balance.checked_add(amount_in).ok_or(ErrorCode::Overflow)?;
            pool.x_balance = pool.x_balance.checked_sub(amount_out).ok_or(ErrorCode::Underflow)?;
        }

        // 2. 从 DEX Vault 转出到用户账户 (DEX PDA 签名)
        let pool_seeds = &[
            b"mock_dex_pool".as_ref(),
            pool_name.as_bytes(), // 使用复制的池子名字作为种子
            &[pool_bump]
        ];
        let signer_seeds = &[&pool_seeds[..]];

        token::transfer(
            CpiContext::new_with_signer(
                token_program.to_account_info(),
                Transfer {
                    from: if input_is_x { ctx.accounts.token_y_vault.to_account_info() } else { ctx.accounts.token_x_vault.to_account_info() }, // 对应DEX Vault
                    to: to_token_account.to_account_info(), // 用户接收的Token
                    authority: ctx.accounts.pool.to_account_info(), // DEX的Pool PDA作为Vault的authority
                },
                signer_seeds,
            ),
            amount_out,
        )?;

        // 发送事件
        emit!(SwapExecuted {
            pool_name: pool_name.clone(),
            amount_in,
            amount_out,
            user: ctx.accounts.user_authority.key(),
            timestamp: Clock::get()?.unix_timestamp,
        });

        msg!("Swapped {} for {} on DEX '{}'", amount_in, amount_out, pool_name);
        Ok(())
    }
}

// ---------------------------------------------------------------- //
//                          账户定义                               //
// ---------------------------------------------------------------- //

#[derive(Accounts)]
#[instruction(pool_name: String, initial_x_amount: u64, initial_y_amount: u64)]
pub struct InitializePool<'info> {
    #[account(
        init,
        payer = initializer,
        seeds = [b"mock_dex_pool", pool_name.as_bytes()], // 使用 pool_name 作为 PDA 种子
        bump,
        space = 8 + 8 + 8 + 32, // Discriminator + x_balance + y_balance + name (max 32 bytes)
    )]
    pub pool: Account<'info, MockDexPool>,

    #[account(mut)]
    pub initializer: Signer<'info>, // 支付租金和提供初始流动性的签名者

    #[account(mut)]
    pub initializer_token_x_account: Account<'info, TokenAccount>, // 初始流动性Token X的来源
    #[account(mut)]
    pub initializer_token_y_account: Account<'info, TokenAccount>, // 初始流动性Token Y的来源

    #[account(
        init,
        payer = initializer,
        token::mint = token_x_mint,
        token::authority = pool, // pool PDA 是 token_x_vault 的 authority
    )]
    pub token_x_vault: Account<'info, TokenAccount>,
    #[account(
        init,
        payer = initializer,
        token::mint = token_y_mint,
        token::authority = pool, // pool PDA 是 token_y_vault 的 authority
    )]
    pub token_y_vault: Account<'info, TokenAccount>,

    pub token_x_mint: Account<'info, Mint>,
    pub token_y_mint: Account<'info, Mint>,

    pub token_program: Program<'info, Token>,
    pub system_program: Program<'info, System>,
    pub rent: Sysvar<'info, Rent>,
}

#[derive(Accounts)]
#[instruction(trade_id: u64)]
pub struct ExecuteTradeFromState<'info> {
    #[account(
        mut,
        seeds = [b"dex_trade_state", trade_id.to_le_bytes().as_ref()],
        bump = dex_trade_state.bump
    )]
    pub dex_trade_state: Account<'info, DexTradeState>,

    #[account(
        mut,
        seeds = [b"mock_dex_pool", pool.name.as_bytes()],
        bump,
    )]
    pub pool: Account<'info, MockDexPool>,

    pub system_program: Program<'info, System>,
}

#[derive(Accounts)]
pub struct Swap<'info> {
    #[account(
        mut,
        seeds = [b"mock_dex_pool", pool.name.as_bytes()], // 使用 pool 的名字作为种子
        bump,
    )]
    pub pool: Account<'info, MockDexPool>,

    /// CHECK: 用户的输入Token账户 (可以是 Token X 或 Token Y)
    /// 必须是 mut 因为会从中转出Token
    #[account(mut)]
    pub token_in_account: Account<'info, TokenAccount>,

    /// CHECK: DEX 的 Token X Vault
    #[account(mut)]
    pub token_x_vault: Account<'info, TokenAccount>,
    /// CHECK: DEX 的 Token Y Vault
    #[account(mut)]
    pub token_y_vault: Account<'info, TokenAccount>,

    /// CHECK: 用户的 Token X 账户 (可能用于接收或发送)
    #[account(mut)]
    pub user_token_x: Account<'info, TokenAccount>,
    /// CHECK: 用户的 Token Y 账户 (可能用于接收或发送)
    #[account(mut)]
    pub user_token_y: Account<'info, TokenAccount>,

    /// 用户的签名 authority (例如：套利机器人 PDA)
    /// 这个账户必须签名从 `token_in_account` 到 `DEX Vault` 的转账
    pub user_authority: Signer<'info>,

    pub token_program: Program<'info, Token>,
}

#[account]
pub struct MockDexPool {
    pub x_balance: u64,
    pub y_balance: u64,
    pub name: String, // 存储池子名称，用于PDA种子和区分
}

#[event]
pub struct SwapExecuted {
    pub pool_name: String,
    pub amount_in: u64,
    pub amount_out: u64,
    pub user: Pubkey,
    pub timestamp: i64,
}

#[event]
pub struct PoolInitialized {
    pub pool_name: String,
    pub initial_x_amount: u64,
    pub initial_y_amount: u64,
    pub initializer: Pubkey,
    pub timestamp: i64,
}

#[event]
pub struct TradeExecutedFromState {
    pub trade_id: u64,
    pub amount_in: u64,
    pub amount_out: u64,
    pub trader: Pubkey,
    pub pool_name: String,
    pub timestamp: i64,
}

#[error_code]
pub enum ErrorCode {
    #[msg("Invalid pool name provided.")]
    InvalidPoolName,
    #[msg("Insufficient liquidity in DEX pool for this swap.")]
    InsufficientLiquidity,
    #[msg("Slippage too high. Amount out is less than min_amount_out.")]
    SlippageTooHigh,
    #[msg("Invalid input token account for swap.")]
    InvalidTokenInAccount,
    #[msg("Calculation overflow.")]
    Overflow,
    #[msg("Calculation underflow.")]
    Underflow,
    #[msg("Invalid token account owner.")]
    InvalidTokenAccountOwner,
    #[msg("Invalid pool authority.")]
    InvalidPoolAuthority,
    #[msg("Invalid trade status.")]
    InvalidTradeStatus,
    #[msg("Invalid trade ID.")]
    InvalidTradeId,
}