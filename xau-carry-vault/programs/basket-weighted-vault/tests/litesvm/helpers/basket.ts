import {
  AnchorError,
  BN,
  BorshAccountsCoder,
  EventParser,
  LangErrorCode,
  Idl,
  IdlAccounts,
  IdlTypes,
  Program,
} from "@anchor-lang/core";
import {
  ACCOUNT_SIZE,
  Account,
  ExtensionType,
  MINT_SIZE,
  MintLayout,
  TOKEN_2022_PROGRAM_ID,
  TOKEN_PROGRAM_ID,
  createAssociatedTokenAccountInstruction,
  createInitializeMint2Instruction,
  createInitializeScaledUiAmountConfigInstruction,
  createInitializeTransferFeeConfigInstruction,
  createMintToInstruction,
  createUpdateMultiplierDataInstruction,
  getAssociatedTokenAddressSync,
  getMintLen,
  unpackAccount,
} from "@solana/spl-token";
import {
  AccountMeta,
  Keypair,
  LAMPORTS_PER_SOL,
  PublicKey,
  SystemProgram,
  Transaction,
} from "@solana/web3.js";
import { expect } from "chai";
import { readFileSync } from "fs";
import * as path from "path";
import idl from "../../../../../target/idl/basket_weighted_vault.json";
import xauIdl from "../../../../../target/idl/xau_carry_vault.json";
import { BasketWeightedVault } from "../../../../../target/types/basket_weighted_vault";
import { LiteSVMProvider, LiteSVMTransactionError } from "./litesvm-provider";

export const PROGRAM_ID = new PublicKey(idl.address);
export const XAU_PROGRAM_ID = new PublicKey(xauIdl.address);
export const PYTH_RECEIVER_ID = new PublicKey(
  "rec5EKMGg6MxZYaMdyBfgwp4d5rB9T1VQH5pJv5LtFJ"
);
const UPGRADEABLE_LOADER_ID = new PublicKey(
  "BPFLoaderUpgradeab1e11111111111111111111111"
);
const PROGRAM_PATH = path.join(
  __dirname,
  "../../../../../target/deploy/basket_weighted_vault.so"
);
const PRICE_UPDATE_DISCRIMINATOR = [34, 241, 35, 99, 157, 126, 244, 205];

export const VXAU = 0;
export const PAXG = 1;
export const USDY = 2;
export const SPYX = 3;
export const NUM_ASSETS = 4;

export const DECIMALS = [6, 8, 6, 8];
export const WEIGHTS = [4_000, 2_000, 2_500, 1_500];
export const UNITS = [400_000n, 5_000n, 200_000n, 25_000n];
export const FEEDS = [0, 1, 2, 3].map((i) => new Array<number>(32).fill(i));
export const PYTH_PRICES = [
  0n,
  400_000_000_000n,
  125_000_000n,
  60_000_000_000n,
];
export const PYTH_EXPONENT = -8;

export const T0 = 1_800_000_000n;
export const DURATION = 1_200n;
export const COOLDOWN = 600n;
export const MAX_AGE_TRADED = 60n;
export const MAX_AGE_NAV = 259_200n;
export const BAND_BPS = 200;
export const MAX_LOT_USD = 5_000_000_000n;
export const MIN_LOT_USD = 100_000_000n;
export const START_PREMIUM_BPS = 200n;
export const MAX_DISCOUNT_BPS = 100n;

export const ONE_SHARE = 1_000_000n;
export const MIN_LOCKED_SHARES = 1_000n;
export const D18 = 10n ** 18n;
export const BPS = 10_000n;

export type BasketConfig = IdlAccounts<BasketWeightedVault>["basketConfig"];
export type Auction = IdlAccounts<BasketWeightedVault>["auction"];
export type UserPosition = IdlAccounts<BasketWeightedVault>["userPosition"];
export type BasketSettings = IdlTypes<BasketWeightedVault>["basketSettings"];
export type BasketErrorName =
  | Capitalize<BasketWeightedVault["errors"][number]["name"]>
  | keyof typeof LangErrorCode;

export interface User {
  keypair: Keypair;
  accounts: PublicKey[];
}

