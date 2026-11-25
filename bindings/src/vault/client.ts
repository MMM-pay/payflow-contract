import {SpendLimit, UpgradeRecord, ContractEvent} from './types.js';
import {Result, Spec, AssembledTransaction, Client as ContractClient, ClientOptions as ContractClientOptions, MethodOptions, ExternalExecutableRef} from '@stellar/stellar-sdk/contract';
import {Address, xdr} from '@stellar/stellar-sdk';

export interface Client {
  /**
   * Admin address configured for this vault.
   */
  admin(options?: MethodOptions): Promise<AssembledTransaction<Result<string, Error>>>;
  /**
   * Debit a subscriber and pay `to`. Restricted to the subscription
   * contract, which is the direct caller and is therefore implicitly
   * authorized by the host when it invokes this function.
   *
   * # Auth
   * Requires authorization from the registered `subscription` contract.
   *
   * # Events
   * Emits [`Debit`].
   *
   * # Errors
   * Returns [`Error::SubscriptionNotSet`] if no subscription contract is set,
   * [`Error::InvalidAmount`] if `amount <= 0`, or [`Error::InsufficientBalance`]
   * if `amount` exceeds `user`'s unreserved balance. This entry point
   * is not tied to a mandate, so it can never draw on a reserve; use
   * [`Vault::debit_mandate`] for a mandate's own charge.
   */
  debit(args: { user: string | Address; token: string | Address; to: string | Address; amount: bigint }, options?: MethodOptions): Promise<AssembledTransaction<Result<null, Error>>>;
  /**
   * Total balance of `user` in `token` held in the vault, reserved or
   * not. See `unreserved_balance` for the part outside every reserve.
   */
  balance(args: { user: string | Address; token: string | Address }, options?: MethodOptions): Promise<AssembledTransaction<bigint>>;
  /**
   * Move `amount` of `token` from the subscriber's wallet into the vault.
   *
   * # Auth
   * Requires authorization from `user`.
   *
   * # Events
   * Emits [`Deposit`].
   *
   * # Errors
   * Returns [`Error::InvalidAmount`] if `amount <= 0`, [`Error::TokenNotAllowed`]
   * if the allowlist is enabled and `token` is not allowed,
   * [`Error::DepositCapExceeded`] if the deposit would exceed the
   * subscriber's cap, or [`Error::RateLimitExceeded`] if the
   * (user, token) pair has used its deposit/withdraw budget for this
   * window (#314).
   */
  deposit(args: { user: string | Address; token: string | Address; amount: bigint }, options?: MethodOptions): Promise<AssembledTransaction<Result<null, Error>>>;
  /**
   * The version of this contract's crate.
   */
  version(options?: MethodOptions): Promise<AssembledTransaction<string>>;
  /**
   * The reserve `user` earmarked for `mandate_id` in `token`. 0 when
   * none is set.
   */
  reserved(args: { user: string | Address; token: string | Address; mandate_id: bigint }, options?: MethodOptions): Promise<AssembledTransaction<bigint>>;
  /**
   * Return uncommitted funds to the subscriber. Always available: the vault
   * never locks a balance, so a subscriber can exit at any time.
   *
   * Only the unreserved part of the balance can be withdrawn. A reserve
   * is the subscriber's own earmark (#295) and `set_reserve(.., 0)`
   * releases it at any time, so reserving never traps funds: it only
   * stops them leaving by accident while a mandate is counting on them.
   *
   * # Auth
   * Requires authorization from `user`.
   *
   * # Events
   * Emits [`Withdraw`].
   *
   * # Errors
   * Returns [`Error::InvalidAmount`] if `amount <= 0`,
   * [`Error::InsufficientBalance`] if `amount` exceeds the user's
   * unreserved balance, or [`Error::RateLimitExceeded`] if the
   * (user, token) pair has used its deposit/withdraw budget for this
   * window (#314).
   */
  withdraw(args: { user: string | Address; token: string | Address; amount: bigint }, options?: MethodOptions): Promise<AssembledTransaction<Result<null, Error>>>;
  /**
   * Whether `debit_mandate(user, token, mandate_id, _, amount)` would
   * succeed right now on balance and spend-limit grounds: the
   * unreserved balance plus the mandate's reserve covers `amount`, and
   * the subscriber's spend limit has room for it. Reads storage only.
   *
   * The subscription contract uses this to prove on chain that a due
   * charge would fail before flagging it (#296).
   */
  can_debit(args: { user: string | Address; token: string | Address; mandate_id: bigint; amount: bigint }, options?: MethodOptions): Promise<AssembledTransaction<boolean>>;
  /**
   * Whether `user` may deposit `token` into the vault. Returns `true`
   * when the allowlist is disabled, or when it is enabled and `token`
   * is explicitly allowed. This is a convenience view so callers do not
   * need to read both `allowlist_enabled` and `token_allowed`.
   */
  can_deposit(args: { user: string | Address; token: string | Address }, options?: MethodOptions): Promise<AssembledTransaction<boolean>>;
  /**
   * Debit a subscriber once and pay two recipients from that debit.
   *
   * `charge` splits every settlement into a merchant share and a protocol
   * fee. Settling that with two `debit` calls repeated the authorization
   * check, the `Subscription` read and the balance read and write for the
   * same total. `debit_split` performs them once, then makes the same two
   * token transfers and emits one [`Debit`] per non-empty leg, with the
   * running balances two sequential `debit` calls would have reported:
   * the first event carries the balance after `amount_a`, the second the
   * balance after both legs.
   *
   * `amount_a` must be positive, like `debit`. `amount_b` may be zero: a
   * settlement with no protocol fee then has a single leg and emits a
   * single [`Debit`], exactly as one `debit` call would have.
   *
   * # Auth
   * Requires authorization from the registered `subscription` contract.
   *
   * # Events
   * Emits one [`Debit`] per non-empty leg.
   *
   * # Errors
   * Returns [`Error::SubscriptionNotSet`] if no subscription contract is
   * set, [`Error::InvalidAmount`] if `amount_a <= 0` or `
   */
  debit_split(args: { user: string | Address; token: string | Address; to_a: string | Address; amount_a: bigint; to_b: string | Address; amount_b: bigint }, options?: MethodOptions): Promise<AssembledTransaction<Result<null, Error>>>;
  /**
   * The deposit cap for (user, token). Zero means unlimited.
   */
  deposit_cap(args: { user: string | Address; token: string | Address }, options?: MethodOptions): Promise<AssembledTransaction<bigint>>;
  /**
   * Credit `user`'s balance with tokens paid by a different account
   * `from` — third-party funding (merged from #136). `from` authorizes
   * and pays; `user` receives the spendable balance.
   *
   * Applies the same allowlist, deposit-cap and per-window operation
   * checks as `deposit`, so funding on someone's behalf cannot bypass
   * them. (#136 predated those features; enforcing them here keeps
   * third-party funding consistent with a direct deposit.)
   */
  deposit_for(args: { from: string | Address; user: string | Address; token: string | Address; amount: bigint }, options?: MethodOptions): Promise<AssembledTransaction<Result<null, Error>>>;
  /**
   * Earmark `amount` of the subscriber's `token` balance for one
   * mandate (#295), replacing any reserve it already had. `0` releases
   * the reserve.
   *
   * While reserved, those funds can be spent only by `mandate_id`'s
   * own charges (through `debit_mandate` / `debit_split_mandate`, which
   * draw the reserve down first). Other mandates' debits, plain
   * `debit`/`debit_split`, `withdraw` and `withdraw_all` see only the
   * unreserved balance. The subscriber can release a reserve at any
   * time, so a reserve never traps funds.
   *
   * The vault does not know mandates: `mandate_id` is the subscription
   * contract's id, which that contract passes on every mandate debit.
   * Reserving for an id that is not the subscriber's own mandate only
   * ever withholds the subscriber's money from themselves.
   *
   * # Auth
   * Requires authorization from `user`.
   *
   * # Events
   * Emits [`ReserveSet`].
   *
   * # Errors
   * Returns [`Error::InvalidAmount`] if `amount < 0`, or
   * [`Error::ReserveExceedsBalance`] if the subscriber's total reserve
   * for `token` would exceed their balance.
   */
  set_reserve(args: { user: string | Address; token: string | Address; mandate_id: bigint; amount: bigint }, options?: MethodOptions): Promise<AssembledTransaction<Result<null, Error>>>;
  /**
   * The user's stored spending limit. `amount == 0` means no limit.
   */
  spend_limit(args: { user: string | Address; token: string | Address }, options?: MethodOptions): Promise<AssembledTransaction<SpendLimit>>;
  /**
   * Return uncommitted funds to a destination chosen by the subscriber.
   *
   * Identical to [`Vault::withdraw`] except that the tokens are sent to
   * `to` instead of the caller. Only `user` authorizes: `to` is a
   * destination, not a second signer, so funds can be swept to a
   * custody, hardware or treasury address without giving that address
   * authority over the vault.
   *
   * Only the unreserved part of the balance can be withdrawn, exactly as
   * with `withdraw`. A destination may be `user` itself, in which case
   * this behaves like `withdraw` but reports [`WithdrawTo`].
   *
   * # Auth
   * Requires authorization from `user`. `to` needs no authorization.
   *
   * # Events
   * Emits [`WithdrawTo`].
   *
   * # Errors
   * Returns [`Error::InvalidAmount`] if `amount <= 0`,
   * [`Error::InsufficientBalance`] if `amount` exceeds the user's
   * unreserved balance, or [`Error::RateLimitExceeded`] if the
   * (user, token) pair has used its deposit/withdraw budget for this
   * window (#314).
   */
  withdraw_to(args: { user: string | Address; token: string | Address; to: string | Address; amount: bigint }, options?: MethodOptions): Promise<AssembledTransaction<Result<null, Error>>>;
  /**
   * Address of the subscription contract currently allowed to debit funds.
   */
  subscription(options?: MethodOptions): Promise<AssembledTransaction<Result<string, Error>>>;
  /**
   * Hand the admin role to another address. Only the current admin may
   * call this, and the new admin takes effect immediately: the old one
   * keeps no authority. A deployment whose admin key is lost or
   * rotated is otherwise stuck with it for the life of the contract.
   *
   * # Auth
   * Requires authorization from the current admin.
   */
  update_admin(args: { new_admin: string | Address }, options?: MethodOptions): Promise<AssembledTransaction<Result<null, Error>>>;
  /**
   * Replace this contract's wasm with code at `new_wasm_hash`. Only the
   * admin may upgrade; persistent state is untouched. (From #158.)
   *
   * Records the upgrade in the contract's upgrade history, which can be
   * queried via `upgrade_history` for transparency.
   */
  upgrade_wasm(args: { new_wasm_hash: Uint8Array }, options?: MethodOptions): Promise<AssembledTransaction<Result<null, Error>>>;
  /**
   * Withdraw the entire balance in one call.
   *
   * Exiting must not be a race: with `withdraw`, a charge that settles
   * between reading the balance and withdrawing makes the withdraw fail
   * with [`Error::InsufficientBalance`]. This takes whatever is there
   * instead, so a subscriber leaving the protocol cannot be thwarted by
   * a keeper.
   *
   * "Entire" means everything not reserved for a mandate (#295): a
   * reserve stays behind until the subscriber releases it with
   * `set_reserve(.., 0)`, after which this takes the rest.
   *
   * # Auth
   * Requires authorization from `user`.
   *
   * # Events
   * Emits [`Withdraw`] for a non-empty balance, and nothing at all for an
   * empty one.
   *
   * # Errors
   * Never fails on the balance itself: an empty balance returns `0`
   * without a token transfer. Returns [`Error::RateLimitExceeded`] if the
   * (user, token) pair has used its deposit/withdraw budget for this
   * window (#314).
   */
  withdraw_all(args: { user: string | Address; token: string | Address }, options?: MethodOptions): Promise<AssembledTransaction<Result<bigint, Error>>>;
  /**
   * Debit a subscriber on behalf of one mandate (#295).
   *
   * Identical to [`Vault::debit`], except that `mandate_id`'s own
   * reserve is spendable too and is drawn down first: the mandate can
   * spend its reserve plus the unreserved balance, while every other
   * mandate's reserve stays out of reach.
   *
   * # Auth
   * Requires authorization from the registered `subscription` contract.
   *
   * # Events
   * Emits [`Debit`].
   *
   * # Errors
   * As [`Vault::debit`], with [`Error::InsufficientBalance`] meaning
   * `amount` exceeds the unreserved balance plus this mandate's reserve.
   */
  debit_mandate(args: { user: string | Address; token: string | Address; mandate_id: bigint; to: string | Address; amount: bigint }, options?: MethodOptions): Promise<AssembledTransaction<Result<null, Error>>>;
  /**
   * Whether `token` is on the allowlist. Only meaningful while the
   * allowlist is enabled; absent means not allowed.
   */
  token_allowed(args: { token: string | Address }, options?: MethodOptions): Promise<AssembledTransaction<boolean>>;
  /**
   * Amount the subscriber would need to deposit for `periods` charges
   * of `charge_amount` each under `mandate_id`.
   *
   * The available amount includes the mandate's own reserve and
   * unreserved funds, but excludes reserves belonging to other mandates.
   * Pass the mandate's frozen per-charge amount. This view does not
   * change state or check the mandate's lifecycle or charge caps.
   *
   * # Errors
   * Returns [`Error::InvalidAmount`] if `charge_amount` is not positive,
   * or [`Error::AmountOverflow`] if the target total overflows `i128`.
   */
  top_up_target(args: { user: string | Address; token: string | Address; mandate_id: bigint; charge_amount: bigint; periods: number }, options?: MethodOptions): Promise<AssembledTransaction<Result<bigint, Error>>>;
  /**
   * Total reserved by `user` in `token` across every mandate.
   */
  reserved_total(args: { user: string | Address; token: string | Address }, options?: MethodOptions): Promise<AssembledTransaction<bigint>>;
  /**
   * Cap how much a subscriber may hold for one token.
   *
   * While the protocol is unaudited, the cost of a bug grows with the
   * amount of subscriber funds the vault can hold, so the admin can cap
   * the balance a subscriber may keep per token. The cap is per
   * (user, token): one subscriber's cap never affects another.
   *
   * A cap of 0 means unlimited, which is the default and what an
   * existing deployment runs with until an admin sets a cap.
   *
   * The cap only gates new deposits. A balance that is already above a
   * newly set cap can still be withdrawn, and can still be debited by the
   * subscription contract: the cap limits how much can be added, and
   * never traps funds.
   *
   * # Auth
   * Requires authorization from the contract admin.
   *
   * # Events
   * Emits [`DepositCapSet`].
   *
   * # Errors
   * Returns [`Error::NotInitialized`] if no admin is configured, or
   * [`Error::InvalidAmount`] if `amount < 0`.
   */
  set_deposit_cap(args: { user: string | Address; token: string | Address; amount: bigint }, options?: MethodOptions): Promise<AssembledTransaction<Result<null, Error>>>;
  /**
   * Set or replace a per-period ceiling on total debits for
   * `(user, token)`, across all of the user's mandates in that token.
   * Only the user themself can set it, and it can only make charges
   * fail — it never moves funds and never blocks `withdraw`.
   *
   * `amount == 0` disables the limit and clears the window tracker.
   * When a new limit is set, the window tracker is cleared too, so the
   * new ceiling is judged on usage from this point on.
   */
  set_spend_limit(args: { user: string | Address; token: string | Address; amount: bigint; window_secs: bigint }, options?: MethodOptions): Promise<AssembledTransaction<Result<null, Error>>>;
  /**
   * Get the contract's upgrade history.
   *
   * Returns a vector of upgrade records, oldest first. Each record
   * contains the version, ledger sequence, and wasm hash. The initial
   * deployment is not recorded; only explicit upgrades via `upgrade_wasm`
   * appear here.
   */
  upgrade_history(options?: MethodOptions): Promise<AssembledTransaction<Array<UpgradeRecord>>>;
  /**
   * Point the vault at the subscription contract allowed to debit it.
   *
   * # Auth
   * Requires authorization from the contract admin.
   *
   * # Events
   * Emits no events.
   *
   * # Errors
   * Returns [`Error::NotInitialized`] if no admin is configured.
   */
  set_subscription(args: { subscription: string | Address }, options?: MethodOptions): Promise<AssembledTransaction<Result<null, Error>>>;
  /**
   * Whether the token allowlist is enforced. Off unless the admin turned it
   * on.
   */
  allowlist_enabled(options?: MethodOptions): Promise<AssembledTransaction<boolean>>;
  /**
   * Add or remove one token from the allowlist. Removing a token blocks
   * new deposits of it; balances already held can still be withdrawn, so
   * allowlisting can never trap funds.
   *
   * # Auth
   * Requires authorization from the contract admin.
   *
   * # Events
   * Emits no events.
   *
   * # Errors
   * Returns [`Error::NotInitialized`] if no admin is configured.
   */
  set_token_allowed(args: { token: string | Address; allowed: boolean }, options?: MethodOptions): Promise<AssembledTransaction<Result<null, Error>>>;
  /**
   * The part of `user`'s `token` balance not reserved for any mandate:
   * what `withdraw` and non-mandate debits can reach.
   */
  unreserved_balance(args: { user: string | Address; token: string | Address }, options?: MethodOptions): Promise<AssembledTransaction<bigint>>;
  /**
   * Debit a subscriber once and pay multiple recipients from that debit.
   *
   * This generalizes [`Vault::debit_split_mandate`] to an arbitrary
   * number of recipients, so a single charge can settle the merchant
   * share, the protocol fee, a keeper tip and a referral share in one
   * cross-contract call. The subscription contract uses this to avoid
   * repeating the authorization check and balance read/write for each
   * leg.
   *
   * `legs` is a vector of `(to, amount)` pairs. The first amount must
   * be positive; subsequent amounts may be zero (they are skipped). The
   * sum of all amounts is checked against the subscriber's spendable
   * balance and spend limit exactly once.
   *
   * # Auth
   * Requires authorization from the registered `subscription` contract.
   *
   * # Events
   * Emits one [`Debit`] per non-zero leg, with balances reflecting the
   * running total after each leg (matching what sequential `debit`
   * calls would produce).
   *
   * # Errors
   * Returns [`Error::SubscriptionNotSet`] if no subscription contract is
   * set, [`Error::InvalidAmount`] if `legs` is empty, the first amount
   *
   */
  debit_multi_mandate(args: { user: string | Address; token: string | Address; mandate_id: bigint; legs: Array<[string | Address, bigint]> }, options?: MethodOptions): Promise<AssembledTransaction<Result<null, Error>>>;
  /**
   * [`Vault::debit_split`] on behalf of one mandate (#295): the
   * mandate's own reserve is spendable too and is drawn down first.
   *
   * # Auth
   * Requires authorization from the registered `subscription` contract.
   *
   * # Events
   * Emits one [`Debit`] per non-empty leg.
   *
   * # Errors
   * As [`Vault::debit_split`], with [`Error::InsufficientBalance`]
   * meaning the total exceeds the unreserved balance plus this
   * mandate's reserve.
   */
  debit_split_mandate(args: { user: string | Address; token: string | Address; mandate_id: bigint; to_a: string | Address; amount_a: bigint; to_b: string | Address; amount_b: bigint }, options?: MethodOptions): Promise<AssembledTransaction<Result<null, Error>>>;
  /**
   * Turn the token allowlist on or off. Off by default, so a deployment
   * (testnet included) keeps accepting every SEP-41 token until the admin
   * opts in. Toggling it moves no funds: it only gates future deposits.
   *
   * # Auth
   * Requires authorization from the contract admin.
   *
   * # Events
   * Emits no events.
   *
   * # Errors
   * Returns [`Error::NotInitialized`] if no admin is configured.
   */
  set_allowlist_enabled(args: { enabled: boolean }, options?: MethodOptions): Promise<AssembledTransaction<Result<null, Error>>>;
}

