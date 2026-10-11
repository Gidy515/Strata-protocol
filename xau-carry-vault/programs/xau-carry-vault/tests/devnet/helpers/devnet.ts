import { AnchorProvider, BN, Program, Wallet } from "@anchor-lang/core";
import {
  ASSOCIATED_TOKEN_PROGRAM_ID,
  MINT_SIZE,
  TOKEN_PROGRAM_ID,
  createAssociatedTokenAccountIdempotentInstruction,
  createInitializeMint2Instruction,
  createMintToInstruction,
  getAccount,
  getAssociatedTokenAddressSync,
} from "@solana/spl-token";
import {
  Connection,
  Keypair,
  PublicKey,
  SystemProgram,
  Transaction,
} from "@solana/web3.js";
import { readFileSync } from "fs";
import { homedir } from "os";
import * as path from "path";
import idl from "../../../../../target/idl/xau_carry_vault.json";
import { XauCarryVault } from "../../../../../target/types/xau_carry_vault";

export const USDC = 1_000_000n;

const UPGRADEABLE_LOADER_ID = new PublicKey(
  "BPFLoaderUpgradeab1e11111111111111111111111"
);

export function bn(value: bigint): BN {
  return new BN(value.toString());
}

export function big(value: BN | number): bigint {
  return BigInt(value.toString());
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

function u64Seed(value: bigint): Buffer {
  const seed = Buffer.alloc(8);
  seed.writeBigUInt64LE(value);
  return seed;
}

export class DevnetVaultV1 {
  readonly payer = loadWallet();
  readonly provider = new AnchorProvider(
    new Connection(
      process.env.ANCHOR_PROVIDER_URL ?? "https://api.devnet.solana.com",
      { commitment: "confirmed", fetch: fetchWithRetry }
    ),
    new Wallet(this.payer),
    { commitment: "confirmed" }
  );
  readonly program = new Program<XauCarryVault>(
    idl as XauCarryVault,
    this.provider
  );
  readonly signatures: Record<string, string> = {};

  usdcMint = PublicKey.default;
  state = PublicKey.default;
  custody = PublicKey.default;
  vxauMint = PublicKey.default;
  strategy = PublicKey.default;

  get connection(): Connection {
    return this.provider.connection;
  }

  get user(): PublicKey {
    return this.payer.publicKey;
  }

  get userUsdc(): PublicKey {
    return getAssociatedTokenAddressSync(this.usdcMint, this.user);
  }

  get userVxau(): PublicKey {
    return getAssociatedTokenAddressSync(this.vxauMint, this.user);
  }

  record(label: string, signature: string): string {
    this.signatures[label] = signature;
    return signature;
  }

  // Mock USDC: a fresh 6-decimal mint per run, so every run gets its own vault.
  async createUsdc(amount: bigint): Promise<void> {
    const mint = Keypair.generate();
    const rent = await this.connection.getMinimumBalanceForRentExemption(
      MINT_SIZE
    );
    this.usdcMint = mint.publicKey;
    await this.provider.sendAndConfirm(
      new Transaction().add(
        SystemProgram.createAccount({
          fromPubkey: this.user,
          newAccountPubkey: mint.publicKey,
          space: MINT_SIZE,
          lamports: rent,
          programId: TOKEN_PROGRAM_ID,
        }),
        createInitializeMint2Instruction(mint.publicKey, 6, this.user, null),
        createAssociatedTokenAccountIdempotentInstruction(
          this.user,
          this.userUsdc,
          this.user,
          mint.publicKey
        ),
        createMintToInstruction(
          mint.publicKey,
          this.userUsdc,
          this.user,
          amount
        )
      ),
      [mint]
    );
    const programId = this.program.programId;
    [this.state] = PublicKey.findProgramAddressSync(
      [Buffer.from("vault_v1"), this.usdcMint.toBuffer()],
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

  withdrawalRequest(nonce: bigint): PublicKey {
    return PublicKey.findProgramAddressSync(
      [
        Buffer.from("withdrawal_request"),
        this.state.toBuffer(),
        this.user.toBuffer(),
        u64Seed(nonce),
      ],
      this.program.programId
    )[0];
  }

  withdrawalEscrow(nonce: bigint): PublicKey {
    return PublicKey.findProgramAddressSync(
      [
        Buffer.from("withdrawal_escrow"),
        this.withdrawalRequest(nonce).toBuffer(),
      ],
      this.program.programId
    )[0];
  }

  fetchState() {
    return this.program.account.vaultV1State.fetch(this.state);
  }

  async balance(account: PublicKey): Promise<bigint> {
    return (await getAccount(this.connection, account, "confirmed")).amount;
  }

  async exists(account: PublicKey): Promise<boolean> {
    return (await this.connection.getAccountInfo(account)) !== null;
  }

  async initialize(): Promise<string> {
    const programId = this.program.programId;
    const signature = await this.program.methods
      .initializeVaultV1()
      .accountsPartial({
        admin: this.user,
        program: programId,
        programData: PublicKey.findProgramAddressSync(
          [programId.toBuffer()],
          UPGRADEABLE_LOADER_ID
        )[0],
        usdcMint: this.usdcMint,
        vaultV1State: this.state,
        vaultV1UsdcAccount: this.custody,
        vxauMint: this.vxauMint,
      })
      .rpc();
    return this.record("initialize_vault_v1", signature);
  }

  readIdleNav() {
    return this.program.methods
      .readIdleNavV1()
      .accountsPartial({
        vaultV1State: this.state,
        usdcMint: this.usdcMint,
        vaultV1UsdcAccount: this.custody,
        vxauMint: this.vxauMint,
        strategyAccount: this.strategy,
      })
      .view();
  }

  private userAccounts() {
    return {
      vaultV1State: this.state,
      usdcMint: this.usdcMint,
      vaultV1UsdcAccount: this.custody,
      vxauMint: this.vxauMint,
      userUsdcAta: this.userUsdc,
      userVxauAta: this.userVxau,
      strategyAccount: this.strategy,
      tokenProgram: TOKEN_PROGRAM_ID,
      associatedTokenProgram: ASSOCIATED_TOKEN_PROGRAM_ID,
      systemProgram: SystemProgram.programId,
    };
  }

  async deposit(amount: bigint, minSharesOut: bigint): Promise<string> {
    const signature = await this.program.methods
      .depositUsdcV1(bn(amount), bn(minSharesOut))
      .accountsPartial({ user: this.user, ...this.userAccounts() })
      .rpc();
    return this.record("deposit_usdc_v1", signature);
  }

  async withdraw(shares: bigint, minAssetsOut: bigint): Promise<string> {
    const signature = await this.program.methods
      .withdrawUsdcV1(bn(shares), bn(minAssetsOut))
      .accountsPartial({ user: this.user, ...this.userAccounts() })
      .rpc();
    return this.record("withdraw_usdc_v1", signature);
  }

  async requestWithdrawal(
    nonce: bigint,
    shares: bigint,
    minAssetsOut: bigint
  ): Promise<string> {
    const signature = await this.program.methods
      .requestWithdrawalV1(bn(nonce), bn(shares), bn(minAssetsOut))
      .accountsPartial({
        user: this.user,
        vaultV1State: this.state,
        vxauMint: this.vxauMint,
        userVxauAta: this.userVxau,
        withdrawalRequest: this.withdrawalRequest(nonce),
        withdrawalEscrow: this.withdrawalEscrow(nonce),
        tokenProgram: TOKEN_PROGRAM_ID,
        systemProgram: SystemProgram.programId,
      })
      .rpc();
    return this.record(`request_withdrawal_v1(${nonce})`, signature);
  }

  async completeWithdrawal(nonce: bigint, cancel: boolean): Promise<string> {
    const accounts = {
      payer: this.user,
      owner: this.user,
      vaultV1State: this.state,
      usdcMint: this.usdcMint,
      vxauMint: this.vxauMint,
      vaultV1UsdcAccount: this.custody,
      withdrawalRequest: this.withdrawalRequest(nonce),
      withdrawalEscrow: this.withdrawalEscrow(nonce),
      ownerUsdcAta: this.userUsdc,
      ownerVxauAta: this.userVxau,
      strategyAccount: this.strategy,
      tokenProgram: TOKEN_PROGRAM_ID,
      associatedTokenProgram: ASSOCIATED_TOKEN_PROGRAM_ID,
      systemProgram: SystemProgram.programId,
    };
    const builder = cancel
      ? this.program.methods.cancelWithdrawalV1()
      : this.program.methods.settleWithdrawalV1();
    const signature = await builder.accountsPartial(accounts).rpc();
    return this.record(
      `${cancel ? "cancel" : "settle"}_withdrawal_v1(${nonce})`,
      signature
    );
  }

  async setDepositsPaused(paused: boolean): Promise<string> {
    const state = await this.fetchState();
    const signature = await this.program.methods
      .setVaultControlsV1({
        depositsPaused: paused,
        withdrawalsPaused: state.withdrawalsPaused,
        strategyPaused: state.strategyPaused,
        maxOrderCollateral: state.maxOrderCollateral,
        maxOrderSize: state.maxOrderSize,
      })
      .accountsPartial({ admin: this.user, vaultV1State: this.state })
      .rpc();
    return this.record(`set_vault_controls_v1(${paused})`, signature);
  }

  async proposeAdmin(newAdmin: PublicKey, signer?: Keypair): Promise<string> {
    const admin = signer ?? this.payer;
    const signature = await this.program.methods
      .proposeVaultAdminV1(newAdmin)
      .accountsPartial({ admin: admin.publicKey, vaultV1State: this.state })
      .signers(signer ? [signer] : [])
      .rpc();
    return this.record("propose_vault_admin_v1", signature);
  }

  async acceptAdmin(newAdmin: Keypair): Promise<string> {
    const signature = await this.program.methods
      .acceptVaultAdminV1()
      .accountsPartial({
        newAdmin: newAdmin.publicKey,
        vaultV1State: this.state,
      })
      .signers([newAdmin])
      .rpc();
    return this.record("accept_vault_admin_v1", signature);
  }
}