export interface Options {
  multiplier?: number;
  multiplierChangeTs?: bigint;
  spyxWithTransferFee?: boolean;
  vxauFreezeAuthority?: boolean;
}

export interface PriceOverrides {
  feedId?: number[];
  conf?: bigint;
  verified?: boolean;
  exponent?: number;
}

export interface BasketEvent {
  name: string;
  data: Record<string, unknown>;
}

export interface InitArgs {
  tokenMints: PublicKey[];
  weights: number[];
  settings: BasketSettings;
}

export class TestBasket {
  readonly provider = new LiteSVMProvider();
  readonly program = new Program<BasketWeightedVault>(
    idl as BasketWeightedVault,
    this.provider
  );
  authority = Keypair.generate();
  readonly vault = Keypair.generate().publicKey;
  readonly vaultV1State = Keypair.generate().publicKey;
  readonly basketConfig: PublicKey;
  readonly programData: PublicKey;
  readonly tokenPrograms = [
    TOKEN_PROGRAM_ID,
    TOKEN_PROGRAM_ID,
    TOKEN_PROGRAM_ID,
    TOKEN_2022_PROGRAM_ID,
  ];
  readonly priceAccounts = [0, 1, 2, 3].map(() => Keypair.generate().publicKey);
  mints: PublicKey[] = [];
  vaultTokens: PublicKey[] = [];

  private constructor() {
    this.provider.addProgram(
      PROGRAM_ID,
      readFileSync(PROGRAM_PATH),
      UPGRADEABLE_LOADER_ID
    );
    this.programData = PublicKey.findProgramAddressSync(
      [PROGRAM_ID.toBuffer()],
      UPGRADEABLE_LOADER_ID
    )[0];
    this.basketConfig = PublicKey.findProgramAddressSync(
      [Buffer.from("basket_config"), this.vault.toBuffer()],
      PROGRAM_ID
    )[0];
  }

  static async create(options: Options = {}): Promise<TestBasket> {
    const basket = new TestBasket();
    basket.provider.airdrop(
      basket.authority.publicKey,
      BigInt(10 * LAMPORTS_PER_SOL)
    );
    basket.setUpgradeAuthority(basket.authority.publicKey);
    basket.warp(T0);

    const vxauMint = PublicKey.findProgramAddressSync(
      [Buffer.from("vxau_mint"), basket.vaultV1State.toBuffer()],
      XAU_PROGRAM_ID
    )[0];
    await basket.putVaultV1State(basket.vaultV1State, vxauMint, XAU_PROGRAM_ID);
    basket.putVxauMint(vxauMint, options.vxauFreezeAuthority ?? false);
    const [paxg, usdy] = basket.createClassicMints(
      DECIMALS[PAXG],
      DECIMALS[USDY]
    );
    const spyx = options.spyxWithTransferFee
      ? basket.createTransferFeeMint(DECIMALS[SPYX])
      : basket.createScaledUiMint(
          DECIMALS[SPYX],
          options.multiplier ?? 1,
          options.multiplierChangeTs
        );
    basket.mints = [vxauMint, paxg, usdy, spyx];
    basket.vaultTokens = basket.mints.map((mint) =>
      vaultTokenAddress(basket.basketConfig, mint)
    );

    for (const i of [PAXG, USDY, SPYX]) {
      basket.putPrice(i, PYTH_PRICES[i], T0);
    }
    return basket;
  }

  static async initialized(options: Options = {}): Promise<TestBasket> {
    const basket = await TestBasket.create(options);
    await basket.initialize();
    return basket;
  }

  static async withHoldings(
    shares = 100_000n,
    options: Options = {}
  ): Promise<{ basket: TestBasket; depositor: User }> {
    const basket = await TestBasket.initialized(options);
    const depositor = basket.user(recipeAmounts(shares));
    await basket.deposit(depositor, recipeAmounts(shares), 0n);
    return { basket, depositor };
  }

