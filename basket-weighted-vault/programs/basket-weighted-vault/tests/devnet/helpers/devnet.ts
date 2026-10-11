import { AnchorProvider, BN, Program, Wallet } from "@anchor-lang/core";
import {
  ASSOCIATED_TOKEN_PROGRAM_ID,
  ExtensionType,
  MINT_SIZE,
  TOKEN_2022_PROGRAM_ID,
  TOKEN_PROGRAM_ID,
  createAssociatedTokenAccountIdempotentInstruction,
  createInitializeMint2Instruction,
  createInitializeScaledUiAmountConfigInstruction,
  createMintToInstruction,
  getAccount,
  getAssociatedTokenAddressSync,
  getMintLen,
} from "@solana/spl-token";
import {
  AccountMeta,
  Connection,
  Keypair,
  PublicKey,
  SystemProgram,
  Transaction,
} from "@solana/web3.js";
import { readFileSync } from "fs";
import { homedir } from "os";
import * as path from "path";
import basketIdl from "../../../../../target/idl/basket_weighted_vault.json";
import { BasketWeightedVault } from "../../../../../target/types/basket_weighted_vault";
import vaultV1Idl from "../../../../../../xau-carry-vault/target/idl/xau_carry_vault.json";
import { XauCarryVault } from "../../../../../../xau-carry-vault/target/types/xau_carry_vault";

export const VXAU = 0;
export const PAXG = 1;
export const USDY = 2;
export const SPYX = 3;

export const DECIMALS = [6, 8, 6, 8];
export const WEIGHTS = [4_000, 2_000, 2_500, 1_500];
const D18 = 10n ** 18n;

// Devnet has no fresh Pyth updates without a paid Hermes key, so the devnet basket uses fixed USD prices.
export const FIXED_PRICES_D18 = [
  1n * D18,
  4_250n * D18,
  (115n * D18) / 100n,
  772n * D18,
];

// Units per share worth $30 vXAU, $30 PAXG, $25 USDY, $15 SPYx, so PAXG starts overweight and vXAU underweight.
export const UNITS = [30_000_000n, 705_882n, 21_739_130n, 1_943_005n];
export const ONE_SHARE = 1_000_000n;

export const AUCTION_DURATION_S = 120;
export const START_PREMIUM_BPS = 200n;
export const MAX_LOT_USD = 1_000_000_000n;
export const MIN_LOT_USD = 10_000_000n;

const UPGRADEABLE_LOADER_ID = new PublicKey(
  "BPFLoaderUpgradeab1e11111111111111111111111"
);

export function bn(value: bigint): BN {
  return new BN(value.toString());
}

export function big(value: BN | number): bigint {
  return BigInt(value.toString());
}

export function programData(programId: PublicKey): PublicKey {
  return PublicKey.findProgramAddressSync(
    [programId.toBuffer()],
    UPGRADEABLE_LOADER_ID
  )[0];
}

export function explorer(signature: string): string {
  return `https://explorer.solana.com/tx/${signature}?cluster=devnet`;
}

// Public devnet RPC rate-limits bursts; wait and retry quietly instead of web3.js logging each 429.
async function fetchWithRetry(
  input: Parameters<typeof fetch>[0],
  init?: Parameters<typeof fetch>[1]
): Promise<Response> {
  for (let attempt = 0; ; attempt++) {
    const response = await fetch(input, init);
    if (response.status !== 429 || attempt === 8) {
      return response;
    }
    await new Promise((resolve) => setTimeout(resolve, 500 * 2 ** attempt));
  }
}

function loadWallet(): Keypair {
  const file =
    process.env.ANCHOR_WALLET ?? path.join(homedir(), ".config/solana/id.json");
  return Keypair.fromSecretKey(
    Uint8Array.from(JSON.parse(readFileSync(file, "utf8")))
  );
}

export class Devnet {
  readonly payer = loadWallet();
  readonly provider = new AnchorProvider(
    new Connection(
      process.env.ANCHOR_PROVIDER_URL ?? "https://api.devnet.solana.com",
      { commitment: "confirmed", fetch: fetchWithRetry }
    ),
    new Wallet(this.payer),
    { commitment: "confirmed" }
  );
  readonly basket = new Program<BasketWeightedVault>(
    basketIdl as BasketWeightedVault,
    this.provider
  );
  readonly vaultV1 = new Program<XauCarryVault>(
    vaultV1Idl as XauCarryVault,
    this.provider
  );
  readonly signatures: Record<string, string> = {};

