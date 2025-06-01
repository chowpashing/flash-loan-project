import * as anchor from "@coral-xyz/anchor";
import { Program } from "@coral-xyz/anchor";
import { MockDex } from "../target/types/mock_dex";
import { PublicKey, SystemProgram, SYSVAR_RENT_PUBKEY } from "@solana/web3.js";
import { TOKEN_PROGRAM_ID, createMint, createAccount, mintTo } from "@solana/spl-token";
import { assert } from "chai";
import { BN } from "@coral-xyz/anchor";

describe("mock-dex", () => {
  const provider = anchor.AnchorProvider.env();
  anchor.setProvider(provider);

  const program = anchor.workspace.MockDex as Program<MockDex>;

  // 测试账户
  let tokenXMint: PublicKey;
  let tokenYMint: PublicKey;
  let userTokenXAccount: PublicKey;
  let userTokenYAccount: PublicKey;
  let poolName = "test-pool";
  let mockDexPool: PublicKey;
  let tokenXVault: PublicKey;
  let tokenYVault: PublicKey;
  let mockDexPoolBump: number;
  let tokenXVaultBump: number;
  let tokenYVaultBump: number;

  before(async () => {
    console.log("开始初始化测试环境...");
    
    // 创建代币
    tokenXMint = await createMint(provider.connection, provider.wallet.payer, provider.wallet.publicKey, null, 9);
    tokenYMint = await createMint(provider.connection, provider.wallet.payer, provider.wallet.publicKey, null, 9);

    // 创建用户代币账户
    userTokenXAccount = await createAccount(provider.connection, provider.wallet.payer, tokenXMint, provider.wallet.publicKey);
    userTokenYAccount = await createAccount(provider.connection, provider.wallet.payer, tokenYMint, provider.wallet.publicKey);

    // 铸造代币
    await mintTo(provider.connection, provider.wallet.payer, tokenXMint, userTokenXAccount, provider.wallet.publicKey, 1_000_000_000);
    await mintTo(provider.connection, provider.wallet.payer, tokenYMint, userTokenYAccount, provider.wallet.publicKey, 1_000_000_000);

    // 检查代币账户所有者
    const tokenXAccountInfo = await provider.connection.getAccountInfo(userTokenXAccount);
    const tokenYAccountInfo = await provider.connection.getAccountInfo(userTokenYAccount);
    console.log("Token X Account Owner:", tokenXAccountInfo?.owner.toBase58());
    console.log("Token Y Account Owner:", tokenYAccountInfo?.owner.toBase58());
    console.log("Token Program ID:", TOKEN_PROGRAM_ID.toBase58());

    // 检查代币余额
    const tokenXBalance = await provider.connection.getTokenAccountBalance(userTokenXAccount);
    const tokenYBalance = await provider.connection.getTokenAccountBalance(userTokenYAccount);
    console.log("Token X Balance:", tokenXBalance.value.uiAmount);
    console.log("Token Y Balance:", tokenYBalance.value.uiAmount);

    // 检查 provider wallet 的 SOL 余额
    const solBalance = await provider.connection.getBalance(provider.wallet.publicKey);
    console.log("Provider SOL Balance:", solBalance / 1e9, "SOL");

    // 计算 PDAs
    [mockDexPool, mockDexPoolBump] = PublicKey.findProgramAddressSync(
      [Buffer.from("mock_dex_pool"), Buffer.from(poolName)],
      program.programId
    );

    [tokenXVault, tokenXVaultBump] = PublicKey.findProgramAddressSync(
      [Buffer.from("token_x_vault"), mockDexPool.toBuffer()],
      program.programId
    );

    [tokenYVault, tokenYVaultBump] = PublicKey.findProgramAddressSync(
      [Buffer.from("token_y_vault"), mockDexPool.toBuffer()],
      program.programId
    );
  });

  it("初始化交易池", async () => {
    const initialXAmount = new BN(100_000_000);
    const initialYAmount = new BN(100_000_000);

    await program.methods
      .initializePool(poolName, initialXAmount, initialYAmount)
      .accounts({
        pool: mockDexPool,
        initializer: provider.wallet.publicKey,
        initializerTokenXAccount: userTokenXAccount,
        initializerTokenYAccount: userTokenYAccount,
        tokenXVault,
        tokenYVault,
        tokenXMint,
        tokenYMint,
        tokenProgram: TOKEN_PROGRAM_ID,
        systemProgram: SystemProgram.programId,
        rent: SYSVAR_RENT_PUBKEY,
      } as any)
      .signers([])
      .rpc();

    const poolAccount = await program.account.mockDexPool.fetch(mockDexPool);
    assert.equal(poolAccount.xBalance.toString(), initialXAmount.toString());
    assert.equal(poolAccount.yBalance.toString(), initialYAmount.toString());
    assert.equal(poolAccount.name, poolName);
  });

  it("执行代币交换", async () => {
    const amountIn = new BN(10_000_000);
    const minAmountOut = new BN(9_000_000);

    await program.methods
      .swap(amountIn, minAmountOut, poolName)
      .accounts({
        pool: mockDexPool,
        tokenInAccount: userTokenXAccount,
        tokenXVault,
        tokenYVault,
        userTokenX: userTokenXAccount,
        userTokenY: userTokenYAccount,
        userAuthority: provider.wallet.publicKey,
        tokenProgram: TOKEN_PROGRAM_ID,
      } as any)
      .rpc();

    const poolAccount = await program.account.mockDexPool.fetch(mockDexPool);
    assert.isTrue(poolAccount.xBalance.gt(new BN(0)));
    assert.isTrue(poolAccount.yBalance.gt(new BN(0)));
  });
}); 