  defaultInitArgs(): InitArgs {
    return {
      tokenMints: [...this.mints],
      weights: [...WEIGHTS],
      settings: {
        assets: [0, 1, 2, 3].map((i) => ({
          priceSource:
            i === VXAU
              ? { fixed: { priceUsdD18: bn(D18) } }
              : { pyth: { feedId: FEEDS[i] } },
          bandBps: BAND_BPS,
          maxLotUsd: bn(MAX_LOT_USD),
        })),
        initialUnits: UNITS.map(bn),
        rebalance: {
          maxAgeTradedS: Number(MAX_AGE_TRADED),
          maxAgeNavS: Number(MAX_AGE_NAV),
          maxConfBps: 100,
          startPremiumBps: Number(START_PREMIUM_BPS),
          maxDiscountBps: Number(MAX_DISCOUNT_BPS),
          auctionDurationS: Number(DURATION),
          cooldownS: Number(COOLDOWN),
          minLotUsd: bn(MIN_LOT_USD),
        },
      },
    };
  }

  initialize(
    args: InitArgs = this.defaultInitArgs(),
    mints: PublicKey[] = this.mints,
    vaultV1State: PublicKey = this.vaultV1State
  ): Promise<string> {
    const accounts: Record<string, PublicKey> = {
      authority: this.authority.publicKey,
      program: PROGRAM_ID,
      programData: this.programData,
      vault: this.vault,
      basketConfig: this.basketConfig,
      vaultV1State,
      systemProgram: SystemProgram.programId,
    };
    mints.forEach((mint, i) => {
      accounts[`mint${i}`] = mint;
      accounts[`vaultToken${i}`] = vaultTokenAddress(this.basketConfig, mint);
      accounts[`tokenProgram${i}`] = this.tokenPrograms[i];
    });
    return this.program.methods
      .initializeBasketConfig(args.tokenMints, args.weights, args.settings)
      .accountsPartial(accounts)
      .signers([this.authority])
      .rpc();
  }

  depositAccounts(user: User): Record<string, PublicKey> {
    const accounts: Record<string, PublicKey> = {
      user: user.keypair.publicKey,
      basketConfig: this.basketConfig,
      vault: this.vault,
      position: this.positionAddress(user.keypair.publicKey),
      systemProgram: SystemProgram.programId,
    };
    this.mints.forEach((mint, i) => {
      accounts[`mint${i}`] = mint;
      accounts[`userToken${i}`] = user.accounts[i];
      accounts[`vaultToken${i}`] = this.vaultTokens[i];
      accounts[`tokenProgram${i}`] = this.tokenPrograms[i];
    });
    return accounts;
  }

  deposit(
    user: User,
    maxAmounts: bigint[],
    minSharesOut: bigint,
    overrides: Record<string, PublicKey> = {}
  ): Promise<string> {
    return this.program.methods
      .depositToBasketV2(maxAmounts.map(bn), bn(minSharesOut))
      .accountsPartial({ ...this.depositAccounts(user), ...overrides })
      .signers([user.keypair])
      .rpc();
  }

  openAuctionRemainingAccounts(): AccountMeta[] {
    const readonly = (pubkey: PublicKey): AccountMeta => ({
      pubkey,
      isSigner: false,
      isWritable: false,
    });
    return [
      ...this.vaultTokens.map(readonly),
      ...[PAXG, USDY, SPYX].map((i) => readonly(this.priceAccounts[i])),
      readonly(this.mints[SPYX]),
    ];
  }

  openAuction(
    opener: Keypair,
    sellIndex: number,
    buyIndex: number,
    remaining: AccountMeta[] = this.openAuctionRemainingAccounts()
  ): Promise<string> {
    return this.program.methods
      .openAuction(sellIndex, buyIndex)
      .accountsPartial({
        opener: opener.publicKey,
        basketConfig: this.basketConfig,
        auction: this.auctionAddress(this.fetchBasket().auctionNonce),
        systemProgram: SystemProgram.programId,
      })
      .remainingAccounts(remaining)
      .signers([opener])
      .rpc();
  }

