import { expect } from "chai";
import {
  DURATION,
  PAXG,
  T0,
  TestBasket,
  VXAU,
  expectBasketError,
  recipeAmounts,
} from "./helpers/basket";

const RALLY_PAXG = 480_000_000_000n;
const BIDDER_FUNDS = [
  100_000_000_000n,
  10_000_000_000n,
  100_000_000_000n,
  100_000_000_000n,
];

describe("set_paused", () => {
  it("lets the authority pause and unpause deposits", async () => {
    const { basket } = await TestBasket.withHoldings(1_000n);
    const user = basket.user(recipeAmounts(2_000n));

    await basket.setPaused(basket.authority, true);
    expect(basket.fetchBasket().paused).to.be.true;
    await expectBasketError(
      basket.deposit(user, recipeAmounts(1_000n), 0n),
      "Paused"
    );

    await basket.setPaused(basket.authority, false);
    expect(basket.fetchBasket().paused).to.be.false;
    await basket.deposit(user, recipeAmounts(1_000n), 0n);
  });

  it("rejects a stranger", async () => {
    const basket = await TestBasket.initialized();

    await expectBasketError(
      basket.setPaused(basket.funded(), true),
      "Unauthorized"
    );
    expect(basket.fetchBasket().paused).to.be.false;
  });

  it("blocks new auctions and bids but still lets auctions close", async () => {
    const { basket } = await TestBasket.withHoldings(100_000n);
    basket.putPrice(PAXG, RALLY_PAXG, T0);
    const opener = basket.funded();
    const bidder = basket.user(BIDDER_FUNDS);
    await basket.openAuction(opener, PAXG, VXAU);

    await basket.setPaused(basket.authority, true);

    await expectBasketError(
      basket.bid(bidder, 0, 1_000n, 10n ** 12n),
      "Paused"
    );
    basket.warp(T0 + DURATION + 1n);
    await basket.closeAuction(0, opener.publicKey);
    expect(basket.fetchBasket().activeAuction).to.be.false;
    await expectBasketError(basket.openAuction(opener, PAXG, VXAU), "Paused");
  });
  it("emits BasketPauseChanged", async () => {
    const basket = await TestBasket.initialized();

    await basket.setPaused(basket.authority, true);

    const [event] = basket.events();
    expect(event.name).to.equal("basketPauseChanged");
    expect(event.data.paused).to.be.true;
  });
});