export class Client extends ContractClient {
  constructor(public readonly options: ContractClientOptions) {
    super(
      new Spec(["AAAABAAAACZFcnJvcnMgcmV0dXJuZWQgYnkgdGhlIHZhdWx0IGNvbnRyYWN0LgAAAAAAAAAAAAVFcnJvcgAAAAAAAA8AAAAqVGhlIGNvbnRyYWN0IGhhcyBhbHJlYWR5IGJlZW4gaW5pdGlhbGl6ZWQuAAAAAAASQWxyZWFkeUluaXRpYWxpemVkAAAAAAu5AAAANFRoZSBjb250cmFjdCBoYXMgbm90IGJlZW4gaW5pdGlhbGl6ZWQgd2l0aCBhbiBhZG1pbi4AAAAOTm90SW5pdGlhbGl6ZWQAAAAAC7oAAABCQW4gYW1vdW50IHdhcyB6ZXJvIG9yIG5lZ2F0aXZlLCBvciBhbiBpbnZhbGlkIHBhcmFtZXRlciB3YXMgZ2l2ZW4uAAAAAAANSW52YWxpZEFtb3VudAAAAAAAC7sAAABMVGhlIHVzZXIncyBzcGVuZGFibGUgdmF1bHQgYmFsYW5jZSBpcyBpbnN1ZmZpY2llbnQgZm9yIHRoZSByZXF1ZXN0ZWQgYW1vdW50LgAAABNJbnN1ZmZpY2llbnRCYWxhbmNlAAAAC7wAAAA1Tm8gc3Vic2NyaXB0aW9uIGNvbnRyYWN0IGFkZHJlc3MgaGFzIGJlZW4gY29uZmlndXJlZC4AAAAAAAASU3Vic2NyaXB0aW9uTm90U2V0AAAAAAu9AAAAdlRoZSBhbGxvd2xpc3QgaXMgZW5hYmxlZCBhbmQgdGhpcyB0b2tlbiBpcyBub3Qgb24gaXQuIERlcG9zaXRzIG9mCnVubGlzdGVkIHRva2VucyBhcmUgcmVqZWN0ZWQ7IHdpdGhkcmF3YWxzIG5ldmVyIGFyZS4AAAAAAA9Ub2tlbk5vdEFsbG93ZWQAAAALvgAAAMhUaGUgZGVwb3NpdCB3b3VsZCB0YWtlIHRoZSBzdWJzY3JpYmVyJ3MgYmFsYW5jZSBmb3IgdGhpcyB0b2tlbiBhYm92ZQp0aGUgY2FwIHRoZSBhZG1pbiBzZXQuIEEgY2FwIG9mIDAgbWVhbnMgdW5saW1pdGVkLCBzbyB0aGlzIGVycm9yIG9ubHkKZmlyZXMgd2hpbGUgYSBjYXAgaXMgaW4gZm9yY2UuIFdpdGhkcmF3YWxzIGFyZSBuZXZlciBibG9ja2VkLgAAABJEZXBvc2l0Q2FwRXhjZWVkZWQAAAAAC78AAACTVGhlIHN1YnNjcmliZXIgaGFzIG5vdCBvcHRlZCBpbiB0byBhbGxvd2FuY2UgdG9wLXVwIGZvciB0aGlzIG1hbmRhdGUuCkFsbG93YW5jZSBmYWxsYmFjayBpcyBkaXNhYmxlZCBieSBkZWZhdWx0IGFuZCBtdXN0IGJlIGVuYWJsZWQgcGVyIHN1YnNjcmliZXIuAAAAABNBbGxvd2FuY2VOb3RFbmFibGVkAAAAC8AAAACfVGhlIHNob3J0ZmFsbCBjb3VsZCBub3QgYmUgY292ZXJlZCBieSB0aGUgU0VQLTQxIGFsbG93YW5jZSBncmFudGVkIHRvIHRoZSB2YXVsdCwKZWl0aGVyIGJlY2F1c2UgdGhlIGFsbG93YW5jZSBpcyB0b28gc21hbGwgb3IgdGhlIHN1YnNjcmliZXIgbmV2ZXIgZ3JhbnRlZCBvbmUuAAAAABVBbGxvd2FuY2VJbnN1ZmZpY2llbnQAAAAAAAvBAAAAWVRoZSBTRVAtNDEgYWxsb3dhbmNlIGdyYW50ZWQgdG8gdGhlIHZhdWx0IGhhcyBleHBpcmVkIChpdHMgZXhwaXJhdGlvbiBsZWRnZXIgaGFzIHBhc3NlZCkuAAAAAAAAEEFsbG93YW5jZUV4cGlyZWQAAAvCAAAAzUFuIGFtb3VudCBjYWxjdWxhdGlvbiB3b3VsZCBvdmVyZmxvdyBgaTEyOGAuIGBkZWJpdF9zcGxpdGAgcmVqZWN0cyBhCndyYXBwZWQgdG90YWwgcmF0aGVyIHRoYW4gdW5kZXItY2hhcmdpbmc7IGB0b3BfdXBfdGFyZ2V0YCByZWplY3RzIGEKd3JhcHBlZCBmdW5kaW5nIHRhcmdldCByYXRoZXIgdGhhbiByZXBvcnRpbmcgYW4gaW5jb3JyZWN0IHNob3J0ZmFsbC4AAAAAAAAOQW1vdW50T3ZlcmZsb3cAAAAAC8MAAACBVGhlIGRlYml0IHdvdWxkIHRha2UgdGhlIHVzZXIgb3ZlciB0aGVpciBwZXItd2luZG93IHNwZW5kIGxpbWl0LgpOZXcgdmFyaWFudHMgZ28gYXQgdGhlIGVuZCBzbyBleGlzdGluZyBjb2RlcyBrZWVwIHRoZWlyIG51bWJlcnMuAAAAAAAAElNwZW5kTGltaXRFeGNlZWRlZAAAAAALxAAAAEFgc2V0X3NwZW5kX2xpbWl0YCB3YXMgZ2l2ZW4gYSBwb3NpdGl2ZSBhbW91bnQgd2l0aCBhIHplcm8gd2luZG93LgAAAAAAAA1JbnZhbGlkV2luZG93AAAAAAALxQAAAHJgc2V0X3Jlc2VydmVgIHdvdWxkIGVhcm1hcmsgbW9yZSB0aGFuIHRoZSBzdWJzY3JpYmVyJ3MgYmFsYW5jZSBmb3IKdGhpcyB0b2tlbiBhY3Jvc3MgYWxsIG9mIHRoZWlyIG1hbmRhdGVzICgjMjk1KS4AAAAAABVSZXNlcnZlRXhjZWVkc0JhbGFuY2UAAAAAAAvGAAAA11RoZSAodXNlciwgdG9rZW4pIHBhaXIgaGFzIHJlYWNoZWQgdGhlIHBlci13aW5kb3cgbGltaXQgb24KZGVwb3NpdC93aXRoZHJhdyBvcGVyYXRpb25zICgjMzE0KS4gVGhlIGxpbWl0IGJvdW5kcyByYXBpZCBjeWNsaW5nIHRoYXQKcmVwZWF0ZWRseSByZXdyaXRlcyBiYWxhbmNlcyBhbmQgZW1pdHMgZXZlbnRzOyBpdCByZXNldHMgd2hlbiB0aGUKd2luZG93IHJvbGxzIG92ZXIuAAAAABFSYXRlTGltaXRFeGNlZWRlZAAAAAAAC8c=", "AAAAAQAAAMpBIHN1YnNjcmliZXIncyBvcHRpb25hbCBjZWlsaW5nIG9uIHRvdGFsIGRlYml0cyBwZXIgZml4ZWQgdGltZSB3aW5kb3csCmFjcm9zcyBhbGwgb2YgdGhlaXIgbWFuZGF0ZXMgZm9yIG9uZSB0b2tlbi4gU2V0IGJ5IHRoZSBzdWJzY3JpYmVyCnRoZW1zZWx2ZXM7IGl0IGNhbiBvbmx5IGV2ZXIgbWFrZSBjaGFyZ2VzIGZhaWwsIG5ldmVyIG1vdmUgZnVuZHMuAAAAAAAAAAAAClNwZW5kTGltaXQAAAAAAAIAAACCTWF4aW11bSB0b3RhbCBkZWJpdCB3aXRoaW4gb25lIHdpbmRvdywgaW4gdGhlIHRva2VuJ3Mgc21hbGxlc3QgdW5pdC4KTm8gbGltaXQgaXMgc3RvcmVkIChhbmQgdGhlIGdldHRlciByZXBvcnRzIDApIHdoZW4gdGhpcyBpcyAwLgAAAAAABmFtb3VudAAAAAAACwAAADxXaW5kb3cgbGVuZ3RoIGluIHNlY29uZHMuIE1lYW5pbmdmdWwgb25seSB3aGVuIGBhbW91bnQgPiAwYC4AAAALd2luZG93X3NlY3MAAAAABg==", "AAAAAQAAACFBIHNpbmdsZSBjb250cmFjdCB1cGdyYWRlIHJlY29yZC4AAAAAAAAAAAAADVVwZ3JhZGVSZWNvcmQAAAAAAAADAAAANExlZGdlciBzZXF1ZW5jZSBudW1iZXIgd2hlbiB0aGUgdXBncmFkZSB3YXMgYXBwbGllZC4AAAAGbGVkZ2VyAAAAAAAEAAAAPFNlbWFudGljIHZlcnNpb24gb2YgdGhlIG5ldyBjb2RlIChmcm9tIGBDQVJHT19QS0dfVkVSU0lPTmApLgAAAAd2ZXJzaW9uAAAAABAAAAAaSGFzaCBvZiB0aGUgbmV3IHdhc20gY29kZS4AAAAAAAl3YXNtX2hhc2gAAAAAAAPuAAAAIA==", "AAAABQAAAE1FbWl0dGVkIHdoZW4gdGhlIHN1YnNjcmlwdGlvbiBjb250cmFjdCBkZWJpdHMgYSBzdWJzY3JpYmVyIHRvIHBheSBhIG1lcmNoYW50LgAAAAAAAAAAAAAFRGViaXQAAAAAAAABAAAABWRlYml0AAAAAAAABQAAABBBY2NvdW50IGRlYml0ZWQuAAAABHVzZXIAAAATAAAAAQAAACxUb2tlbiBjb250cmFjdCBhZGRyZXNzIG9mIHRoZSBkZWJpdGVkIGFzc2V0LgAAAAV0b2tlbgAAAAAAABMAAAABAAAAJ1JlY2lwaWVudCBhZGRyZXNzIHJlY2VpdmluZyB0aGUgdG9rZW5zLgAAAAACdG8AAAAAABMAAAABAAAAJEFtb3VudCB0cmFuc2ZlcnJlZCB0byB0aGUgcmVjaXBpZW50LgAAAAZhbW91bnQAAAAAAAsAAAAAAAAAPVJlbWFpbmluZyBzcGVuZGFibGUgYmFsYW5jZSBvZiB0aGUgc3Vic2NyaWJlciBmb3IgdGhpcyB0b2tlbi4AAAAAAAAHYmFsYW5jZQAAAAALAAAAAAAAAAI=", "AAAABQAAADRFbWl0dGVkIHdoZW4gYSBzdWJzY3JpYmVyIGZ1bmRzIHRoZWlyIHZhdWx0IGJhbGFuY2UuAAAAAAAAAAdEZXBvc2l0AAAAAAEAAAAHZGVwb3NpdAAAAAAEAAAAIUFjY291bnQgdGhhdCBkZXBvc2l0ZWQgdGhlIGZ1bmRzLgAAAAAAAAR1c2VyAAAAEwAAAAEAAAAuVG9rZW4gY29udHJhY3QgYWRkcmVzcyBvZiB0aGUgZGVwb3NpdGVkIGFzc2V0LgAAAAAABXRva2VuAAAAAAAAEwAAAAEAAAAbQW1vdW50IG9mIHRva2VucyBkZXBvc2l0ZWQuAAAAAAZhbW91bnQAAAAAAAsAAAAAAAAAO1VwZGF0ZWQgc3BlbmRhYmxlIGJhbGFuY2Ugb2YgdGhlIHN1YnNjcmliZXIgZm9yIHRoaXMgdG9rZW4uAAAAAAdiYWxhbmNlAAAAAAsAAAAAAAAAAg==", "AAAABQAAAC9FbWl0dGVkIHdoZW4gYSBzdWJzY3JpYmVyIHB1bGxzIGZ1bmRzIGJhY2sgb3V0LgAAAAAAAAAACFdpdGhkcmF3AAAAAQAAAAh3aXRoZHJhdwAAAAQAAAAgQWNjb3VudCB0aGF0IHdpdGhkcmV3IHRoZSBmdW5kcy4AAAAEdXNlcgAAABMAAAABAAAALlRva2VuIGNvbnRyYWN0IGFkZHJlc3Mgb2YgdGhlIHdpdGhkcmF3biBhc3NldC4AAAAAAAV0b2tlbgAAAAAAABMAAAABAAAAG0Ftb3VudCBvZiB0b2tlbnMgd2l0aGRyYXduLgAAAAAGYW1vdW50AAAAAAALAAAAAAAAAD1SZW1haW5pbmcgc3BlbmRhYmxlIGJhbGFuY2Ugb2YgdGhlIHN1YnNjcmliZXIgZm9yIHRoaXMgdG9rZW4uAAAAAAAAB2JhbGFuY2UAAAAACwAAAAAAAAAC", "AAAABQAAANpFbWl0dGVkIHdoZW4gb25lIGFjY291bnQgcGF5cyBpbnRvIGFub3RoZXIgYWNjb3VudCdzIHZhdWx0IGJhbGFuY2UsIGZvcgpleGFtcGxlIGFuIGVtcGxveWVyIGZ1bmRpbmcgYW4gZW1wbG95ZWUncyBzdWJzY3JpcHRpb25zLiBgZnJvbWAgaXMgdGhlCnBheWVyOiB0aGUgYWNjb3VudCB3aG9zZSB0b2tlbnMgbW92ZWQgYW5kIHdob3NlIGF1dGhvcml6YXRpb24gd2FzIHJlcXVpcmVkLgAAAAAAAAAAAApEZXBvc2l0Rm9yAAAAAAABAAAAC2RlcG9zaXRfZm9yAAAAAAUAAAAAAAAABGZyb20AAAATAAAAAQAAAAAAAAAEdXNlcgAAABMAAAABAAAAAAAAAAV0b2tlbgAAAAAAABMAAAABAAAAAAAAAAZhbW91bnQAAAAAAAsAAAAAAAAAAAAAAAdiYWxhbmNlAAAAAAsAAAAAAAAAAg==", "AAAABQAAAatFbWl0dGVkIHdoZW4gYSBzdWJzY3JpYmVyIHNldHMsIGNoYW5nZXMgb3IgcmVsZWFzZXMgdGhlIHJlc2VydmUgZWFybWFya2VkCmZvciBvbmUgbWFuZGF0ZSAoIzI5NSkuIGBhbW91bnRgIGlzIHRoZSBtYW5kYXRlJ3MgbmV3IHJlc2VydmUgKDAgbWVhbnMKcmVsZWFzZWQpIGFuZCBgcmVzZXJ2ZWRfdG90YWxgIHRoZSBzdWJzY3JpYmVyJ3MgdG90YWwgcmVzZXJ2ZSBmb3IgdGhlCnRva2VuIGFmdGVyd2FyZHMsIHNvIGFuIGluZGV4ZXIgY2FuIHRyYWNrIGJvdGggd2l0aG91dCBhIGNvbnRyYWN0IHJlYWQuCgpBIGRlYml0IHRoYXQgZHJhd3MgYSByZXNlcnZlIGRvd24gZG9lcyBub3QgZW1pdCB0aGlzIGV2ZW50OyBpdHMgYERlYml0YAphbHJlYWR5IHJlcG9ydHMgdGhlIG1vdmVtZW50LCBhbmQgYHJlc2VydmVkYCByZXR1cm5zIHRoZSByZW1haW5kZXIuAAAAAAAAAAAKUmVzZXJ2ZVNldAAAAAAAAQAAAAtyZXNlcnZlX3NldAAAAAAFAAAAJlN1YnNjcmliZXIgd2hvc2UgYmFsYW5jZSBpcyBlYXJtYXJrZWQuAAAAAAAEdXNlcgAAABMAAAABAAAAJFRva2VuIHRoZSByZXNlcnZlIGlzIGRlbm9taW5hdGVkIGluLgAAAAV0b2tlbgAAAAAAABMAAAABAAAAJU1hbmRhdGUgdGhlIHJlc2VydmUgaXMgZWFybWFya2VkIGZvci4AAAAAAAAKbWFuZGF0ZV9pZAAAAAAABgAAAAEAAAA/TmV3IHJlc2VydmUgZm9yIHRoaXMgbWFuZGF0ZS4gMCBtZWFucyB0aGUgcmVzZXJ2ZSB3YXMgcmVsZWFzZWQuAAAAAAZhbW91bnQAAAAAAAsAAAAAAAAAQ1RvdGFsIHJlc2VydmVkIGJ5IGB1c2VyYCBpbiBgdG9rZW5gIGFjcm9zcyBhbGwgbWFuZGF0ZXMgYWZ0ZXJ3YXJkcy4AAAAADnJlc2VydmVkX3RvdGFsAAAAAAALAAAAAAAAAAI=", "AAAABQAAAJdFbWl0dGVkIHdoZW4gYSBzdWJzY3JpYmVyIHB1bGxzIGZ1bmRzIG91dCB0byBhIGRlc3RpbmF0aW9uIG90aGVyIHRoYW4KdGhlbXNlbHZlcy4gVGhlIHN1YnNjcmliZXIgYXV0aG9yaXplcyB0aGUgY2FsbDsgdGhlIGRlc3RpbmF0aW9uIGRvZXMgbm90Cm5lZWQgdG8uAAAAAAAAAAAKV2l0aGRyYXdUbwAAAAAAAQAAAAt3aXRoZHJhd190bwAAAAAFAAAAIkFjY291bnQgd2hvc2UgYmFsYW5jZSB3YXMgZGViaXRlZC4AAAAAAAR1c2VyAAAAEwAAAAEAAAAhQWNjb3VudCB0aGF0IHJlY2VpdmVkIHRoZSB0b2tlbnMuAAAAAAAAAnRvAAAAAAATAAAAAQAAAC5Ub2tlbiBjb250cmFjdCBhZGRyZXNzIG9mIHRoZSB3aXRoZHJhd24gYXNzZXQuAAAAAAAFdG9rZW4AAAAAAAATAAAAAQAAABtBbW91bnQgb2YgdG9rZW5zIHdpdGhkcmF3bi4AAAAABmFtb3VudAAAAAAACwAAAAAAAAA9UmVtYWluaW5nIHNwZW5kYWJsZSBiYWxhbmNlIG9mIHRoZSBzdWJzY3JpYmVyIGZvciB0aGlzIHRva2VuLgAAAAAAAAdiYWxhbmNlAAAAAAsAAAAAAAAAAg==", "AAAABQAAAEZFbWl0dGVkIHdoZW4gdGhlIGFkbWluIHNldHMgdGhlIHBlci10b2tlbiBkZXBvc2l0IGNhcCBvZiBhIHN1YnNjcmliZXIuAAAAAAAAAAAADURlcG9zaXRDYXBTZXQAAAAAAAABAAAAD2RlcG9zaXRfY2FwX3NldAAAAAADAAAAJkFjY291bnQgd2hvc2UgZGVwb3NpdCBjYXAgd2FzIHVwZGF0ZWQuAAAAAAAEdXNlcgAAABMAAAABAAAAKlRva2VuIGNvbnRyYWN0IGFkZHJlc3Mgc3ViamVjdCB0byB0aGUgY2FwLgAAAAAABXRva2VuAAAAAAAAEwAAAAEAAAAiVGhlIG5ldyBjYXAuIFplcm8gbWVhbnMgdW5saW1pdGVkLgAAAAAAA2NhcAAAAAALAAAAAAAAAAI=", "AAAABQAAAJJFbWl0dGVkIHdoZW4gdGhlIGFkbWluIHJlcGxhY2VzIHRoaXMgY29udHJhY3QncyB3YXNtLiBQZXJzaXN0ZW50IHN0YXRlIGlzCnVudG91Y2hlZCBieSBhbiB1cGdyYWRlOyB0aGUgbmV3IGNvZGUgcmVhZHMgdGhlIGtleXMgdGhlIG9sZCBjb2RlIHdyb3RlLgAAAAAAAAAAABBDb250cmFjdFVwZ3JhZGVkAAAAAQAAABFjb250cmFjdF91cGdyYWRlZAAAAAAAAAIAAAAAAAAABWFkbWluAAAAAAAAEwAAAAEAAAAAAAAADW5ld193YXNtX2hhc2gAAAAAAAPuAAAAIAAAAAAAAAAC", "AAAAAAAAAChBZG1pbiBhZGRyZXNzIGNvbmZpZ3VyZWQgZm9yIHRoaXMgdmF1bHQuAAAABWFkbWluAAAAAAAAAAAAAAEAAAPpAAAAEwAAAAM=", "AAAAAAAAAnZEZWJpdCBhIHN1YnNjcmliZXIgYW5kIHBheSBgdG9gLiBSZXN0cmljdGVkIHRvIHRoZSBzdWJzY3JpcHRpb24KY29udHJhY3QsIHdoaWNoIGlzIHRoZSBkaXJlY3QgY2FsbGVyIGFuZCBpcyB0aGVyZWZvcmUgaW1wbGljaXRseQphdXRob3JpemVkIGJ5IHRoZSBob3N0IHdoZW4gaXQgaW52b2tlcyB0aGlzIGZ1bmN0aW9uLgoKIyBBdXRoClJlcXVpcmVzIGF1dGhvcml6YXRpb24gZnJvbSB0aGUgcmVnaXN0ZXJlZCBgc3Vic2NyaXB0aW9uYCBjb250cmFjdC4KCiMgRXZlbnRzCkVtaXRzIFtgRGViaXRgXS4KCiMgRXJyb3JzClJldHVybnMgW2BFcnJvcjo6U3Vic2NyaXB0aW9uTm90U2V0YF0gaWYgbm8gc3Vic2NyaXB0aW9uIGNvbnRyYWN0IGlzIHNldCwKW2BFcnJvcjo6SW52YWxpZEFtb3VudGBdIGlmIGBhbW91bnQgPD0gMGAsIG9yIFtgRXJyb3I6Okluc3VmZmljaWVudEJhbGFuY2VgXQppZiBgYW1vdW50YCBleGNlZWRzIGB1c2VyYCdzIHVucmVzZXJ2ZWQgYmFsYW5jZS4gVGhpcyBlbnRyeSBwb2ludAppcyBub3QgdGllZCB0byBhIG1hbmRhdGUsIHNvIGl0IGNhbiBuZXZlciBkcmF3IG9uIGEgcmVzZXJ2ZTsgdXNlCltgVmF1bHQ6OmRlYml0X21hbmRhdGVgXSBmb3IgYSBtYW5kYXRlJ3Mgb3duIGNoYXJnZS4AAAAAAAVkZWJpdAAAAAAAAAQAAAAAAAAABHVzZXIAAAATAAAAAAAAAAV0b2tlbgAAAAAAABMAAAAAAAAAAnRvAAAAAAATAAAAAAAAAAZhbW91bnQAAAAAAAsAAAABAAAD6QAAAAIAAAAD", "AAAAAAAAAINUb3RhbCBiYWxhbmNlIG9mIGB1c2VyYCBpbiBgdG9rZW5gIGhlbGQgaW4gdGhlIHZhdWx0LCByZXNlcnZlZCBvcgpub3QuIFNlZSBgdW5yZXNlcnZlZF9iYWxhbmNlYCBmb3IgdGhlIHBhcnQgb3V0c2lkZSBldmVyeSByZXNlcnZlLgAAAAAHYmFsYW5jZQAAAAACAAAAAAAAAAR1c2VyAAAAEwAAAAAAAAAFdG9rZW4AAAAAAAATAAAAAQAAAAs=", "AAAAAAAAAeVNb3ZlIGBhbW91bnRgIG9mIGB0b2tlbmAgZnJvbSB0aGUgc3Vic2NyaWJlcidzIHdhbGxldCBpbnRvIHRoZSB2YXVsdC4KCiMgQXV0aApSZXF1aXJlcyBhdXRob3JpemF0aW9uIGZyb20gYHVzZXJgLgoKIyBFdmVudHMKRW1pdHMgW2BEZXBvc2l0YF0uCgojIEVycm9ycwpSZXR1cm5zIFtgRXJyb3I6OkludmFsaWRBbW91bnRgXSBpZiBgYW1vdW50IDw9IDBgLCBbYEVycm9yOjpUb2tlbk5vdEFsbG93ZWRgXQppZiB0aGUgYWxsb3dsaXN0IGlzIGVuYWJsZWQgYW5kIGB0b2tlbmAgaXMgbm90IGFsbG93ZWQsCltgRXJyb3I6OkRlcG9zaXRDYXBFeGNlZWRlZGBdIGlmIHRoZSBkZXBvc2l0IHdvdWxkIGV4Y2VlZCB0aGUKc3Vic2NyaWJlcidzIGNhcCwgb3IgW2BFcnJvcjo6UmF0ZUxpbWl0RXhjZWVkZWRgXSBpZiB0aGUKKHVzZXIsIHRva2VuKSBwYWlyIGhhcyB1c2VkIGl0cyBkZXBvc2l0L3dpdGhkcmF3IGJ1ZGdldCBmb3IgdGhpcwp3aW5kb3cgKCMzMTQpLgAAAAAAAAdkZXBvc2l0AAAAAAMAAAAAAAAABHVzZXIAAAATAAAAAAAAAAV0b2tlbgAAAAAAABMAAAAAAAAABmFtb3VudAAAAAAACwAAAAEAAAPpAAAAAgAAAAM=", "AAAAAAAAACVUaGUgdmVyc2lvbiBvZiB0aGlzIGNvbnRyYWN0J3MgY3JhdGUuAAAAAAAAB3ZlcnNpb24AAAAAAAAAAAEAAAAQ", "AAAAAAAAAE1UaGUgcmVzZXJ2ZSBgdXNlcmAgZWFybWFya2VkIGZvciBgbWFuZGF0ZV9pZGAgaW4gYHRva2VuYC4gMCB3aGVuCm5vbmUgaXMgc2V0LgAAAAAAAAhyZXNlcnZlZAAAAAMAAAAAAAAABHVzZXIAAAATAAAAAAAAAAV0b2tlbgAAAAAAABMAAAAAAAAACm1hbmRhdGVfaWQAAAAAAAYAAAABAAAACw==", "AAAAAAAAAt5SZXR1cm4gdW5jb21taXR0ZWQgZnVuZHMgdG8gdGhlIHN1YnNjcmliZXIuIEFsd2F5cyBhdmFpbGFibGU6IHRoZSB2YXVsdApuZXZlciBsb2NrcyBhIGJhbGFuY2UsIHNvIGEgc3Vic2NyaWJlciBjYW4gZXhpdCBhdCBhbnkgdGltZS4KCk9ubHkgdGhlIHVucmVzZXJ2ZWQgcGFydCBvZiB0aGUgYmFsYW5jZSBjYW4gYmUgd2l0aGRyYXduLiBBIHJlc2VydmUKaXMgdGhlIHN1YnNjcmliZXIncyBvd24gZWFybWFyayAoIzI5NSkgYW5kIGBzZXRfcmVzZXJ2ZSguLiwgMClgCnJlbGVhc2VzIGl0IGF0IGFueSB0aW1lLCBzbyByZXNlcnZpbmcgbmV2ZXIgdHJhcHMgZnVuZHM6IGl0IG9ubHkKc3RvcHMgdGhlbSBsZWF2aW5nIGJ5IGFjY2lkZW50IHdoaWxlIGEgbWFuZGF0ZSBpcyBjb3VudGluZyBvbiB0aGVtLgoKIyBBdXRoClJlcXVpcmVzIGF1dGhvcml6YXRpb24gZnJvbSBgdXNlcmAuCgojIEV2ZW50cwpFbWl0cyBbYFdpdGhkcmF3YF0uCgojIEVycm9ycwpSZXR1cm5zIFtgRXJyb3I6OkludmFsaWRBbW91bnRgXSBpZiBgYW1vdW50IDw9IDBgLApbYEVycm9yOjpJbnN1ZmZpY2llbnRCYWxhbmNlYF0gaWYgYGFtb3VudGAgZXhjZWVkcyB0aGUgdXNlcidzCnVucmVzZXJ2ZWQgYmFsYW5jZSwgb3IgW2BFcnJvcjo6UmF0ZUxpbWl0RXhjZWVkZWRgXSBpZiB0aGUKKHVzZXIsIHRva2VuKSBwYWlyIGhhcyB1c2VkIGl0cyBkZXBvc2l0L3dpdGhkcmF3IGJ1ZGdldCBmb3IgdGhpcwp3aW5kb3cgKCMzMTQpLgAAAAAACHdpdGhkcmF3AAAAAwAAAAAAAAAEdXNlcgAAABMAAAAAAAAABXRva2VuAAAAAAAAEwAAAAAAAAAGYW1vdW50AAAAAAALAAAAAQAAA+kAAAACAAAAAw==", "AAAAAAAAAW9XaGV0aGVyIGBkZWJpdF9tYW5kYXRlKHVzZXIsIHRva2VuLCBtYW5kYXRlX2lkLCBfLCBhbW91bnQpYCB3b3VsZApzdWNjZWVkIHJpZ2h0IG5vdyBvbiBiYWxhbmNlIGFuZCBzcGVuZC1saW1pdCBncm91bmRzOiB0aGUKdW5yZXNlcnZlZCBiYWxhbmNlIHBsdXMgdGhlIG1hbmRhdGUncyByZXNlcnZlIGNvdmVycyBgYW1vdW50YCwgYW5kCnRoZSBzdWJzY3JpYmVyJ3Mgc3BlbmQgbGltaXQgaGFzIHJvb20gZm9yIGl0LiBSZWFkcyBzdG9yYWdlIG9ubHkuCgpUaGUgc3Vic2NyaXB0aW9uIGNvbnRyYWN0IHVzZXMgdGhpcyB0byBwcm92ZSBvbiBjaGFpbiB0aGF0IGEgZHVlCmNoYXJnZSB3b3VsZCBmYWlsIGJlZm9yZSBmbGFnZ2luZyBpdCAoIzI5NikuAAAAAAljYW5fZGViaXQAAAAAAAAEAAAAAAAAAAR1c2VyAAAAEwAAAAAAAAAFdG9rZW4AAAAAAAATAAAAAAAAAAptYW5kYXRlX2lkAAAAAAAGAAAAAAAAAAZhbW91bnQAAAAAAAsAAAABAAAAAQ==", "AAAAAAAAAQJXaGV0aGVyIGB1c2VyYCBtYXkgZGVwb3NpdCBgdG9rZW5gIGludG8gdGhlIHZhdWx0LiBSZXR1cm5zIGB0cnVlYAp3aGVuIHRoZSBhbGxvd2xpc3QgaXMgZGlzYWJsZWQsIG9yIHdoZW4gaXQgaXMgZW5hYmxlZCBhbmQgYHRva2VuYAppcyBleHBsaWNpdGx5IGFsbG93ZWQuIFRoaXMgaXMgYSBjb252ZW5pZW5jZSB2aWV3IHNvIGNhbGxlcnMgZG8gbm90Cm5lZWQgdG8gcmVhZCBib3RoIGBhbGxvd2xpc3RfZW5hYmxlZGAgYW5kIGB0b2tlbl9hbGxvd2VkYC4AAAAAAAtjYW5fZGVwb3NpdAAAAAACAAAAAAAAAAR1c2VyAAAAEwAAAAAAAAAFdG9rZW4AAAAAAAATAAAAAQAAAAE=", "AAAAAAAABABEZWJpdCBhIHN1YnNjcmliZXIgb25jZSBhbmQgcGF5IHR3byByZWNpcGllbnRzIGZyb20gdGhhdCBkZWJpdC4KCmBjaGFyZ2VgIHNwbGl0cyBldmVyeSBzZXR0bGVtZW50IGludG8gYSBtZXJjaGFudCBzaGFyZSBhbmQgYSBwcm90b2NvbApmZWUuIFNldHRsaW5nIHRoYXQgd2l0aCB0d28gYGRlYml0YCBjYWxscyByZXBlYXRlZCB0aGUgYXV0aG9yaXphdGlvbgpjaGVjaywgdGhlIGBTdWJzY3JpcHRpb25gIHJlYWQgYW5kIHRoZSBiYWxhbmNlIHJlYWQgYW5kIHdyaXRlIGZvciB0aGUKc2FtZSB0b3RhbC4gYGRlYml0X3NwbGl0YCBwZXJmb3JtcyB0aGVtIG9uY2UsIHRoZW4gbWFrZXMgdGhlIHNhbWUgdHdvCnRva2VuIHRyYW5zZmVycyBhbmQgZW1pdHMgb25lIFtgRGViaXRgXSBwZXIgbm9uLWVtcHR5IGxlZywgd2l0aCB0aGUKcnVubmluZyBiYWxhbmNlcyB0d28gc2VxdWVudGlhbCBgZGViaXRgIGNhbGxzIHdvdWxkIGhhdmUgcmVwb3J0ZWQ6CnRoZSBmaXJzdCBldmVudCBjYXJyaWVzIHRoZSBiYWxhbmNlIGFmdGVyIGBhbW91bnRfYWAsIHRoZSBzZWNvbmQgdGhlCmJhbGFuY2UgYWZ0ZXIgYm90aCBsZWdzLgoKYGFtb3VudF9hYCBtdXN0IGJlIHBvc2l0aXZlLCBsaWtlIGBkZWJpdGAuIGBhbW91bnRfYmAgbWF5IGJlIHplcm86IGEKc2V0dGxlbWVudCB3aXRoIG5vIHByb3RvY29sIGZlZSB0aGVuIGhhcyBhIHNpbmdsZSBsZWcgYW5kIGVtaXRzIGEKc2luZ2xlIFtgRGViaXRgXSwgZXhhY3RseSBhcyBvbmUgYGRlYml0YCBjYWxsIHdvdWxkIGhhdmUuCgojIEF1dGgKUmVxdWlyZXMgYXV0aG9yaXphdGlvbiBmcm9tIHRoZSByZWdpc3RlcmVkIGBzdWJzY3JpcHRpb25gIGNvbnRyYWN0LgoKIyBFdmVudHMKRW1pdHMgb25lIFtgRGViaXRgXSBwZXIgbm9uLWVtcHR5IGxlZy4KCiMgRXJyb3JzClJldHVybnMgW2BFcnJvcjo6U3Vic2NyaXB0aW9uTm90U2V0YF0gaWYgbm8gc3Vic2NyaXB0aW9uIGNvbnRyYWN0IGlzCnNldCwgW2BFcnJvcjo6SW52YWxpZEFtb3VudGBdIGlmIGBhbW91bnRfYSA8PSAwYCBvciBgAAAAC2RlYml0X3NwbGl0AAAAAAYAAAAAAAAABHVzZXIAAAATAAAAAAAAAAV0b2tlbgAAAAAAABMAAAAAAAAABHRvX2EAAAATAAAAAAAAAAhhbW91bnRfYQAAAAsAAAAAAAAABHRvX2IAAAATAAAAAAAAAAhhbW91bnRfYgAAAAsAAAABAAAD6QAAAAIAAAAD", "AAAAAAAAADhUaGUgZGVwb3NpdCBjYXAgZm9yICh1c2VyLCB0b2tlbikuIFplcm8gbWVhbnMgdW5saW1pdGVkLgAAAAtkZXBvc2l0X2NhcAAAAAACAAAAAAAAAAR1c2VyAAAAEwAAAAAAAAAFdG9rZW4AAAAAAAATAAAAAQAAAAs=", "AAAAAAAAAa9DcmVkaXQgYHVzZXJgJ3MgYmFsYW5jZSB3aXRoIHRva2VucyBwYWlkIGJ5IGEgZGlmZmVyZW50IGFjY291bnQKYGZyb21gIOKAlCB0aGlyZC1wYXJ0eSBmdW5kaW5nIChtZXJnZWQgZnJvbSAjMTM2KS4gYGZyb21gIGF1dGhvcml6ZXMKYW5kIHBheXM7IGB1c2VyYCByZWNlaXZlcyB0aGUgc3BlbmRhYmxlIGJhbGFuY2UuCgpBcHBsaWVzIHRoZSBzYW1lIGFsbG93bGlzdCwgZGVwb3NpdC1jYXAgYW5kIHBlci13aW5kb3cgb3BlcmF0aW9uCmNoZWNrcyBhcyBgZGVwb3NpdGAsIHNvIGZ1bmRpbmcgb24gc29tZW9uZSdzIGJlaGFsZiBjYW5ub3QgYnlwYXNzCnRoZW0uICgjMTM2IHByZWRhdGVkIHRob3NlIGZlYXR1cmVzOyBlbmZvcmNpbmcgdGhlbSBoZXJlIGtlZXBzCnRoaXJkLXBhcnR5IGZ1bmRpbmcgY29uc2lzdGVudCB3aXRoIGEgZGlyZWN0IGRlcG9zaXQuKQAAAAALZGVwb3NpdF9mb3IAAAAABAAAAAAAAAAEZnJvbQAAABMAAAAAAAAABHVzZXIAAAATAAAAAAAAAAV0b2tlbgAAAAAAABMAAAAAAAAABmFtb3VudAAAAAAACwAAAAEAAAPpAAAAAgAAAAM=", "AAAAAAAAA+pFYXJtYXJrIGBhbW91bnRgIG9mIHRoZSBzdWJzY3JpYmVyJ3MgYHRva2VuYCBiYWxhbmNlIGZvciBvbmUKbWFuZGF0ZSAoIzI5NSksIHJlcGxhY2luZyBhbnkgcmVzZXJ2ZSBpdCBhbHJlYWR5IGhhZC4gYDBgIHJlbGVhc2VzCnRoZSByZXNlcnZlLgoKV2hpbGUgcmVzZXJ2ZWQsIHRob3NlIGZ1bmRzIGNhbiBiZSBzcGVudCBvbmx5IGJ5IGBtYW5kYXRlX2lkYCdzCm93biBjaGFyZ2VzICh0aHJvdWdoIGBkZWJpdF9tYW5kYXRlYCAvIGBkZWJpdF9zcGxpdF9tYW5kYXRlYCwgd2hpY2gKZHJhdyB0aGUgcmVzZXJ2ZSBkb3duIGZpcnN0KS4gT3RoZXIgbWFuZGF0ZXMnIGRlYml0cywgcGxhaW4KYGRlYml0YC9gZGViaXRfc3BsaXRgLCBgd2l0aGRyYXdgIGFuZCBgd2l0aGRyYXdfYWxsYCBzZWUgb25seSB0aGUKdW5yZXNlcnZlZCBiYWxhbmNlLiBUaGUgc3Vic2NyaWJlciBjYW4gcmVsZWFzZSBhIHJlc2VydmUgYXQgYW55CnRpbWUsIHNvIGEgcmVzZXJ2ZSBuZXZlciB0cmFwcyBmdW5kcy4KClRoZSB2YXVsdCBkb2VzIG5vdCBrbm93IG1hbmRhdGVzOiBgbWFuZGF0ZV9pZGAgaXMgdGhlIHN1YnNjcmlwdGlvbgpjb250cmFjdCdzIGlkLCB3aGljaCB0aGF0IGNvbnRyYWN0IHBhc3NlcyBvbiBldmVyeSBtYW5kYXRlIGRlYml0LgpSZXNlcnZpbmcgZm9yIGFuIGlkIHRoYXQgaXMgbm90IHRoZSBzdWJzY3JpYmVyJ3Mgb3duIG1hbmRhdGUgb25seQpldmVyIHdpdGhob2xkcyB0aGUgc3Vic2NyaWJlcidzIG1vbmV5IGZyb20gdGhlbXNlbHZlcy4KCiMgQXV0aApSZXF1aXJlcyBhdXRob3JpemF0aW9uIGZyb20gYHVzZXJgLgoKIyBFdmVudHMKRW1pdHMgW2BSZXNlcnZlU2V0YF0uCgojIEVycm9ycwpSZXR1cm5zIFtgRXJyb3I6OkludmFsaWRBbW91bnRgXSBpZiBgYW1vdW50IDwgMGAsIG9yCltgRXJyb3I6OlJlc2VydmVFeGNlZWRzQmFsYW5jZWBdIGlmIHRoZSBzdWJzY3JpYmVyJ3MgdG90YWwgcmVzZXJ2ZQpmb3IgYHRva2VuYCB3b3VsZCBleGNlZWQgdGhlaXIgYmFsYW5jZS4AAAAAAAtzZXRfcmVzZXJ2ZQAAAAAEAAAAAAAAAAR1c2VyAAAAEwAAAAAAAAAFdG9rZW4AAAAAAAATAAAAAAAAAAptYW5kYXRlX2lkAAAAAAAGAAAAAAAAAAZhbW91bnQAAAAAAAsAAAABAAAD6QAAAAIAAAAD", "AAAAAAAAAD9UaGUgdXNlcidzIHN0b3JlZCBzcGVuZGluZyBsaW1pdC4gYGFtb3VudCA9PSAwYCBtZWFucyBubyBsaW1pdC4AAAAAC3NwZW5kX2xpbWl0AAAAAAIAAAAAAAAABHVzZXIAAAATAAAAAAAAAAV0b2tlbgAAAAAAABMAAAABAAAH0AAAAApTcGVuZExpbWl0AAA=", "AAAAAAAAA5BSZXR1cm4gdW5jb21taXR0ZWQgZnVuZHMgdG8gYSBkZXN0aW5hdGlvbiBjaG9zZW4gYnkgdGhlIHN1YnNjcmliZXIuCgpJZGVudGljYWwgdG8gW2BWYXVsdDo6d2l0aGRyYXdgXSBleGNlcHQgdGhhdCB0aGUgdG9rZW5zIGFyZSBzZW50IHRvCmB0b2AgaW5zdGVhZCBvZiB0aGUgY2FsbGVyLiBPbmx5IGB1c2VyYCBhdXRob3JpemVzOiBgdG9gIGlzIGEKZGVzdGluYXRpb24sIG5vdCBhIHNlY29uZCBzaWduZXIsIHNvIGZ1bmRzIGNhbiBiZSBzd2VwdCB0byBhCmN1c3RvZHksIGhhcmR3YXJlIG9yIHRyZWFzdXJ5IGFkZHJlc3Mgd2l0aG91dCBnaXZpbmcgdGhhdCBhZGRyZXNzCmF1dGhvcml0eSBvdmVyIHRoZSB2YXVsdC4KCk9ubHkgdGhlIHVucmVzZXJ2ZWQgcGFydCBvZiB0aGUgYmFsYW5jZSBjYW4gYmUgd2l0aGRyYXduLCBleGFjdGx5IGFzCndpdGggYHdpdGhkcmF3YC4gQSBkZXN0aW5hdGlvbiBtYXkgYmUgYHVzZXJgIGl0c2VsZiwgaW4gd2hpY2ggY2FzZQp0aGlzIGJlaGF2ZXMgbGlrZSBgd2l0aGRyYXdgIGJ1dCByZXBvcnRzIFtgV2l0aGRyYXdUb2BdLgoKIyBBdXRoClJlcXVpcmVzIGF1dGhvcml6YXRpb24gZnJvbSBgdXNlcmAuIGB0b2AgbmVlZHMgbm8gYXV0aG9yaXphdGlvbi4KCiMgRXZlbnRzCkVtaXRzIFtgV2l0aGRyYXdUb2BdLgoKIyBFcnJvcnMKUmV0dXJucyBbYEVycm9yOjpJbnZhbGlkQW1vdW50YF0gaWYgYGFtb3VudCA8PSAwYCwKW2BFcnJvcjo6SW5zdWZmaWNpZW50QmFsYW5jZWBdIGlmIGBhbW91bnRgIGV4Y2VlZHMgdGhlIHVzZXIncwp1bnJlc2VydmVkIGJhbGFuY2UsIG9yIFtgRXJyb3I6OlJhdGVMaW1pdEV4Y2VlZGVkYF0gaWYgdGhlCih1c2VyLCB0b2tlbikgcGFpciBoYXMgdXNlZCBpdHMgZGVwb3NpdC93aXRoZHJhdyBidWRnZXQgZm9yIHRoaXMKd2luZG93ICgjMzE0KS4AAAALd2l0aGRyYXdfdG8AAAAABAAAAAAAAAAEdXNlcgAAABMAAAAAAAAABXRva2VuAAAAAAAAEwAAAAAAAAACdG8AAAAAABMAAAAAAAAABmFtb3VudAAAAAAACwAAAAEAAAPpAAAAAgAAAAM=", "AAAAAAAAAEZBZGRyZXNzIG9mIHRoZSBzdWJzY3JpcHRpb24gY29udHJhY3QgY3VycmVudGx5IGFsbG93ZWQgdG8gZGViaXQgZnVuZHMuAAAAAAAMc3Vic2NyaXB0aW9uAAAAAAAAAAEAAAPpAAAAEwAAAAM=", "AAAAAAAAATlIYW5kIHRoZSBhZG1pbiByb2xlIHRvIGFub3RoZXIgYWRkcmVzcy4gT25seSB0aGUgY3VycmVudCBhZG1pbiBtYXkKY2FsbCB0aGlzLCBhbmQgdGhlIG5ldyBhZG1pbiB0YWtlcyBlZmZlY3QgaW1tZWRpYXRlbHk6IHRoZSBvbGQgb25lCmtlZXBzIG5vIGF1dGhvcml0eS4gQSBkZXBsb3ltZW50IHdob3NlIGFkbWluIGtleSBpcyBsb3N0IG9yCnJvdGF0ZWQgaXMgb3RoZXJ3aXNlIHN0dWNrIHdpdGggaXQgZm9yIHRoZSBsaWZlIG9mIHRoZSBjb250cmFjdC4KCiMgQXV0aApSZXF1aXJlcyBhdXRob3JpemF0aW9uIGZyb20gdGhlIGN1cnJlbnQgYWRtaW4uAAAAAAAADHVwZGF0ZV9hZG1pbgAAAAEAAAAAAAAACW5ld19hZG1pbgAAAAAAABMAAAABAAAD6QAAAAIAAAAD", "AAAAAAAAAPdSZXBsYWNlIHRoaXMgY29udHJhY3QncyB3YXNtIHdpdGggY29kZSBhdCBgbmV3X3dhc21faGFzaGAuIE9ubHkgdGhlCmFkbWluIG1heSB1cGdyYWRlOyBwZXJzaXN0ZW50IHN0YXRlIGlzIHVudG91Y2hlZC4gKEZyb20gIzE1OC4pCgpSZWNvcmRzIHRoZSB1cGdyYWRlIGluIHRoZSBjb250cmFjdCdzIHVwZ3JhZGUgaGlzdG9yeSwgd2hpY2ggY2FuIGJlCnF1ZXJpZWQgdmlhIGB1cGdyYWRlX2hpc3RvcnlgIGZvciB0cmFuc3BhcmVuY3kuAAAAAAx1cGdyYWRlX3dhc20AAAABAAAAAAAAAA1uZXdfd2FzbV9oYXNoAAAAAAAD7gAAACAAAAABAAAD6QAAAAIAAAAD", "AAAAAAAAA1lXaXRoZHJhdyB0aGUgZW50aXJlIGJhbGFuY2UgaW4gb25lIGNhbGwuCgpFeGl0aW5nIG11c3Qgbm90IGJlIGEgcmFjZTogd2l0aCBgd2l0aGRyYXdgLCBhIGNoYXJnZSB0aGF0IHNldHRsZXMKYmV0d2VlbiByZWFkaW5nIHRoZSBiYWxhbmNlIGFuZCB3aXRoZHJhd2luZyBtYWtlcyB0aGUgd2l0aGRyYXcgZmFpbAp3aXRoIFtgRXJyb3I6Okluc3VmZmljaWVudEJhbGFuY2VgXS4gVGhpcyB0YWtlcyB3aGF0ZXZlciBpcyB0aGVyZQppbnN0ZWFkLCBzbyBhIHN1YnNjcmliZXIgbGVhdmluZyB0aGUgcHJvdG9jb2wgY2Fubm90IGJlIHRod2FydGVkIGJ5CmEga2VlcGVyLgoKIkVudGlyZSIgbWVhbnMgZXZlcnl0aGluZyBub3QgcmVzZXJ2ZWQgZm9yIGEgbWFuZGF0ZSAoIzI5NSk6IGEKcmVzZXJ2ZSBzdGF5cyBiZWhpbmQgdW50aWwgdGhlIHN1YnNjcmliZXIgcmVsZWFzZXMgaXQgd2l0aApgc2V0X3Jlc2VydmUoLi4sIDApYCwgYWZ0ZXIgd2hpY2ggdGhpcyB0YWtlcyB0aGUgcmVzdC4KCiMgQXV0aApSZXF1aXJlcyBhdXRob3JpemF0aW9uIGZyb20gYHVzZXJgLgoKIyBFdmVudHMKRW1pdHMgW2BXaXRoZHJhd2BdIGZvciBhIG5vbi1lbXB0eSBiYWxhbmNlLCBhbmQgbm90aGluZyBhdCBhbGwgZm9yIGFuCmVtcHR5IG9uZS4KCiMgRXJyb3JzCk5ldmVyIGZhaWxzIG9uIHRoZSBiYWxhbmNlIGl0c2VsZjogYW4gZW1wdHkgYmFsYW5jZSByZXR1cm5zIGAwYAp3aXRob3V0IGEgdG9rZW4gdHJhbnNmZXIuIFJldHVybnMgW2BFcnJvcjo6UmF0ZUxpbWl0RXhjZWVkZWRgXSBpZiB0aGUKKHVzZXIsIHRva2VuKSBwYWlyIGhhcyB1c2VkIGl0cyBkZXBvc2l0L3dpdGhkcmF3IGJ1ZGdldCBmb3IgdGhpcwp3aW5kb3cgKCMzMTQpLgAAAAAAAAx3aXRoZHJhd19hbGwAAAACAAAAAAAAAAR1c2VyAAAAEwAAAAAAAAAFdG9rZW4AAAAAAAATAAAAAQAAA+kAAAALAAAAAw==", "AAAAAAAAANBSdW5zIG9uY2UsIGluIHRoZSBzYW1lIHRyYW5zYWN0aW9uIHRoYXQgZGVwbG95cyB0aGUgY29udHJhY3QuIFdpdGggYQpzZXBhcmF0ZSBgaW5pdGlhbGl6ZWAgY2FsbCwgYW55b25lIHdhdGNoaW5nIHRoZSBsZWRnZXIgY291bGQgY2FsbCBpdApiZXR3ZWVuIHRoZSBkZXBsb3kgYW5kIHRoZSBvd25lcidzIG93biBjYWxsIGFuZCBtYWtlIHRoZW1zZWx2ZXMgYWRtaW4uAAAADV9fY29uc3RydWN0b3IAAAAAAAABAAAAAAAAAAVhZG1pbgAAAAAAABMAAAAA", "AAAAAAAAAhJEZWJpdCBhIHN1YnNjcmliZXIgb24gYmVoYWxmIG9mIG9uZSBtYW5kYXRlICgjMjk1KS4KCklkZW50aWNhbCB0byBbYFZhdWx0OjpkZWJpdGBdLCBleGNlcHQgdGhhdCBgbWFuZGF0ZV9pZGAncyBvd24KcmVzZXJ2ZSBpcyBzcGVuZGFibGUgdG9vIGFuZCBpcyBkcmF3biBkb3duIGZpcnN0OiB0aGUgbWFuZGF0ZSBjYW4Kc3BlbmQgaXRzIHJlc2VydmUgcGx1cyB0aGUgdW5yZXNlcnZlZCBiYWxhbmNlLCB3aGlsZSBldmVyeSBvdGhlcgptYW5kYXRlJ3MgcmVzZXJ2ZSBzdGF5cyBvdXQgb2YgcmVhY2guCgojIEF1dGgKUmVxdWlyZXMgYXV0aG9yaXphdGlvbiBmcm9tIHRoZSByZWdpc3RlcmVkIGBzdWJzY3JpcHRpb25gIGNvbnRyYWN0LgoKIyBFdmVudHMKRW1pdHMgW2BEZWJpdGBdLgoKIyBFcnJvcnMKQXMgW2BWYXVsdDo6ZGViaXRgXSwgd2l0aCBbYEVycm9yOjpJbnN1ZmZpY2llbnRCYWxhbmNlYF0gbWVhbmluZwpgYW1vdW50YCBleGNlZWRzIHRoZSB1bnJlc2VydmVkIGJhbGFuY2UgcGx1cyB0aGlzIG1hbmRhdGUncyByZXNlcnZlLgAAAAAADWRlYml0X21hbmRhdGUAAAAAAAAFAAAAAAAAAAR1c2VyAAAAEwAAAAAAAAAFdG9rZW4AAAAAAAATAAAAAAAAAAptYW5kYXRlX2lkAAAAAAAGAAAAAAAAAAJ0bwAAAAAAEwAAAAAAAAAGYW1vdW50AAAAAAALAAAAAQAAA+kAAAACAAAAAw==", "AAAAAAAAAG5XaGV0aGVyIGB0b2tlbmAgaXMgb24gdGhlIGFsbG93bGlzdC4gT25seSBtZWFuaW5nZnVsIHdoaWxlIHRoZQphbGxvd2xpc3QgaXMgZW5hYmxlZDsgYWJzZW50IG1lYW5zIG5vdCBhbGxvd2VkLgAAAAAADXRva2VuX2FsbG93ZWQAAAAAAAABAAAAAAAAAAV0b2tlbgAAAAAAABMAAAABAAAAAQ==", "AAAAAAAAAf9BbW91bnQgdGhlIHN1YnNjcmliZXIgd291bGQgbmVlZCB0byBkZXBvc2l0IGZvciBgcGVyaW9kc2AgY2hhcmdlcwpvZiBgY2hhcmdlX2Ftb3VudGAgZWFjaCB1bmRlciBgbWFuZGF0ZV9pZGAuCgpUaGUgYXZhaWxhYmxlIGFtb3VudCBpbmNsdWRlcyB0aGUgbWFuZGF0ZSdzIG93biByZXNlcnZlIGFuZAp1bnJlc2VydmVkIGZ1bmRzLCBidXQgZXhjbHVkZXMgcmVzZXJ2ZXMgYmVsb25naW5nIHRvIG90aGVyIG1hbmRhdGVzLgpQYXNzIHRoZSBtYW5kYXRlJ3MgZnJvemVuIHBlci1jaGFyZ2UgYW1vdW50LiBUaGlzIHZpZXcgZG9lcyBub3QKY2hhbmdlIHN0YXRlIG9yIGNoZWNrIHRoZSBtYW5kYXRlJ3MgbGlmZWN5Y2xlIG9yIGNoYXJnZSBjYXBzLgoKIyBFcnJvcnMKUmV0dXJucyBbYEVycm9yOjpJbnZhbGlkQW1vdW50YF0gaWYgYGNoYXJnZV9hbW91bnRgIGlzIG5vdCBwb3NpdGl2ZSwKb3IgW2BFcnJvcjo6QW1vdW50T3ZlcmZsb3dgXSBpZiB0aGUgdGFyZ2V0IHRvdGFsIG92ZXJmbG93cyBgaTEyOGAuAAAAAA10b3BfdXBfdGFyZ2V0AAAAAAAABQAAAAAAAAAEdXNlcgAAABMAAAAAAAAABXRva2VuAAAAAAAAEwAAAAAAAAAKbWFuZGF0ZV9pZAAAAAAABgAAAAAAAAANY2hhcmdlX2Ftb3VudAAAAAAAAAsAAAAAAAAAB3BlcmlvZHMAAAAABAAAAAEAAAPpAAAACwAAAAM=", "AAAAAAAAADlUb3RhbCByZXNlcnZlZCBieSBgdXNlcmAgaW4gYHRva2VuYCBhY3Jvc3MgZXZlcnkgbWFuZGF0ZS4AAAAAAAAOcmVzZXJ2ZWRfdG90YWwAAAAAAAIAAAAAAAAABHVzZXIAAAATAAAAAAAAAAV0b2tlbgAAAAAAABMAAAABAAAACw==", "AAAAAAAAA1NDYXAgaG93IG11Y2ggYSBzdWJzY3JpYmVyIG1heSBob2xkIGZvciBvbmUgdG9rZW4uCgpXaGlsZSB0aGUgcHJvdG9jb2wgaXMgdW5hdWRpdGVkLCB0aGUgY29zdCBvZiBhIGJ1ZyBncm93cyB3aXRoIHRoZQphbW91bnQgb2Ygc3Vic2NyaWJlciBmdW5kcyB0aGUgdmF1bHQgY2FuIGhvbGQsIHNvIHRoZSBhZG1pbiBjYW4gY2FwCnRoZSBiYWxhbmNlIGEgc3Vic2NyaWJlciBtYXkga2VlcCBwZXIgdG9rZW4uIFRoZSBjYXAgaXMgcGVyCih1c2VyLCB0b2tlbik6IG9uZSBzdWJzY3JpYmVyJ3MgY2FwIG5ldmVyIGFmZmVjdHMgYW5vdGhlci4KCkEgY2FwIG9mIDAgbWVhbnMgdW5saW1pdGVkLCB3aGljaCBpcyB0aGUgZGVmYXVsdCBhbmQgd2hhdCBhbgpleGlzdGluZyBkZXBsb3ltZW50IHJ1bnMgd2l0aCB1bnRpbCBhbiBhZG1pbiBzZXRzIGEgY2FwLgoKVGhlIGNhcCBvbmx5IGdhdGVzIG5ldyBkZXBvc2l0cy4gQSBiYWxhbmNlIHRoYXQgaXMgYWxyZWFkeSBhYm92ZSBhCm5ld2x5IHNldCBjYXAgY2FuIHN0aWxsIGJlIHdpdGhkcmF3biwgYW5kIGNhbiBzdGlsbCBiZSBkZWJpdGVkIGJ5IHRoZQpzdWJzY3JpcHRpb24gY29udHJhY3Q6IHRoZSBjYXAgbGltaXRzIGhvdyBtdWNoIGNhbiBiZSBhZGRlZCwgYW5kCm5ldmVyIHRyYXBzIGZ1bmRzLgoKIyBBdXRoClJlcXVpcmVzIGF1dGhvcml6YXRpb24gZnJvbSB0aGUgY29udHJhY3QgYWRtaW4uCgojIEV2ZW50cwpFbWl0cyBbYERlcG9zaXRDYXBTZXRgXS4KCiMgRXJyb3JzClJldHVybnMgW2BFcnJvcjo6Tm90SW5pdGlhbGl6ZWRgXSBpZiBubyBhZG1pbiBpcyBjb25maWd1cmVkLCBvcgpbYEVycm9yOjpJbnZhbGlkQW1vdW50YF0gaWYgYGFtb3VudCA8IDBgLgAAAAAPc2V0X2RlcG9zaXRfY2FwAAAAAAMAAAAAAAAABHVzZXIAAAATAAAAAAAAAAV0b2tlbgAAAAAAABMAAAAAAAAABmFtb3VudAAAAAAACwAAAAEAAAPpAAAAAgAAAAM=", "AAAAAAAAAatTZXQgb3IgcmVwbGFjZSBhIHBlci1wZXJpb2QgY2VpbGluZyBvbiB0b3RhbCBkZWJpdHMgZm9yCmAodXNlciwgdG9rZW4pYCwgYWNyb3NzIGFsbCBvZiB0aGUgdXNlcidzIG1hbmRhdGVzIGluIHRoYXQgdG9rZW4uCk9ubHkgdGhlIHVzZXIgdGhlbXNlbGYgY2FuIHNldCBpdCwgYW5kIGl0IGNhbiBvbmx5IG1ha2UgY2hhcmdlcwpmYWlsIOKAlCBpdCBuZXZlciBtb3ZlcyBmdW5kcyBhbmQgbmV2ZXIgYmxvY2tzIGB3aXRoZHJhd2AuCgpgYW1vdW50ID09IDBgIGRpc2FibGVzIHRoZSBsaW1pdCBhbmQgY2xlYXJzIHRoZSB3aW5kb3cgdHJhY2tlci4KV2hlbiBhIG5ldyBsaW1pdCBpcyBzZXQsIHRoZSB3aW5kb3cgdHJhY2tlciBpcyBjbGVhcmVkIHRvbywgc28gdGhlCm5ldyBjZWlsaW5nIGlzIGp1ZGdlZCBvbiB1c2FnZSBmcm9tIHRoaXMgcG9pbnQgb24uAAAAAA9zZXRfc3BlbmRfbGltaXQAAAAABAAAAAAAAAAEdXNlcgAAABMAAAAAAAAABXRva2VuAAAAAAAAEwAAAAAAAAAGYW1vdW50AAAAAAALAAAAAAAAAAt3aW5kb3dfc2VjcwAAAAAGAAAAAQAAA+kAAAACAAAAAw==", "AAAAAAAAAPhHZXQgdGhlIGNvbnRyYWN0J3MgdXBncmFkZSBoaXN0b3J5LgoKUmV0dXJucyBhIHZlY3RvciBvZiB1cGdyYWRlIHJlY29yZHMsIG9sZGVzdCBmaXJzdC4gRWFjaCByZWNvcmQKY29udGFpbnMgdGhlIHZlcnNpb24sIGxlZGdlciBzZXF1ZW5jZSwgYW5kIHdhc20gaGFzaC4gVGhlIGluaXRpYWwKZGVwbG95bWVudCBpcyBub3QgcmVjb3JkZWQ7IG9ubHkgZXhwbGljaXQgdXBncmFkZXMgdmlhIGB1cGdyYWRlX3dhc21gCmFwcGVhciBoZXJlLgAAAA91cGdyYWRlX2hpc3RvcnkAAAAAAAAAAAEAAAPqAAAH0AAAAA1VcGdyYWRlUmVjb3JkAAAA", "AAAAAAAAANtQb2ludCB0aGUgdmF1bHQgYXQgdGhlIHN1YnNjcmlwdGlvbiBjb250cmFjdCBhbGxvd2VkIHRvIGRlYml0IGl0LgoKIyBBdXRoClJlcXVpcmVzIGF1dGhvcml6YXRpb24gZnJvbSB0aGUgY29udHJhY3QgYWRtaW4uCgojIEV2ZW50cwpFbWl0cyBubyBldmVudHMuCgojIEVycm9ycwpSZXR1cm5zIFtgRXJyb3I6Ok5vdEluaXRpYWxpemVkYF0gaWYgbm8gYWRtaW4gaXMgY29uZmlndXJlZC4AAAAAEHNldF9zdWJzY3JpcHRpb24AAAABAAAAAAAAAAxzdWJzY3JpcHRpb24AAAATAAAAAQAAA+kAAAACAAAAAw==", "AAAAAAAAAEtXaGV0aGVyIHRoZSB0b2tlbiBhbGxvd2xpc3QgaXMgZW5mb3JjZWQuIE9mZiB1bmxlc3MgdGhlIGFkbWluIHR1cm5lZCBpdApvbi4AAAAAEWFsbG93bGlzdF9lbmFibGVkAAAAAAAAAAAAAAEAAAAB", "AAAAAAAAAUVBZGQgb3IgcmVtb3ZlIG9uZSB0b2tlbiBmcm9tIHRoZSBhbGxvd2xpc3QuIFJlbW92aW5nIGEgdG9rZW4gYmxvY2tzCm5ldyBkZXBvc2l0cyBvZiBpdDsgYmFsYW5jZXMgYWxyZWFkeSBoZWxkIGNhbiBzdGlsbCBiZSB3aXRoZHJhd24sIHNvCmFsbG93bGlzdGluZyBjYW4gbmV2ZXIgdHJhcCBmdW5kcy4KCiMgQXV0aApSZXF1aXJlcyBhdXRob3JpemF0aW9uIGZyb20gdGhlIGNvbnRyYWN0IGFkbWluLgoKIyBFdmVudHMKRW1pdHMgbm8gZXZlbnRzLgoKIyBFcnJvcnMKUmV0dXJucyBbYEVycm9yOjpOb3RJbml0aWFsaXplZGBdIGlmIG5vIGFkbWluIGlzIGNvbmZpZ3VyZWQuAAAAAAAAEXNldF90b2tlbl9hbGxvd2VkAAAAAAAAAgAAAAAAAAAFdG9rZW4AAAAAAAATAAAAAAAAAAdhbGxvd2VkAAAAAAEAAAABAAAD6QAAAAIAAAAD", "AAAAAAAAAHRUaGUgcGFydCBvZiBgdXNlcmAncyBgdG9rZW5gIGJhbGFuY2Ugbm90IHJlc2VydmVkIGZvciBhbnkgbWFuZGF0ZToKd2hhdCBgd2l0aGRyYXdgIGFuZCBub24tbWFuZGF0ZSBkZWJpdHMgY2FuIHJlYWNoLgAAABJ1bnJlc2VydmVkX2JhbGFuY2UAAAAAAAIAAAAAAAAABHVzZXIAAAATAAAAAAAAAAV0b2tlbgAAAAAAABMAAAABAAAACw==", "AAAAAAAABABEZWJpdCBhIHN1YnNjcmliZXIgb25jZSBhbmQgcGF5IG11bHRpcGxlIHJlY2lwaWVudHMgZnJvbSB0aGF0IGRlYml0LgoKVGhpcyBnZW5lcmFsaXplcyBbYFZhdWx0OjpkZWJpdF9zcGxpdF9tYW5kYXRlYF0gdG8gYW4gYXJiaXRyYXJ5Cm51bWJlciBvZiByZWNpcGllbnRzLCBzbyBhIHNpbmdsZSBjaGFyZ2UgY2FuIHNldHRsZSB0aGUgbWVyY2hhbnQKc2hhcmUsIHRoZSBwcm90b2NvbCBmZWUsIGEga2VlcGVyIHRpcCBhbmQgYSByZWZlcnJhbCBzaGFyZSBpbiBvbmUKY3Jvc3MtY29udHJhY3QgY2FsbC4gVGhlIHN1YnNjcmlwdGlvbiBjb250cmFjdCB1c2VzIHRoaXMgdG8gYXZvaWQKcmVwZWF0aW5nIHRoZSBhdXRob3JpemF0aW9uIGNoZWNrIGFuZCBiYWxhbmNlIHJlYWQvd3JpdGUgZm9yIGVhY2gKbGVnLgoKYGxlZ3NgIGlzIGEgdmVjdG9yIG9mIGAodG8sIGFtb3VudClgIHBhaXJzLiBUaGUgZmlyc3QgYW1vdW50IG11c3QKYmUgcG9zaXRpdmU7IHN1YnNlcXVlbnQgYW1vdW50cyBtYXkgYmUgemVybyAodGhleSBhcmUgc2tpcHBlZCkuIFRoZQpzdW0gb2YgYWxsIGFtb3VudHMgaXMgY2hlY2tlZCBhZ2FpbnN0IHRoZSBzdWJzY3JpYmVyJ3Mgc3BlbmRhYmxlCmJhbGFuY2UgYW5kIHNwZW5kIGxpbWl0IGV4YWN0bHkgb25jZS4KCiMgQXV0aApSZXF1aXJlcyBhdXRob3JpemF0aW9uIGZyb20gdGhlIHJlZ2lzdGVyZWQgYHN1YnNjcmlwdGlvbmAgY29udHJhY3QuCgojIEV2ZW50cwpFbWl0cyBvbmUgW2BEZWJpdGBdIHBlciBub24temVybyBsZWcsIHdpdGggYmFsYW5jZXMgcmVmbGVjdGluZyB0aGUKcnVubmluZyB0b3RhbCBhZnRlciBlYWNoIGxlZyAobWF0Y2hpbmcgd2hhdCBzZXF1ZW50aWFsIGBkZWJpdGAKY2FsbHMgd291bGQgcHJvZHVjZSkuCgojIEVycm9ycwpSZXR1cm5zIFtgRXJyb3I6OlN1YnNjcmlwdGlvbk5vdFNldGBdIGlmIG5vIHN1YnNjcmlwdGlvbiBjb250cmFjdCBpcwpzZXQsIFtgRXJyb3I6OkludmFsaWRBbW91bnRgXSBpZiBgbGVnc2AgaXMgZW1wdHksIHRoZSBmaXJzdCBhbW91bnQKAAAAE2RlYml0X211bHRpX21hbmRhdGUAAAAABAAAAAAAAAAEdXNlcgAAABMAAAAAAAAABXRva2VuAAAAAAAAEwAAAAAAAAAKbWFuZGF0ZV9pZAAAAAAABgAAAAAAAAAEbGVncwAAA+oAAAPtAAAAAgAAABMAAAALAAAAAQAAA+kAAAACAAAAAw==", "AAAAAAAAAY9bYFZhdWx0OjpkZWJpdF9zcGxpdGBdIG9uIGJlaGFsZiBvZiBvbmUgbWFuZGF0ZSAoIzI5NSk6IHRoZQptYW5kYXRlJ3Mgb3duIHJlc2VydmUgaXMgc3BlbmRhYmxlIHRvbyBhbmQgaXMgZHJhd24gZG93biBmaXJzdC4KCiMgQXV0aApSZXF1aXJlcyBhdXRob3JpemF0aW9uIGZyb20gdGhlIHJlZ2lzdGVyZWQgYHN1YnNjcmlwdGlvbmAgY29udHJhY3QuCgojIEV2ZW50cwpFbWl0cyBvbmUgW2BEZWJpdGBdIHBlciBub24tZW1wdHkgbGVnLgoKIyBFcnJvcnMKQXMgW2BWYXVsdDo6ZGViaXRfc3BsaXRgXSwgd2l0aCBbYEVycm9yOjpJbnN1ZmZpY2llbnRCYWxhbmNlYF0KbWVhbmluZyB0aGUgdG90YWwgZXhjZWVkcyB0aGUgdW5yZXNlcnZlZCBiYWxhbmNlIHBsdXMgdGhpcwptYW5kYXRlJ3MgcmVzZXJ2ZS4AAAAAE2RlYml0X3NwbGl0X21hbmRhdGUAAAAABwAAAAAAAAAEdXNlcgAAABMAAAAAAAAABXRva2VuAAAAAAAAEwAAAAAAAAAKbWFuZGF0ZV9pZAAAAAAABgAAAAAAAAAEdG9fYQAAABMAAAAAAAAACGFtb3VudF9hAAAACwAAAAAAAAAEdG9fYgAAABMAAAAAAAAACGFtb3VudF9iAAAACwAAAAEAAAPpAAAAAgAAAAM=", "AAAAAAAAAWdUdXJuIHRoZSB0b2tlbiBhbGxvd2xpc3Qgb24gb3Igb2ZmLiBPZmYgYnkgZGVmYXVsdCwgc28gYSBkZXBsb3ltZW50Cih0ZXN0bmV0IGluY2x1ZGVkKSBrZWVwcyBhY2NlcHRpbmcgZXZlcnkgU0VQLTQxIHRva2VuIHVudGlsIHRoZSBhZG1pbgpvcHRzIGluLiBUb2dnbGluZyBpdCBtb3ZlcyBubyBmdW5kczogaXQgb25seSBnYXRlcyBmdXR1cmUgZGVwb3NpdHMuCgojIEF1dGgKUmVxdWlyZXMgYXV0aG9yaXphdGlvbiBmcm9tIHRoZSBjb250cmFjdCBhZG1pbi4KCiMgRXZlbnRzCkVtaXRzIG5vIGV2ZW50cy4KCiMgRXJyb3JzClJldHVybnMgW2BFcnJvcjo6Tm90SW5pdGlhbGl6ZWRgXSBpZiBubyBhZG1pbiBpcyBjb25maWd1cmVkLgAAAAAVc2V0X2FsbG93bGlzdF9lbmFibGVkAAAAAAAAAQAAAAAAAAAHZW5hYmxlZAAAAAABAAAAAQAAA+kAAAACAAAAAw=="]),
      options
    );
  }

