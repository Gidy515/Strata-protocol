import { Keypair } from "@solana/web3.js";
import { expect } from "chai";
import { big, DevnetVaultV1, explorer, USDC } from "./helpers/devnet";

async function expectError(promise: Promise<unknown>, code: string) {
  try {
    await promise;
  } catch (error) {
    expect(String(error)).to.contain(code);
    return;
  }
  expect.fail(`expected ${code}`);
}

describe("xau_carry_vault on devnet", () => {
  const vault = new DevnetVaultV1();

  before(async () => {
    await vault.createUsdc(1_000n * USDC);
  });

  after(() => {
    console.log("\n  Devnet transactions:");
    for (const [label, signature] of Object.entries(vault.signatures)) {
      console.log(`    ${label.padEnd(30)} ${explorer(signature)}`);
    }
  });

  it("initializes the vault, custody and vXAU mint", async () => {
    await vault.initialize();

    const state = await vault.fetchState();
    expect(state.admin.equals(vault.user)).to.be.true;
    expect(state.usdcMint.equals(vault.usdcMint)).to.be.true;
    expect(state.vxauMint.equals(vault.vxauMint)).to.be.true;
    expect(big(state.totalDeposited)).to.equal(0n);
  });

  it("reports an empty idle NAV", async () => {
    const nav = await vault.readIdleNav();

    expect(big(nav.netAssets)).to.equal(0n);
    expect(big(nav.shareSupply)).to.equal(0n);
  });

  it("mints vXAU 1:1 for the first USDC deposit", async () => {
    await vault.deposit(700n * USDC, 700n * USDC);

    expect(await vault.balance(vault.userVxau)).to.equal(700n * USDC);
    expect(await vault.balance(vault.custody)).to.equal(700n * USDC);
    expect(big((await vault.fetchState()).totalDeposited)).to.equal(
      700n * USDC
    );
  });

  it("redeems vXAU for USDC at NAV", async () => {
    const usdcBefore = await vault.balance(vault.userUsdc);

    await vault.withdraw(100n * USDC, 100n * USDC);

    expect(await vault.balance(vault.userVxau)).to.equal(600n * USDC);
    expect(await vault.balance(vault.userUsdc)).to.equal(
      usdcBefore + 100n * USDC
    );
    expect(await vault.balance(vault.custody)).to.equal(600n * USDC);
  });

  it("escrows a queued withdrawal and returns it on cancel", async () => {
    await vault.requestWithdrawal(0n, 50n * USDC, 50n * USDC);
    expect(await vault.balance(vault.userVxau)).to.equal(550n * USDC);
    expect(await vault.balance(vault.withdrawalEscrow(0n))).to.equal(
      50n * USDC
    );

    await vault.completeWithdrawal(0n, true);

    expect(await vault.balance(vault.userVxau)).to.equal(600n * USDC);
    expect(await vault.exists(vault.withdrawalRequest(0n))).to.be.false;
  });

  it("settles a queued withdrawal at NAV", async () => {
    const usdcBefore = await vault.balance(vault.userUsdc);
    await vault.requestWithdrawal(1n, 100n * USDC, 100n * USDC);

    await vault.completeWithdrawal(1n, false);

    expect(await vault.balance(vault.userUsdc)).to.equal(
      usdcBefore + 100n * USDC
    );
    expect(await vault.balance(vault.custody)).to.equal(500n * USDC);
    expect(await vault.exists(vault.withdrawalRequest(1n))).to.be.false;
    const nav = await vault.readIdleNav();
    expect(big(nav.netAssets)).to.equal(500n * USDC);
    expect(big(nav.shareSupply)).to.equal(500n * USDC);
  });

  it("pauses deposits and unpauses them again", async () => {
    await vault.setDepositsPaused(true);
    await expectError(vault.deposit(USDC, USDC), "DepositsPaused");

    await vault.setDepositsPaused(false);
    await vault.deposit(USDC, USDC);
    expect(await vault.balance(vault.userVxau)).to.equal(501n * USDC);
  });

  it("rejects admin changes from anyone but the admin", async () => {
    const stranger = Keypair.generate();

    await expectError(
      vault.proposeAdmin(stranger.publicKey, stranger),
      "UnauthorizedStrategyAdmin"
    );
  });

  it("hands the admin role over in two steps", async () => {
    const newAdmin = Keypair.generate();

    await vault.proposeAdmin(newAdmin.publicKey);
    await vault.acceptAdmin(newAdmin);

    expect((await vault.fetchState()).admin.equals(newAdmin.publicKey)).to.be
      .true;
  });
});
