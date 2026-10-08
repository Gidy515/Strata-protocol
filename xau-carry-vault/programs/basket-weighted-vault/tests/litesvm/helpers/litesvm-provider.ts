import { Provider, utils } from "@anchor-lang/core";
import { fromVersionedTransaction } from "@solana/compat";
import { address, lamports } from "@solana/kit";
import {
  AccountInfo,
  Connection,
  Keypair,
  PublicKey,
  Signer,
  Transaction,
  VersionedTransaction,
} from "@solana/web3.js";
import {
  FailedTransactionMetadata,
  LiteSVM,
  TransactionMetadata,
} from "litesvm";
import { inspect } from "util";

export class LiteSVMTransactionError extends Error {
  constructor(readonly error: unknown, readonly logs: string[]) {
    super(`transaction failed: ${inspect(error)}\n${logs.join("\n")}`);
  }
}

export class LiteSVMProvider implements Provider {
  readonly svm = new LiteSVM();
  readonly payer = Keypair.generate();
  lastResult: TransactionMetadata | null = null;
  lastTxBytes = 0;

  readonly connection = new Proxy({} as Connection, {
    get(_, property) {
      throw new Error(
        `LiteSVMProvider has no RPC connection (tried to use connection.${String(
          property
        )})`
      );
    },
  });

  constructor() {
    this.airdrop(this.payer.publicKey, 1_000_000_000_000n);
  }

  get publicKey(): PublicKey {
    return this.payer.publicKey;
  }

  async send(
    tx: Transaction | VersionedTransaction,
    signers: Signer[] = []
  ): Promise<string> {
    return this.sendSync(tx, signers);
  }

  async sendAndConfirm(
    tx: Transaction | VersionedTransaction,
    signers: Signer[] = []
  ): Promise<string> {
    return this.sendSync(tx, signers);
  }

  addProgram(programId: PublicKey, bytes: Uint8Array, loader: PublicKey): void {
    this.svm.addProgramWithLoader(
      address(programId.toBase58()),
      bytes,
      address(loader.toBase58())
    );
  }

  airdrop(to: PublicKey, amount: bigint): void {
    const result = this.svm.airdrop(address(to.toBase58()), lamports(amount));
    if (result instanceof FailedTransactionMetadata) {
      throw new LiteSVMTransactionError(result.err(), result.meta().logs());
    }
  }

  getAccountInfo(pubkey: PublicKey): AccountInfo<Buffer> | null {
    const account = this.svm.getAccount(address(pubkey.toBase58()));
    if (!account.exists) {
      return null;
    }
    return {
      data: Buffer.from(account.data),
      owner: new PublicKey(account.programAddress),
      lamports: Number(account.lamports),
      executable: account.executable,
      rentEpoch: 0,
    };
  }

  setAccount(
    pubkey: PublicKey,
    owner: PublicKey,
    data: Uint8Array,
    balance?: bigint
  ): void {
    this.svm.setAccount({
      address: address(pubkey.toBase58()),
      programAddress: address(owner.toBase58()),
      data,
      executable: false,
      lamports: lamports(
        balance ?? this.svm.minimumBalanceForRentExemption(BigInt(data.length))
      ),
      space: BigInt(data.length),
    });
  }

  sendSync(
    tx: Transaction | VersionedTransaction,
    signers: Signer[] = []
  ): string {
    this.svm.expireBlockhash();
    const blockhash = this.svm.latestBlockhash();

    let versioned: VersionedTransaction;
    if (tx instanceof VersionedTransaction) {
      tx.message.recentBlockhash = blockhash;
      versioned = tx;
    } else {
      tx.feePayer ??= this.payer.publicKey;
      tx.recentBlockhash = blockhash;
      versioned = new VersionedTransaction(tx.compileMessage());
    }

    const message = versioned.message;
    const required = message.staticAccountKeys.slice(
      0,
      message.header.numRequiredSignatures
    );
    const signing = [this.payer, ...signers].filter(
      (signer, index, all) =>
        required.some((key) => key.equals(signer.publicKey)) &&
        all.findIndex((other) => other.publicKey.equals(signer.publicKey)) ===
          index
    );
    versioned.sign(signing);
    this.lastTxBytes = versioned.serialize().length;

    const result = this.svm.sendTransaction(
      fromVersionedTransaction(versioned)
    );
    if (result instanceof FailedTransactionMetadata) {
      throw new LiteSVMTransactionError(result.err(), result.meta().logs());
    }
    this.lastResult = result;
    return utils.bytes.bs58.encode(versioned.signatures[0]);
  }
}
