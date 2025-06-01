use anchor_lang::prelude::*;

declare_id!("5kPAZ9Gox4F1rnWT3owq5S319A2sG5hdivMGPBg934tW");

#[account]
pub struct FlashLoanState {
    pub loan_id: u64,
    pub borrower: Pubkey,
    pub amount: u64,
    pub fee: u64,
    pub status: LoanStatus,
    pub arbitrage_bot: Option<Pubkey>,
    pub profit: u64,
    pub created_at: i64,
    pub bump: u8,
}

#[account]
pub struct DexTradeState {
    pub trade_id: u64,
    pub trader: Pubkey,
    pub amount_in: u64,
    pub amount_out: u64,
    pub token_in_mint: Pubkey,
    pub token_out_mint: Pubkey,
    pub status: TradeStatus,
    pub expected_min_out: u64,
    pub actual_amount_out: u64,
    pub created_at: i64,
    pub bump: u8,
}

#[account]
pub struct PoolLendingState {
    pub lending_id: u64,
    pub borrower: Pubkey,
    pub amount: u64,
    pub pool_id: Pubkey,
    pub status: LendingStatus,
    pub borrowed_at: i64,
    pub repaid_at: Option<i64>,
    pub interest_rate: u64, // 基点 (bps)
    pub bump: u8,
}

#[account]
pub struct MockPoolState {
    pub pool_id: u64,
    pub balance: u64,
    pub fee_bps: u16,
    pub authority: Pubkey,
    pub total_borrowed: u64,
    pub total_repaid: u64,
    pub active_loans: u64,
    pub created_at: i64,
    pub last_updated: i64,
    pub status: PoolStatus,
    pub bump: u8,
}

#[account]
pub struct TransactionRecord {
    pub transaction_id: u64,
    pub user: Pubkey,
    pub loan_amount: u64,
    pub fee: u64,
    pub profit: u64,
    pub net_profit: u64,
    pub timestamp: i64,
    pub bump: u8,
}

#[derive(AnchorSerialize, AnchorDeserialize, Clone, PartialEq, Eq, Debug)]
pub enum LoanStatus {
    Active,        // 资金已借出，等待套利
    Arbitraging,   // 套利进行中
    Completed,     // 套利完成，可以还款
    Repaid,        // 已还款
    Failed,        // 失败状态
}

#[derive(AnchorSerialize, AnchorDeserialize, Clone, PartialEq, Eq, Debug)]
pub enum TradeStatus {
    Pending,       // 交易等待执行
    Executing,     // 交易执行中
    Completed,     // 交易完成
    Failed,        // 交易失败
}

#[derive(AnchorSerialize, AnchorDeserialize, Clone, PartialEq, Eq, Debug)]
pub enum LendingStatus {
    Requested,     // 借贷请求已创建
    Approved,      // 借贷已批准，资金已转出
    Active,        // 借贷活跃中
    Repaid,        // 已还款
    Defaulted,     // 违约
}

#[derive(AnchorSerialize, AnchorDeserialize, Clone, PartialEq, Eq, Debug)]
pub enum PoolStatus {
    Initializing,  // 池子初始化中
    Active,        // 池子活跃可用
    Paused,        // 池子暂停
    Emergency,     // 紧急状态
    Deprecated,    // 已弃用
}

impl FlashLoanState {
    pub const SPACE: usize = 8 + // discriminator
        8 + // loan_id
        32 + // borrower
        8 + // amount
        8 + // fee
        1 + // status
        1 + 32 + // Option<Pubkey>
        8 + // profit
        8 + // created_at
        1; // bump

    /// 检查是否可以开始套利
    pub fn can_start_arbitrage(&self) -> bool {
        self.status == LoanStatus::Active
    }

    /// 检查是否可以还款
    pub fn can_repay(&self) -> bool {
        matches!(self.status, LoanStatus::Completed | LoanStatus::Active)
    }

    /// 检查是否已完成
    pub fn is_completed(&self) -> bool {
        self.status == LoanStatus::Completed
    }

    /// 获取总还款金额
    pub fn total_repay_amount(&self) -> u64 {
        self.amount + self.fee
    }

    /// 获取净利润
    pub fn net_profit(&self) -> u64 {
        self.profit
    }
}

impl DexTradeState {
    pub const SPACE: usize = 8 + // discriminator
        8 + // trade_id
        32 + // trader
        8 + // amount_in
        8 + // amount_out
        32 + // token_in_mint
        32 + // token_out_mint
        1 + // status
        8 + // expected_min_out
        8 + // actual_amount_out
        8 + // created_at
        1; // bump

    /// 检查交易是否可以执行
    pub fn can_execute(&self) -> bool {
        self.status == TradeStatus::Pending
    }

    /// 检查交易是否完成
    pub fn is_completed(&self) -> bool {
        self.status == TradeStatus::Completed
    }

    /// 检查交易是否失败
    pub fn is_failed(&self) -> bool {
        self.status == TradeStatus::Failed
    }

