import * as anchor from "@coral-xyz/anchor";
import { Program } from "@coral-xyz/anchor";
import { StrataProtocol } from "../target/types/strata_protocol";
import { 
  TOKEN_PROGRAM_ID, 
  createMint, 
  createAccount, 
  mintTo, 
  getAccount 
} from "@solana/spl-token";
import { assert } from "chai";

describe("strata_protocol - Vault 2 Deposit", () => {
  const provider = anchor.AnchorProvider.env();
  anchor.setProvider(provider);

  const program = anchor.workspace.StrataProtocol as Program<StrataProtocol>;
  const user = provider.wallet;

  let basketConfigPda: anchor.web3.PublicKey;
  let vaultV2StatePda: anchor.web3.PublicKey;
  let vxauMint: anchor.web3.PublicKey;
  let rwaMint: anchor.web3.PublicKey;
  let basketMint: anchor.web3.PublicKey;

  let userVxauAta: anchor.web3.PublicKey;
  let userRwaAta: anchor.web3.PublicKey;
  let userBasketAta: anchor.web3.PublicKey;

  let vaultVxauAccount: anchor.web3.PublicKey;
  let vaultRwaAccount: anchor.web3.PublicKey;

  before(async () => {
    // 1. Derive PDAs
    [basketConfigPda] = anchor.web3.PublicKey.findProgramAddressSync(
      [Buffer.from("basket_config"), Buffer.from("v1")],
      program.programId
    );

    [vaultV2StatePda] = anchor.web3.PublicKey.findProgramAddressSync(
      [Buffer.from("vault_v2"), basketConfigPda.toBuffer()],
      program.programId
    );

    // 2. Setup Mock Mints & ATAs on Localnet
    vxauMint = await createMint(provider.connection, user.payer, user.publicKey, null, 6);
    rwaMint = await createMint(provider.connection, user.payer, user.publicKey, null, 6);
    basketMint = await createMint(provider.connection, user.payer, vaultV2StatePda, null, 6);

    userVxauAta = await createAccount(provider.connection, user.payer, vxauMint, user.publicKey);
    userRwaAta = await createAccount(provider.connection, user.payer, rwaMint, user.publicKey);
    userBasketAta = await createAccount(provider.connection, user.payer, basketMint, user.publicKey);

    vaultVxauAccount = await createAccount(provider.connection, user.payer, vxauMint, vaultV2StatePda);
    vaultRwaAccount = await createAccount(provider.connection, user.payer, rwaMint, vaultV2StatePda);

    // 3. Mint initial mock tokens to user
    await mintTo(provider.connection, user.payer, vxauMint, userVxauAta, user.payer, 1000_000000);
    await mintTo(provider.connection, user.payer, rwaMint, userRwaAta, user.payer, 1000_000000);
  });

  it("Deposits vXAU and RWA to Vault 2 and Mints BASKET tokens", async () => {
    const vxauAmount = new anchor.BN(100_000000); // 100 vXAU
    const rwaAmount = new anchor.BN(100_000000);  // 100 RWA

    await program.methods
      .depositToBasketV2(vxauAmount, rwaAmount)
      .accounts({
        user: user.publicKey,
        basketConfig: basketConfigPda,
        vaultV2State: vaultV2StatePda,
        userVxauAta: userVxauAta,
        userRwaAta: userRwaAta,
        vaultV2VxauAccount: vaultVxauAccount,
        vaultV2RwaAccount: vaultRwaAccount,
        basketMint: basketMint,
        userBasketAta: userBasketAta,
        tokenProgram: TOKEN_PROGRAM_ID,
      })
      .rpc();

    // Verify BASKET Token balance after mint
    const userBasketAccountInfo = await getAccount(provider.connection, userBasketAta);
    assert.equal(userBasketAccountInfo.amount.toString(), "200000000"); // 100 + 100 = 200 BASKET
  });
});