  get connection(): Connection {
    return this.provider.connection;
  }

  async send(
    label: string,
    tx: Transaction,
    signers: Keypair[] = []
  ): Promise<string> {
    const signature = await this.provider.sendAndConfirm(tx, signers);
    this.signatures[label] = signature;
    return signature;
  }

  record(label: string, signature: string): string {
    this.signatures[label] = signature;
    return signature;
  }

  async createMint(
    decimals: number,
    tokenProgram: PublicKey = TOKEN_PROGRAM_ID
  ): Promise<PublicKey> {
    const mint = Keypair.generate();
    const rent = await this.connection.getMinimumBalanceForRentExemption(
      MINT_SIZE
    );
    await this.send(
      `create mint ${mint.publicKey.toBase58().slice(0, 4)}`,
      new Transaction().add(
        SystemProgram.createAccount({
          fromPubkey: this.payer.publicKey,
          newAccountPubkey: mint.publicKey,
          space: MINT_SIZE,
          lamports: rent,
          programId: tokenProgram,
        }),
        createInitializeMint2Instruction(
          mint.publicKey,
          decimals,
          this.payer.publicKey,
          null,
          tokenProgram
        )
      ),
      [mint]
    );
    return mint.publicKey;
  }

  // Mock SPYx: a Token-2022 mint with the Scaled UI Amount extension, like the real one.
  async createScaledUiMint(decimals: number): Promise<PublicKey> {
    const mint = Keypair.generate();
    const space = getMintLen([ExtensionType.ScaledUiAmountConfig]);
    const rent = await this.connection.getMinimumBalanceForRentExemption(space);
    await this.send(
      "create SPYx mint",
      new Transaction().add(
        SystemProgram.createAccount({
          fromPubkey: this.payer.publicKey,
          newAccountPubkey: mint.publicKey,
          space,
          lamports: rent,
          programId: TOKEN_2022_PROGRAM_ID,
        }),
        createInitializeScaledUiAmountConfigInstruction(
          mint.publicKey,
          this.payer.publicKey,
          1,
          TOKEN_2022_PROGRAM_ID
        ),
        createInitializeMint2Instruction(
          mint.publicKey,
          decimals,
          this.payer.publicKey,
          null,
          TOKEN_2022_PROGRAM_ID
        )
      ),
      [mint]
    );
    return mint.publicKey;
  }

  async tokenAccount(
    mint: PublicKey,
    tokenProgram: PublicKey,
    mintAmount = 0n
  ): Promise<PublicKey> {
    const account = getAssociatedTokenAddressSync(
      mint,
      this.payer.publicKey,
      false,
      tokenProgram
    );
    const tx = new Transaction().add(
      createAssociatedTokenAccountIdempotentInstruction(
        this.payer.publicKey,
        account,
        this.payer.publicKey,
        mint,
        tokenProgram
      )
    );
    if (mintAmount > 0n) {
      tx.add(
        createMintToInstruction(
          mint,
          account,
          this.payer.publicKey,
          mintAmount,
          [],
          tokenProgram
        )
      );
    }
    await this.send(`token account ${mint.toBase58().slice(0, 4)}`, tx);
    return account;
  }

  async balance(account: PublicKey, tokenProgram: PublicKey): Promise<bigint> {
    return (
      await getAccount(this.connection, account, "confirmed", tokenProgram)
    ).amount;
  }
}

export class DevnetVaultV1 {
  readonly state: PublicKey;
  readonly custody: PublicKey;
  readonly vxauMint: PublicKey;
  readonly strategy: PublicKey;

  constructor(readonly devnet: Devnet, readonly usdcMint: PublicKey) {
    const programId = devnet.vaultV1.programId;
    [this.state] = PublicKey.findProgramAddressSync(
      [Buffer.from("vault_v1"), usdcMint.toBuffer()],
      programId
    );
    [this.custody] = PublicKey.findProgramAddressSync(
      [Buffer.from("token_vault"), this.state.toBuffer()],
      programId
    );
    [this.vxauMint] = PublicKey.findProgramAddressSync(
      [Buffer.from("vxau_mint"), this.state.toBuffer()],
      programId
    );
    [this.strategy] = PublicKey.findProgramAddressSync(
      [Buffer.from("strategy_v1"), this.state.toBuffer()],
      programId
    );
  }

