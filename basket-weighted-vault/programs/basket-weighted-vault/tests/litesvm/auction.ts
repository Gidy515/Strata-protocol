import { BN } from "@anchor-lang/core";
import { Keypair, PublicKey } from "@solana/web3.js";
import { expect } from "chai";
import {
  big,
  buyAmount,
  COOLDOWN,
  DURATION,
  expectBasketError,
  expectedAuction,
  expectFailure,
  FEEDS,
  MAX_AGE_NAV,
  MAX_AGE_TRADED,
  PAXG,
  priceAt,
  PYTH_PRICES,
  recipeAmounts,
  SPYX,
  T0,
  TestBasket,
  usdPrices,
  USDY,
  User,
  VXAU,
} from "./helpers/basket";

const HOLDINGS = recipeAmounts(100_000n);
const RALLY_PAXG = 480_000_000_000n;
const RALLY_SPYX = 96_000_000_000n;
const BIDDER_FUNDS = [
  100_000_000_000n,
  10_000_000_000n,
  100_000_000_000n,
  100_000_000_000n,
];

async function goldRally(options = {}) {
  const { basket } = await TestBasket.withHoldings(100_000n, options);
  basket.putPrice(PAXG, RALLY_PAXG, T0);
  const prices = usdPrices([
    0n,
    RALLY_PAXG,
    PYTH_PRICES[USDY],
    PYTH_PRICES[SPYX],
  ]);
  return {
    basket,
    opener: basket.funded(),
    bidder: basket.user(BIDDER_FUNDS),
    expected: expectedAuction(HOLDINGS, prices, PAXG, VXAU),
  };
}

async function spyxRally(options = {}) {
  const { basket } = await TestBasket.withHoldings(100_000n, options);
  basket.putPrice(SPYX, RALLY_SPYX, T0);
  const prices = usdPrices([
    0n,
    PYTH_PRICES[PAXG],
    PYTH_PRICES[USDY],
    RALLY_SPYX,
  ]);
  return {
    basket,
    opener: basket.funded(),
    bidder: basket.user(BIDDER_FUNDS),
    expected: expectedAuction(HOLDINGS, prices, SPYX, USDY),
  };
}

function refreshTradedPrices(basket: TestBasket, at: bigint) {
  basket.putPrice(PAXG, RALLY_PAXG, at);
  basket.putPrice(USDY, PYTH_PRICES[USDY], at);
  basket.putPrice(SPYX, PYTH_PRICES[SPYX], at);
}

function balancesOf(basket: TestBasket, bidder: User) {
  return {
    vault: basket.balances(basket.vaultTokens),
    bidder: basket.balances(bidder.accounts),
  };
}