  bid(
    bidder: User,
    nonce: BN | number,
    sellAmount: bigint,
    maxBuyAmount: bigint,
    receiver?: PublicKey,
    overrides: Record<string, PublicKey> = {}
  ): Promise<string> {
    const auction = this.fetchAuction(nonce);
    const [sell, buy] = [auction.sellIndex, auction.buyIndex];
    return this.program.methods
      .bid(bn(sellAmount), bn(maxBuyAmount))
      .accountsPartial({
        bidder: bidder.keypair.publicKey,
        basketConfig: this.basketConfig,
        auction: this.auctionAddress(nonce),
        sellMint: this.mints[sell],
        buyMint: this.mints[buy],
        vaultSellAccount: this.vaultTokens[sell],
        vaultBuyAccount: this.vaultTokens[buy],
        bidderSellAccount: receiver ?? bidder.accounts[sell],
        bidderBuyAccount: bidder.accounts[buy],
        sellTokenProgram: this.tokenPrograms[sell],
        buyTokenProgram: this.tokenPrograms[buy],
        ...overrides,
      })
      .signers([bidder.keypair])
      .rpc();
  }

  closeAuction(nonce: BN | number, opener: PublicKey): Promise<string> {
    return this.program.methods
      .closeAuction()
      .accountsPartial({
        basketConfig: this.basketConfig,
        auction: this.auctionAddress(nonce),
        opener,
      })
      .rpc();
  }

  setPaused(signer: Keypair, paused: boolean): Promise<string> {
    return this.program.methods
      .setPaused(paused)
      .accountsPartial({
        authority: signer.publicKey,
        basketConfig: this.basketConfig,
      })
      .signers([signer])
      .rpc();
  }

  user(amounts: bigint[]): User {
    const keypair = this.funded();
    const owner = keypair.publicKey;
    const accounts = this.mints.map((mint, i) =>
      getAssociatedTokenAddressSync(mint, owner, false, this.tokenPrograms[i])
    );
    const payer = this.provider.publicKey;
    const tx = new Transaction();
    this.mints.forEach((mint, i) => {
      tx.add(
        createAssociatedTokenAccountInstruction(
          payer,
          accounts[i],
          owner,
          mint,
          this.tokenPrograms[i]
        )
      );
      if (i !== VXAU && amounts[i] > 0n) {
        tx.add(
          createMintToInstruction(
            mint,
            accounts[i],
            payer,
            amounts[i],
            [],
            this.tokenPrograms[i]
          )
        );
      }
    });
    this.provider.sendSync(tx);
    this.setTokenAmount(accounts[VXAU], amounts[VXAU]);
    return { keypair, accounts };
  }

  tokenAccountFor(owner: PublicKey, index: number, amount: bigint): PublicKey {
    const account = Keypair.generate().publicKey;
    const data = Buffer.alloc(ACCOUNT_SIZE);
    data.set(this.mints[index].toBuffer(), 0);
    data.set(owner.toBuffer(), 32);
    data.writeBigUInt64LE(amount, 64);
    data[108] = 1;
    this.provider.setAccount(account, this.tokenPrograms[index], data);
    return account;
  }

  funded(): Keypair {
    const keypair = Keypair.generate();
    this.provider.airdrop(keypair.publicKey, BigInt(LAMPORTS_PER_SOL));
    return keypair;
  }

  putPrice(
    index: number,
    price: bigint,
    publishTime: bigint,
    overrides: PriceOverrides = {}
  ): void {
    const verified = overrides.verified ?? true;
    const data = Buffer.alloc(
      8 + 32 + (verified ? 1 : 2) + 32 + 8 * 2 + 4 + 8 * 5
    );
    let offset = 0;
    data.set(PRICE_UPDATE_DISCRIMINATOR, offset);
    offset += 8;
    data.set(Keypair.generate().publicKey.toBuffer(), offset);
    offset += 32;
    if (verified) {
      data[offset++] = 1;
    } else {
      data[offset++] = 0;
      data[offset++] = 5;
    }
    data.set(overrides.feedId ?? FEEDS[index], offset);
    offset += 32;
    data.writeBigInt64LE(price, offset);
    offset += 8;
    data.writeBigUInt64LE(overrides.conf ?? price / 1_000n, offset);
    offset += 8;
    data.writeInt32LE(overrides.exponent ?? PYTH_EXPONENT, offset);
    offset += 4;
    data.writeBigInt64LE(publishTime, offset);
    offset += 8;
    data.writeBigInt64LE(publishTime - 1n, offset);
    offset += 8;
    data.writeBigInt64LE(price, offset);
    offset += 8;
    data.writeBigUInt64LE(0n, offset);
    offset += 8;
    data.writeBigUInt64LE(1n, offset);
    this.provider.setAccount(this.priceAccounts[index], PYTH_RECEIVER_ID, data);
  }