  async initialize(): Promise<string> {
    const programId = this.devnet.vaultV1.programId;
    const signature = await this.devnet.vaultV1.methods
      .initializeVaultV1()
      .accountsPartial({
        admin: this.devnet.payer.publicKey,
        program: programId,
        programData: programData(programId),
        usdcMint: this.usdcMint,
        vaultV1State: this.state,
        vaultV1UsdcAccount: this.custody,
        vxauMint: this.vxauMint,
      })
      .rpc();
    return this.devnet.record("initialize_vault_v1", signature);
  }

  async deposit(amount: bigint): Promise<string> {
    const user = this.devnet.payer.publicKey;
    const signature = await this.devnet.vaultV1.methods
      .depositUsdcV1(bn(amount), bn(1n))
      .accountsPartial({
        user,
        vaultV1State: this.state,
        usdcMint: this.usdcMint,
        vaultV1UsdcAccount: this.custody,
        vxauMint: this.vxauMint,
        userUsdcAta: getAssociatedTokenAddressSync(this.usdcMint, user),
        userVxauAta: getAssociatedTokenAddressSync(this.vxauMint, user),
        strategyAccount: this.strategy,
        tokenProgram: TOKEN_PROGRAM_ID,
        associatedTokenProgram: ASSOCIATED_TOKEN_PROGRAM_ID,
        systemProgram: SystemProgram.programId,
      })
      .rpc();
    return this.devnet.record("deposit_usdc_v1", signature);
  }
}

export class DevnetBasket {
  readonly config: PublicKey;
  readonly vaultTokens: PublicKey[];
  readonly position: PublicKey;
  userAccounts: PublicKey[] = [];

  constructor(
    readonly devnet: Devnet,
    readonly vault: PublicKey,
    readonly mints: PublicKey[],
    readonly tokenPrograms: PublicKey[]
  ) {
    const programId = devnet.basket.programId;
    [this.config] = PublicKey.findProgramAddressSync(
      [Buffer.from("basket_config"), vault.toBuffer()],
      programId
    );
    this.vaultTokens = mints.map(
      (mint) =>
        PublicKey.findProgramAddressSync(
          [Buffer.from("asset_vault"), this.config.toBuffer(), mint.toBuffer()],
          programId
        )[0]
    );
    [this.position] = PublicKey.findProgramAddressSync(
      [
        Buffer.from("position"),
        this.config.toBuffer(),
        devnet.payer.publicKey.toBuffer(),
      ],
      programId
    );
  }

  auctionAddress(nonce: bigint): PublicKey {
    const seed = Buffer.alloc(8);
    seed.writeBigUInt64LE(nonce);
    return PublicKey.findProgramAddressSync(
      [Buffer.from("auction"), this.config.toBuffer(), seed],
      this.devnet.basket.programId
    )[0];
  }

  fetch() {
    return this.devnet.basket.account.basketConfig.fetch(this.config);
  }

  fetchAuction(nonce: bigint) {
    return this.devnet.basket.account.auction.fetch(this.auctionAddress(nonce));
  }

  fetchPosition() {
    return this.devnet.basket.account.userPosition.fetch(this.position);
  }

  async initialize(vaultV1State: PublicKey): Promise<string> {
    const programId = this.devnet.basket.programId;
    const accounts: Record<string, PublicKey> = {
      authority: this.devnet.payer.publicKey,
      program: programId,
      programData: programData(programId),
      vault: this.vault,
      basketConfig: this.config,
      vaultV1State,
      systemProgram: SystemProgram.programId,
    };
    this.mints.forEach((mint, i) => {
      accounts[`mint${i}`] = mint;
      accounts[`vaultToken${i}`] = this.vaultTokens[i];
      accounts[`tokenProgram${i}`] = this.tokenPrograms[i];
    });
    const signature = await this.devnet.basket.methods
      .initializeBasketConfig(this.mints, WEIGHTS, {
        assets: FIXED_PRICES_D18.map((price) => ({
          priceSource: { fixed: { priceUsdD18: bn(price) } },
          bandBps: 200,
          maxLotUsd: bn(MAX_LOT_USD),
        })),
        initialUnits: UNITS.map(bn),
        rebalance: {
          maxAgeTradedS: 60,
          maxAgeNavS: 3 * 24 * 60 * 60,
          maxConfBps: 100,
          startPremiumBps: Number(START_PREMIUM_BPS),
          maxDiscountBps: 100,
          auctionDurationS: AUCTION_DURATION_S,
          cooldownS: 60,
          minLotUsd: bn(MIN_LOT_USD),
        },
      })
      .accountsPartial(accounts)
      .rpc();
    return this.devnet.record("initialize_basket_config", signature);
  }

