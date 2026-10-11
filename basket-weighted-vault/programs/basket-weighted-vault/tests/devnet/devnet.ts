import { TOKEN_2022_PROGRAM_ID, TOKEN_PROGRAM_ID } from "@solana/spl-token";
import { Keypair, PublicKey } from "@solana/web3.js";
import { expect } from "chai";
import {
  big,
  Devnet,
  DevnetBasket,
  DevnetVaultV1,
  DECIMALS,
  explorer,
  FIXED_PRICES_D18,
  MAX_LOT_USD,
  ONE_SHARE,
  PAXG,
  SPYX,
  START_PREMIUM_BPS,
  UNITS,
  USDY,
  VXAU,
  WEIGHTS,
} from "./helpers/devnet";

const USDC = 1_000_000n;
const SHARES = 10n;
const MIN_LOCKED_SHARES = 1_000n;
const D18 = 10n ** 18n;
const BPS = 10_000n;

const ceilDiv = (a: bigint, b: bigint) => (a + b - 1n) / b;

function expectedLot(holdings: bigint[]) {
  const values = holdings.map(
    (raw, i) => (raw * FIXED_PRICES_D18[i]) / 10n ** BigInt(DECIMALS[i])
  );
  const total = values.reduce((a, b) => a + b, 0n);
  const surplus = values[PAXG] - ceilDiv(total * BigInt(WEIGHTS[PAXG]), BPS);
  const deficit = (total * BigInt(WEIGHTS[VXAU])) / BPS - values[VXAU];
  const cap = MAX_LOT_USD * 10n ** 12n;
  const lotUsd = [surplus, deficit, cap].reduce((a, b) => (a < b ? a : b));
  const sellTotal =
    (lotUsd * 10n ** BigInt(DECIMALS[PAXG])) / FIXED_PRICES_D18[PAXG];
  const ratio =
    (FIXED_PRICES_D18[PAXG] * 10n ** BigInt(DECIMALS[VXAU]) * D18) /
    (FIXED_PRICES_D18[VXAU] * 10n ** BigInt(DECIMALS[PAXG]));
  const startPrice = ceilDiv(ratio * (BPS + START_PREMIUM_BPS), BPS);
  return { sellTotal, startPrice };
}

async function expectError(promise: Promise<unknown>, code: string) {
  try {
    await promise;
  } catch (error) {
    expect(String(error)).to.contain(code);
    return;
  }
  expect.fail(`expected ${code}`);
}