  events(): BasketEvent[] {
    const logs = this.provider.lastResult?.logs() ?? [];
    const parser = new EventParser(PROGRAM_ID, this.program.coder);
    return [...parser.parseLogs(logs)].map((event) => ({
      name: event.name,
      data: event.data as Record<string, unknown>,
    }));
  }

  classicMint(decimals: number): PublicKey {
    return this.createClassicMints(decimals)[0];
  }

  async putVaultV1State(
    key: PublicKey,
    vxauMint: PublicKey,
    owner: PublicKey
  ): Promise<void> {
    const coder = new BorshAccountsCoder(xauIdl as Idl);
    const data = await coder.encode("VaultV1State", {
      admin: Keypair.generate().publicKey,
      usdc_mint: Keypair.generate().publicKey,
      vxau_mint: vxauMint,
      total_deposited: new BN(0),
      bump: 255,
    });
    this.provider.setAccount(key, owner, data);
  }

  putVxauMint(address: PublicKey, withFreezeAuthority: boolean): void {
    const data = Buffer.alloc(MINT_SIZE);
    MintLayout.encode(
      {
        mintAuthorityOption: 1,
        mintAuthority: this.vaultV1State,
        supply: 0n,
        decimals: DECIMALS[VXAU],
        isInitialized: true,
        freezeAuthorityOption: withFreezeAuthority ? 1 : 0,
        freezeAuthority: withFreezeAuthority
          ? Keypair.generate().publicKey
          : PublicKey.default,
      },
      data
    );
    this.provider.setAccount(address, TOKEN_PROGRAM_ID, data);
  }

  setTokenAmount(tokenAccount: PublicKey, amount: bigint): void {
    const info = this.provider.getAccountInfo(tokenAccount)!;
    const data = Buffer.from(info.data);
    data.writeBigUInt64LE(amount, 64);
    this.provider.setAccount(
      tokenAccount,
      info.owner,
      data,
      BigInt(info.lamports)
    );
  }

  setUpgradeAuthority(authority: PublicKey | null): void {
    const info = this.provider.getAccountInfo(this.programData)!;
    const data = Buffer.from(info.data);
    data[12] = authority ? 1 : 0;
    data.set((authority ?? PublicKey.default).toBuffer(), 13);
    this.provider.setAccount(
      this.programData,
      UPGRADEABLE_LOADER_ID,
      data,
      BigInt(info.lamports)
    );
  }

  warp(unixTimestamp: bigint): void {
    const clock = this.provider.svm.getClock();
    clock.unixTimestamp = unixTimestamp;
    this.provider.svm.setClock(clock);
  }

  now(): bigint {
    return this.provider.svm.getClock().unixTimestamp;
  }

  fetchBasket(): BasketConfig {
    return this.program.coder.accounts.decode<BasketConfig>(
      "basketConfig",
      this.accountInfo(this.basketConfig).data
    );
  }

  fetchAuction(nonce: BN | number): Auction {
    return this.program.coder.accounts.decode<Auction>(
      "auction",
      this.accountInfo(this.auctionAddress(nonce)).data
    );
  }

  fetchPosition(user: PublicKey): UserPosition {
    return this.program.coder.accounts.decode<UserPosition>(
      "userPosition",
      this.accountInfo(this.positionAddress(user)).data
    );
  }

  auctionAddress(nonce: BN | number): PublicKey {
    return PublicKey.findProgramAddressSync(
      [
        Buffer.from("auction"),
        this.basketConfig.toBuffer(),
        new BN(nonce).toArrayLike(Buffer, "le", 8),
      ],
      PROGRAM_ID
    )[0];
  }

  positionAddress(user: PublicKey): PublicKey {
    return PublicKey.findProgramAddressSync(
      [Buffer.from("position"), this.basketConfig.toBuffer(), user.toBuffer()],
      PROGRAM_ID
    )[0];
  }

  tokenAccount(address: PublicKey): Account {
    const info = this.accountInfo(address);
    return unpackAccount(address, info, info.owner);
  }