    /// 计算滑点
    pub fn calculate_slippage(&self) -> u64 {
        if self.expected_min_out > self.actual_amount_out {
            self.expected_min_out - self.actual_amount_out
        } else {
            0
        }
    }
}

impl PoolLendingState {
    pub const SPACE: usize = 8 + // discriminator
        8 + // lending_id
        32 + // borrower
        8 + // amount
        32 + // pool_id
        1 + // status
        8 + // borrowed_at
        1 + 8 + // Option<i64>
        8 + // interest_rate
        1; // bump

    /// 检查是否可以借贷
    pub fn can_lend(&self) -> bool {
        self.status == LendingStatus::Requested
    }

    /// 检查是否可以还款
    pub fn can_repay(&self) -> bool {
        matches!(self.status, LendingStatus::Approved | LendingStatus::Active)
    }

    /// 检查是否已还款
    pub fn is_repaid(&self) -> bool {
        self.status == LendingStatus::Repaid
    }

    /// 计算借贷时长（秒）
    pub fn get_borrow_duration(&self, current_time: i64) -> u64 {
        if let Some(repaid_at) = self.repaid_at {
            (repaid_at - self.borrowed_at) as u64
        } else {
            (current_time - self.borrowed_at) as u64
        }
    }

    /// 计算利息
    pub fn calculate_interest(&self, current_time: i64) -> u64 {
        let duration_secs = self.get_borrow_duration(current_time);
        let duration_hours = duration_secs / 3600; // 转换为小时
        
        // 简单利息计算：amount * rate * time
        // 这里假设 interest_rate 是年化利率的基点
        self.amount
            .checked_mul(self.interest_rate)
            .and_then(|v| v.checked_mul(duration_hours))
            .and_then(|v| v.checked_div(10000))  // 转换基点
            .and_then(|v| v.checked_div(8760))   // 转换为年化 (365 * 24 hours)
            .unwrap_or(0)
    }
}

impl MockPoolState {
    pub const SPACE: usize = 8 + // discriminator
        8 + // pool_id
        8 + // balance
        2 + // fee_bps
        32 + // authority
        8 + // total_borrowed
        8 + // total_repaid
        8 + // active_loans
        8 + // created_at
        8 + // last_updated
        1 + // status
        1; // bump

    /// 检查池子是否可以借贷
    pub fn can_lend(&self) -> bool {
        self.status == PoolStatus::Active
    }

    /// 检查池子是否有足够资金
    pub fn has_sufficient_funds(&self, amount: u64) -> bool {
        self.balance >= amount
    }

    /// 计算借贷费用
    pub fn calculate_fee(&self, amount: u64) -> Result<u64> {
        let fee = (amount as u128)
            .checked_mul(self.fee_bps as u128)
            .and_then(|v| v.checked_div(10_000))
            .map(|v| v as u64);
            
        fee.ok_or_else(|| anchor_lang::error::Error::from(anchor_lang::error::ErrorCode::AccountNotEnoughKeys))
    }

    /// 获取池子利用率（借出资金 / 总资金）
    pub fn get_utilization_rate(&self) -> u64 {
        if self.balance + self.total_borrowed == 0 {
            return 0;
        }
        (self.total_borrowed * 10000) / (self.balance + self.total_borrowed) // 返回基点
    }

    /// 检查是否处于紧急状态
    pub fn is_emergency(&self) -> bool {
        self.status == PoolStatus::Emergency
    }

    /// 更新最后操作时间
    pub fn update_timestamp(&mut self) -> Result<()> {
        self.last_updated = Clock::get()?.unix_timestamp;
        Ok(())
    }
}

impl TransactionRecord {
    pub const SPACE: usize = 8 + // discriminator
        8 + // transaction_id
        32 + // user
        8 + // loan_amount
        8 + // fee
        8 + // profit
        8 + // net_profit
        8 + // timestamp
        1; // bump

    /// 计算投资回报率（ROI）基点
    pub fn calculate_roi_bps(&self) -> u64 {
        if self.loan_amount == 0 {
            return 0;
        }
        // ROI = (净利润 / 借款金额) * 10000 (基点)
        (self.net_profit * 10000) / self.loan_amount
    }

    /// 计算有效年化收益率（假设操作时间为分钟级）
    pub fn calculate_annualized_return(&self, operation_duration_minutes: u64) -> u64 {
        if operation_duration_minutes == 0 || self.loan_amount == 0 {
            return 0;
        }
        
        let roi_per_minute = self.net_profit / self.loan_amount;
        let minutes_per_year = 365 * 24 * 60;
        
        roi_per_minute * minutes_per_year
    }

    /// 检查交易是否盈利
    pub fn is_profitable(&self) -> bool {
        self.net_profit > 0
    }
}

#[derive(Accounts)]
pub struct DummyAccounts<'info> {
    pub signer: Signer<'info>,
}

#[program]
pub mod shared {
    use super::*;
    
    // 空程序，仅用于生成 IDL
    pub fn dummy(_ctx: Context<DummyAccounts>) -> Result<()> {
        Ok(())
    }
} 