describe("open_auction", () => {
  it("opens an auction sized to the smallest of surplus, deficit and cap", async () => {
    const { basket, opener, expected } = await goldRally();

    await basket.openAuction(opener, PAXG, VXAU);

    const auction = basket.fetchAuction(0);
    expect(auction.basketConfig.equals(basket.basketConfig)).to.be.true;
    expect(auction.opener.equals(opener.publicKey)).to.be.true;
    expect([auction.sellIndex, auction.buyIndex]).to.deep.equal([PAXG, VXAU]);
    expect(big(auction.sellTotal)).to.equal(expected.sellTotal);
    expect(big(auction.sellTotal)).to.equal(33_333_333n);
    expect(big(auction.sellRemaining)).to.equal(expected.sellTotal);
    expect(big(auction.startPriceD18)).to.equal(expected.startPrice);
    expect(big(auction.endPriceD18)).to.equal(expected.endPrice);
    expect(big(auction.startTs)).to.equal(T0);
    expect(big(auction.endTs)).to.equal(T0 + DURATION);

    const config = basket.fetchBasket();
    expect(config.activeAuction).to.be.true;
    expect(big(config.auctionNonce)).to.equal(1n);
  });

  it("rejects a sell token that is inside its band", async () => {
    const { basket } = await TestBasket.withHoldings(100_000n);
    basket.putPrice(PAXG, 440_000_000_000n, T0);

    await expectBasketError(
      basket.openAuction(basket.funded(), PAXG, VXAU),
      "NotOverweight"
    );
  });

  it("rejects a buy token that is not underweight", async () => {
    const { basket, opener } = await goldRally();
    basket.putPrice(USDY, 140_000_000n, T0);

    await expectBasketError(
      basket.openAuction(opener, PAXG, USDY),
      "NotUnderweight"
    );
  });

  it("needs fresh prices for traded tokens but accepts older valuation prices", async () => {
    const stale = await goldRally();
    stale.basket.putPrice(PAXG, RALLY_PAXG, T0 - MAX_AGE_TRADED - 1n);
    await expectBasketError(
      stale.basket.openAuction(stale.opener, PAXG, VXAU),
      "StalePrice"
    );

    const weekend = await goldRally();
    weekend.basket.putPrice(SPYX, PYTH_PRICES[SPYX], T0 - 2n * 86_400n);
    await weekend.basket.openAuction(weekend.opener, PAXG, VXAU);

    const ancient = await goldRally();
    ancient.basket.putPrice(SPYX, PYTH_PRICES[SPYX], T0 - MAX_AGE_NAV - 1n);
    await expectBasketError(
      ancient.basket.openAuction(ancient.opener, PAXG, VXAU),
      "StalePrice"
    );
  });

  it("rejects selling SPYx when its multiplier changes during the auction", async () => {
    const change = T0 + DURATION + 300n;

    const selling = await spyxRally({ multiplierChangeTs: change });
    await expectBasketError(
      selling.basket.openAuction(selling.opener, SPYX, USDY),
      "MultiplierChangeWindow"
    );

    const valuing = await goldRally({ multiplierChangeTs: change });
    await valuing.basket.openAuction(valuing.opener, PAXG, VXAU);
  });

  it("rejects remaining accounts that do not match the basket", async () => {
    const { basket, opener } = await goldRally();
    const remaining = basket.openAuctionRemainingAccounts();

    const swapped = [...remaining];
    [swapped[0], swapped[1]] = [swapped[1], swapped[0]];
    await expectBasketError(
      basket.openAuction(opener, PAXG, VXAU, swapped),
      "InvalidVaultAccount"
    );

    await expectBasketError(
      basket.openAuction(opener, PAXG, VXAU, remaining.slice(0, -1)),
      "InvalidRemainingAccounts"
    );

    const fakePrice = [...remaining];
    fakePrice[4] = { ...fakePrice[4], pubkey: basket.vaultTokens[PAXG] };
    await expectBasketError(
      basket.openAuction(opener, PAXG, VXAU, fakePrice),
      "InvalidPriceAccount"
    );
  });

  it("allows only one live auction", async () => {
    const { basket, opener } = await goldRally();

    await basket.openAuction(opener, PAXG, VXAU);

    await expectBasketError(
      basket.openAuction(opener, PAXG, VXAU),
      "AuctionActive"
    );
  });
  it("emits AuctionOpened", async () => {
    const { basket, opener, expected } = await goldRally();

    await basket.openAuction(opener, PAXG, VXAU);

    const [event] = basket.events();
    expect(event.name).to.equal("auctionOpened");
    expect(big(event.data.nonce as BN)).to.equal(0n);
    expect(big(event.data.sellTotal as BN)).to.equal(expected.sellTotal);
    expect(big(event.data.lotUsdD18 as BN)).to.equal(expected.lotUsd);
    expect(big(event.data.endTs as BN)).to.equal(T0 + DURATION);
  });

  it("rejects the same token on both sides or an unknown token", async () => {
    const { basket, opener } = await goldRally();

    await expectBasketError(
      basket.openAuction(opener, PAXG, PAXG),
      "InvalidAssetIndex"
    );
    await expectBasketError(
      basket.openAuction(opener, 4, VXAU),
      "InvalidAssetIndex"
    );
  });

  it("rejects an empty basket", async () => {
    const basket = await TestBasket.initialized();

    await expectBasketError(
      basket.openAuction(basket.funded(), PAXG, VXAU),
      "EmptyBasket"
    );
  });

  it("rejects a rebalance smaller than the minimum lot", async () => {
    const { basket } = await TestBasket.withHoldings(100n);
    basket.putPrice(PAXG, RALLY_PAXG, T0);

    await expectBasketError(
      basket.openAuction(basket.funded(), PAXG, VXAU),
      "LotTooSmall"
    );
  });

  it("rejects a price that is not fully verified", async () => {
    const { basket, opener } = await goldRally();
    basket.putPrice(PAXG, RALLY_PAXG, T0, { verified: false });

    await expectBasketError(
      basket.openAuction(opener, PAXG, VXAU),
      "PriceNotVerified"
    );
  });

  it("rejects a price from a different feed", async () => {
    const { basket, opener } = await goldRally();
    basket.putPrice(PAXG, RALLY_PAXG, T0, { feedId: FEEDS[USDY] });

    await expectBasketError(
      basket.openAuction(opener, PAXG, VXAU),
      "WrongPriceFeed"
    );
  });

  it("rejects a price that is too uncertain", async () => {
    const { basket, opener } = await goldRally();
    basket.putPrice(PAXG, RALLY_PAXG, T0, { conf: RALLY_PAXG / 50n });

    await expectBasketError(
      basket.openAuction(opener, PAXG, VXAU),
      "PriceConfidence"
    );
  });

  it("rejects a zero price", async () => {
    const { basket, opener } = await goldRally();
    basket.putPrice(USDY, 0n, T0);

    await expectBasketError(
      basket.openAuction(opener, PAXG, VXAU),
      "InvalidPrice"
    );
  });

  it("rejects a mint that is not the basket's SPYx", async () => {
    const { basket, opener } = await goldRally();
    const remaining = basket.openAuctionRemainingAccounts();
    remaining[remaining.length - 1] = {
      ...remaining[remaining.length - 1],
      pubkey: basket.mints[USDY],
    };

    await expectBasketError(
      basket.openAuction(opener, PAXG, VXAU, remaining),
      "InvalidScaledUiMint"
    );
  });

  it("values SPYx with its dividend and split multiplier", async () => {
    const { basket, opener } = await goldRally({ multiplier: 1.5 });

    await expectBasketError(
      basket.openAuction(opener, PAXG, VXAU),
      "NotOverweight"
    );
  });

  it("rejects a multiplier change close to now even when SPYx is only valued", async () => {
    const { basket, opener } = await goldRally({
      multiplierChangeTs: T0 + 300n,
    });

    await expectBasketError(
      basket.openAuction(opener, PAXG, VXAU),
      "MultiplierChangeWindow"
    );
  });
});