  async deposit(maxAmounts: bigint[], minSharesOut: bigint): Promise<string> {
    const accounts: Record<string, PublicKey> = {
      user: this.devnet.payer.publicKey,
      basketConfig: this.config,
      vault: this.vault,
      position: this.position,
      systemProgram: SystemProgram.programId,
    };
    this.mints.forEach((mint, i) => {
      accounts[`mint${i}`] = mint;
      accounts[`userToken${i}`] = this.userAccounts[i];
      accounts[`vaultToken${i}`] = this.vaultTokens[i];
      accounts[`tokenProgram${i}`] = this.tokenPrograms[i];
    });
    const signature = await this.devnet.basket.methods
      .depositToBasketV2(maxAmounts.map(bn), bn(minSharesOut))
      .accountsPartial(accounts)
      .rpc();
    return this.devnet.record("deposit_to_basket_v2", signature);
  }

  // All prices are fixed on devnet, so only the four token accounts and the SPYx mint are passed.
  openAuctionRemainingAccounts(): AccountMeta[] {
    return [...this.vaultTokens, this.mints[SPYX]].map((pubkey) => ({
      pubkey,
      isSigner: false,
      isWritable: false,
    }));
  }

  async openAuction(sell: number, buy: number): Promise<string> {
    const nonce = big((await this.fetch()).auctionNonce);
    const signature = await this.devnet.basket.methods
      .openAuction(sell, buy)
      .accountsPartial({
        opener: this.devnet.payer.publicKey,
        basketConfig: this.config,
        auction: this.auctionAddress(nonce),
        systemProgram: SystemProgram.programId,
      })
      .remainingAccounts(this.openAuctionRemainingAccounts())
      .rpc();
    return this.devnet.record("open_auction", signature);
  }

  async bid(
    nonce: bigint,
    sellAmount: bigint,
    maxBuyAmount: bigint
  ): Promise<string> {
    const auction = await this.fetchAuction(nonce);
    const [sell, buy] = [auction.sellIndex, auction.buyIndex];
    const signature = await this.devnet.basket.methods
      .bid(bn(sellAmount), bn(maxBuyAmount))
      .accountsPartial({
        bidder: this.devnet.payer.publicKey,
        basketConfig: this.config,
        auction: this.auctionAddress(nonce),
        sellMint: this.mints[sell],
        buyMint: this.mints[buy],
        vaultSellAccount: this.vaultTokens[sell],
        vaultBuyAccount: this.vaultTokens[buy],
        bidderSellAccount: this.userAccounts[sell],
        bidderBuyAccount: this.userAccounts[buy],
        sellTokenProgram: this.tokenPrograms[sell],
        buyTokenProgram: this.tokenPrograms[buy],
      })
      .rpc();
    return this.devnet.record("bid", signature);
  }

  async closeAuction(nonce: bigint): Promise<string> {
    const signature = await this.devnet.basket.methods
      .closeAuction()
      .accountsPartial({
        basketConfig: this.config,
        auction: this.auctionAddress(nonce),
        opener: this.devnet.payer.publicKey,
      })
      .rpc();
    return this.devnet.record("close_auction", signature);
  }

  async setPaused(paused: boolean, signer?: Keypair): Promise<string> {
    const authority = signer ?? this.devnet.payer;
    const signature = await this.devnet.basket.methods
      .setPaused(paused)
      .accountsPartial({
        authority: authority.publicKey,
        basketConfig: this.config,
      })
      .signers(signer ? [signer] : [])
      .rpc();
    return this.devnet.record(`set_paused(${paused})`, signature);
  }
}
