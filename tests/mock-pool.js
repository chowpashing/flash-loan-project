const anchor = require("@coral-xyz/anchor");
const assert = require("assert");

describe("mock_pool", () => {
  const provider = anchor.AnchorProvider.env();
  anchor.setProvider(provider);
  const program = anchor.workspace.MockPool;

  let poolAccount = anchor.web3.Keypair.generate();

  it("Initializes pool", async () => {
    await program.methods
      .initialize(new anchor.BN(1_000_000), 100) // 100万 lamports, 1% fee
      .accounts({
        pool: poolAccount.publicKey,
        authority: provider.wallet.publicKey,
        systemProgram: anchor.web3.SystemProgram.programId,
      })
      .signers([poolAccount])
      .rpc();

    const pool = await program.account.pool.fetch(poolAccount.publicKey);
    console.log('Pool object:', pool);
    console.log('Pool fields:', {
      balance: pool.balance?.toNumber(),
      feeBps: pool.feeBps,
      authority: pool.authority?.toBase58()
    });
    
    assert.equal(pool.balance.toNumber(), 1_000_000);
    assert.equal(pool.feeBps, 100);
    assert.equal(pool.authority.toBase58(), provider.wallet.publicKey.toBase58());
  });

  it("Lends funds", async () => {
    await program.methods
      .lend(new anchor.BN(500_000))
      .accounts({
        pool: poolAccount.publicKey,
      })
      .rpc();

    const pool = await program.account.pool.fetch(poolAccount.publicKey);
    assert.equal(pool.balance.toNumber(), 500_000); // 100万 - 50万
  });

  it("Repays funds", async () => {
    const repayAmount = 500_000;
    await program.methods
      .repay(new anchor.BN(repayAmount))
      .accounts({
        pool: poolAccount.publicKey,
      })
      .rpc();

    const pool = await program.account.pool.fetch(poolAccount.publicKey);
    const expectedFee = (repayAmount * 100) / 10_000; // 1% fee
    assert.equal(pool.balance.toNumber(), 500_000 + repayAmount + expectedFee);
  });

  it("Fails to lend too much", async () => {
    let error = null;
    try {
      await program.methods
        .lend(new anchor.BN(10_000_000)) // 超过余额
        .accounts({
          pool: poolAccount.publicKey,
        })
        .rpc();
    } catch (e) {
      error = e;
    }
    assert.ok(error);
    assert.match(error.message, /InsufficientFunds/);
  });
});