describe("bid", () => {
  it("fills at the current point on the falling price line", async () => {
    const { basket, opener, bidder, expected } = await goldRally();
    await basket.openAuction(opener, PAXG, VXAU);
    const before = balancesOf(basket, bidder);

    basket.warp(T0 + 900n);
    const price = priceAt(
      expected.startPrice,
      expected.endPrice,
      T0,
      T0 + DURATION,
      T0 + 900n
    );
    const owed = buyAmount(expected.sellTotal, price);

    await expectBasketError(
      basket.bid(bidder, 0, expected.sellTotal, owed - 1n),
      "SlippageExceeded"
    );
    await basket.bid(bidder, 0, expected.sellTotal, owed);

    const after = balancesOf(basket, bidder);
    expect(after.vault[VXAU]).to.equal(before.vault[VXAU] + owed);
    expect(after.vault[PAXG]).to.equal(before.vault[PAXG] - expected.sellTotal);
    expect(after.bidder[VXAU]).to.equal(before.bidder[VXAU] - owed);
    expect(after.bidder[PAXG]).to.equal(
      before.bidder[PAXG] + expected.sellTotal
    );
    const auction = basket.fetchAuction(0);
    expect(big(auction.sellRemaining)).to.equal(0n);
    expect(big(auction.buyReceived)).to.equal(owed);

    await expectBasketError(basket.bid(bidder, 0, 1n, owed), "AuctionFilled");
  });

  it("caps a bid at what is left and rejects bids after the end", async () => {
    const { basket, opener, bidder, expected } = await goldRally();
    await basket.openAuction(opener, PAXG, VXAU);

    await basket.bid(
      bidder,
      0,
      10_000_000n,
      buyAmount(10_000_000n, expected.startPrice)
    );
    await basket.bid(
      bidder,
      0,
      BigInt(Number.MAX_SAFE_INTEGER),
      BigInt(Number.MAX_SAFE_INTEGER)
    );

    const auction = basket.fetchAuction(0);
    expect(big(auction.sellRemaining)).to.equal(0n);
    expect(big(auction.buyReceived)).to.equal(
      buyAmount(10_000_000n, expected.startPrice) +
        buyAmount(expected.sellTotal - 10_000_000n, expected.startPrice)
    );

    const late = await goldRally();
    await late.basket.openAuction(late.opener, PAXG, VXAU);
    late.basket.warp(T0 + DURATION + 1n);
    await expectBasketError(
      late.basket.bid(late.bidder, 0, 1_000n, 10n ** 12n),
      "AuctionNotLive"
    );
  });

  it("sells a Token-2022 token through transfer_checked", async () => {
    const { basket, opener, bidder, expected } = await spyxRally();
    await basket.openAuction(opener, SPYX, USDY);
    const before = balancesOf(basket, bidder);

    const owed = buyAmount(expected.sellTotal, expected.startPrice);
    await basket.bid(bidder, 0, expected.sellTotal, owed);

    const after = balancesOf(basket, bidder);
    expect(after.bidder[SPYX]).to.equal(
      before.bidder[SPYX] + expected.sellTotal
    );
    expect(after.vault[USDY]).to.equal(before.vault[USDY] + owed);
  });

  it("cannot pay out into the basket's own account", async () => {
    const { basket, opener, bidder } = await goldRally();
    await basket.openAuction(opener, PAXG, VXAU);

    await expectFailure(
      basket.bid(bidder, 0, 1_000n, 10n ** 12n, basket.vaultTokens[PAXG])
    );
  });
  it("emits AuctionBid", async () => {
    const { basket, opener, bidder, expected } = await goldRally();
    await basket.openAuction(opener, PAXG, VXAU);
    const owed = buyAmount(10_000_000n, expected.startPrice);

    await basket.bid(bidder, 0, 10_000_000n, owed);

    const [event] = basket.events();
    expect(event.name).to.equal("auctionBid");
    expect((event.data.bidder as PublicKey).equals(bidder.keypair.publicKey)).to
      .be.true;
    expect(big(event.data.sellAmount as BN)).to.equal(10_000_000n);
    expect(big(event.data.buyAmount as BN)).to.equal(owed);
    expect(big(event.data.sellRemaining as BN)).to.equal(
      expected.sellTotal - 10_000_000n
    );
  });

  it("rejects a zero amount", async () => {
    const { basket, opener, bidder } = await goldRally();
    await basket.openAuction(opener, PAXG, VXAU);

    await expectBasketError(
      basket.bid(bidder, 0, 0n, 10n ** 12n),
      "ZeroAmount"
    );
  });

  it("accepts a bid at the last second at the end price", async () => {
    const { basket, opener, bidder, expected } = await goldRally();
    await basket.openAuction(opener, PAXG, VXAU);
    basket.warp(T0 + DURATION);

    await basket.bid(
      bidder,
      0,
      1_000_000n,
      buyAmount(1_000_000n, expected.endPrice)
    );

    expect(big(basket.fetchAuction(0).buyReceived)).to.equal(
      buyAmount(1_000_000n, expected.endPrice)
    );
    await expectBasketError(
      basket.closeAuction(0, opener.publicKey),
      "AuctionStillLive"
    );
  });

  it("buys a Token-2022 token from the bidder", async () => {
    const { basket } = await TestBasket.withHoldings(100_000n);
    basket.putPrice(SPYX, 30_000_000_000n, T0);
    const prices = usdPrices([
      0n,
      PYTH_PRICES[PAXG],
      PYTH_PRICES[USDY],
      30_000_000_000n,
    ]);
    const expected = expectedAuction(HOLDINGS, prices, VXAU, SPYX);
    const bidder = basket.user(BIDDER_FUNDS);
    await basket.openAuction(basket.funded(), VXAU, SPYX);
    const before = balancesOf(basket, bidder);

    const owed = buyAmount(expected.sellTotal, expected.startPrice);
    await basket.bid(bidder, 0, expected.sellTotal, owed);

    const after = balancesOf(basket, bidder);
    expect(after.vault[SPYX]).to.equal(before.vault[SPYX] + owed);
    expect(after.bidder[SPYX]).to.equal(before.bidder[SPYX] - owed);
    expect(after.bidder[VXAU]).to.equal(
      before.bidder[VXAU] + expected.sellTotal
    );
  });

  it("rejects paying from someone else's token account", async () => {
    const { basket, opener, bidder } = await goldRally();
    await basket.openAuction(opener, PAXG, VXAU);
    const other = basket.user(BIDDER_FUNDS);

    await expectBasketError(
      basket.bid(bidder, 0, 1_000n, 10n ** 12n, undefined, {
        bidderBuyAccount: other.accounts[VXAU],
      }),
      "ConstraintTokenOwner"
    );
  });

  it("rejects a mint that is not the auction's", async () => {
    const { basket, opener, bidder } = await goldRally();
    await basket.openAuction(opener, PAXG, VXAU);

    await expectBasketError(
      basket.bid(bidder, 0, 1_000n, 10n ** 12n, undefined, {
        sellMint: basket.mints[USDY],
      }),
      "ConstraintAddress"
    );
  });
});

