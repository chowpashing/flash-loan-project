use anchor_lang::prelude::*;
use anchor_spl::token::{self, Mint, Token, TokenAccount, Transfer};

// 确保这里的 Program ID 与你 build 后生成的实际 ID 匹配
declare_id!("CP8F2b4Dh43ovvwJ6MBYXx9gKuFZ4zFvw9y74Ahk2wy6");

#[program]
pub mod mock_dex {
    use super::*;

    /// 初始化一个模拟的流动性池 (DEX Instance)
    /// 每个池子由一个唯一的 `pool_name` 字符串区分
    /// 遵循CEI模式：Check-Effects-Interactions
    pub fn initialize_pool(
        ctx: Context<InitializePool>,
        pool_name: String, // 用于区分不同池子的唯一名称
        initial_x_amount: u64,
        initial_y_amount: u64,
    ) -> Result<()> {
        let pool = &mut ctx.accounts.pool;
        
        // === CHECK 阶段：所有验证和检查 ===
        
        // 验证 pool_name 长度
        require!(!pool_name.is_empty() && pool_name.len() <= 32, ErrorCode::InvalidPoolName);

        // 验证初始金额
        require!(initial_x_amount > 0 && initial_y_amount > 0, ErrorCode::InvalidAmount);

        // 验证初始化者的代币余额
        require!(
            ctx.accounts.initializer_token_x_account.amount >= initial_x_amount,
            ErrorCode::InsufficientLiquidity
        );
        require!(
            ctx.accounts.initializer_token_y_account.amount >= initial_y_amount,
            ErrorCode::InsufficientLiquidity
        );
        
        // 验证代币账户所有者
        require!(
            ctx.accounts.initializer_token_x_account.owner == ctx.accounts.initializer.key(),
            ErrorCode::InvalidTokenAccountOwner
        );
        require!(
            ctx.accounts.initializer_token_y_account.owner == ctx.accounts.initializer.key(),
            ErrorCode::InvalidTokenAccountOwner
        );

        // === EFFECTS 阶段：更新所有状态 ===
        
        // 设置池子状态（在转账之前）
        pool.x_balance = initial_x_amount;
        pool.y_balance = initial_y_amount;
        pool.name = pool_name.clone();

        msg!("🏊‍♀️ Pool状态已设置: '{}' with X: {}, Y: {}", pool_name, initial_x_amount, initial_y_amount);

        // === INTERACTIONS 阶段：所有外部调用 ===
        
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

        msg!("📥 Token X 转移完成: {}", initial_x_amount);

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

        msg!("📥 Token Y 转移完成: {}", initial_y_amount);

        // 发送事件
        emit!(PoolInitialized {
            pool_name: pool_name.clone(),
            initial_x_amount,
            initial_y_amount,
            initializer: ctx.accounts.initializer.key(),
            timestamp: Clock::get()?.unix_timestamp,
        });

        msg!("✅ Initialized DEX pool '{}' with X: {} and Y: {}", pool_name, initial_x_amount, initial_y_amount);
        Ok(())
    }