describe("basket_weighted_vault on devnet", () => {
  const devnet = new Devnet();
  const tokenPrograms = [
    TOKEN_PROGRAM_ID,
    TOKEN_PROGRAM_ID,
    TOKEN_PROGRAM_ID,
    TOKEN_2022_PROGRAM_ID,
  ];
  let vaultV1: DevnetVaultV1;
  let basket: DevnetBasket;

  // Setup: a fresh Vault 1 on the deployed program mints the real vXAU, then mock PAXG, USDY and SPYx are created.
  before(async () => {
    const usdc = await devnet.createMint(6);
    await devnet.tokenAccount(usdc, TOKEN_PROGRAM_ID, 1_000n * USDC);
    vaultV1 = new DevnetVaultV1(devnet, usdc);
    await vaultV1.initialize();
    await vaultV1.deposit(700n * USDC);

    const paxg = await devnet.createMint(DECIMALS[PAXG]);
    const usdy = await devnet.createMint(DECIMALS[USDY]);
    const spyx = await devnet.createScaledUiMint(DECIMALS[SPYX]);
    const mints = [vaultV1.vxauMint, paxg, usdy, spyx];
    basket = new DevnetBasket(devnet, vaultV1.state, mints, tokenPrograms);
    basket.userAccounts = await Promise.all(
      mints.map((mint, i) =>
        devnet.tokenAccount(
          mint,
          tokenPrograms[i],
          i === VXAU ? 0n : UNITS[i] * SHARES
        )
      )
    );
  });

  after(() => {
    console.log("\n  Devnet transactions:");
    for (const [label, signature] of Object.entries(devnet.signatures)) {
      if (!/^(create|token|initialize_vault|deposit_usdc)/.test(label)) {
        console.log(`    ${label.padEnd(26)} ${explorer(signature)}`);
      }
    }
  });

  it("initializes the basket with Vault 1's vXAU", async () => {
    await basket.initialize(vaultV1.state);

    const config = await basket.fetch();
    expect(config.authority.equals(devnet.payer.publicKey)).to.be.true;
    expect(config.tokenMints.map((m: PublicKey) => m.toBase58())).to.deep.equal(
      basket.mints.map((m) => m.toBase58())
    );
    expect(config.weights).to.deep.equal(WEIGHTS);
    expect(config.assets[SPYX].scaledUi).to.be.true;
  });

  it("deposits all four tokens and records the shares", async () => {
    await basket.deposit(
      UNITS.map((units) => units * SHARES),
      SHARES * ONE_SHARE - MIN_LOCKED_SHARES
    );

    const position = await basket.fetchPosition();
    expect(big(position.shares)).to.equal(
      SHARES * ONE_SHARE - MIN_LOCKED_SHARES
    );
    for (const [i, account] of basket.vaultTokens.entries()) {
      expect(await devnet.balance(account, tokenPrograms[i])).to.equal(
        UNITS[i] * SHARES
      );
    }
  });

  it("opens an auction selling overweight PAXG for underweight vXAU", async () => {
    await basket.openAuction(PAXG, VXAU);

    const expected = expectedLot(UNITS.map((units) => units * SHARES));
    const auction = await basket.fetchAuction(0n);
    expect(auction.sellIndex).to.equal(PAXG);
    expect(auction.buyIndex).to.equal(VXAU);
    expect(big(auction.sellTotal)).to.equal(expected.sellTotal);
    expect(big(auction.startPriceD18)).to.equal(expected.startPrice);
    expect((await basket.fetch()).activeAuction).to.be.true;
  });

  it("fills the whole lot in one bid at or below the start price", async () => {
    const auction = await basket.fetchAuction(0n);
    const sellTotal = big(auction.sellTotal);
    const maxBuy = ceilDiv(sellTotal * big(auction.startPriceD18), D18);
    const vaultBefore = await Promise.all(
      [PAXG, VXAU].map((i) =>
        devnet.balance(basket.vaultTokens[i], tokenPrograms[i])
      )
    );

    await basket.bid(0n, sellTotal, maxBuy);

    const filled = await basket.fetchAuction(0n);
    const received = big(filled.buyReceived);
    expect(big(filled.sellRemaining)).to.equal(0n);
    expect(received > 0n && received <= maxBuy).to.be.true;
    expect(
      await devnet.balance(basket.vaultTokens[PAXG], TOKEN_PROGRAM_ID)
    ).to.equal(vaultBefore[0] - sellTotal);
    expect(
      await devnet.balance(basket.vaultTokens[VXAU], TOKEN_PROGRAM_ID)
    ).to.equal(vaultBefore[1] + received);
  });

  it("closes the sold-out auction and frees the basket", async () => {
    await basket.closeAuction(0n);

    expect(await devnet.connection.getAccountInfo(basket.auctionAddress(0n))).to
      .be.null;
    const config = await basket.fetch();
    expect(config.activeAuction).to.be.false;
    expect(big(config.auctionNonce)).to.equal(1n);
  });

  it("rejects a pause from anyone but the basket authority", async () => {
    await expectError(
      basket.setPaused(true, Keypair.generate()),
      "Unauthorized"
    );
  });

  it("pauses deposits and unpauses them again", async () => {
    await basket.setPaused(true);
    await expectError(
      basket.deposit(
        UNITS.map(() => 1n),
        0n
      ),
      "Paused"
    );

    await basket.setPaused(false);
    expect((await basket.fetch()).paused).to.be.false;
  });
});
