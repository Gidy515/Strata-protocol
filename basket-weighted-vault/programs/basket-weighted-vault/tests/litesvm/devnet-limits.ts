import {
  buyAmount,
  expectedAuction,
  expectFitsDevnet,
  PAXG,
  PYTH_PRICES,
  recipeAmounts,
  SPYX,
  T0,
  TestBasket,
  usdPrices,
  USDY,
  VXAU,
} from "./helpers/basket";

const BIDDER_FUNDS = [
  100_000_000_000n,
  10_000_000_000n,
  100_000_000_000n,
  100_000_000_000n,
];

describe("devnet limits", () => {
  it("initializes and deposits within the default compute and size limits", async () => {
    const basket = await TestBasket.create();

    await basket.initialize();
    expectFitsDevnet(basket, "initialize_basket_config");

    const user = basket.user(recipeAmounts(101_000n));
    await basket.deposit(user, recipeAmounts(100_000n), 0n);
    expectFitsDevnet(basket, "deposit_to_basket_v2 (first)");
    await basket.deposit(user, recipeAmounts(1_000n), 0n);
    expectFitsDevnet(basket, "deposit_to_basket_v2 (later)");

    await basket.setPaused(basket.authority, true);
    expectFitsDevnet(basket, "set_paused");
  });

  it("runs an auction within the default compute and size limits", async () => {
    const { basket } = await TestBasket.withHoldings(100_000n);
    const rally = 480_000_000_000n;
    basket.putPrice(PAXG, rally, T0);
    const prices = usdPrices([0n, rally, PYTH_PRICES[USDY], PYTH_PRICES[SPYX]]);
    const expected = expectedAuction(
      recipeAmounts(100_000n),
      prices,
      PAXG,
      VXAU
    );
    const opener = basket.funded();

    await basket.openAuction(opener, PAXG, VXAU);
    expectFitsDevnet(basket, "open_auction");
    await basket.bid(
      basket.user(BIDDER_FUNDS),
      0,
      expected.sellTotal,
      buyAmount(expected.sellTotal, expected.startPrice)
    );
    expectFitsDevnet(basket, "bid");
    await basket.closeAuction(0, opener.publicKey);
    expectFitsDevnet(basket, "close_auction");
  });

  it("sells a Token-2022 token within the default compute and size limits", async () => {
    const { basket } = await TestBasket.withHoldings(100_000n);
    const rally = 96_000_000_000n;
    basket.putPrice(SPYX, rally, T0);
    const prices = usdPrices([0n, PYTH_PRICES[PAXG], PYTH_PRICES[USDY], rally]);
    const expected = expectedAuction(
      recipeAmounts(100_000n),
      prices,
      SPYX,
      USDY
    );

    await basket.openAuction(basket.funded(), SPYX, USDY);
    await basket.bid(
      basket.user(BIDDER_FUNDS),
      0,
      expected.sellTotal,
      buyAmount(expected.sellTotal, expected.startPrice)
    );
    expectFitsDevnet(basket, "bid (Token-2022)");
  });
});