   static deploy<T = Client>(args: { admin: string | Address }, options: MethodOptions & Omit<ContractClientOptions, 'contractId'> & { salt?: Uint8Array; address?: string; } & ({ wasmHash: Uint8Array | string; format?: "hex" | "base64"; externalRef?: never; } | { externalRef: ExternalExecutableRef; wasmHash?: never; format?: never; })): Promise<AssembledTransaction<T>> {
    return ContractClient.deploy(args, options);
  }
  public readonly fromJson = {
    admin : this.txFromJson<Result<string, Error>>,  debit : this.txFromJson<Result<null, Error>>,  balance : this.txFromJson<bigint>,  deposit : this.txFromJson<Result<null, Error>>,  version : this.txFromJson<string>,  reserved : this.txFromJson<bigint>,  withdraw : this.txFromJson<Result<null, Error>>,  can_debit : this.txFromJson<boolean>,  can_deposit : this.txFromJson<boolean>,  debit_split : this.txFromJson<Result<null, Error>>,  deposit_cap : this.txFromJson<bigint>,  deposit_for : this.txFromJson<Result<null, Error>>,  set_reserve : this.txFromJson<Result<null, Error>>,  spend_limit : this.txFromJson<SpendLimit>,  withdraw_to : this.txFromJson<Result<null, Error>>,  subscription : this.txFromJson<Result<string, Error>>,  update_admin : this.txFromJson<Result<null, Error>>,  upgrade_wasm : this.txFromJson<Result<null, Error>>,  withdraw_all : this.txFromJson<Result<bigint, Error>>,  debit_mandate : this.txFromJson<Result<null, Error>>,  token_allowed : this.txFromJson<boolean>,  top_up_target : this.txFromJson<Result<bigint, Error>>,  reserved_total : this.txFromJson<bigint>,  set_deposit_cap : this.txFromJson<Result<null, Error>>,  set_spend_limit : this.txFromJson<Result<null, Error>>,  upgrade_history : this.txFromJson<Array<UpgradeRecord>>,  set_subscription : this.txFromJson<Result<null, Error>>,  allowlist_enabled : this.txFromJson<boolean>,  set_token_allowed : this.txFromJson<Result<null, Error>>,  unreserved_balance : this.txFromJson<bigint>,  debit_multi_mandate : this.txFromJson<Result<null, Error>>,  debit_split_mandate : this.txFromJson<Result<null, Error>>,  set_allowlist_enabled : this.txFromJson<Result<null, Error>>
  };

