import { BN } from "@anchor-lang/core";
import { PublicKey } from "@solana/web3.js";
import { expect } from "chai";
import {
  big,
  expectBasketError,
  expectFailure,
  MIN_LOCKED_SHARES,
  ONE_SHARE,
  PAXG,
  recipeAmounts,
  SPYX,
  T0,
  TestBasket,
  UNITS,
  USDY,
  VXAU,
} from "./helpers/basket";

describe("deposit_to_basket_v2", () => {
  it("prices the first deposit with the recipe and locks the minimum shares", async () => {
    const basket = await TestBasket.initialized();
    const user = basket.user(recipeAmounts(200_000n));

    await basket.deposit(user, recipeAmounts(100_000n), 0n);

    const shares = 100_000n * ONE_SHARE;
    const position = basket.fetchPosition(user.keypair.publicKey);
    expect(position.owner.equals(user.keypair.publicKey)).to.be.true;
    expect(position.basketConfig.equals(basket.basketConfig)).to.be.true;
    expect(big(position.shares)).to.equal(shares - MIN_LOCKED_SHARES);
    expect(big(position.createdTs)).to.equal(T0);
    expect(big(basket.fetchBasket().totalShares)).to.equal(shares);
    expect(basket.balances(basket.vaultTokens)).to.deep.equal(
      recipeAmounts(100_000n)
    );
    expect(basket.balances(user.accounts)).to.deep.equal(
      recipeAmounts(100_000n)
    );
  });

  it("issues later shares pro-rata and leaves the excess with the user", async () => {
    const { basket } = await TestBasket.withHoldings(100_000n);
    const offer = recipeAmounts(10_000n);
    offer[PAXG] *= 2n;
    const bob = basket.user(offer);

    await basket.deposit(bob, offer, 0n);

    expect(big(basket.fetchPosition(bob.keypair.publicKey).shares)).to.equal(
      10_000n * ONE_SHARE
    );
    expect(big(basket.fetchBasket().totalShares)).to.equal(
      110_000n * ONE_SHARE
    );
    expect(basket.balances(bob.accounts)).to.deep.equal([
      0n,
      UNITS[PAXG] * 10_000n,
      0n,
      0n,
    ]);
    expect(basket.balances(basket.vaultTokens)).to.deep.equal(
      recipeAmounts(110_000n)
    );
  });

  it("adds repeated deposits to one position", async () => {
    const basket = await TestBasket.initialized();
    const user = basket.user(recipeAmounts(3_000n));

    await basket.deposit(user, recipeAmounts(1_000n), 0n);
    basket.warp(T0 + 10n);
    await basket.deposit(user, recipeAmounts(2_000n), 0n);

    const position = basket.fetchPosition(user.keypair.publicKey);
    expect(big(position.shares)).to.equal(
      3_000n * ONE_SHARE - MIN_LOCKED_SHARES
    );
    expect(big(position.createdTs)).to.equal(T0);
    expect(big(position.lastDepositTs)).to.equal(T0 + 10n);
  });

  it("rejects a first deposit no bigger than the locked minimum", async () => {
    const basket = await TestBasket.initialized();
    const user = basket.user(recipeAmounts(1n));

    await expectBasketError(
      basket.deposit(
        user,
        UNITS.map((units) => units / 1_000n),
        0n
      ),
      "FirstDepositTooSmall"
    );
  });

  it("enforces the user's minimum shares", async () => {
    const basket = await TestBasket.initialized();
    const user = basket.user(recipeAmounts(1_000n));
    const shares = 1_000n * ONE_SHARE;

    await expectBasketError(
      basket.deposit(user, recipeAmounts(1_000n), shares),
      "SlippageExceeded"
    );
    await basket.deposit(
      user,
      recipeAmounts(1_000n),
      shares - MIN_LOCKED_SHARES
    );
  });

  it("rejects paying from someone else's token account", async () => {
    const { basket, depositor } = await TestBasket.withHoldings(1_000n);
    const thief = basket.user([0n, 0n, 0n, 0n]);

    await expectBasketError(
      basket.deposit(thief, recipeAmounts(1n), 0n, {
        userToken0: depositor.accounts[VXAU],
      }),
      "ConstraintTokenOwner"
    );
  });

  it("rejects a token account that is not the basket's", async () => {
    const { basket, depositor } = await TestBasket.withHoldings(1_000n);
    const fakeVault = basket.tokenAccountFor(
      depositor.keypair.publicKey,
      VXAU,
      0n
    );

    await expectBasketError(
      basket.deposit(depositor, recipeAmounts(1n), 0n, {
        vaultToken0: fakeVault,
      }),
      "InvalidVaultAccount"
    );
  });

  it("rejects a mint that is not the basket's token", async () => {
    const { basket, depositor } = await TestBasket.withHoldings(1_000n);

    await expectBasketError(
      basket.deposit(depositor, recipeAmounts(1n), 0n, {
        mint1: basket.mints[USDY],
      }),
      "MintMismatch"
    );
  });
  it("emits BasketDeposited", async () => {
    const basket = await TestBasket.initialized();
    const user = basket.user(recipeAmounts(1_000n));

    await basket.deposit(user, recipeAmounts(1_000n), 0n);

    const [event] = basket.events();
    expect(event.name).to.equal("basketDeposited");
    expect((event.data.user as PublicKey).equals(user.keypair.publicKey)).to.be
      .true;
    expect((event.data.amounts as BN[]).map(big)).to.deep.equal(
      recipeAmounts(1_000n)
    );
    expect(big(event.data.shares as BN)).to.equal(
      1_000n * ONE_SHARE - MIN_LOCKED_SHARES
    );
    expect(big(event.data.totalShares as BN)).to.equal(1_000n * ONE_SHARE);
  });

  it("does not take a token the basket holds none of", async () => {
    const { basket } = await TestBasket.withHoldings(100_000n);
    basket.setTokenAmount(basket.vaultTokens[USDY], 0n);
    const user = basket.user(recipeAmounts(1_000n));

    await basket.deposit(user, recipeAmounts(1_000n), 0n);

    expect(big(basket.fetchPosition(user.keypair.publicKey).shares)).to.equal(
      1_000n * ONE_SHARE
    );
    expect(basket.balance(user.accounts[USDY])).to.equal(
      recipeAmounts(1_000n)[USDY]
    );
    expect(basket.balance(basket.vaultTokens[USDY])).to.equal(0n);
  });

  it("rejects a later deposit that buys no shares", async () => {
    const { basket } = await TestBasket.withHoldings(1_000n);
    const user = basket.user(recipeAmounts(1n));

    await expectBasketError(
      basket.deposit(user, [0n, 0n, 0n, 0n], 0n),
      "ZeroShares"
    );
  });

  it("rejects amounts too large to turn into shares", async () => {
    const basket = await TestBasket.initialized();
    const user = basket.user(recipeAmounts(1n));
    const max = 2n ** 64n - 1n;

    await expectBasketError(
      basket.deposit(user, [max, max, max, max], 0n),
      "MathOverflow"
    );
  });

  it("rejects a user token account for another token", async () => {
    const { basket, depositor } = await TestBasket.withHoldings(1_000n);
    const usdyAccount = basket.tokenAccountFor(
      depositor.keypair.publicKey,
      USDY,
      1_000_000n
    );

    await expectBasketError(
      basket.deposit(depositor, recipeAmounts(1n), 0n, {
        userToken1: usdyAccount,
      }),
      "ConstraintTokenMint"
    );
  });

  it("rejects the wrong token program", async () => {
    const { basket, depositor } = await TestBasket.withHoldings(1_000n);

    await expectFailure(
      basket.deposit(depositor, recipeAmounts(1n), 0n, {
        tokenProgram1: basket.tokenPrograms[SPYX],
      })
    );
  });
});
