import {Address} from '@stellar/stellar-sdk';

    /**
 * Errors returned by the vault contract.
 */
export const Error = {
  /**
   * The contract has already been initialized.
   */
  3001 : { message: "AlreadyInitialized" },
  /**
   * The contract has not been initialized with an admin.
   */
  3002 : { message: "NotInitialized" },
  /**
   * An amount was zero or negative, or an invalid parameter was given.
   */
  3003 : { message: "InvalidAmount" },
  /**
   * The user's spendable vault balance is insufficient for the requested amount.
   */
  3004 : { message: "InsufficientBalance" },
  /**
   * No subscription contract address has been configured.
   */
  3005 : { message: "SubscriptionNotSet" },
  /**
   * The allowlist is enabled and this token is not on it. Deposits of
   * unlisted tokens are rejected; withdrawals never are.
   */
  3006 : { message: "TokenNotAllowed" },
  /**
   * The deposit would take the subscriber's balance for this token above
   * the cap the admin set. A cap of 0 means unlimited, so this error only
   * fires while a cap is in force. Withdrawals are never blocked.
   */
  3007 : { message: "DepositCapExceeded" },
  /**
   * The subscriber has not opted in to allowance top-up for this mandate.
   * Allowance fallback is disabled by default and must be enabled per subscriber.
   */
  3008 : { message: "AllowanceNotEnabled" },
  /**
   * The shortfall could not be covered by the SEP-41 allowance granted to the vault,
   * either because the allowance is too small or the subscriber never granted one.
   */
  3009 : { message: "AllowanceInsufficient" },
  /**
   * The SEP-41 allowance granted to the vault has expired (its expiration ledger has passed).
   */
  3010 : { message: "AllowanceExpired" },
  /**
   * `amount_a + amount_b` passed to `debit_split` would overflow `i128`.
   * The split is rejected rather than wrapped, because a wrapped total
   * would under-charge the subscriber.
   */
  3011 : { message: "AmountOverflow" },
  /**
   * The debit would take the user over their per-window spend limit.
   * New variants go at the end so existing codes keep their numbers.
   */
  3012 : { message: "SpendLimitExceeded" },
  /**
   * `set_spend_limit` was given a positive amount with a zero window.
   */
  3013 : { message: "InvalidWindow" },
  /**
   * `set_reserve` would earmark more than the subscriber's balance for
   * this token across all of their mandates (#295).
   */
  3014 : { message: "ReserveExceedsBalance" },
  /**
   * The (user, token) pair has reached the per-window limit on
   * deposit/withdraw operations (#314). The limit bounds rapid cycling that
   * repeatedly rewrites balances and emits events; it resets when the
   * window rolls over.
   */
  3015 : { message: "RateLimitExceeded" },
  /**
   * A token transfer did not move exactly the requested amount, so the
   * token is not safe for the vault's exact-amount accounting (for example
   * a fee-on-transfer token). The whole operation is rolled back.
   */
  3016 : { message: "UnexpectedTransferAmount" },
  /**
   * A transfer destination is the zero/unset address: the all-zero contract
   * or account strkey. Sending funds there would burn them (#236).
   */
  3017 : { message: "ZeroAddress" },
  /**
   * A bulk reserve update contains no entries.
   */
  3018 : { message: "EmptyReserveBatch" },
  /**
   * A bulk reserve update exceeds `MAX_RESERVE_UPDATES`.
   */
  3019 : { message: "ReserveBatchTooLarge" },
  /**
   * A bulk reserve update contains the same mandate more than once.
   */
  3020 : { message: "DuplicateReserve" }
}

/**
 * A subscriber's optional ceiling on total debits per fixed time window,
 * across all of their mandates for one token. Set by the subscriber
 * themselves; it can only ever make charges fail, never move funds.
 */
export interface SpendLimit {
  /**
   * Maximum total debit within one window, in the token's smallest unit.
   * No limit is stored (and the getter reports 0) when this is 0.
   */
  amount: bigint;
  /**
   * Window length in seconds. Meaningful only when `amount > 0`.
   */
  window_secs: bigint;
}

/**
 * One mandate's requested reserve in a bulk reserve update.
 */
export interface ReserveUpdate {
  /**
   * New reserved amount; zero releases the reserve.
   */
  amount: bigint;
  /**
   * Mandate whose reserve is being replaced.
   */
  mandate_id: bigint;
}

/**
 * A single contract upgrade record.
 */