  /** @deprecated Use fromJson instead. */
  public readonly fromJSON = this.fromJson;

  /**
   * Parse a raw contract event (topics + data) into a typed {@link ContractEvent}.
   */
  parseEvent(topics: xdr.ScVal[] | string[], data: xdr.ScVal | string): ContractEvent | undefined {
    return this.spec.parseEvent(topics, data) as ContractEvent | undefined;
  }
  /**
   * Build a topics filter row for the "Debit" event, for use in `Api.EventFilter.topics` when calling `server.getEvents`. Omitted fields match any value.
   */
  debitEventFilter(topicValues?: { user?: string | Address; token?: string | Address; to?: string | Address }): string[] {
    return this.spec.eventTopicFilter("Debit", topicValues);
  }
  /**
   * Build a topics filter row for the "Deposit" event, for use in `Api.EventFilter.topics` when calling `server.getEvents`. Omitted fields match any value.
   */
  depositEventFilter(topicValues?: { user?: string | Address; token?: string | Address }): string[] {
    return this.spec.eventTopicFilter("Deposit", topicValues);
  }
  /**
   * Build a topics filter row for the "Withdraw" event, for use in `Api.EventFilter.topics` when calling `server.getEvents`. Omitted fields match any value.
   */
  withdrawEventFilter(topicValues?: { user?: string | Address; token?: string | Address }): string[] {
    return this.spec.eventTopicFilter("Withdraw", topicValues);
  }
  /**
   * Build a topics filter row for the "DepositFor" event, for use in `Api.EventFilter.topics` when calling `server.getEvents`. Omitted fields match any value.
   */
  depositForEventFilter(topicValues?: { from?: string | Address; user?: string | Address; token?: string | Address }): string[] {
    return this.spec.eventTopicFilter("DepositFor", topicValues);
  }
  /**
   * Build a topics filter row for the "ReserveSet" event, for use in `Api.EventFilter.topics` when calling `server.getEvents`. Omitted fields match any value.
   */
  reserveSetEventFilter(topicValues?: { user?: string | Address; token?: string | Address; mandate_id?: bigint }): string[] {
    return this.spec.eventTopicFilter("ReserveSet", topicValues);
  }
  /**
   * Build a topics filter row for the "WithdrawTo" event, for use in `Api.EventFilter.topics` when calling `server.getEvents`. Omitted fields match any value.
   */
  withdrawToEventFilter(topicValues?: { user?: string | Address; to?: string | Address; token?: string | Address }): string[] {
    return this.spec.eventTopicFilter("WithdrawTo", topicValues);
  }
  /**
   * Build a topics filter row for the "DepositCapSet" event, for use in `Api.EventFilter.topics` when calling `server.getEvents`. Omitted fields match any value.
   */
  depositCapSetEventFilter(topicValues?: { user?: string | Address; token?: string | Address }): string[] {
    return this.spec.eventTopicFilter("DepositCapSet", topicValues);
  }
  /**
   * Build a topics filter row for the "ContractUpgraded" event, for use in `Api.EventFilter.topics` when calling `server.getEvents`. Omitted fields match any value.
   */
  contractUpgradedEventFilter(topicValues?: { admin?: string | Address }): string[] {
    return this.spec.eventTopicFilter("ContractUpgraded", topicValues);
  }
}