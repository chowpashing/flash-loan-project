import * as anchor from "@coral-xyz/anchor";
import { Program, BN } from "@coral-xyz/anchor";
import { PublicKey, Keypair, SystemProgram, LAMPORTS_PER_SOL } from "@solana/web3.js";
import { expect } from "chai";

describe("Flash Loan + Arbitrage Bot 共享状态集成测试", () => {
  const provider = anchor.AnchorProvider.env();
  anchor.setProvider(provider);

  // 加载程序
  const flashLoanProgram = anchor.workspace.FlashLoan;
  const arbitrageBotProgram = anchor.workspace.ArbitrageBot;
  const mockPoolProgram = anchor.workspace.MockPool;

  let userKeypair: Keypair;
  let mockPoolStatePda: PublicKey;
  let flashLoanStatePda: PublicKey;
  let poolLendingStatePda: PublicKey;
  let arbitrageBotStatePda: PublicKey;

  before(async () => {
    // 创建用户账户
    userKeypair = Keypair.generate();
    
    // 给用户空投SOL
    const signature = await provider.connection.requestAirdrop(
      userKeypair.publicKey,
      10 * LAMPORTS_PER_SOL
    );
    await provider.connection.confirmTransaction(signature);

    // 获取mock pool state的PDA
    [mockPoolStatePda] = PublicKey.findProgramAddressSync(
      [Buffer.from("mock_pool_state")],
      mockPoolProgram.programId
    );

    // 初始化mock pool状态
    const initialBalance = new BN(5 * LAMPORTS_PER_SOL); // 5 SOL
    const feeBps = 100; // 1%

    try {
      await mockPoolProgram.methods
        .initialize(initialBalance, feeBps)
        .accounts({
          poolState: mockPoolStatePda,
          authority: userKeypair.publicKey,
          systemProgram: SystemProgram.programId,
        })
        .signers([userKeypair])
        .rpc();
      
      console.log("✅ Mock Pool State 初始化成功");
    } catch (error) {
      console.log("Pool可能已经初始化:", error.message);
    }

    // 获取flash loan state PDA
    [flashLoanStatePda] = PublicKey.findProgramAddressSync(
      [Buffer.from("flash_loan_state"), userKeypair.publicKey.toBuffer()],
      flashLoanProgram.programId
    );

    // 获取pool lending state PDA
    [poolLendingStatePda] = PublicKey.findProgramAddressSync(
      [
        Buffer.from("pool_lending_state"), 
        userKeypair.publicKey.toBuffer(),
        mockPoolStatePda.toBuffer()
      ],
      flashLoanProgram.programId
    );

    // 获取arbitrage bot state PDA
    [arbitrageBotStatePda] = PublicKey.findProgramAddressSync(
      [Buffer.from("arbitrage_bot")],
      arbitrageBotProgram.programId
    );
  });

  it("查询Mock Pool初始状态", async () => {
    try {
      await mockPoolProgram.methods
        .getPoolInfo()
        .accounts({
          poolState: mockPoolStatePda,
        })
        .rpc();

      // 也可以直接获取账户数据
      const poolStateAccount = await mockPoolProgram.account.mockPoolState.fetch(mockPoolStatePda);
      console.log("池子状态:", poolStateAccount);

      expect(poolStateAccount.balance.toNumber()).to.be.greaterThan(0);
      expect(poolStateAccount.feeBps).to.equal(100);
      expect(poolStateAccount.activeLoans.toNumber()).to.equal(0);
      
      console.log("✅ 池子状态查询成功");
    } catch (error) {
      console.error("池子状态查询失败:", error);
      throw error;
    }
  });

  it("测试Flash Loan基本功能 - 通过共享状态", async () => {
    const loanAmount = new BN(1 * LAMPORTS_PER_SOL); // 1 SOL

    console.log("开始执行闪电贷...");
    
    // 执行闪电贷
    try {
      const tx = await flashLoanProgram.methods
        .flashLoan(loanAmount)
        .accounts({
          mockPoolState: mockPoolStatePda,
          user: userKeypair.publicKey,
          flashLoanState: flashLoanStatePda,
          poolLendingState: poolLendingStatePda,
          systemProgram: SystemProgram.programId,
        })
        .signers([userKeypair])
        .rpc();

      console.log("闪电贷交易签名:", tx);

      // 验证闪电贷状态
      const flashLoanState = await flashLoanProgram.account.flashLoanState.fetch(flashLoanStatePda);
      console.log("闪电贷状态:", flashLoanState);

      // 验证池子借贷状态
      const poolLendingState = await flashLoanProgram.account.poolLendingState.fetch(poolLendingStatePda);
      console.log("池子借贷状态:", poolLendingState);

      // 验证池子状态更新
      const poolState = await mockPoolProgram.account.mockPoolState.fetch(mockPoolStatePda);
      console.log("更新后的池子状态:", poolState);

      expect(flashLoanState.amount.toString()).to.equal(loanAmount.toString());
      expect(flashLoanState.borrower.toString()).to.equal(userKeypair.publicKey.toString());
      expect(poolLendingState.amount.toString()).to.equal(loanAmount.toString());
      expect(poolState.activeLoans.toNumber()).to.equal(1);
      
      console.log("✅ 闪电贷创建成功");
    } catch (error) {
      console.error("闪电贷失败:", error);
      throw error;
    }
  });

  it("测试Arbitrage Bot初始化", async () => {
    try {
      // 初始化套利机器人
      const tx = await arbitrageBotProgram.methods
        .initializeBot()
        .accounts({
          arbitrageBot: arbitrageBotStatePda,
          owner: userKeypair.publicKey,
          systemProgram: SystemProgram.programId,
        })
        .signers([userKeypair])
        .rpc();

      console.log("套利机器人初始化交易签名:", tx);

      // 验证套利机器人状态
      const arbBotState = await arbitrageBotProgram.account.arbitrageBotState.fetch(arbitrageBotStatePda);
      console.log("套利机器人状态:", arbBotState);

      expect(arbBotState.owner.toString()).to.equal(userKeypair.publicKey.toString());
      expect(arbBotState.totalTrades.toNumber()).to.equal(0);
      expect(arbBotState.totalProfit.toNumber()).to.equal(0);
      
      console.log("✅ 套利机器人初始化成功");
    } catch (error) {
      console.error("套利机器人初始化失败:", error);
      throw error;
    }
  });

  it("测试Flash Loan状态查询", async () => {
    try {
      // 查询闪电贷状态
      await flashLoanProgram.methods
        .getFlashLoanState(userKeypair.publicKey)
        .accounts({
          flashLoanState: flashLoanStatePda,
        })
        .rpc();

      console.log("✅ 闪电贷状态查询成功");
    } catch (error) {
      console.error("闪电贷状态查询失败:", error);
      throw error;
    }
  });

  it("测试Flash Loan还款 - 通过共享状态", async () => {
    try {
      // 偿还闪电贷
      const tx = await flashLoanProgram.methods
        .repayFlashLoan()
        .accounts({
          mockPoolState: mockPoolStatePda,
          user: userKeypair.publicKey,
          flashLoanState: flashLoanStatePda,
          poolLendingState: poolLendingStatePda,
          systemProgram: SystemProgram.programId,
        })
        .signers([userKeypair])
        .rpc();

      console.log("闪电贷还款交易签名:", tx);

      // 验证还款后的状态
      const flashLoanState = await flashLoanProgram.account.flashLoanState.fetch(flashLoanStatePda);
      console.log("还款后闪电贷状态:", flashLoanState);

      const poolLendingState = await flashLoanProgram.account.poolLendingState.fetch(poolLendingStatePda);
      console.log("还款后池子借贷状态:", poolLendingState);

      const poolState = await mockPoolProgram.account.mockPoolState.fetch(mockPoolStatePda);
      console.log("还款后池子状态:", poolState);

      // 检查状态是否为已还款
      expect(flashLoanState.status).to.have.property("repaid");
      expect(poolLendingState.status).to.have.property("repaid");
      expect(poolState.activeLoans.toNumber()).to.equal(0);
      
      console.log("✅ 闪电贷还款成功");
    } catch (error) {
      console.error("闪电贷还款失败:", error);
      throw error;
    }
  });

  it("测试Pool管理功能", async () => {
    try {
      // 测试紧急暂停
      await mockPoolProgram.methods
        .emergencyPause()
        .accounts({
          poolState: mockPoolStatePda,
          authority: userKeypair.publicKey,
        })
        .signers([userKeypair])
        .rpc();

      let poolState = await mockPoolProgram.account.mockPoolState.fetch(mockPoolStatePda);
      console.log("暂停后池子状态:", poolState.status);

      // 测试恢复池子
      await mockPoolProgram.methods
        .resumePool()
        .accounts({
          poolState: mockPoolStatePda,
          authority: userKeypair.publicKey,
        })
        .signers([userKeypair])
        .rpc();

      poolState = await mockPoolProgram.account.mockPoolState.fetch(mockPoolStatePda);
      console.log("恢复后池子状态:", poolState.status);

      console.log("✅ 池子管理功能测试成功");
    } catch (error) {
      console.error("池子管理功能测试失败:", error);
      throw error;
    }
  });

  it("检查账户余额变化", async () => {
    // 检查用户账户余额
    const userBalance = await provider.connection.getBalance(userKeypair.publicKey);
    console.log("用户当前余额:", userBalance / LAMPORTS_PER_SOL, "SOL");

    // 检查池子账户余额
    const poolBalance = await provider.connection.getBalance(mockPoolStatePda);
    console.log("池子当前余额:", poolBalance / LAMPORTS_PER_SOL, "SOL");

    // 检查池子状态中记录的余额
    const poolState = await mockPoolProgram.account.mockPoolState.fetch(mockPoolStatePda);
    console.log("池子状态记录的余额:", poolState.balance.toNumber() / LAMPORTS_PER_SOL, "SOL");
    console.log("总借出金额:", poolState.totalBorrowed.toNumber() / LAMPORTS_PER_SOL, "SOL");
    console.log("总还款金额:", poolState.totalRepaid.toNumber() / LAMPORTS_PER_SOL, "SOL");

    console.log("✅ 余额检查完成");
  });
}); 