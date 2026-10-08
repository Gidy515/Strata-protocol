import { Keypair, PublicKey } from "@solana/web3.js";
import { expect } from "chai";
import {
  DECIMALS,
  PROGRAM_ID,
  SPYX,
  TestBasket,
  UNITS,
  USDY,
  PAXG,
  VXAU,
  WEIGHTS,
  big,
  expectBasketError,
  expectFailure,
  vaultTokenAddress,
} from "./helpers/basket";

describe("initialize_basket_config", () => {
  it("creates the basket config and its four token vaults", async () => {
    const basket = await TestBasket.initialized();

    const config = basket.fetchBasket();
    expect(config.authority.equals(basket.authority.publicKey)).to.be.true;
    expect(config.vault.equals(basket.vault)).to.be.true;
    expect(config.tokenMints.map((m) => m.toBase58())).to.deep.equal(
      basket.mints.map((m) => m.toBase58())
    );
    expect(config.weights).to.deep.equal(WEIGHTS);
    expect(config.initialUnits.map(big)).to.deep.equal(UNITS);
    expect(big(config.totalShares)).to.equal(0n);
    expect(big(config.auctionNonce)).to.equal(0n);
    expect(config.activeAuction).to.be.false;
    expect(config.paused).to.be.false;

    expect(
      PublicKey.createProgramAddressSync(
        [
          Buffer.from("basket_config"),
          basket.vault.toBuffer(),
          Buffer.from([config.bump]),
        ],
        PROGRAM_ID
      ).equals(basket.basketConfig)
    ).to.be.true;

    config.assets.forEach((asset, i) => {
      expect(asset.vaultToken.equals(basket.vaultTokens[i])).to.be.true;
      expect(asset.tokenProgram.equals(basket.tokenPrograms[i])).to.be.true;
      expect(asset.decimals).to.equal(DECIMALS[i]);
      expect(asset.scaledUi).to.equal(i === SPYX);

      const vault = basket.tokenAccount(basket.vaultTokens[i]);
      expect(vault.mint.equals(basket.mints[i])).to.be.true;
      expect(vault.owner.equals(basket.basketConfig)).to.be.true;
      expect(vault.amount).to.equal(0n);
    });
  });

  it("cannot be initialized twice", async () => {
    const basket = await TestBasket.initialized();

    await expectFailure(basket.initialize());
  });

  it("rejects anyone but the program's upgrade authority", async () => {
    const basket = await TestBasket.create();
    basket.authority = basket.funded();

    await expectBasketError(basket.initialize(), "Unauthorized");
  });

  it("rejects a program with no upgrade authority", async () => {
    const basket = await TestBasket.create();
    basket.setUpgradeAuthority(null);

    await expectBasketError(basket.initialize(), "Unauthorized");
  });

  it("rejects a vXAU mint that Vault 1 did not record", async () => {
    const basket = await TestBasket.create();
    const lookalike = Keypair.generate().publicKey;
    basket.putVxauMint(lookalike, false);
    const mints = [lookalike, ...basket.mints.slice(1)];
    const args = { ...basket.defaultInitArgs(), tokenMints: mints };

    await expectBasketError(basket.initialize(args, mints), "InvalidVxauMint");
  });

  it("rejects a fake Vault 1 state owned by another program", async () => {
    const basket = await TestBasket.create();
    const fake = Keypair.generate().publicKey;
    await basket.putVaultV1State(
      fake,
      basket.mints[VXAU],
      Keypair.generate().publicKey
    );

    await expectBasketError(
      basket.initialize(basket.defaultInitArgs(), basket.mints, fake),
      "AccountOwnedByWrongProgram"
    );
  });

  it("rejects vXAU with a freeze authority", async () => {
    const basket = await TestBasket.create({ vxauFreezeAuthority: true });

    await expectBasketError(basket.initialize(), "InvalidVxauMint");
  });

  it("rejects token_mints that do not match the mint accounts", async () => {
    const basket = await TestBasket.create();
    const args = basket.defaultInitArgs();
    [args.tokenMints[PAXG], args.tokenMints[USDY]] = [
      args.tokenMints[USDY],
      args.tokenMints[PAXG],
    ];

    await expectBasketError(basket.initialize(args), "MintMismatch");
  });

  it("rejects weights that do not sum to 100%", async () => {
    const basket = await TestBasket.create();
    const args = basket.defaultInitArgs();
    args.weights[USDY] = 2_400;

    await expectBasketError(basket.initialize(args), "InvalidWeightSum");
  });

  it("rejects a band as large as the weight", async () => {
    const basket = await TestBasket.create();
    const args = basket.defaultInitArgs();
    args.settings.assets[PAXG].bandBps = 2_000;

    await expectBasketError(basket.initialize(args), "InvalidTargetWeights");
  });

  it("rejects a zero starting recipe", async () => {
    const basket = await TestBasket.create();
    const args = basket.defaultInitArgs();
    args.settings.initialUnits[PAXG] = args.settings.initialUnits[PAXG].muln(0);

    await expectBasketError(basket.initialize(args), "InvalidInitialUnits");
  });

  it("rejects an unset Pyth feed id", async () => {
    const basket = await TestBasket.create();
    const args = basket.defaultInitArgs();
    args.settings.assets[PAXG].priceSource = {
      pyth: { feedId: new Array(32).fill(0) },
    };

    await expectBasketError(basket.initialize(args), "InvalidPriceSource");
  });

  it("rejects unsafe auction settings", async () => {
    const basket = await TestBasket.create();

    const discount = basket.defaultInitArgs();
    discount.settings.rebalance.maxDiscountBps = 5_000;
    await expectBasketError(basket.initialize(discount), "InvalidConfig");

    const confidence = basket.defaultInitArgs();
    confidence.settings.rebalance.maxConfBps = 5_000;
    await expectBasketError(basket.initialize(confidence), "InvalidConfig");

    const freshness = basket.defaultInitArgs();
    freshness.settings.rebalance.maxAgeTradedS =
      freshness.settings.rebalance.maxAgeNavS + 1;
    await expectBasketError(basket.initialize(freshness), "InvalidConfig");

    await basket.initialize();
  });

  it("rejects a Token-2022 mint with a transfer fee", async () => {
    const basket = await TestBasket.create({ spyxWithTransferFee: true });

    await expectBasketError(basket.initialize(), "UnsupportedMintExtension");
  });

  it("derives each vault token account from the basket and mint", async () => {
    const basket = await TestBasket.initialized();

    basket.mints.forEach((mint, i) => {
      expect(
        vaultTokenAddress(basket.basketConfig, mint).equals(
          basket.vaultTokens[i]
        )
      ).to.be.true;
    });
  });
  it("emits BasketInitialized", async () => {
    const basket = await TestBasket.initialized();

    const [event] = basket.events();
    expect(event.name).to.equal("basketInitialized");
    expect((event.data.basketConfig as PublicKey).equals(basket.basketConfig))
      .to.be.true;
    expect((event.data.vault as PublicKey).equals(basket.vault)).to.be.true;
    expect(
      (event.data.authority as PublicKey).equals(basket.authority.publicKey)
    ).to.be.true;
  });

  it("rejects a mint with too many decimals", async () => {
    const basket = await TestBasket.create();
    const mints = [...basket.mints];
    mints[USDY] = basket.classicMint(19);
    const args = { ...basket.defaultInitArgs(), tokenMints: mints };

    await expectBasketError(
      basket.initialize(args, mints),
      "InvalidMintDecimals"
    );
  });

  it("rejects the same mint twice", async () => {
    const basket = await TestBasket.create();
    const mints = [...basket.mints];
    mints[USDY] = mints[PAXG];
    const args = { ...basket.defaultInitArgs(), tokenMints: mints };

    await expectFailure(basket.initialize(args, mints));
  });
});