describe("close_auction", () => {
  it("refunds the opener's rent after expiry and starts the cooldown", async () => {
    const { basket, opener, bidder, expected } = await goldRally();
    await basket.openAuction(opener, PAXG, VXAU);
    await basket.bid(
      bidder,
      0,
      2_000_000n,
      buyAmount(2_000_000n, expected.startPrice)
    );

    await expectBasketError(
      basket.closeAuction(0, opener.publicKey),
      "AuctionStillLive"
    );

    const rent = basket.lamports(basket.auctionAddress(0));
    const openerBefore = basket.lamports(opener.publicKey);
    const closedAt = T0 + DURATION + 1n;
    basket.warp(closedAt);
    await basket.closeAuction(0, opener.publicKey);

    expect(basket.accountExists(basket.auctionAddress(0))).to.be.false;
    expect(basket.lamports(opener.publicKey)).to.equal(openerBefore + rent);
    const config = basket.fetchBasket();
    expect(config.activeAuction).to.be.false;
    expect(big(config.lastAuctionEndTs)).to.equal(closedAt);

    refreshTradedPrices(basket, closedAt);
    await expectBasketError(basket.openAuction(opener, PAXG, VXAU), "Cooldown");

    const reopenAt = closedAt + COOLDOWN;
    basket.warp(reopenAt);
    refreshTradedPrices(basket, reopenAt);
    await basket.openAuction(opener, PAXG, VXAU);
    expect(big(basket.fetchAuction(1).nonce)).to.equal(1n);
  });

  it("closes a sold-out auction straight away", async () => {
    const { basket, opener, bidder, expected } = await goldRally();
    await basket.openAuction(opener, PAXG, VXAU);
    await basket.bid(bidder, 0, expected.sellTotal, 10n ** 15n);

    await basket.closeAuction(0, opener.publicKey);

    expect(basket.fetchBasket().activeAuction).to.be.false;
  });

  it("refunds rent only to the recorded opener", async () => {
    const { basket, opener } = await goldRally();
    await basket.openAuction(opener, PAXG, VXAU);
    basket.warp(T0 + DURATION + 1n);

    await expectFailure(basket.closeAuction(0, Keypair.generate().publicKey));
    await basket.closeAuction(0, opener.publicKey);
  });
  it("emits AuctionClosed", async () => {
    const { basket, opener, bidder, expected } = await goldRally();
    await basket.openAuction(opener, PAXG, VXAU);
    const owed = buyAmount(expected.sellTotal, expected.startPrice);
    await basket.bid(bidder, 0, expected.sellTotal, owed);

    await basket.closeAuction(0, opener.publicKey);

    const [event] = basket.events();
    expect(event.name).to.equal("auctionClosed");
    expect(big(event.data.sellSold as BN)).to.equal(expected.sellTotal);
    expect(big(event.data.buyReceived as BN)).to.equal(owed);
  });
});