export interface UpgradeRecord {
  /**
   * Ledger sequence number when the upgrade was applied.
   */
  ledger: number;
  /**
   * Semantic version of the new code (from `CARGO_PKG_VERSION`).
   */
  version: string;
  /**
   * Hash of the new wasm code.
   */
  wasm_hash: Uint8Array;
}

/**
 * Emitted when the subscription contract debits a subscriber to pay a merchant.
 */
export interface DebitEvent {
  name: "Debit";
  data: {
    /**
     * Account debited.
     */
    user: string;
    /**
     * Token contract address of the debited asset.
     */
    token: string;
    /**
     * Recipient address receiving the tokens.
     */
    to: string;
    /**
     * Amount transferred to the recipient.
     */
    amount?: bigint;
    /**
     * Remaining spendable balance of the subscriber for this token.
     */
    balance?: bigint;
  };
}

/**
 * Emitted when a subscriber funds their vault balance.
 */
export interface DepositEvent {
  name: "Deposit";
  data: {
    /**
     * Account that deposited the funds.
     */
    user: string;
    /**
     * Token contract address of the deposited asset.
     */
    token: string;
    /**
     * Amount of tokens deposited.
     */
    amount?: bigint;
    /**
     * Updated spendable balance of the subscriber for this token.
     */
    balance?: bigint;
  };
}

/**
 * Emitted when a subscriber pulls funds back out.
 */
export interface WithdrawEvent {
  name: "Withdraw";
  data: {
    /**
     * Account that withdrew the funds.
     */
    user: string;
    /**
     * Token contract address of the withdrawn asset.
     */
    token: string;
    /**
     * Amount of tokens withdrawn.
     */
    amount?: bigint;
    /**
     * Remaining spendable balance of the subscriber for this token.
     */
    balance?: bigint;
  };
}

/**
 * Emitted when one account pays into another account's vault balance, for
 * example an employer funding an employee's subscriptions. `from` is the
 * payer: the account whose tokens moved and whose authorization was required.
 */
export interface DepositForEvent {
  name: "DepositFor";
  data: {
    from: string;
    user: string;
    token: string;
    amount?: bigint;
    balance?: bigint;
  };
}

/**
 * Emitted when a subscriber sets, changes or releases the reserve earmarked
 * for one mandate (#295). `amount` is the mandate's new reserve (0 means
 * released) and `reserved_total` the subscriber's total reserve for the
 * token afterwards, so an indexer can track both without a contract read.
 *
 * A debit that draws a reserve down does not emit this event; its `Debit`
 * already reports the movement, and `reserved` returns the remainder.
 */
export interface ReserveSetEvent {
  name: "ReserveSet";
  data: {
    /**
     * Subscriber whose balance is earmarked.
     */
    user: string;
    /**
     * Token the reserve is denominated in.
     */
    token: string;
    /**
     * Mandate the reserve is earmarked for.
     */
    mandate_id: bigint;
    /**
     * New reserve for this mandate. 0 means the reserve was released.
     */
    amount?: bigint;
    /**
     * Total reserved by `user` in `token` across all mandates afterwards.
     */
    reserved_total?: bigint;
  };
}

/**
 * Emitted when a subscriber pulls funds out to a destination other than
 * themselves. The subscriber authorizes the call; the destination does not
 * need to.
 */
export interface WithdrawToEvent {
  name: "WithdrawTo";
  data: {
    /**
     * Account whose balance was debited.
     */
    user: string;
    /**
     * Account that received the tokens.
     */
    to: string;
    /**
     * Token contract address of the withdrawn asset.
     */
    token: string;
    /**
     * Amount of tokens withdrawn.
     */
    amount?: bigint;
    /**
     * Remaining spendable balance of the subscriber for this token.
     */
    balance?: bigint;
  };
}

/**
 * Emitted when the admin sets the per-token deposit cap of a subscriber.
 */
export interface DepositCapSetEvent {
  name: "DepositCapSet";
  data: {
    /**
     * Account whose deposit cap was updated.
     */
    user: string;
    /**
     * Token contract address subject to the cap.
     */
    token: string;
    /**
     * The new cap. Zero means unlimited.
     */
    cap?: bigint;
  };
}

/**
 * Emitted when the admin replaces this contract's wasm. Persistent state is
 * untouched by an upgrade; the new code reads the keys the old code wrote.
 */
export interface ContractUpgradedEvent {
  name: "ContractUpgraded";
  data: {
    admin: string;
    new_wasm_hash?: Uint8Array;
  };
}
    export type ContractEvent = DebitEvent | DepositEvent | WithdrawEvent | DepositForEvent | ReserveSetEvent | WithdrawToEvent | DepositCapSetEvent | ContractUpgradedEvent;