  balance(tokenAccount: PublicKey): bigint {
    return this.tokenAccount(tokenAccount).amount;
  }

  balances(accounts: PublicKey[]): bigint[] {
    return accounts.map((account) => this.balance(account));
  }

  lamports(address: PublicKey): bigint {
    const info = this.provider.getAccountInfo(address);
    return info ? BigInt(info.lamports) : 0n;
  }

  accountExists(address: PublicKey): boolean {
    return this.provider.getAccountInfo(address) !== null;
  }

  private accountInfo(address: PublicKey) {
    const info = this.provider.getAccountInfo(address);
    if (info === null) {
      throw new Error(`account ${address.toBase58()} does not exist`);
    }
    return info;
  }

  private createClassicMints(...decimals: number[]): PublicKey[] {
    const payer = this.provider.publicKey;
    const rent = Number(
      this.provider.svm.minimumBalanceForRentExemption(BigInt(MINT_SIZE))
    );
    const mints = decimals.map(() => Keypair.generate());
    const tx = new Transaction();
    mints.forEach((mint, i) => {
      tx.add(
        SystemProgram.createAccount({
          fromPubkey: payer,
          newAccountPubkey: mint.publicKey,
          space: MINT_SIZE,
          lamports: rent,
          programId: TOKEN_PROGRAM_ID,
        }),
        createInitializeMint2Instruction(
          mint.publicKey,
          decimals[i],
          payer,
          null
        )
      );
    });
    this.provider.sendSync(tx, mints);
    return mints.map((mint) => mint.publicKey);
  }

  private createScaledUiMint(
    decimals: number,
    multiplier: number,
    changeTs?: bigint
  ): PublicKey {
    const payer = this.provider.publicKey;
    const mint = Keypair.generate();
    const space = getMintLen([ExtensionType.ScaledUiAmountConfig]);
    const tx = new Transaction().add(
      SystemProgram.createAccount({
        fromPubkey: payer,
        newAccountPubkey: mint.publicKey,
        space,
        lamports: Number(
          this.provider.svm.minimumBalanceForRentExemption(BigInt(space))
        ),
        programId: TOKEN_2022_PROGRAM_ID,
      }),
      createInitializeScaledUiAmountConfigInstruction(
        mint.publicKey,
        payer,
        multiplier,
        TOKEN_2022_PROGRAM_ID
      ),
      createInitializeMint2Instruction(
        mint.publicKey,
        decimals,
        payer,
        null,
        TOKEN_2022_PROGRAM_ID
      )
    );
    if (changeTs !== undefined) {
      tx.add(
        createUpdateMultiplierDataInstruction(
          mint.publicKey,
          payer,
          multiplier,
          changeTs,
          [],
          TOKEN_2022_PROGRAM_ID
        )
      );
    }
    this.provider.sendSync(tx, [mint]);
    return mint.publicKey;
  }

  private createTransferFeeMint(decimals: number): PublicKey {
    const payer = this.provider.publicKey;
    const mint = Keypair.generate();
    const space = getMintLen([ExtensionType.TransferFeeConfig]);
    const tx = new Transaction().add(
      SystemProgram.createAccount({
        fromPubkey: payer,
        newAccountPubkey: mint.publicKey,
        space,
        lamports: Number(
          this.provider.svm.minimumBalanceForRentExemption(BigInt(space))
        ),
        programId: TOKEN_2022_PROGRAM_ID,
      }),
      createInitializeTransferFeeConfigInstruction(
        mint.publicKey,
        payer,
        payer,
        100,
        1_000_000n,
        TOKEN_2022_PROGRAM_ID
      ),
      createInitializeMint2Instruction(
        mint.publicKey,
        decimals,
        payer,
        null,
        TOKEN_2022_PROGRAM_ID
      )
    );
    this.provider.sendSync(tx, [mint]);
    return mint.publicKey;
  }
}

export function vaultTokenAddress(
  basketConfig: PublicKey,
  mint: PublicKey
): PublicKey {
  return PublicKey.findProgramAddressSync(
    [Buffer.from("asset_vault"), basketConfig.toBuffer(), mint.toBuffer()],
    PROGRAM_ID
  )[0];
}