    /// 真正的AMM兑换功能 - 使用恒定乘积模型 (x * y = k)
    /// 遵循CEI模式：Check-Effects-Interactions
    pub fn swap(
        ctx: Context<Swap>,
        amount_in: u64, // 卖出多少
        min_amount_out: u64, // 至少得到多少 (滑点保护)
        pool_name: String, // 池子名称
    ) -> Result<()> {
        let pool = &mut ctx.accounts.pool;
        let token_program = &ctx.accounts.token_program;

        // 复制 pool 的名称和 bump，避免借用冲突
        let pool_bump = ctx.bumps.pool;

        // === CHECK 阶段：所有验证和检查 ===
        
        require!(!pool_name.is_empty(), ErrorCode::InvalidPoolName);
        require!(amount_in > 0, ErrorCode::InvalidAmount);

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

        // AMM 恒定乘积计算 (x * y = k)
        let (reserve_in, reserve_out) = if input_is_x {
            (pool.x_balance, pool.y_balance)
        } else {
            (pool.y_balance, pool.x_balance)
        };

        // 检查流动性
        require!(reserve_in > 0 && reserve_out > 0, ErrorCode::InsufficientLiquidity);

        // 计算手续费 (0.3% = 30 bps)
        let fee_bps = 30u64;
        let amount_in_with_fee = amount_in
            .checked_mul(10000 - fee_bps)
            .ok_or(ErrorCode::Overflow)?;

        // AMM 恒定乘积公式计算输出
        let numerator = amount_in_with_fee
            .checked_mul(reserve_out)
            .ok_or(ErrorCode::Overflow)?;
        
        let denominator = reserve_in
            .checked_mul(10000)
            .ok_or(ErrorCode::Overflow)?
            .checked_add(amount_in_with_fee)
            .ok_or(ErrorCode::Overflow)?;

        let amount_out = numerator
            .checked_div(denominator)
            .ok_or(ErrorCode::Underflow)?;

        // 滑点保护：确保输出不少于最小预期
        require!(amount_out >= min_amount_out, ErrorCode::SlippageTooHigh);

        // 确保池子有足够的储备
        require!(amount_out < reserve_out, ErrorCode::InsufficientLiquidity);

        // 计算价格影响 (用于事件记录)
        let price_before = if reserve_in > 0 { 
            (reserve_out * 10000) / reserve_in 
        } else { 
            0 
        };
        
        let new_reserve_in = reserve_in + amount_in;
        let new_reserve_out = reserve_out - amount_out;
        let price_after = if new_reserve_in > 0 { 
            (new_reserve_out * 10000) / new_reserve_in 
        } else { 
            0 
        };

        let price_impact_bps = if price_before > 0 {
            ((price_before.max(price_after) - price_before.min(price_after)) * 10000) / price_before
        } else {
            0
        };

        // === EFFECTS 阶段：更新所有状态 ===
        
        // 更新池子储备状态（在所有外部转账之前）
        if input_is_x {
            pool.x_balance = pool.x_balance.checked_add(amount_in).ok_or(ErrorCode::Overflow)?;
            pool.y_balance = pool.y_balance.checked_sub(amount_out).ok_or(ErrorCode::Underflow)?;
        } else {
            pool.y_balance = pool.y_balance.checked_add(amount_in).ok_or(ErrorCode::Overflow)?;
            pool.x_balance = pool.x_balance.checked_sub(amount_out).ok_or(ErrorCode::Underflow)?;
        }

        msg!("💰 Pool状态已更新: X={}, Y={}", pool.x_balance, pool.y_balance);

        // === INTERACTIONS 阶段：所有外部调用 ===
        
        // 1. 从用户账户转入到 DEX Vault
        token::transfer(
            CpiContext::new(
                token_program.to_account_info(),
                Transfer {
                    from: from_token_account.to_account_info(),
                    to: if input_is_x { 
                        ctx.accounts.token_x_vault.to_account_info() 
                    } else { 
                        ctx.accounts.token_y_vault.to_account_info() 
                    },
                    authority: ctx.accounts.user_authority.to_account_info(),
                },
            ),
            amount_in,
        )?;

        msg!("📥 转入完成: {} tokens", amount_in);

        // 2. 从 DEX Vault 转出到用户账户
        let pool_seeds = &[
            b"mock_dex_pool".as_ref(),
            pool_name.as_bytes(),
            &[pool_bump]
        ];
        let signer_seeds = &[&pool_seeds[..]];

        token::transfer(
            CpiContext::new_with_signer(
                token_program.to_account_info(),
                Transfer {
                    from: if input_is_x { 
                        ctx.accounts.token_y_vault.to_account_info() 
                    } else { 
                        ctx.accounts.token_x_vault.to_account_info() 
                    },
                    to: to_token_account.to_account_info(),
                    authority: ctx.accounts.pool.to_account_info(),
                },
                signer_seeds,
            ),
            amount_out,
        )?;

        msg!("📤 转出完成: {} tokens", amount_out);

        // 发送增强事件
        emit!(SwapExecuted {
            pool_name: pool_name.clone(),
            amount_in,
            amount_out,
            price_impact_bps,
            user: ctx.accounts.user_authority.key(),
            timestamp: Clock::get()?.unix_timestamp,
        });

        msg!(
            "✅ AMM Swap: {} -> {} (滑点: {}bps) on DEX '{}'", 
            amount_in, 
            amount_out, 
            price_impact_bps,
            pool_name
        );
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
        seeds = [b"mock_dex_pool", pool_name.as_bytes()],
        bump,
        space = 8 + 8 + 8 + 32,
    )]
    pub pool: Account<'info, MockDexPool>,

    #[account(mut)]
    pub initializer: Signer<'info>,

    #[account(mut)]
    pub initializer_token_x_account: Account<'info, TokenAccount>,
    #[account(mut)]
    pub initializer_token_y_account: Account<'info, TokenAccount>,

    #[account(
        init,
        payer = initializer,
        seeds = [b"token_x_vault", pool.key().as_ref()],
        bump,
        token::mint = token_x_mint,
        token::authority = pool,
    )]
    pub token_x_vault: Account<'info, TokenAccount>,
    #[account(
        init,
        payer = initializer,
        seeds = [b"token_y_vault", pool.key().as_ref()],
        bump,
        token::mint = token_y_mint,
        token::authority = pool,
    )]
    pub token_y_vault: Account<'info, TokenAccount>,

    pub token_x_mint: Account<'info, Mint>,
    pub token_y_mint: Account<'info, Mint>,

    pub token_program: Program<'info, Token>,
    pub system_program: Program<'info, System>,
    pub rent: Sysvar<'info, Rent>,
}

#[derive(Accounts)]
#[instruction(amount_in: u64, min_amount_out: u64, pool_name: String)]
pub struct Swap<'info> {
    #[account(
        mut,
        seeds = [b"mock_dex_pool", pool_name.as_bytes()], // 使用传入的 pool_name 作为种子
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
    pub price_impact_bps: u64,
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
    #[msg("Invalid amount provided.")]
    InvalidAmount,
}