export function recipeAmounts(shares: bigint): bigint[] {
  return UNITS.map((units) => units * shares);
}

export function bn(value: bigint): BN {
  return new BN(value.toString());
}

export function big(value: BN | number): bigint {
  return BigInt(value.toString());
}

export function priceD18(pythPrice: bigint): bigint {
  return pythPrice * 10n ** BigInt(18 + PYTH_EXPONENT);
}

export function usdPrices(pythPrices: bigint[] = PYTH_PRICES): bigint[] {
  return pythPrices.map((price, i) => (i === VXAU ? D18 : priceD18(price)));
}

export function valueUsd(raw: bigint, price: bigint, decimals: number): bigint {
  return (raw * price) / 10n ** BigInt(decimals);
}

function mulDivCeil(a: bigint, b: bigint, d: bigint): bigint {
  return (a * b + d - 1n) / d;
}

export function expectedAuction(
  balances: bigint[],
  prices: bigint[],
  sell: number,
  buy: number
): { lotUsd: bigint; sellTotal: bigint; startPrice: bigint; endPrice: bigint } {
  const values = balances.map((raw, i) =>
    valueUsd(raw, prices[i], DECIMALS[i])
  );
  const total = values.reduce((a, b) => a + b, 0n);
  const surplus = values[sell] - mulDivCeil(total, BigInt(WEIGHTS[sell]), BPS);
  const deficit = (total * BigInt(WEIGHTS[buy])) / BPS - values[buy];
  const cap = MAX_LOT_USD * 10n ** 12n;
  const lotUsd = [surplus, deficit, cap].reduce((a, b) => (a < b ? a : b));
  const sellTotal = (lotUsd * 10n ** BigInt(DECIMALS[sell])) / prices[sell];
  const ratio =
    (prices[sell] * 10n ** BigInt(DECIMALS[buy]) * D18) /
    (prices[buy] * 10n ** BigInt(DECIMALS[sell]));
  return {
    lotUsd,
    sellTotal,
    startPrice: mulDivCeil(ratio, BPS + START_PREMIUM_BPS, BPS),
    endPrice: mulDivCeil(ratio, BPS - MAX_DISCOUNT_BPS, BPS),
  };
}

export function priceAt(
  start: bigint,
  end: bigint,
  startTs: bigint,
  endTs: bigint,
  now: bigint
): bigint {
  if (now <= startTs) return start;
  if (now >= endTs) return end;
  return start - ((start - end) * (now - startTs)) / (endTs - startTs);
}

export function buyAmount(fill: bigint, price: bigint): bigint {
  return mulDivCeil(fill, price, D18);
}

export async function expectBasketError(
  transaction: Promise<unknown>,
  expected: BasketErrorName
): Promise<void> {
  let error: unknown;
  try {
    await transaction;
  } catch (err) {
    error = err;
  }
  expect(error, `expected the transaction to fail with ${expected}`).to.exist;

  const logs =
    error instanceof LiteSVMTransactionError
      ? error.logs
      : (error as { logs?: string[] }).logs ?? [];
  const anchorError =
    error instanceof AnchorError ? error : AnchorError.parse(logs);
  expect(anchorError, `expected ${expected}, got: ${error}`).to.not.be.null;
  expect(anchorError!.error.errorCode.code.toLowerCase()).to.equal(
    expected.toLowerCase()
  );
}

export async function expectFailure(
  transaction: Promise<unknown>
): Promise<void> {
  let failed = false;
  try {
    await transaction;
  } catch {
    failed = true;
  }
  expect(failed, "expected the transaction to fail").to.be.true;
}

const MAX_TX_BYTES = 1_232;
const DEFAULT_CU_LIMIT = 200_000;

export function expectFitsDevnet(basket: TestBasket, name: string): void {
  const units = Number(basket.provider.lastResult!.computeUnitsConsumed());
  const bytes = basket.provider.lastTxBytes;
  expect(bytes, `${name} transaction is ${bytes} bytes`).to.be.at.most(
    MAX_TX_BYTES
  );
  expect(units, `${name} uses ${units} CU`).to.be.at.most(DEFAULT_CU_LIMIT);
}
