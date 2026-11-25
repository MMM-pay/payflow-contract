import {ChargeResult, Mandate, ChargeQuote, ChargeSimulation, UpgradeRecord, EstimatedEnd, MandateSummary, ContractEvent} from './types.js';
import {Result, Spec, AssembledTransaction, Client as ContractClient, ClientOptions as ContractClientOptions, MethodOptions, ExternalExecutableRef} from '@stellar/stellar-sdk/contract';
import {Address, xdr} from '@stellar/stellar-sdk';

export interface Client {
  /**
   * Address of the contract administrator.
   */
  admin(options?: MethodOptions): Promise<AssembledTransaction<Result<string, Error>>>;
  /**
   * Address of the configured vault contract.
   */
  vault(options?: MethodOptions): Promise<AssembledTransaction<Result<string, Error>>>;
  /**
   * Terminate a mandate permanently. Only the subscriber may do this, and it
   * can be done at any time without the merchant's involvement.
   *
   * This is [`Subscription::cancel_with_reason`] with
   * [`CANCEL_REASON_NONE`]: the mandate is cancelled exactly as before and
   * the [`Cancelled`] event reports reason `0`.
   *
   * # Auth
   * Requires authorization from `subscriber`.
   *
   * # Events
   * Emits [`MandateUpdated`] and [`Cancelled`].
   *
   * # Errors
   * Returns [`Error::MandateNotFound`] if the mandate does not exist,
   * [`Error::NotSubscriber`] if the caller is not the subscriber, or
   * [`Error::MandateNotActive`] if the mandate is already cancelled or completed.
   */
  cancel(args: { subscriber: string | Address; mandate_id: bigint }, options?: MethodOptions): Promise<AssembledTransaction<Result<null, Error>>>;
  /**
   * Execute a due charge.
   *
   * Open to any caller: the mandate itself is the authorization, so the
   * merchant, a keeper bot, or the subscriber may all settle it. `keeper`
   * must authorize this call, because the plan's tip is paid to it; that is
   * the only signature required, and it grants no power over the mandate.
   * Competing keepers are what keeps billing alive without trusting one
   * operator to stay online.
   *
   * The tip is paid out of the merchant's share (see the plan's
   * `keeper_bps`), so the subscriber is debited exactly `mandate.amount`
   * whether or not a tip is paid, and the protocol fee is unaffected.
   *
   * `next_charge` advances to `now + period` rather than
   * `next_charge + period`. A keeper outage therefore delays billing instead
   * of accumulating a backlog that could drain a vault in one burst. The
   * tradeoff is slow forward drift, which favours the subscriber.
   *
   * See [ADR 0005](../../docs/adr/0005-permissionless-charge.md) for a
   * detailed explanation of why permissionless charging is safe and
   * keeper operational guidance.
   *
   * # Auth
   * Requi
   */
  charge(args: { keeper: string | Address; mandate_id: bigint }, options?: MethodOptions): Promise<AssembledTransaction<Result<ChargeResult, Error>>>;
  /**
   * True when `charge` would succeed on timing and status grounds. Does not
   * check vault balance; a funded-ness check is the caller's job.
   */
  is_due(args: { mandate_id: bigint }, options?: MethodOptions): Promise<AssembledTransaction<Result<boolean, Error>>>;
  /**
   * Refund part of what this mandate paid the merchant.
   *
   * The merchant returns money from its own wallet, straight to the
   * subscriber's wallet, and the contract records the refund on the
   * mandate and emits `Refunded`, so the dispute trail lives on chain
   * instead of in ad-hoc transfers nobody can attribute. It deliberately
   * does not touch the vault: crediting internal balances without moving
   * tokens would break the vault's invariant that internal balances sum to
   * its holdings.
   *
   * Total refunds are capped at what the merchant actually collected for
   * this mandate (price minus the frozen fee and tip, per settled charge),
   * so a merchant can never "refund" more than it ever received. The cap
   * holds regardless of mandate status, which is why a cancelled mandate
   * remains refundable.
   *
   * # Auth
   * Requires authorization from `merchant`.
   *
   * # Events
   * Emits [`MandateUpdated`] and [`Refunded`].
   *
   * # Errors
   * Returns [`Error::MandateNotFound`] if the mandate does not exist,
   * [`Error::NotMerchant`] if the caller is not the merchant,
   * [`Error::InvalidAmou
   */
  refund(args: { merchant: string | Address; mandate_id: bigint; amount: bigint }, options?: MethodOptions): Promise<AssembledTransaction<Result<null, Error>>>;
  /**
   * Current protocol fee rate in basis points.
   */
  fee_bps(options?: MethodOptions): Promise<AssembledTransaction<Result<number, Error>>>;
  /**
   * Move a mandate onto a different plan.
   *
   * Moving to a more expensive plan collects the price difference for the
   * part of the current period that is still ahead, prorated and rounded
   * in the subscriber's favour. Moving to a cheaper plan takes effect at
   * the next charge and never refunds anything: the subscriber has
   * already paid for the period they are in.
   *
   * The rules that keep this safe:
   *
   * - Only the subscriber may call it, and only for their own mandate.
   * - The new plan must be active, denominated in the same token (a
   * mandate's token is frozen when it is opened) and belong to the same
   * merchant (the mandate's merchant decides who is paid and who may
   * refund).
   * - The amount charged now is the price difference multiplied by the
   * fraction of the current period that is left, truncated towards zero.
   * The subscriber therefore never pays more than the exact difference,
   * and never less than zero.
   * - The billing date does not move: the next charge still falls due at
   * `next_charge`, and carries the new plan's price from then on.
   *
   * See [
   */
  upgrade(args: { subscriber: string | Address; mandate_id: bigint; new_plan_id: bigint }, options?: MethodOptions): Promise<AssembledTransaction<Result<null, Error>>>;
  /**
   * The version of this contract's crate.
   */
  version(options?: MethodOptions): Promise<AssembledTransaction<string>>;
  /**
   * Change the seat count on an active mandate.
   *
   * Seat-based plans price by the number of users on a team. This function
   * lets the subscriber grow or shrink the seat count; the new count takes
   * effect from the next charge so no proration is needed. Only the
   * subscriber may call it, the mandate must be active (not cancelled or
   * completed), and `seats` must be at least 1.
   *
   * # Auth
   * Requires authorization from `subscriber`.
   *
   * # Events
   * Emits [`MandateUpdated`] and [`SeatsChanged`].
   *
   * # Errors
   * Returns [`Error::MandateNotFound`] if the mandate does not exist,
   * [`Error::NotSubscriber`] if the caller is not the subscriber,
   * [`Error::MandateNotActive`] if the mandate is cancelled or completed, or
   * [`Error::InvalidSeats`] if `seats` is zero.
   */
  set_seats(args: { subscriber: string | Address; mandate_id: bigint; seats: number }, options?: MethodOptions): Promise<AssembledTransaction<Result<null, Error>>>;
  /**
   * Open a mandate against a published plan.
   *
   * The plan's terms are copied into the mandate here and never re-read, so
   * a merchant cannot change the price of a subscription after the fact.
   * The first charge is due immediately: this is `subscribe_at(..., now)`.
   *
   * # Auth
   * Requires authorization from `subscriber`.
   *
   * # Events
   * Emits [`MandateUpdated`] and [`Subscribed`].
   *
   * # Errors
   * Returns [`Error::InvalidMaxTotal`] if `max_total < 0`,
   * [`Error::PlanInactive`] if the target plan is inactive, or
   * [`Error::NotInitialized`] if configuration keys are missing.
   */
  subscribe(args: { subscriber: string | Address; plan_id: bigint; max_charges: number; max_total: bigint; referrer: string | Address | null }, options?: MethodOptions): Promise<AssembledTransaction<Result<bigint, Error>>>;
  /**
   * Pause or resume billing without losing the mandate's history.
   *
   * An open-ended pause: a mandate paused this way stays paused until it is
   * resumed here. `pause_until` is the alternative for a pause that should
   * lift by itself.
   *
   * # Auth
   * Requires authorization from `subscriber`.
   *
   * # Events
   * Emits [`MandateUpdated`] and [`PauseChanged`].
   *
   * # Errors
   * Returns [`Error::MandateNotFound`] if the mandate does not exist,
   * [`Error::NotSubscriber`] if the caller is not the subscriber, or
   * [`Error::MandateNotActive`] if the mandate cannot be transitioned (e.g. cancelled/completed).
   */
  set_paused(args: { subscriber: string | Address; mandate_id: bigint; paused: boolean }, options?: MethodOptions): Promise<AssembledTransaction<Result<null, Error>>>;
  /**
   * Permanently cancel every cancellable mandate in one page of the
   * subscriber's index. The scan is capped at `MAX_PAGE`; reading index
   * slots directly avoids touching the separate count entry, keeping a
   * full page within the 100-entry transaction limit.
   *
   * Mandates already cancelled or completed are skipped, so calling it
   * twice is harmless. Returns how many this call cancelled. Each
   * [`Cancelled`] event reports reason [`CANCEL_REASON_NONE`]: a
   * subscriber who clears out everything is not explaining one by one.
   *
   * # Auth
   * Requires authorization from `subscriber`.
   *
   * # Events
   * Emits one [`Cancelled`] per mandate it cancels.
   */
  cancel_page(args: { subscriber: string | Address; start: number; limit: number }, options?: MethodOptions): Promise<AssembledTransaction<Result<number, Error>>>;
  /**
   * End a mandate from the merchant's side, for example when the service is
   * discontinued. Deactivating a plan only stops new subscriptions; this is
   * how a merchant stops billing the ones already open. Like `cancel`, it
   * is permanent, and it can only ever reduce what the subscriber pays.
   *
   * # Auth
   * Requires authorization from `merchant`.
   *
   * # Events
   * Emits [`MandateUpdated`] and [`MandateEnded`].
   *
   * # Errors
   * Returns [`Error::MandateNotFound`] if the mandate does not exist,
   * [`Error::NotMerchant`] if the caller is not the merchant, or
   * [`Error::MandateNotActive`] if the mandate is already cancelled or completed.
   */
  end_mandate(args: { merchant: string | Address; mandate_id: bigint }, options?: MethodOptions): Promise<AssembledTransaction<Result<null, Error>>>;
  /**
   * Look up a mandate by its unique ID.
   */
  get_mandate(args: { mandate_id: bigint }, options?: MethodOptions): Promise<AssembledTransaction<Result<Mandate, Error>>>;
  /**
   * Pause billing until a given date, without needing a transaction from
   * the subscriber afterwards.
   *
   * The mandate is paused like `set_paused` does, but it carries a
   * schedule: once the ledger clock reaches `until`, `charge` and `is_due`
   * treat it as Active again on their own. The resume is therefore free —
   * no keeper window is missed because the subscriber forgot to resume,
   * which is what loses merchants their returning customers.
   *
   * `next_charge` is pushed to at least `until`, so billing continues from
   * the end of the pause rather than from a date that has already passed.
   *
   * # Auth
   * Requires authorization from `subscriber`.
   *
   * # Events
   * Emits [`MandateUpdated`] and [`PauseChanged`].
   *
   * # Errors
   * Returns [`Error::MandateNotFound`] if the mandate does not exist,
   * [`Error::NotSubscriber`] if the caller is not the subscriber,
   * [`Error::MandateNotActive`] if the mandate is cancelled or completed, or
   * [`Error::InvalidUntil`] if `until` is earlier than or equal to current timestamp.
   */
  pause_until(args: { subscriber: string | Address; mandate_id: bigint; until: bigint }, options?: MethodOptions): Promise<AssembledTransaction<Result<null, Error>>>;
  /**
   * Change the protocol fee for mandates opened from now on.
   *
   * Safe to call at any time: every open mandate carries the fee it was
   * created with, so this cannot reprice an existing subscriber. The
   * `MAX_FEE_BPS` ceiling still applies.
   *
   * # Auth
   * Requires authorization from the contract admin.
   *
   * # Events
   * Emits [`FeeChanged`].
   *
   * # Errors
   * Returns [`Error::NotInitialized`] if no admin is set, or
   * [`Error::FeeTooHigh`] if `new_fee_bps > MAX_FEE_BPS`.
   */
  set_fee_bps(args: { new_fee_bps: number }, options?: MethodOptions): Promise<AssembledTransaction<Result<null, Error>>>;
  /**
   * Accept a pending admin handover (#238). Step two of the handshake:
   * only the address the current admin proposed may accept, and the
   * role moves only when it does. The pending proposal is consumed, so
   * a second accept by the same address fails.
   *
   * # Auth
   * Requires authorization from `new_admin`.
   *
   * # Errors
   * Returns [`Error::PendingAdminNotFound`] if no handover is pending,
   * or [`Error::NotPendingAdmin`] if `new_admin` is not the proposed
   * successor.
   */
  accept_admin(args: { new_admin: string | Address }, options?: MethodOptions): Promise<AssembledTransaction<Result<null, Error>>>;
  /**
   * Ids of due mandates opened against `merchant`'s plans, in index
   * order. A keeper settling one merchant otherwise has to read every
   * mandate the merchant has ever had and test each one itself.
   *
   * Reads at most one page of index slots and their mandate entries.
   * It cannot afford a plan-registry read per candidate within the
   * ledger's 100-entry budget, so it reports the mandates whose own
   * schedule is due; `is_due` and `charge` add the plan-pause check and
   * are the final authority before settlement (#315).
   */
  due_mandates(args: { merchant: string | Address; start: number; limit: number }, options?: MethodOptions): Promise<AssembledTransaction<Array<bigint>>>;
  /**
   * Read-only preview of what `charge` would pay right now.
   *
   * `amount` is the subscriber's total debit, `fee` the protocol share
   * and `merchant_amount` the merchant's net share after both the fee
   * and the keeper tip. `due_at` is the mandate's next charge
   * timestamp, and `chargeable` mirrors `is_due`: active status, cap
   * not reached, and the ledger time has reached `due_at`. Like
   * `is_due`, it does not check the vault balance. Quoting reads
   * storage but never writes it.
   *
   * The split comes from the same private helper `charge` uses, so a
   * client can display the exact integers settlement will move rather
   * than reimplementing the rounding.
   */
  quote_charge(args: { mandate_id: bigint }, options?: MethodOptions): Promise<AssembledTransaction<Result<ChargeQuote, Error>>>;
  /**
   * Open a mandate whose first charge falls due at `start` rather than
   * now, so a subscriber can set up billing ahead of time.
   *
   * `start` must be the current ledger timestamp or later, and no more
   * than `MAX_START_OFFSET` (one year) after it: a mandate may be
   * scheduled in advance, but neither backdated nor locked away in the
   * distant future. Everything else is identical to `subscribe` — the
   * plan's terms are copied into the mandate and never re-read — and
   * `Subscribed` carries `start` as `next_charge`, so indexers learn when
   * billing begins from the event alone.
   *
   * # Auth
   * Requires authorization from `subscriber`.
   *
   * # Events
   * Emits [`MandateUpdated`] and [`Subscribed`].
   *
   * # Errors
   * Returns [`Error::InvalidStart`] if `start` is before the current
   * ledger timestamp or more than `MAX_START_OFFSET` after it,
   * [`Error::InvalidMaxTotal`] if `max_total < 0`, [`Error::PlanInactive`]
   * if the target plan is inactive, or [`Error::NotInitialized`] if
   * configuration keys are missing.
   */
  subscribe_at(args: { subscriber: string | Address; plan_id: bigint; max_charges: number; max_total: bigint; start: bigint; referrer: string | Address | null }, options?: MethodOptions): Promise<AssembledTransaction<Result<bigint, Error>>>;
  /**
   * Replace this contract's wasm with code at `new_wasm_hash`. Only the
   * admin may upgrade; persistent state is untouched. (From #158.)
   *
   * Records the upgrade in the contract's upgrade history, which can be
   * queried via `upgrade_history` for transparency.
   */
  upgrade_wasm(args: { new_wasm_hash: Uint8Array }, options?: MethodOptions): Promise<AssembledTransaction<Result<null, Error>>>;
  /**
   * Estimate when the mandate's final capped charge is scheduled.
   *
   * The estimate follows the current schedule, including any drift from
   * late charges. Returns `None` for an open-ended mandate and its actual
   * final charge time once the cap has been reached.
   *
   * # Errors
   * Returns [`Error::MandateNotFound`] if the mandate does not exist, or
   * [`Error::ScheduleOverflow`] if the remaining schedule overflows `u64`.
   */
  estimated_end(args: { mandate_id: bigint }, options?: MethodOptions): Promise<AssembledTransaction<Result<bigint | null, Error>>>;
  /**
   * Ids of the mandates opened against plan `plan_id`, oldest first,
   * starting at position `start`. Returns at most `min(limit, MAX_PAGE)`.
   */
  plan_mandates(args: { plan_id: bigint; start: number; limit: number }, options?: MethodOptions): Promise<AssembledTransaction<Array<bigint>>>;
  /**
   * Address of the configured plan registry contract.
   */
  plan_registry(options?: MethodOptions): Promise<AssembledTransaction<Result<string, Error>>>;
  /**
   * Propose handing the admin role to `new_admin` (#238).
   *
   * This is step one of a two-step handover. The current admin keeps
   * the role until `new_admin` accepts with `accept_admin`; proposing
   * alone changes no authority, so a mistyped or hostile proposal can
   * be replaced before anyone accepts it. A later proposal to another
   * address overwrites the pending one.
   *
   * # Auth
   * Requires authorization from the current admin.
   *
   * # Errors
   * Returns [`Error::NotInitialized`] if no admin is configured.
   */
  propose_admin(args: { new_admin: string | Address }, options?: MethodOptions): Promise<AssembledTransaction<Result<null, Error>>>;
  /**
   * The id the next opened mandate will get. Mandates are numbered from 1,
   * so `next_mandate_id() - 1` is the number of mandates opened so far.
   */
  next_mandate_id(options?: MethodOptions): Promise<AssembledTransaction<Result<bigint, Error>>>;
  /**
   * Whether the admin has paused the protocol (#285).
   */
  protocol_paused(options?: MethodOptions): Promise<AssembledTransaction<boolean>>;
  /**
   * Raise or lower a mandate's `max_charges` (0 = unlimited). Only the
   * subscriber may change it, and not below the charges already made.
   * Reaching the new cap completes the mandate and releases its seat.
   * (From #158.)
   */
  set_max_charges(args: { subscriber: string | Address; mandate_id: bigint; max_charges: number }, options?: MethodOptions): Promise<AssembledTransaction<Result<null, Error>>>;
  /**
   * Preview the exact recipients and amounts the next `charge` would
   * transfer, without requiring authorization or changing state.
   *
   * `chargeable` covers protocol and plan pauses, timing, mandate status
   * and spend caps. Like [`quote_charge`](Self::quote_charge), it does
   * not check authorization or vault funds. `keeper` is the proposed
   * charge caller and receives the tip if one is due.
   *
   * # Errors
   * Returns [`Error::MandateNotFound`] if the mandate does not exist,
   * [`Error::PlanNotFound`] if the registry cannot provide its pause
   * state,
   * [`Error::NotInitialized`] if configuration is missing, or
   * [`Error::AmountOverflow`] if the split calculation overflows.
   */
  simulate_charge(args: { keeper: string | Address; mandate_id: bigint }, options?: MethodOptions): Promise<AssembledTransaction<Result<ChargeSimulation, Error>>>;
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
   * Ids of the mandates opened against `merchant`'s plans, oldest first,
   * starting at position `start`. Returns at most `min(limit, MAX_PAGE)`.
   */
  merchant_mandates(args: { merchant: string | Address; start: number; limit: number }, options?: MethodOptions): Promise<AssembledTransaction<Array<bigint>>>;
  /**
   * Terminate a mandate permanently and record why.
   *
   * Behave exactly like [`Subscription::cancel`] — same authorization,
   * same state change, same events — and additionally report the
   * subscriber's reason in the [`Cancelled`] event, so a merchant can
   * measure churn from chain data alone instead of asking its users.
   *
   * The reason cannot change what the call does: the mandate is
   * cancelled either way, so a code can never keep a mandate alive, move
   * money, or make a charge cost more. The set is closed:
   *
   * | Code | Constant | Meaning |
   * |---|---|---|
   * | 0 | [`CANCEL_REASON_NONE`] | No reason given; what `cancel` emits |
   * | 1 | [`CANCEL_REASON_TOO_EXPENSIVE`] | Too expensive |
   * | 2 | [`CANCEL_REASON_NOT_USING`] | No longer using the service |
   * | 3 | [`CANCEL_REASON_MISSING_FEATURE`] | Missing a needed feature |
   * | 4 | [`CANCEL_REASON_SERVICE_QUALITY`] | Unhappy with the service |
   * | 5 | [`CANCEL_REASON_OTHER`] | Something else |
   *
   * # Auth
   * Requires authorization from `subscriber`.
   *
   * # Events
   * Emits [`MandateUpdated`] and [`Cancelled`],
   */
  cancel_with_reason(args: { subscriber: string | Address; mandate_id: bigint; reason: number }, options?: MethodOptions): Promise<AssembledTransaction<Result<null, Error>>>;
  /**
   * Record that a due charge fails, and notify the merchant the first
   * time it does (#296).
   *
   * A failed `charge` is rolled back together with any event it would
   * emit, so a keeper whose charge fails calls this instead. It proves
   * the failure on chain before recording anything: the mandate must be
   * due exactly as `charge` requires, and the vault must be unable to
   * cover the charge right now (balance, this mandate's reserve and the
   * subscriber's spend limit all considered). A flag can therefore
   * never be raised against a mandate that would have paid.
   *
   * The first flag in a run of failures stores the timestamp and emits
   * [`ChargeFailed`] with the merchant as a topic, so off-chain dunning
   * can react promptly. Further flags return `false` and emit nothing,
   * so retries cannot spam the merchant. The next successful `charge`
   * ends the run; the failure after that notifies again.
   *
   * Permissionless, like `charge`: it moves no money and only records
   * a fact anyone can check.
   *
   * # Auth
   * None.
   *
   * # Events
   * Emits [`ChargeFailed`] on the first flag of
   */
  flag_failed_charge(args: { mandate_id: bigint }, options?: MethodOptions): Promise<AssembledTransaction<Result<boolean, Error>>>;
  /**
   * Whether `mandate_id` is inside the grace window it was opened with
   * (#312), so a wallet can warn the subscriber before the mandate
   * lapses.
   *
   * The window starts when the charge falls due (`next_charge`), not
   * when a keeper gets around to flagging the failure, and lasts for the
   * mandate's frozen grace window. A mandate with a zero window is never
   * in grace. Indefinitely paused, cancelled and completed mandates are
   * not in grace either; a scheduled pause that has run its course is
   * treated as active, matching `charge`.
   *
   * The window is inclusive of its start and exclusive of its end: at
   * exactly `next_charge + grace_period_secs` the grace period has
   * elapsed. This is a pure read: no storage writes and no events.
   *
   * # Errors
   * Returns [`Error::MandateNotFound`] if the mandate does not exist.
   */
  is_in_grace_period(args: { mandate_id: bigint }, options?: MethodOptions): Promise<AssembledTransaction<Result<boolean, Error>>>;
  /**
   * Number of mandates ever opened against plan `plan_id` (from #134).
   */
  plan_mandate_count(args: { plan_id: bigint }, options?: MethodOptions): Promise<AssembledTransaction<number>>;
  /**
   * When the mandate's current run of failed charges was first flagged
   * by `flag_failed_charge`, or `None` if its last charge attempt has
   * not been flagged as failed (#296).
   */
  first_failed_charge(args: { mandate_id: bigint }, options?: MethodOptions): Promise<AssembledTransaction<bigint | null>>;
  /**
   * Pause or resume the whole protocol during an incident (#285).
   * Admin only.
   *
   * While paused, `subscribe`, `subscribe_at`, `subscribe_with_deposit`,
   * `charge` and `upgrade` fail with [`Error::ProtocolPaused`] before
   * touching any state. Everything that lets a subscriber or merchant
   * get out keeps working: every view, `cancel`, `cancel_page`,
   * `end_mandate`, per-mandate pausing, `refund`, and the vault's
   * withdrawals (the vault is a separate contract and never reads this
   * switch). A mandate that falls due during the pause is not lost: its
   * `next_charge` is unchanged, so it is chargeable as soon as the
   * protocol resumes.
   *
   * Setting the switch to its current value is a no-op and emits no
   * event; otherwise [`ProtocolPauseChanged`] is published.
   */
  set_protocol_paused(args: { paused: boolean }, options?: MethodOptions): Promise<AssembledTransaction<Result<null, Error>>>;
  /**
   * Ids of the mandates `subscriber` opened, oldest first, starting at
   * position `start`. Returns at most `min(limit, MAX_PAGE)` ids.
   */
  subscriber_mandates(args: { subscriber: string | Address; start: number; limit: number }, options?: MethodOptions): Promise<AssembledTransaction<Array<bigint>>>;
  /**
   * Accept a pending mandate ownership transfer. Future charges are debited
   * from the new subscriber's vault balance; the merchant index and all
   * frozen mandate terms remain unchanged.
   */
  accept_mandate_owner(args: { new_subscriber: string | Address; mandate_id: bigint }, options?: MethodOptions): Promise<AssembledTransaction<Result<null, Error>>>;
  /**
   * Total committed spend of `subscriber` in `token` per `period_secs`,
   * normalising mandates with different billing periods to the period the
   * caller asks about. Monthly and weekly mandates therefore add up to one
   * figure the subscriber can fund their vault against.
   *
   * Only mandates that `charge` would treat as active are counted:
   * cancelled, completed, indefinitely paused and not-yet-due-scheduled
   * mandates contribute nothing. At most `MAX_PAGE` mandates (the first
   * page of the subscriber's index) are read, so the call stays well
   * inside the ledger's 100-entry footprint limit.
   */
  committed_per_period(args: { subscriber: string | Address; token: string | Address; period_secs: bigint }, options?: MethodOptions): Promise<AssembledTransaction<Result<bigint, Error>>>;
  /**
   * [`estimated_end`](Self::estimated_end) expressed as a timestamp,
   * an expected ledger sequence and a period count (#287), so UIs need
   * no extra math. Returns `None` for an open-ended mandate.
   *
   * The ledger figure assumes [`SECONDS_PER_LEDGER`]-second ledgers and
   * saturates at `u32::MAX`; it is a hint, not a guarantee.
   *
   * # Errors
   * Same as [`estimated_end`](Self::estimated_end).
   */
  estimated_end_detail(args: { mandate_id: bigint }, options?: MethodOptions): Promise<AssembledTransaction<Result<EstimatedEnd | null, Error>>>;
  /**
   * Grace window in seconds frozen into `mandate_id` when it was opened
   * (#312). `0` means the plan had no grace window at sign-up.
   *
   * # Errors
   * Returns [`Error::MandateNotFound`] if the mandate does not exist.
   */
  mandate_grace_period(args: { mandate_id: bigint }, options?: MethodOptions): Promise<AssembledTransaction<Result<bigint, Error>>>;
  /**
   * Number of currently-open mandates against `plan_id`; the count the
   * plan's `max_subscribers` cap is checked against (from #158).
   */
  plan_subscriber_count(args: { plan_id: bigint }, options?: MethodOptions): Promise<AssembledTransaction<number>>;
  /**
   * Propose moving a mandate to `new_subscriber`. The current subscriber
   * supplies the position from `subscriber_mandates`; this lets transfers
   * update old indexes in constant time, including mandates created before
   * reverse-position metadata existed.
   */
  propose_mandate_owner(args: { subscriber: string | Address; mandate_id: bigint; new_subscriber: string | Address; subscriber_position: number }, options?: MethodOptions): Promise<AssembledTransaction<Result<null, Error>>>;
  /**
   * How many mandates have been opened against `merchant`'s plans.
   */
  merchant_mandate_count(args: { merchant: string | Address }, options?: MethodOptions): Promise<AssembledTransaction<number>>;
  /**
   * Committed revenue per period for `plan_id`. This is the sum of all
   * active mandates' per-period amounts, normalized to the plan's billing
   * period. Returns 0 if the plan has no active mandates or does not exist.
   */
  plan_committed_revenue(args: { plan_id: bigint }, options?: MethodOptions): Promise<AssembledTransaction<bigint>>;
  /**
   * Fund the vault and open a mandate in one transaction, so a new
   * subscriber signs once instead of signing a deposit and then a
   * subscribe — the gap between the two is where sign-ups were lost.
   *
   * The subscriber's one `require_auth` at the top is the only signature
   * needed: the nested `vault.deposit` (and the token transfer it makes)
   * run as sub-invocations of this same authorization, and the mandate
   * is then created through the normal `subscribe_at` path (start =
   * now), so it carries the full, current mandate terms.
   *
   * Atomic by construction: if the deposit fails the whole invocation
   * aborts and no mandate is created; if mandate creation then fails,
   * the deposit is rolled back with it.
   *
   * (Merged from #134, extended with `max_total` for parity with the
   * current `subscribe`, which #134 predated.)
   *
   * # Auth
   * Requires authorization from `subscriber`.
   *
   * # Errors
   * As `subscribe`, plus any error the vault's `deposit` can return.
   */
  subscribe_with_deposit(args: { subscriber: string | Address; plan_id: bigint; max_charges: number; max_total: bigint; deposit: bigint; referrer: string | Address | null }, options?: MethodOptions): Promise<AssembledTransaction<Result<bigint, Error>>>;
  /**
   * How many mandates `subscriber` has opened, in any state.
   */
  subscriber_mandate_count(args: { subscriber: string | Address }, options?: MethodOptions): Promise<AssembledTransaction<number>>;
  /**
   * Extend the storage TTL of a page of `merchant`'s open mandates in
   * one call (#284), instead of one transaction per mandate.
   *
   * Walks positions `start .. start + min(limit, MAX_PAGE)` of the
   * merchant index and, for every mandate that is still `Active` or
   * `Paused`, bumps the mandate entry and its index slot to the
   * standard persistent TTL. The index count is bumped too, so the
   * index stays readable. Cancelled and completed mandates are skipped:
   * they can no longer be charged, so keeping them alive only costs rent.
   *
   * Permissionless: extending a TTL changes no contract state that any
   * read can observe, and Soroban lets anyone extend any entry anyway.
   * Works while the protocol is paused, so a long incident cannot let
   * mandates expire. Returns the number of mandates extended.
   *
   * Cost: one call reads at most 1 count + 50 slots + 50 mandates
   * (101 entries, the transaction footprint limit) and writes none, so a
   * keeper covering N open mandates needs `ceil(N / 50)` transactions
   * instead of N.
   */
  extend_merchant_mandates_ttl(args: { merchant: string | Address; start: number; limit: number }, options?: MethodOptions): Promise<AssembledTransaction<number>>;
  /**
   * Summaries of the mandates `subscriber` opened, oldest first, starting at
   * position `start`. Returns at most `min(limit, MAX_PAGE)` summaries.
   * Each summary contains the key fields a wallet needs to display a
   * subscription without fetching the full mandate.
   */
  subscriber_mandate_summaries(args: { subscriber: string | Address; start: number; limit: number }, options?: MethodOptions): Promise<AssembledTransaction<Array<MandateSummary>>>;
}

export class Client extends ContractClient {
  constructor(public readonly options: ContractClientOptions) {
    super(
      new Spec(["AAAABAAAAC1FcnJvcnMgcmV0dXJuZWQgYnkgdGhlIHN1YnNjcmlwdGlvbiBjb250cmFjdC4AAAAAAAAAAAAABUVycm9yAAAAAAAAKgAAACpUaGUgY29udHJhY3QgaGFzIGFscmVhZHkgYmVlbiBpbml0aWFsaXplZC4AAAAAABJBbHJlYWR5SW5pdGlhbGl6ZWQAAAAAB9EAAABCVGhlIGNvbnRyYWN0IGhhcyBub3QgYmVlbiBpbml0aWFsaXplZCB3aXRoIHJlcXVpcmVkIGNvbmZpZ3VyYXRpb24uAAAAAAAOTm90SW5pdGlhbGl6ZWQAAAAAB9IAAAAzVGhlIHJlcXVlc3RlZCBtYW5kYXRlIElEIGRvZXMgbm90IGV4aXN0IGluIHN0b3JhZ2UuAAAAAA9NYW5kYXRlTm90Rm91bmQAAAAH0wAAADhUaGUgY2FsbGVyIGlzIG5vdCB0aGUgc3Vic2NyaWJlciB3aG8gb3BlbmVkIHRoZSBtYW5kYXRlLgAAAA1Ob3RTdWJzY3JpYmVyAAAAAAAH1AAAADxUaGUgbWFuZGF0ZSBpcyBub3QgYWN0aXZlIChwYXVzZWQsIGNhbmNlbGxlZCwgb3IgY29tcGxldGVkKS4AAAAQTWFuZGF0ZU5vdEFjdGl2ZQAAB9UAAABMVGhlIGN1cnJlbnQgbGVkZ2VyIHRpbWVzdGFtcCBpcyBlYXJsaWVyIHRoYW4gdGhlIG1hbmRhdGUncyBuZXh0IGNoYXJnZSB0aW1lLgAAAAZOb3REdWUAAAAAB9YAAABDVGhlIHBsYW4gaXMgaW5hY3RpdmUgYW5kIGRvZXMgbm90IGFjY2VwdCBzdWJzY3JpcHRpb25zIG9yIHVwZ3JhZGVzLgAAAAAMUGxhbkluYWN0aXZlAAAH1wAAAEFUaGUgcmVxdWVzdGVkIHByb3RvY29sIGZlZSBpbiBiYXNpcyBwb2ludHMgZXhjZWVkcyBgTUFYX0ZFRV9CUFNgLgAAAAAAAApGZWVUb29IaWdoAAAAAAfYAAAAMVRoZSBtYW5kYXRlIGhhcyByZWFjaGVkIGl0cyBtYXhpbXVtIGNoYXJnZSBjb3VudC4AAAAAAAARTWF4Q2hhcmdlc1JlYWNoZWQAAAAAAAfZAAAALlRoZSBtYXhpbXVtIGNoYXJnZSBjb3VudCBwYXJhbWV0ZXIgaXMgaW52YWxpZC4AAAAAABFJbnZhbGlkTWF4Q2hhcmdlcwAAAAAAB9oAAAAuVGhlIGNhbGxlciBpcyBub3QgdGhlIG1lcmNoYW50IG9mIHRoZSBtYW5kYXRlLgAAAAAAC05vdE1lcmNoYW50AAAAB9sAAAA1RmVlIG9yIHRpcCBtYXRoIHdvdWxkIG92ZXJmbG93IGBpMTI4YCBhdCB0aGlzIGFtb3VudC4AAAAAAAAOQW1vdW50T3ZlcmZsb3cAAAAAB9wAAABsYHBhdXNlX3VudGlsYCB3YXMgZ2l2ZW4gYSBkYXRlIHRoYXQgaGFzIGFscmVhZHkgcGFzc2VkLiBQYXVzaW5nIG11c3QKbW92ZSBiaWxsaW5nIGZvcndhcmRzLCBuZXZlciBiYWNrd2FyZHMuAAAADEludmFsaWRVbnRpbAAAB90AAABGQSByZWZ1bmQgbGFyZ2VyIHRoYW4gd2hhdCB0aGUgbWVyY2hhbnQgaGFzIGNvbGxlY3RlZCBmb3IgdGhpcyBtYW5kYXRlLgAAAAAAFVJlZnVuZEV4Y2VlZHNSZWNlaXZlZAAAAAAAB94AAAApQSByZWZ1bmQgYW1vdW50IHRoYXQgaXMgemVybyBvciBuZWdhdGl2ZS4AAAAAAAANSW52YWxpZEFtb3VudAAAAAAAB98AAAC3VGhlIHBsYW4gYSBtYW5kYXRlIGlzIGJlaW5nIG1vdmVkIHRvIGlzIGRlbm9taW5hdGVkIGluIGFub3RoZXIgdG9rZW4uCkEgbWFuZGF0ZSdzIHRva2VuIGlzIGZyb3plbiB3aGVuIGl0IGlzIG9wZW5lZCwgc28gYW4gdXBncmFkZSBjYW4gbmV2ZXIKY2hhbmdlIHdoaWNoIGFzc2V0IHRoZSBzdWJzY3JpYmVyIHBheXMgaW4uAAAAAA5EaWZmZXJlbnRUb2tlbgAAAAAH4AAAAM1UaGUgcGxhbiBhIG1hbmRhdGUgaXMgYmVpbmcgbW92ZWQgdG8gYmVsb25ncyB0byBhbm90aGVyIG1lcmNoYW50LgpNb3ZpbmcgYmV0d2VlbiBtZXJjaGFudHMgaXMgYSBjYW5jZWwgYW5kIGEgbmV3IHN1YnNjcmlwdGlvbiwgbm90IGFuCnVwZ3JhZGU6IHRoZSBtYW5kYXRlJ3MgbWVyY2hhbnQgZGVjaWRlcyB3aG8gaXMgcGFpZCBhbmQgd2hvIG1heQpyZWZ1bmQuAAAAAAAAEURpZmZlcmVudE1lcmNoYW50AAAAAAAH4QAAAE9UaGUgZXN0aW1hdGVkIGVuZCB0aW1lc3RhbXAgZm9yIGEgbWFuZGF0ZSdzIHJlbWFpbmluZyBzY2hlZHVsZSBvdmVyZmxvd3MgYHU2NGAuAAAAABBTY2hlZHVsZU92ZXJmbG93AAAH4gAAADpUaGUgc2VhdCBjb3VudCBpcyB6ZXJvLCBvciBleGNlZWRzIHRoZSBwbGFuJ3MgYG1heF9zZWF0c2AuAAAAAAAMSW52YWxpZFNlYXRzAAAH4wAAAF5UaGUgc2VhdCBjb3VudCB3YXMgc3VwcGxpZWQgZm9yIGEgcGxhbiB0aGF0IGlzIG5vdCBzZWF0LWJhc2VkLCBvciB3YXMKb21pdHRlZCBmb3Igb25lIHRoYXQgaXMuAAAAAAASU2VhdHNOb3RBcHBsaWNhYmxlAAAAAAfkAAAAIWBwcmljZSAqIHNlYXRzYCBvdmVyZmxvd3MgYGkxMjhgLgAAAAAAAA1TZWF0c092ZXJmbG93AAAAAAAH5QAAACNBIHRvdGFsLXNwZW5kIGNhcCB0aGF0IGlzIG5lZ2F0aXZlLgAAAAAPSW52YWxpZE1heFRvdGFsAAAAB+YAAABoVGhlIHBhdXNlIGhhcyBsYXN0ZWQgbG9uZ2VyIHRoYW4gdGhlIG1heGltdW0gcGF1c2UgbGVuZ3RoIGZyb3plbiBpbnRvCnRoZSBtYW5kYXRlLCBzbyBpdCBjYW4gYmUgZXhwaXJlZC4AAAAPUGF1c2VOb3RFeHBpcmVkAAAAB+cAAABrVGhlIHBhdXNlIGhhcyBleGNlZWRlZCBpdHMgbWF4aW11bSBsZW5ndGggYW5kIG11c3QgYmUgZXhwaXJlZCB3aXRoCmBleHBpcmVfcGF1c2VgLCBub3QgcmVzdW1lZCBvciBleHRlbmRlZC4AAAAADFBhdXNlRXhwaXJlZAAAB+gAAABBQSBiYXRjaCBzdWJzY3JpcHRpb24gd2FzIGdpdmVuIG1vcmUgcGxhbnMgdGhhbiBgTUFYX0JBVENIX1BMQU5TYC4AAAAAAAANQmF0Y2hUb29MYXJnZQAAAAAAB+kAAAAoQSBiYXRjaCBzdWJzY3JpcHRpb24gd2FzIGdpdmVuIG5vIHBsYW5zLgAAAApFbXB0eUJhdGNoAAAAAAfqAAAAe2BzdWJzY3JpYmVfYXRgIHdhcyBnaXZlbiBhIGBzdGFydGAgYmVmb3JlIHRoZSBjdXJyZW50IGxlZGdlciB0aW1lc3RhbXAKb3IgbW9yZSB0aGFuIGBNQVhfU1RBUlRfT0ZGU0VUYCAob25lIHllYXIpIGFmdGVyIGl0LgAAAAAMSW52YWxpZFN0YXJ0AAAH6wAAAMJBIGNhbmNlbGxhdGlvbiByZWFzb24gb3V0c2lkZSB0aGUgZG9jdW1lbnRlZCBgQ0FOQ0VMX1JFQVNPTl8qYCBjb2RlCnNldCB3YXMgc3VwcGxpZWQuIFRoZSBzZXQgaXMgY2xvc2VkIHNvIGFuIGluZGV4ZXIgY2FuIG1hcCBldmVyeQpgQ2FuY2VsbGVkYCBldmVudCBpdCB3aWxsIGV2ZXIgc2VlIHdpdGhvdXQgYSBjYXRjaC1hbGwgYnJhbmNoLgAAAAAAE0ludmFsaWRDYW5jZWxSZWFzb24AAAAH7AAAAIpUaGUgY2FsbGVyIGdhdmUgYSBwb3NpdGlvbiBpbiBgc3Vic2NyaWJlcl9tYW5kYXRlc2AgdGhhdCBkb2VzIG5vdCBob2xkCnRoaXMgbWFuZGF0ZSwgc28gdGhlIHRyYW5zZmVyIGNhbm5vdCB1cGRhdGUgdGhlIG9sZCBpbmRleCBpbiBwbGFjZS4AAAAAABZJbnZhbGlkU3Vic2NyaWJlckluZGV4AAAAAAftAAAAdWBhY2NlcHRfbWFuZGF0ZV9vd25lcmAgd2FzIGNhbGxlZCBmb3IgYSBtYW5kYXRlIHdpdGggbm8gcGVuZGluZwp0cmFuc2Zlciwgb3IgYWZ0ZXIgdGhlIHRyYW5zZmVyIHdhcyBhbHJlYWR5IGFjY2VwdGVkLgAAAAAAABFOb1BlbmRpbmdUcmFuc2ZlcgAAAAAAB+4AAABtVGhlIGFkZHJlc3MgYWNjZXB0aW5nIHRoZSB0cmFuc2ZlciBpcyBub3QgdGhlIG9uZSB0aGUgY3VycmVudApzdWJzY3JpYmVyIHByb3Bvc2VkIGluIGBwcm9wb3NlX21hbmRhdGVfb3duZXJgLgAAAAAAABBOb3RQcm9wb3NlZE93bmVyAAAH7wAAAHNBIHRyYW5zZmVyIHdhcyBwcm9wb3NlZCB0byB0aGUgbWFuZGF0ZSdzIGN1cnJlbnQgc3Vic2NyaWJlcjsgYSBtb3ZlIHRvCnRoZSBzYW1lIGFkZHJlc3MgaXMgYSBuby1vcCBhbmQgaXMgcmVqZWN0ZWQuAAAAAA5TYW1lU3Vic2NyaWJlcgAAAAAH8AAAAFRUaGUgbmV3IHN1YnNjcmliZXIgYWxyZWFkeSBob2xkcyB0aGUgbWF4aW11bSBudW1iZXIgb2YgbWFuZGF0ZXMgaXRzCmluZGV4IGNhbiB0cmFjay4AAAAQTWFuZGF0ZUluZGV4RnVsbAAAB/EAAABWQSBuZXcgc3Vic2NyaXB0aW9uIHdhcyByZWplY3RlZCBiZWNhdXNlIHRoZSBwbGFuIGlzIGF0IGl0cwpgbWF4X3N1YnNjcmliZXJzYCBzZWF0IGNhcC4AAAAAAAhQbGFuRnVsbAAAB/IAAAC1YGZsYWdfZmFpbGVkX2NoYXJnZWAgd2FzIGNhbGxlZCBmb3IgYSBkdWUgbWFuZGF0ZSB3aG9zZSBjaGFyZ2Ugd291bGQKc3VjY2VlZCByaWdodCBub3c6IHRoZSBzdWJzY3JpYmVyJ3MgdmF1bHQgY292ZXJzIGl0LiBPbmx5IGEgY2hhcmdlCnRoYXQgd291bGQgcmVhbGx5IGZhaWwgY2FuIGJlIGZsYWdnZWQgKCMyOTYpLgAAAAAAABJDaGFyZ2VXb3VsZFN1Y2NlZWQAAAAAB/MAAAA9VGhlIHBsYW4gcmVnaXN0cnkgZGlkIG5vdCByZXR1cm4gYSBwbGFuIGZvciB0aGUgcmVxdWVzdGVkIElELgAAAAAAAAxQbGFuTm90Rm91bmQAAAf0AAAAqVRoZSB2YXVsdCByZWplY3RlZCBhIGRlcG9zaXQgb3IgZGViaXQuIFRoZSBzdWJzY3JpcHRpb24gY2FsbCBmYWlscwp3aXRob3V0IGNoYW5naW5nIG1hbmRhdGUgc3RhdGU7IHRoZSB2YXVsdCdzIG93biBlcnJvciBjb2RlIGlzIHByZXNlcnZlZAppbiB0aGUgaW52b2NhdGlvbiBkaWFnbm9zdGljcy4AAAAAAAAPVmF1bHRDYWxsRmFpbGVkAAAAB/UAAACCVGhlIG1hbmRhdGUncyBwbGFuIGhhcyBjaGFyZ2luZyBwYXVzZWQgYnkgaXRzIG1lcmNoYW50ICgjMzE1KS4gVGhlCm1hbmRhdGUgaXRzZWxmIGlzIHN0aWxsIG9wZW47IHJldHJ5IGFmdGVyIHRoZSBtZXJjaGFudCByZXN1bWVzLgAAAAAAEVBsYW5DaGFyZ2VzUGF1c2VkAAAAAAAH9gAAALtUaGUgYWRtaW4gaGFzIHBhdXNlZCB0aGUgcHJvdG9jb2wgKCMyODUpLiBOZXcgc3Vic2NyaXB0aW9ucywgY2hhcmdlcwphbmQgcGxhbiB1cGdyYWRlcyBhcmUgcmVmdXNlZCB1bnRpbCBpdCBpcyByZXN1bWVkOyByZWFkcywKY2FuY2VsbGF0aW9ucywgcmVmdW5kcyBhbmQgdmF1bHQgd2l0aGRyYXdhbHMgYXJlIHVuYWZmZWN0ZWQuAAAAAA5Qcm90b2NvbFBhdXNlZAAAAAAH9wAAALpBIHJlZnVuZCB0cmFuc2ZlciBkaWQgbm90IGRlbGl2ZXIgZXhhY3RseSB0aGUgcmVjb3JkZWQgcmVmdW5kIGFtb3VudCwKc28gdGhlIHRva2VuIGlzIHVuc2FmZSBmb3IgZXhhY3QtYW1vdW50IGFjY291bnRpbmcgKGZvciBleGFtcGxlIGEKZmVlLW9uLXRyYW5zZmVyIHRva2VuKS4gVGhlIHJlZnVuZCBpcyByb2xsZWQgYmFjay4AAAAAABhVbmV4cGVjdGVkVHJhbnNmZXJBbW91bnQAAAf4AAAAbGBhY2NlcHRfYWRtaW5gIHdhcyBjYWxsZWQgd2l0aCBubyBwZW5kaW5nIGFkbWluIGhhbmRvdmVyLCBvciBhZnRlciB0aGUKcHJvcG9zYWwgd2FzIGFscmVhZHkgYWNjZXB0ZWQgKCMyMzgpLgAAABRQZW5kaW5nQWRtaW5Ob3RGb3VuZAAAB/kAAABpVGhlIGFkZHJlc3MgYWNjZXB0aW5nIHRoZSBhZG1pbiByb2xlIGlzIG5vdCB0aGUgb25lIHRoZSBjdXJyZW50IGFkbWluCnByb3Bvc2VkIGluIGBwcm9wb3NlX2FkbWluYCAoIzIzOCkuAAAAAAAAD05vdFBlbmRpbmdBZG1pbgAAAAf6", "AAAAAQAAAIBBIHN0YW5kaW5nIGF1dGhvcml6YXRpb24gZnJvbSBhIHN1YnNjcmliZXIgdG8gYSBtZXJjaGFudCwgYm91bmRlZCBpbgphbW91bnQsIGZyZXF1ZW5jeSwgdG90YWwgbnVtYmVyIG9mIGNoYXJnZXMgYW5kIHRvdGFsIHNwZW5kLgAAAAAAAAAHTWFuZGF0ZQAAAAAYAAAASEZyb3plbiBhdCBzdWJzY3JpYmUgdGltZS4gQSBsYXRlciBwbGFuIGVkaXQgY2Fubm90IHJlcHJpY2UgdGhpcyBtYW5kYXRlLgAAAAZhbW91bnQAAAAAAAsAAAAvTnVtYmVyIG9mIGNoYXJnZXMgc3VjY2Vzc2Z1bGx5IGV4ZWN1dGVkIHNvIGZhci4AAAAADGNoYXJnZXNfbWFkZQAAAAQAAAApVW5peCBzZWNvbmRzIHdoZW4gdGhlIG1hbmRhdGUgd2FzIG9wZW5lZC4AAAAAAAAKY3JlYXRlZF9hdAAAAAAABgAAAMdQcm90b2NvbCBmZWUgaW4gYmFzaXMgcG9pbnRzLCBmcm96ZW4gYXQgc3Vic2NyaWJlIHRpbWUuIEFuIGFkbWluIGZlZQpjaGFuZ2UgdGhlcmVmb3JlIGNhbm5vdCBhbHRlciB0aGUgZWNvbm9taWNzIG9mIGEgbWFuZGF0ZSB0aGF0IGlzCmFscmVhZHkgb3BlbiDigJQgdGhlIHNhbWUgZ3VhcmFudGVlIHRoZSBwbGFuJ3MgcHJpY2UgYWxyZWFkeSBoYXMuAAAAAAdmZWVfYnBzAAAAAAQAAAAhVW5pcXVlIGlkZW50aWZpZXIgb2YgdGhlIG1hbmRhdGUuAAAAAAAAAmlkAAAAAAAGAAAAtEtlZXBlciB0aXAgaW4gYmFzaXMgcG9pbnRzIG9mIHRoZSBtZXJjaGFudCBzaGFyZSwgZnJvemVuIGF0IHN1YnNjcmliZQp0aW1lIGZvciB0aGUgc2FtZSByZWFzb24gYXMgYGZlZV9icHNgOiBhIGxhdGVyIHBsYW4gY2Fubm90IHJhaXNlIHdoYXQgYW4Kb3BlbiBtYW5kYXRlIHBheXMgd2hvZXZlciBzZXR0bGVzIGl0LgAAAAprZWVwZXJfYnBzAAAAAAAEAAAARFVuaXggc2Vjb25kcyB3aGVuIHRoZSBtb3N0IHJlY2VudCBjaGFyZ2Ugd2FzIGV4ZWN1dGVkLCBvciAwIGlmIG5vbmUuAAAAC2xhc3RfY2hhcmdlAAAAAAYAAAATMCBtZWFucyBvcGVuLWVuZGVkLgAAAAALbWF4X2NoYXJnZXMAAAAABAAAAORNYXhpbXVtIGNvbnRpbnVvdXMgcGF1c2UgbGVuZ3RoIGluIHNlY29uZHMsIGZyb3plbiBhdCBzdWJzY3JpYmUgdGltZQpmcm9tIHRoZSBwbGFuJ3MgYG1heF9wYXVzZV9zZWNzYC4gMCBtZWFucyB1bmxpbWl0ZWQuIE1hbmRhdGVzIG9wZW5lZApiZWZvcmUgdGhpcyBmaWVsZCBleGlzdGVkIGRlY29kZSB0byAwIGFuZCBhcmUgdGhlcmVmb3JlIHVubGltaXRlZCwgc28KdGhleSBhcmUgdW5hZmZlY3RlZC4AAAAObWF4X3BhdXNlX3NlY3MAAAAAAAYAAAC+Q2VpbGluZyBvbiBldmVyeXRoaW5nIHRoaXMgbWFuZGF0ZSBtYXkgZXZlciBkZWJpdCwgZnJvemVuIHdoZW4gdGhlCnN1YnNjcmliZXIgb3BlbmVkIGl0LiAwIG1lYW5zIG9wZW4tZW5kZWQuIE1hbmRhdGVzIG9wZW5lZCBiZWZvcmUgdGhpcwpmaWVsZCBleGlzdGVkIGRlY29kZSB0byAwIGFuZCBhcmUgdGhlcmVmb3JlIHVuY2FwcGVkLgAAAAAACW1heF90b3RhbAAAAAAAAAsAAAAuTWVyY2hhbnQgcmVjZWl2aW5nIHBheW1lbnQgdW5kZXIgdGhpcyBtYW5kYXRlLgAAAAAACG1lcmNoYW50AAAAEwAAAEBVbml4IHNlY29uZHMuIENoYXJnZWFibGUgb25jZSB0aGUgbGVkZ2VyIHRpbWVzdGFtcCByZWFjaGVzIHRoaXMuAAAAC25leHRfY2hhcmdlAAAAAAYAAADCVW5peCBzZWNvbmRzIHdoZW4gYSBzY2hlZHVsZWQgcGF1c2UgKGBwYXVzZV91bnRpbGApIGxpZnRzIGJ5IGl0c2VsZi4KMCBtZWFucyBubyBzY2hlZHVsZTogYSBwYXVzZWQgbWFuZGF0ZSBzdGF5cyBwYXVzZWQgdW50aWwgYHNldF9wYXVzZWRgCnJlc3VtZXMgaXQuIE9ubHkgbWVhbmluZ2Z1bCB3aGlsZSBgc3RhdHVzYCBpcyBgUGF1c2VkYC4AAAAAAAxwYXVzZWRfdW50aWwAAAAGAAAAwEFjY291bnQgdGhhdCByZWNlaXZlcyB0aGUgbWVyY2hhbnQncyBzaGFyZSBvZiBlYWNoIGNoYXJnZSwgZnJvemVuIGZyb20KdGhlIHBsYW4ncyBgcGF5b3V0YCBhdCBzaWduLXVwLiBBIGxhdGVyIHBheW91dCBjaGFuZ2Ugb3IgYSBwbGFuLW93bmVyCnRyYW5zZmVyIG5ldmVyIHJlZGlyZWN0cyB0aGlzIG9wZW4gbWFuZGF0ZSdzIGZ1bmRzLgAAAAZwYXlvdXQAAAAAABMAAAAqQmlsbGluZyBwZXJpb2QgaW4gc2Vjb25kcyBiZXR3ZWVuIGNoYXJnZXMuAAAAAAAGcGVyaW9kAAAAAAAGAAAAQUlEIG9mIHRoZSBwbGFuIGFnYWluc3Qgd2hpY2ggdGhpcyBtYW5kYXRlIHdhcyBvcGVuZWQgb3IgdXBncmFkZWQuAAAAAAAAB3BsYW5faWQAAAAABgAAAHZSZWZlcnJhbCBzaGFyZSBpbiBiYXNpcyBwb2ludHMgb2YgdGhlIG1lcmNoYW50J3Mgc2hhcmUsIGZyb3plbiBmcm9tIHRoZQpwbGFuIGF0IHNpZ24tdXAuIENhcHBlZCBhdCBgTUFYX1JFRkVSUkFMX0JQU2AuAAAAAAAMcmVmZXJyYWxfYnBzAAAABAAAAHZPcHRpb25hbCByZWZlcnJlciBwYWlkIGEgc2hhcmUgb2YgdGhlIG1lcmNoYW50J3MgbW9uZXkgb24gZXZlcnkgY2hhcmdlLgpgTm9uZWAgZGlzYWJsZXMgdGhlIHJlZmVycmFsIGZvciB0aGlzIG1hbmRhdGUuAAAAAAAIcmVmZXJyZXIAAAPoAAAAEwAAAKtUb3RhbCBhbW91bnQgdGhlIG1lcmNoYW50IGhhcyByZWZ1bmRlZCBvbiB0aGlzIG1hbmRhdGUgc28gZmFyLiBUaGUKbWVyY2hhbnQgY2FuIG5ldmVyIHJlZnVuZCBtb3JlIHRoYW4gaXQgaGFzIGNvbGxlY3RlZCwgc28gdGhpcyBjb3VudGVyCmlzIHRoZSBoYXJkIGNhcCBgcmVmdW5kYCBlbmZvcmNlcy4AAAAACHJlZnVuZGVkAAAACwAAAIpOdW1iZXIgb2Ygc2VhdHMgdGhlIHN1YnNjcmliZXIgaXMgcGF5aW5nIGZvci4gYDFgIGZvciBub24tc2VhdCBwbGFucy4KYGFtb3VudGAgYWxyZWFkeSBlcXVhbHMgdGhlIHBlci1zZWF0IHByaWNlIG11bHRpcGxpZWQgYnkgdGhpcyB2YWx1ZS4AAAAAAAVzZWF0cwAAAAAAAAQAAAAnQ3VycmVudCBsaWZlY3ljbGUgc3RhdGUgb2YgdGhlIG1hbmRhdGUuAAAAAAZzdGF0dXMAAAAAB9AAAAANTWFuZGF0ZVN0YXR1cwAAAAAAADFBY2NvdW50IG9mIHRoZSBzdWJzY3JpYmVyIHBheWluZyBmb3IgdGhlIG1hbmRhdGUuAAAAAAAACnN1YnNjcmliZXIAAAAAABMAAAA5VG9rZW4gY29udHJhY3QgYWRkcmVzcyBpbiB3aGljaCBwYXltZW50cyBhcmUgZGVub21pbmF0ZWQuAAAAAAAABXRva2VuAAAAAAAAEwAAAEVUb3RhbCBkZWJpdGVkIGJ5IHNldHRsZWQgY2hhcmdlcyBzbyBmYXIsIHRoZSBmaWd1cmUgYG1heF90b3RhbGAgY2Fwcy4AAAAAAAALdG90YWxfc3BlbnQAAAAACw==", "AAAAAQAAAMxSZWFkLW9ubHkgcHJldmlldyBvZiB3aGF0IGBjaGFyZ2VgIHdvdWxkIHBheSByaWdodCBub3cuCgpDbGllbnRzIHVzZSB0aGlzIGluc3RlYWQgb2YgcmVjb21wdXRpbmcgdGhlIG1lcmNoYW50L2ZlZSBzcGxpdCwgc28gYSBxdW90ZQpjYW4gbmV2ZXIgZGlzYWdyZWUgd2l0aCBzZXR0bGVtZW50IGJlY2F1c2Ugb2YgYSBkaWZmZXJlbnQgcm91bmRpbmcgcnVsZS4AAAAAAAAAC0NoYXJnZVF1b3RlAAAAAAUAAABCVG90YWwgdGhlIHN1YnNjcmliZXIgd291bGQgYmUgZGViaXRlZDogdGhlIG1hbmRhdGUncyBmcm96ZW4gcHJpY2UuAAAAAAAGYW1vdW50AAAAAAALAAAAflRydWUgd2hlbiBgY2hhcmdlYCB3b3VsZCBwYXNzIGl0cyBzdGF0dXMsIGNhcCBhbmQgdGltaW5nIGNoZWNrcyByaWdodApub3cuIFZhdWx0IGJhbGFuY2UgaXMgbm90IGNvbnNpZGVyZWQsIG1hdGNoaW5nIGBpc19kdWVgLgAAAAAACmNoYXJnZWFibGUAAAAAAAEAAAA6VW5peCBzZWNvbmRzIGF0IHdoaWNoIHRoZSBtYW5kYXRlIG5leHQgYmVjb21lcyBjaGFyZ2VhYmxlLgAAAAAABmR1ZV9hdAAAAAAABgAAAEJQcm90b2NvbCBmZWUsIHRydW5jYXRlZCB0byB3aG9sZSBzdHJvb3BzIGV4YWN0bHkgYXMgYGNoYXJnZWAgZG9lcy4AAAAAAANmZWUAAAAACwAAAERUaGUgbWVyY2hhbnQncyBzaGFyZSBhZnRlciBib3RoIHRoZSBwcm90b2NvbCBmZWUgYW5kIHRoZSBrZWVwZXIgdGlwLgAAAA9tZXJjaGFudF9hbW91bnQAAAAACw==", "AAAAAQAAAIVWYWx1ZXMgZnJvbSBhIHNldHRsZWQgY2hhcmdlLiBgbWVyY2hhbnRfYW1vdW50YCBtYXRjaGVzIGBDaGFyZ2VkLmFtb3VudGA6CnRoZSBtZXJjaGFudCdzIHNoYXJlIGFmdGVyIHRoZSBwcm90b2NvbCBmZWUgYW5kIGtlZXBlciB0aXAuAAAAAAAAAAAAAAxDaGFyZ2VSZXN1bHQAAAAFAAAASFRvdGFsIG51bWJlciBvZiBjaGFyZ2VzIGNvbXBsZXRlZCB1bmRlciB0aGlzIG1hbmRhdGUgaW5jbHVkaW5nIHRoaXMgb25lLgAAAAxjaGFyZ2VzX21hZGUAAAAEAAAARFdoZXRoZXIgdGhlIG1hbmRhdGUgaGFzIHJlYWNoZWQgaXRzIGNoYXJnZSBjYXAgYW5kIGlzIG5vdyBjb21wbGV0ZWQuAAAACWNvbXBsZXRlZAAAAAAAAAEAAAAlUHJvdG9jb2wgZmVlIGNvbGxlY3RlZCBvbiB0aGUgY2hhcmdlLgAAAAAAAANmZWUAAAAACwAAAEdBbW91bnQgdHJhbnNmZXJyZWQgdG8gdGhlIG1lcmNoYW50IGFmdGVyIGZlZSBhbmQga2VlcGVyIHRpcCBkZWR1Y3Rpb25zLgAAAAAPbWVyY2hhbnRfYW1vdW50AAAAAAsAAAA5VGltZXN0YW1wIGluIFVuaXggc2Vjb25kcyB3aGVuIHRoZSBuZXh0IGNoYXJnZSBmYWxscyBkdWUuAAAAAAAAC25leHRfY2hhcmdlAAAAAAY=", "AAAAAQAAASJFc3RpbWF0ZWQgZW5kIG9mIGEgY2FwcGVkIG1hbmRhdGUsIGluIHRoZSB1bml0cyBhIFVJIG5lZWRzIHdpdGhvdXQgZXh0cmEKbWF0aCAoIzI4NykuIGB0aW1lc3RhbXBgIGlzIFVuaXggc2Vjb25kcyAoZm9ybWF0IGl0IGFzIElTTy04NjAxIGNsaWVudApzaWRlKSwgYGxlZGdlcmAgdGhlIGxlZGdlciBzZXF1ZW5jZSB0aGF0IHRpbWUgaXMgZXhwZWN0ZWQgdG8gbGFuZCBpbiwgYW5kCmBwZXJpb2RzX3JlbWFpbmluZ2AgaG93IG1hbnkgYmlsbGluZyBwZXJpb2RzIHNlcGFyYXRlIG5vdyBmcm9tIHRoYXQgZW5kLgAAAAAAAAAAAAxFc3RpbWF0ZWRFbmQAAAAEAAAAekxlZGdlciBzZXF1ZW5jZSBleHBlY3RlZCBhdCBgdGltZXN0YW1wYCwgYXNzdW1pbmcKYFNFQ09ORFNfUEVSX0xFREdFUmAtc2Vjb25kIGxlZGdlcnMuIE5ldmVyIGVhcmxpZXIgdGhhbiB0aGUgY3VycmVudCBvbmUuAAAAAAAGbGVkZ2VyAAAAAAAEAAAAX1dob2xlIGJpbGxpbmcgcGVyaW9kcyBmcm9tIG5vdyB1bnRpbCBgdGltZXN0YW1wYCwgcm91bmRlZCB1cC4gWmVybyBvbmNlCnRoZSBlbmQgaXMgaW4gdGhlIHBhc3QuAAAAABFwZXJpb2RzX3JlbWFpbmluZwAAAAAAAAYAAAAzQ2hhcmdlcyBzdGlsbCB0byBiZSBtYWRlIGJlZm9yZSB0aGUgY2FwIGlzIHJlYWNoZWQuAAAAABFyZW1haW5pbmdfY2hhcmdlcwAAAAAAAAQAAABIVW5peCBzZWNvbmRzIG9mIHRoZSBmaW5hbCBzY2hlZHVsZWQgKG9yLCBvbmNlIGNvbXBsZXRlZCwgYWN0dWFsKSBjaGFyZ2UuAAAACXRpbWVzdGFtcAAAAAAAAAY=", "AAAAAgAAACtMaWZlY3ljbGUgc3RhdHVzIG9mIGEgc3Vic2NyaXB0aW9uIG1hbmRhdGUuAAAAAAAAAAANTWFuZGF0ZVN0YXR1cwAAAAAAAAQAAAAAAAAAFENoYXJnZWFibGUgd2hlbiBkdWUuAAAABkFjdGl2ZQAAAAAAAAAAADJUZW1wb3JhcmlseSBub3QgY2hhcmdlYWJsZS4gU3Vic2NyaWJlciBjYW4gcmVzdW1lLgAAAAAABlBhdXNlZAAAAAAAAAAAACtUZXJtaW5hdGVkIGJ5IHRoZSBzdWJzY3JpYmVyLiBJcnJldmVyc2libGUuAAAAAAlDYW5jZWxsZWQAAAAAAAAAAAAAM1JlYWNoZWQgYG1heF9jaGFyZ2VzYCBvciBgbWF4X3RvdGFsYC4gSXJyZXZlcnNpYmxlLgAAAAAJQ29tcGxldGVkAAAA", "AAAAAQAAACFBIHNpbmdsZSBjb250cmFjdCB1cGdyYWRlIHJlY29yZC4AAAAAAAAAAAAADVVwZ3JhZGVSZWNvcmQAAAAAAAADAAAANExlZGdlciBzZXF1ZW5jZSBudW1iZXIgd2hlbiB0aGUgdXBncmFkZSB3YXMgYXBwbGllZC4AAAAGbGVkZ2VyAAAAAAAEAAAAPFNlbWFudGljIHZlcnNpb24gb2YgdGhlIG5ldyBjb2RlIChmcm9tIGBDQVJHT19QS0dfVkVSU0lPTmApLgAAAAd2ZXJzaW9uAAAAABAAAAAaSGFzaCBvZiB0aGUgbmV3IHdhc20gY29kZS4AAAAAAAl3YXNtX2hhc2gAAAAAAAPuAAAAIA==", "AAAAAQAAAIRTdW1tYXJ5IG9mIGEgbWFuZGF0ZSBmb3IgbGlzdCB2aWV3cy4gQ29udGFpbnMgdGhlIGZpZWxkcyBhIHdhbGxldCBuZWVkcyB0bwpkaXNwbGF5IGEgc3Vic2NyaXB0aW9uIHdpdGhvdXQgZmV0Y2hpbmcgdGhlIGZ1bGwgbWFuZGF0ZS4AAAAAAAAADk1hbmRhdGVTdW1tYXJ5AAAAAAAKAAAAGkFtb3VudCBjaGFyZ2VkIHBlciBwZXJpb2QuAAAAAAAGYW1vdW50AAAAAAALAAAAL051bWJlciBvZiBjaGFyZ2VzIHN1Y2Nlc3NmdWxseSBleGVjdXRlZCBzbyBmYXIuAAAAAAxjaGFyZ2VzX21hZGUAAAAEAAAAIVVuaXF1ZSBpZGVudGlmaWVyIG9mIHRoZSBtYW5kYXRlLgAAAAAAAAJpZAAAAAAABgAAACpNYXhpbXVtIG51bWJlciBvZiBjaGFyZ2VzICgwID0gdW5saW1pdGVkKS4AAAAAAAttYXhfY2hhcmdlcwAAAAAEAAAALk1lcmNoYW50IHJlY2VpdmluZyBwYXltZW50IHVuZGVyIHRoaXMgbWFuZGF0ZS4AAAAAAAhtZXJjaGFudAAAABMAAAAsVW5peCBzZWNvbmRzIHdoZW4gdGhlIG5leHQgY2hhcmdlIGZhbGxzIGR1ZS4AAAALbmV4dF9jaGFyZ2UAAAAABgAAABpCaWxsaW5nIHBlcmlvZCBpbiBzZWNvbmRzLgAAAAAABnBlcmlvZAAAAAAABgAAAChQbGFuIElEIHRoaXMgbWFuZGF0ZSB3YXMgb3BlbmVkIGFnYWluc3QuAAAAB3BsYW5faWQAAAAABgAAACdDdXJyZW50IGxpZmVjeWNsZSBzdGF0ZSBvZiB0aGUgbWFuZGF0ZS4AAAAABnN0YXR1cwAAAAAH0AAAAA1NYW5kYXRlU3RhdHVzAAAAAAAAOVRva2VuIGNvbnRyYWN0IGFkZHJlc3MgaW4gd2hpY2ggcGF5bWVudHMgYXJlIGRlbm9taW5hdGVkLgAAAAAAAAV0b2tlbgAAAAAAABM=", "AAAAAQAAAEFFeGFjdCBzZXR0bGVtZW50IHJlY2lwaWVudHMgYW5kIGFtb3VudHMgZm9yIGEgcHJvc3BlY3RpdmUgY2hhcmdlLgAAAAAAAAAAAAAQQ2hhcmdlU2ltdWxhdGlvbgAAAA0AAAApVG90YWwgYW1vdW50IGRlYml0ZWQgZnJvbSB0aGUgc3Vic2NyaWJlci4AAAAAAAAGYW1vdW50AAAAAAALAAAAR1doZXRoZXIgdGltaW5nLCBzdGF0dXMsIGNhcHMgYW5kIGNvbmZpZ3VyZWQgcGF1c2VzIHBlcm1pdCBjaGFyZ2luZyBub3cuAAAAAApjaGFyZ2VhYmxlAAAAAAABAAAAKFVuaXggc2Vjb25kcyBhdCB3aGljaCB0aGUgY2hhcmdlIGlzIGR1ZS4AAAAGZHVlX2F0AAAAAAAGAAAADVByb3RvY29sIGZlZS4AAAAAAAADZmVlAAAAAAsAAAAjQWRkcmVzcyByZWNlaXZpbmcgdGhlIHByb3RvY29sIGZlZS4AAAAABmZlZV90bwAAAAAAEwAAADFQcm9wb3NlZCBrZWVwZXIgYWRkcmVzcyBhbmQgcmVjaXBpZW50IG9mIGFueSB0aXAuAAAAAAAABmtlZXBlcgAAAAAAEwAAADlNZXJjaGFudCdzIG5ldCBzaGFyZSBhZnRlciBmZWVzLCBrZWVwZXIgdGlwIGFuZCByZWZlcnJhbC4AAAAAAAAPbWVyY2hhbnRfYW1vdW50AAAAAAsAAAArQWRkcmVzcyByZWNlaXZpbmcgdGhlIG1lcmNoYW50J3MgbmV0IHNoYXJlLgAAAAAGcGF5b3V0AAAAAAATAAAAMlJlZmVycmFsIHBheW1lbnQgKHplcm8gd2hlbiB0aGVyZSBpcyBubyByZWZlcnJlcikuAAAAAAAIcmVmZXJyYWwAAAALAAAAG1JlZmVycmFsIHJlY2lwaWVudCwgaWYgYW55LgAAAAAIcmVmZXJyZXIAAAPoAAAAEwAAADBTdWJzY3JpYmVyIHdob3NlIHZhdWx0IGJhbGFuY2Ugd291bGQgYmUgZGViaXRlZC4AAAAKc3Vic2NyaWJlcgAAAAAAEwAAAAtLZWVwZXIgdGlwLgAAAAADdGlwAAAAAAsAAAAaVG9rZW4gdXNlZCBmb3IgdGhlIGNoYXJnZS4AAAAAAAV0b2tlbgAAAAAAABM=", "AAAABQAAADdFbWl0dGVkIHdoZW4gYSBtYW5kYXRlIGNoYXJnZSBpcyBzdWNjZXNzZnVsbHkgZXhlY3V0ZWQuAAAAAAAAAAAHQ2hhcmdlZAAAAAABAAAAB2NoYXJnZWQAAAAADAAAABpJRCBvZiB0aGUgY2hhcmdlZCBtYW5kYXRlLgAAAAAACm1hbmRhdGVfaWQAAAAAAAYAAAABAAAAIkFjY291bnQgb2YgdGhlIHN1YnNjcmliZXIgZGViaXRlZC4AAAAAAApzdWJzY3JpYmVyAAAAAAATAAAAAQAAACFBY2NvdW50IG9mIHRoZSBtZXJjaGFudCBjcmVkaXRlZC4AAAAAAAAIbWVyY2hhbnQAAAATAAAAAQAAAK5TRVAtNDEgdG9rZW4gdGhhdCBgYW1vdW50YCwgYGZlZWAgYW5kIGB0aXBgIGFyZSBkZW5vbWluYXRlZCBpbi4gV2l0aG91dAppdCBhbiBpbmRleGVyIG11c3Qgam9pbiBvbiB0aGUgbWFuZGF0ZSB0byB0b3RhbCByZXZlbnVlIHBlciB0b2tlbiwgYW5kCmNhbm5vdCBkbyBzbyBmcm9tIGV2ZW50cyBhbG9uZS4AAAAAAAV0b2tlbgAAAAAAABMAAAAAAAAAOU1lcmNoYW50IHNoYXJlIGFmdGVyIHRoZSBwcm90b2NvbCBmZWUgYW5kIHRoZSBrZWVwZXIgdGlwLgAAAAAAAAZhbW91bnQAAAAAAAsAAAAAAAAAFlByb3RvY29sIGZlZSBkZWR1Y3RlZC4AAAAAAANmZWUAAAAACwAAAAAAAACHUGFpZCB0byBga2VlcGVyYCBvdXQgb2YgdGhlIG1lcmNoYW50IHNoYXJlLiBUaGUgc3Vic2NyaWJlcidzIHRvdGFsIGRlYml0CmlzIGBhbW91bnQgKyBmZWUgKyB0aXBgIGV4YWN0bHkgYXMgaXQgd2FzIGJlZm9yZSB0aXBzIGV4aXN0ZWQuAAAAAAN0aXAAAAAACwAAAAAAAAA2QWNjb3VudCB0aGF0IHNldHRsZWQgdGhpcyBjaGFyZ2UgYW5kIHJlY2VpdmVkIHRoZSB0aXAuAAAAAAAGa2VlcGVyAAAAAAATAAAAAAAAADVUb3RhbCBudW1iZXIgb2YgY2hhcmdlcyBjb21wbGV0ZWQgdW5kZXIgdGhpcyBtYW5kYXRlLgAAAAAAAAxjaGFyZ2VzX21hZGUAAAAEAAAAAAAAAC5Vbml4IHRpbWVzdGFtcCB3aGVuIHRoZSBuZXh0IGNoYXJnZSBmYWxscyBkdWUuAAAAAAALbmV4dF9jaGFyZ2UAAAAABgAAAAAAAABiUmVmZXJyYWwgc2hhcmUgcGFpZCB0byBgcmVmZXJyZXJgIG91dCBvZiB0aGUgbWVyY2hhbnQncyBzaGFyZSwgMCB3aGVuCnRoZSBtYW5kYXRlIGhhcyBubyByZWZlcnJlci4AAAAAAAhyZWZlcnJhbAAAAAsAAAAAAAAAKEFjY291bnQgcGFpZCB0aGUgcmVmZXJyYWwgc2hhcmUsIGlmIGFueS4AAAAIcmVmZXJyZXIAAAPoAAAAEwAAAAAAAAAC", "AAAABQAAARtBIG1lcmNoYW50IHJlZnVuZGVkIHBhcnQgb2Ygd2hhdCBpdCBjb2xsZWN0ZWQgZm9yIGEgbWFuZGF0ZS4gVGhlIGV2ZW50CmxpbmtzIHRoZSByZWZ1bmQgdG8gdGhlIG1hbmRhdGUsIHNvIGJvdGggc2lkZXMgY2FuIHByb3ZlIGEgcmVmdW5kIGZyb20KY2hhaW4gZGF0YSBhbG9uZS4gYHJlZnVuZGVkX3RvdGFsYCBpcyB0aGUgcnVubmluZyB0b3RhbCByZWZ1bmRlZCBvbiB0aGlzCm1hbmRhdGUsIHdoaWNoIGlzIHdoYXQgdGhlIGNvbnRyYWN0IGNhcHMgYXQgdGhlIG1lcmNoYW50J3MgcmVjZWlwdHMuAAAAAAAAAAAIUmVmdW5kZWQAAAABAAAACHJlZnVuZGVkAAAABQAAABtJRCBvZiB0aGUgcmVmdW5kZWQgbWFuZGF0ZS4AAAAACm1hbmRhdGVfaWQAAAAAAAYAAAABAAAAI1N1YnNjcmliZXIgd2hvIHJlY2VpdmVkIHRoZSByZWZ1bmQuAAAAAApzdWJzY3JpYmVyAAAAAAATAAAAAQAAAB9NZXJjaGFudCB3aG8gaXNzdWVkIHRoZSByZWZ1bmQuAAAAAAhtZXJjaGFudAAAABMAAAABAAAALFJlZnVuZCBhbW91bnQgdHJhbnNmZXJyZWQgaW4gdGhpcyBvcGVyYXRpb24uAAAABmFtb3VudAAAAAAACwAAAAAAAAAyQ3VtdWxhdGl2ZSBhbW91bnQgcmVmdW5kZWQgb24gdGhpcyBtYW5kYXRlIHNvIGZhci4AAAAAAA5yZWZ1bmRlZF90b3RhbAAAAAAACwAAAAAAAAAC", "AAAABQAAAXhFbWl0dGVkIHdoZW4gYSBzdWJzY3JpYmVyIG1vdmVzIGEgbWFuZGF0ZSBvbnRvIGEgZGlmZmVyZW50IHBsYW4uCgpgY2hhcmdlZGAgaXMgdGhlIHByb3JhdGVkIGRpZmZlcmVuY2UgY29sbGVjdGVkIGZvciB0aGUgcGFydCBvZiB0aGUKY3VycmVudCBwZXJpb2QgdGhhdCB3YXMgc3RpbGwgYWhlYWQ7IGl0IGlzIHplcm8gZm9yIGEgZG93bmdyYWRlLCB3aGljaAp0YWtlcyBlZmZlY3QgYXQgdGhlIG5leHQgY2hhcmdlIGFuZCBuZXZlciByZWZ1bmRzIGFueXRoaW5nLiBUaGUgZXZlbnQKY2FycmllcyB0aGUgb2xkIGFuZCB0aGUgbmV3IHRlcm1zIHNvIGFuIGluZGV4ZXIgY2FuIHNob3cgdGhlIGNoYW5nZQp3aXRob3V0IGEgZm9sbG93LXVwIGNvbnRyYWN0IHJlYWQuAAAAAAAAAAhVcGdyYWRlZAAAAAEAAAAIdXBncmFkZWQAAAAKAAAAG0lEIG9mIHRoZSB1cGdyYWRlZCBtYW5kYXRlLgAAAAAKbWFuZGF0ZV9pZAAAAAAABgAAAAEAAAAYU3Vic2NyaWJlciB3aG8gdXBncmFkZWQuAAAACnN1YnNjcmliZXIAAAAAABMAAAABAAAALU1lcmNoYW50IHJlY2VpdmluZyBwYXltZW50IHVuZGVyIHRoZSBtYW5kYXRlLgAAAAAAAAhtZXJjaGFudAAAABMAAAABAAAAEVByZXZpb3VzIHBsYW4gSUQuAAAAAAAAC29sZF9wbGFuX2lkAAAAAAYAAAAAAAAADE5ldyBwbGFuIElELgAAAAtuZXdfcGxhbl9pZAAAAAAGAAAAAAAAADlUaGUgcHJpY2UgdGhlIG1hbmRhdGUgcGFpZCBwZXIgcGVyaW9kIGJlZm9yZSB0aGUgdXBncmFkZS4AAAAAAAAKb2xkX2Ftb3VudAAAAAAACwAAAAAAAAA6VGhlIHByaWNlIHRoZSBtYW5kYXRlIHBheXMgcGVyIHBlcmlvZCBmcm9tIHRoZSB1cGdyYWRlIG9uLgAAAAAACm5ld19hbW91bnQAAAAAAAsAAAAAAAAASkRpZmZlcmVuY2UgY29sbGVjdGVkIG5vdywgcHJvcmF0ZWQgYW5kIHJvdW5kZWQgZG93bi4gWmVybyBmb3IgYQpkb3duZ3JhZGUuAAAAAAAHY2hhcmdlZAAAAAALAAAAAAAAACJUaGUgbmV3IHBsYW4ncyBwZXJpb2QsIGluIHNlY29uZHMuAAAAAAAGcGVyaW9kAAAAAAAGAAAAAAAAAD1XaGVuIHRoZSBuZXh0IGNoYXJnZSBmYWxscyBkdWUuIFRoZSB1cGdyYWRlIGRvZXMgbm90IG1vdmUgaXQuAAAAAAAAC25leHRfY2hhcmdlAAAAAAYAAAAAAAAAAg==", "AAAABQAAACxFbWl0dGVkIHdoZW4gYSBzdWJzY3JpYmVyIGNhbmNlbHMgYSBtYW5kYXRlLgAAAAAAAAAJQ2FuY2VsbGVkAAAAAAAAAQAAAAljYW5jZWxsZWQAAAAAAAAEAAAAHElEIG9mIHRoZSBjYW5jZWxsZWQgbWFuZGF0ZS4AAAAKbWFuZGF0ZV9pZAAAAAAABgAAAAEAAAAoQWNjb3VudCBvZiB0aGUgc3Vic2NyaWJlciB3aG8gY2FuY2VsbGVkLgAAAApzdWJzY3JpYmVyAAAAAAATAAAAAQAAADZUb3RhbCBudW1iZXIgb2YgY2hhcmdlcyBjb21wbGV0ZWQgYmVmb3JlIGNhbmNlbGxhdGlvbi4AAAAAAAxjaGFyZ2VzX21hZGUAAAAEAAAAAAAAAPJXaHkgdGhlIHN1YnNjcmliZXIgbGVmdCwgYXMgb25lIG9mIHRoZSBkb2N1bWVudGVkIGBDQU5DRUxfUkVBU09OXypgCmNvZGVzLiBgMGAgbWVhbnMgbm8gcmVhc29uIGdpdmVuLCB3aGljaCBpcyB3aGF0IGBjYW5jZWxgIGFuZApgY2FuY2VsX3BhZ2VgIGVtaXQuIENhcnJpZWQgb24gdGhlIHdpcmUgc28gYSBtZXJjaGFudCBjYW4gY29tcHV0ZQpjaHVybiB3aXRob3V0IGEgY29udHJhY3QgcmVhZCBwZXIgY2FuY2VsbGF0aW9uLgAAAAAABnJlYXNvbgAAAAAABAAAAAAAAAAC", "AAAABQAAAJhFbWl0dGVkIHdoZW4gdGhlIGFkbWluIGNoYW5nZXMgdGhlIHByb3RvY29sIGZlZS4gQXBwbGllcyB0byBtYW5kYXRlcyBvcGVuZWQKYWZ0ZXIgdGhpcyBwb2ludCBvbmx5OyBleGlzdGluZyBtYW5kYXRlcyBrZWVwIHRoZSBmZWUgdGhleSB3ZXJlIG9wZW5lZCB3aXRoLgAAAAAAAAAKRmVlQ2hhbmdlZAAAAAAAAQAAAAtmZWVfY2hhbmdlZAAAAAADAAAAI0FkbWluIGFjY291bnQgdGhhdCBjaGFuZ2VkIHRoZSBmZWUuAAAAAAVhZG1pbgAAAAAAABMAAAABAAAAJlByZXZpb3VzIHByb3RvY29sIGZlZSBpbiBiYXNpcyBwb2ludHMuAAAAAAALb2xkX2ZlZV9icHMAAAAABAAAAAAAAAAhTmV3IHByb3RvY29sIGZlZSBpbiBiYXNpcyBwb2ludHMuAAAAAAAAC25ld19mZWVfYnBzAAAAAAQAAAAAAAAAAg==", "AAAABQAAAD1FbWl0dGVkIHdoZW4gYSBzdWJzY3JpYmVyIG9wZW5zIGEgbmV3IG1hbmRhdGUgYWdhaW5zdCBhIHBsYW4uAAAAAAAAAAAAAApTdWJzY3JpYmVkAAAAAAABAAAACnN1YnNjcmliZWQAAAAAAA0AAAAvVW5pcXVlIGlkZW50aWZpZXIgb2YgdGhlIG5ld2x5IGNyZWF0ZWQgbWFuZGF0ZS4AAAAACm1hbmRhdGVfaWQAAAAAAAYAAAABAAAAGkFjY291bnQgb2YgdGhlIHN1YnNjcmliZXIuAAAAAAAKc3Vic2NyaWJlcgAAAAAAEwAAAAEAAAAYQWNjb3VudCBvZiB0aGUgbWVyY2hhbnQuAAAACG1lcmNoYW50AAAAEwAAAAEAAAAdSUQgb2YgdGhlIHBsYW4gc3Vic2NyaWJlZCB0by4AAAAAAAAHcGxhbl9pZAAAAAAGAAAAAAAAACJBbW91bnQgY2hhcmdlZCBwZXIgYmlsbGluZyBwZXJpb2QuAAAAAAAGYW1vdW50AAAAAAALAAAAAAAAABpCaWxsaW5nIHBlcmlvZCBpbiBzZWNvbmRzLgAAAAAABnBlcmlvZAAAAAAABgAAAAAAAAAsVW5peCB0aW1lc3RhbXAgd2hlbiB0aGUgZmlyc3QgY2hhcmdlIGlzIGR1ZS4AAAALbmV4dF9jaGFyZ2UAAAAABgAAAAAAAACCMCBtZWFucyBvcGVuLWVuZGVkLiBDYXJyaWVkIGluIHRoZSBldmVudCBzbyBpbmRleGVycyBkbyBub3QgaGF2ZSB0bwptYWtlIGEgZm9sbG93LXVwIGNvbnRyYWN0IHJlYWQgdG8gbGVhcm4gYSBtYW5kYXRlJ3Mgc3BlbmQgY2FwLgAAAAAAC21heF9jaGFyZ2VzAAAAAAQAAAAAAAAAEzAgbWVhbnMgb3Blbi1lbmRlZC4AAAAACW1heF90b3RhbAAAAAAAAAsAAAAAAAAAO1RoZSBmZWUgcmF0ZSBmcm96ZW4gaW50byB0aGlzIG1hbmRhdGUsIGZvciB0aGUgc2FtZSByZWFzb24uAAAAAAdmZWVfYnBzAAAAAAQAAAAAAAAAPVRoZSBrZWVwZXIgdGlwIGZyb3plbiBpbnRvIHRoaXMgbWFuZGF0ZSwgZm9yIHRoZSBzYW1lIHJlYXNvbi4AAAAAAAAKa2VlcGVyX2JwcwAAAAAABAAAAAAAAAAuVGhlIHJlZmVycmVyIGZyb3plbiBpbnRvIHRoaXMgbWFuZGF0ZSwgaWYgYW55LgAAAAAACHJlZmVycmVyAAAD6AAAABMAAAAAAAAAVFRoZSByZWZlcnJhbCByYXRlIGZyb3plbiBpbnRvIHRoaXMgbWFuZGF0ZSwgaW4gYmFzaXMgcG9pbnRzIG9mIHRoZQptZXJjaGFudCdzIHNoYXJlLgAAAAxyZWZlcnJhbF9icHMAAAAEAAAAAAAAAAI=", "AAAABQAAAkpNZXJjaGFudCBub3RpZmljYXRpb24gaG9vayAoIzI5Nik6IGVtaXR0ZWQgdGhlIGZpcnN0IHRpbWUgYSBkdWUgbWFuZGF0ZSdzCmNoYXJnZSBmYWlscywgc28gb2ZmLWNoYWluIGR1bm5pbmcgY2FuIHJlYWN0IHByb21wdGx5LgoKQSBmYWlsZWQgYGNoYXJnZWAgaXMgcm9sbGVkIGJhY2sgdG9nZXRoZXIgd2l0aCBhbnkgZXZlbnQgaXQgd291bGQgZW1pdCwgc28KdGhlIGZhaWx1cmUgaXMgcmVjb3JkZWQgYnkgYGZsYWdfZmFpbGVkX2NoYXJnZWAsIHdoaWNoIHByb3ZlcyBvbiBjaGFpbiB0aGF0CnRoZSBjaGFyZ2Ugd291bGQgZmFpbCByaWdodCBub3cuIFRoZSBldmVudCBmaXJlcyBvbmNlIHBlciBydW4gb2YgZmFpbHVyZXM6CmZ1cnRoZXIgZmxhZ3MgYXJlIG5vLW9wcyB1bnRpbCBhIHN1Y2Nlc3NmdWwgYGNoYXJnZWAgY2xlYXJzIHRoZSBydW4sIGFmdGVyCndoaWNoIHRoZSBuZXh0IGZhaWx1cmUgbm90aWZpZXMgYWdhaW4uCgpgbWVyY2hhbnRgIGlzIGEgdG9waWMgc28gYSBtZXJjaGFudCdzIGR1bm5pbmcgc2VydmljZSBjYW4gc3Vic2NyaWJlIHRvIGl0cwpvd24gZmFpbHVyZXMgd2l0aCBhIHNpbmdsZSB0b3BpYyBmaWx0ZXIuAAAAAAAAAAAADENoYXJnZUZhaWxlZAAAAAEAAAANY2hhcmdlX2ZhaWxlZAAAAAAAAAkAAAAmSUQgb2YgdGhlIG1hbmRhdGUgd2hvc2UgY2hhcmdlIGZhaWxlZC4AAAAAAAptYW5kYXRlX2lkAAAAAAAGAAAAAQAAAEFNZXJjaGFudCBvd2VkIHRoZSBjaGFyZ2U7IHRoZSB0b3BpYyBhIGR1bm5pbmcgc2VydmljZSBmaWx0ZXJzIG9uLgAAAAAAAAhtZXJjaGFudAAAABMAAAABAAAAMlN1YnNjcmliZXIgd2hvc2UgdmF1bHQgY291bGQgbm90IGNvdmVyIHRoZSBjaGFyZ2UuAAAAAAAKc3Vic2NyaWJlcgAAAAAAEwAAAAEAAAAfUGxhbiB0aGUgbWFuZGF0ZSBiaWxscyBhZ2FpbnN0LgAAAAAHcGxhbl9pZAAAAAAGAAAAAAAAACNUb2tlbiB0aGUgY2hhcmdlIGlzIGRlbm9taW5hdGVkIGluLgAAAAAFdG9rZW4AAAAAAAATAAAAAAAAADVBbW91bnQgdGhlIGNoYXJnZSBuZWVkZWQ6IHRoZSBtYW5kYXRlJ3MgZnJvemVuIHByaWNlLgAAAAAAAAZhbW91bnQAAAAAAAsAAAAAAAAAMVVuaXggc2Vjb25kcyB0aGUgY2hhcmdlIGZlbGwgZHVlIChgbmV4dF9jaGFyZ2VgKS4AAAAAAAAGZHVlX2F0AAAAAAAGAAAAAAAAACVVbml4IHNlY29uZHMgdGhlIGZhaWx1cmUgd2FzIGZsYWdnZWQuAAAAAAAACWZhaWxlZF9hdAAAAAAAAAYAAAAAAAAAKkNoYXJnZXMgc2V0dGxlZCBzbyBmYXIgdW5kZXIgdGhpcyBtYW5kYXRlLgAAAAAADGNoYXJnZXNfbWFkZQAAAAQAAAAAAAAAAg==", "AAAABQAAAMRFbWl0dGVkIHdoZW4gdGhlIG1lcmNoYW50IGVuZHMgYSBtYW5kYXRlLCBmb3IgZXhhbXBsZSBiZWNhdXNlIGl0IHN0b3BwZWQKb2ZmZXJpbmcgdGhlIHNlcnZpY2UuIFRoZSBtYW5kYXRlIGlzIGNhbmNlbGxlZCBleGFjdGx5IGFzIGlmIHRoZQpzdWJzY3JpYmVyIGhhZCBjYW5jZWxsZWQgaXQ7IHRoZSBldmVudCByZWNvcmRzIHdobyBkaWQgaXQuAAAAAAAAAAxNYW5kYXRlRW5kZWQAAAABAAAADW1hbmRhdGVfZW5kZWQAAAAAAAAEAAAAGElEIG9mIHRoZSBlbmRlZCBtYW5kYXRlLgAAAAptYW5kYXRlX2lkAAAAAAAGAAAAAQAAAB9NZXJjaGFudCB3aG8gZW5kZWQgdGhlIG1hbmRhdGUuAAAAAAhtZXJjaGFudAAAABMAAAABAAAAI1N1YnNjcmliZXIgd2hvc2UgbWFuZGF0ZSB3YXMgZW5kZWQuAAAAAApzdWJzY3JpYmVyAAAAAAATAAAAAAAAADBUb3RhbCBudW1iZXIgb2YgY2hhcmdlcyBjb21wbGV0ZWQgYmVmb3JlIGVuZGluZy4AAAAMY2hhcmdlc19tYWRlAAAABAAAAAAAAAAC", "AAAABQAAACxFbWl0dGVkIHdoZW4gYSBtYW5kYXRlIGlzIHBhdXNlZCBvciByZXN1bWVkLgAAAAAAAAAMUGF1c2VDaGFuZ2VkAAAAAQAAAA1wYXVzZV9jaGFuZ2VkAAAAAAAAAwAAAC1JRCBvZiB0aGUgbWFuZGF0ZSB3aG9zZSBwYXVzZSBzdGF0dXMgY2hhbmdlZC4AAAAAAAAKbWFuZGF0ZV9pZAAAAAAABgAAAAEAAAAaQWNjb3VudCBvZiB0aGUgc3Vic2NyaWJlci4AAAAAAApzdWJzY3JpYmVyAAAAAAATAAAAAQAAAEBXaGV0aGVyIHRoZSBtYW5kYXRlIGlzIG5vdyBwYXVzZWQgKGB0cnVlYCkgb3IgcmVzdW1lZCAoYGZhbHNlYCkuAAAABnBhdXNlZAAAAAAAAQAAAAAAAAAC", "AAAABQAAAKtFbWl0dGVkIHdoZW4gYSBzdWJzY3JpYmVyIGNoYW5nZXMgdGhlIHNlYXQgY291bnQgb24gdGhlaXIgbWFuZGF0ZS4KClRoZSBuZXcgc2VhdCBjb3VudCB0YWtlcyBlZmZlY3QgZnJvbSB0aGUgbmV4dCBjaGFyZ2U7IG5vIG1vbmV5IG1vdmVzIGF0IHRoZQp0aW1lIHRoaXMgZXZlbnQgaXMgZW1pdHRlZC4AAAAAAAAAAAxTZWF0c0NoYW5nZWQAAAABAAAADXNlYXRzX2NoYW5nZWQAAAAAAAAEAAAAK0lEIG9mIHRoZSBtYW5kYXRlIHdob3NlIHNlYXQgY291bnQgY2hhbmdlZC4AAAAACm1hbmRhdGVfaWQAAAAAAAYAAAABAAAAH1N1YnNjcmliZXIgd2hvIG1hZGUgdGhlIGNoYW5nZS4AAAAACnN1YnNjcmliZXIAAAAAABMAAAABAAAAHVNlYXQgY291bnQgYmVmb3JlIHRoZSBjaGFuZ2UuAAAAAAAACW9sZF9zZWF0cwAAAAAAAAQAAAAAAAAAPFNlYXQgY291bnQgYWZ0ZXIgdGhlIGNoYW5nZSwgZWZmZWN0aXZlIGZyb20gdGhlIG5leHQgY2hhcmdlLgAAAAluZXdfc2VhdHMAAAAAAAAEAAAAAAAAAAI=", "AAAABQAAALZFbWl0dGVkIGFmdGVyIGV2ZXJ5IHN0YXRlIGNoYW5nZSB0byBhIG1hbmRhdGUsIGNhcnJ5aW5nIGEgY29tcGxldGUgc25hcHNob3QKb2YgdGhlIG1hbmRhdGUgc28gaW5kZXhlcnMgZG8gbm90IGhhdmUgdG8gcmVjb25zdHJ1Y3Qgc3RhdGUgb3IgcmlzayBkcmlmdApmcm9tIG1pc3NlZCBpbnRlcm1lZGlhdGUgZXZlbnRzLgAAAAAAAAAAAA5NYW5kYXRlVXBkYXRlZAAAAAAAAQAAAA9tYW5kYXRlX3VwZGF0ZWQAAAAAAgAAABpJZCBvZiB0aGUgdXBkYXRlZCBtYW5kYXRlLgAAAAAACm1hbmRhdGVfaWQAAAAAAAYAAAABAAAALkNvbXBsZXRlIHNuYXBzaG90IG9mIHRoZSBtYW5kYXRlIGFmdGVyIHVwZGF0ZS4AAAAAAAdtYW5kYXRlAAAAB9AAAAAHTWFuZGF0ZQAAAAAAAAAAAg==", "AAAABQAAAPhFbWl0dGVkIHdoZW4gdGhlIGFkbWluIHJlcGxhY2VzIHRoaXMgY29udHJhY3QncyB3YXNtLiBQZXJzaXN0ZW50IHN0YXRlIGlzCnVudG91Y2hlZCBieSBhbiB1cGdyYWRlOyB0aGUgbmV3IGNvZGUgcmVhZHMgdGhlIGtleXMgdGhlIG9sZCBjb2RlIHdyb3RlLgpOYW1lZCBgQ29udHJhY3RVcGdyYWRlZGAgYmVjYXVzZSBgVXBncmFkZWRgIGlzIGFscmVhZHkgdGFrZW4gYnkgdGhlCm1hbmRhdGUtcGxhbiB1cGdyYWRlIGV2ZW50IGFib3ZlLgAAAAAAAAAQQ29udHJhY3RVcGdyYWRlZAAAAAEAAAARY29udHJhY3RfdXBncmFkZWQAAAAAAAACAAAAAAAAAAVhZG1pbgAAAAAAABMAAAABAAAAAAAAAA1uZXdfd2FzbV9oYXNoAAAAAAAD7gAAACAAAAAAAAAAAg==", "AAAABQAAAElFbWl0dGVkIHdoZW4gYSBtYW5kYXRlIHJlYWNoZXMgaXRzIG1heGltdW0gYWxsb3dlZCBjaGFyZ2VzIGFuZCBjb21wbGV0ZXMuAAAAAAAAAAAAABBNYW5kYXRlQ29tcGxldGVkAAAAAQAAABFtYW5kYXRlX2NvbXBsZXRlZAAAAAAAAAMAAAAcSUQgb2YgdGhlIGNvbXBsZXRlZCBtYW5kYXRlLgAAAAptYW5kYXRlX2lkAAAAAAAGAAAAAQAAABpBY2NvdW50IG9mIHRoZSBzdWJzY3JpYmVyLgAAAAAACnN1YnNjcmliZXIAAAAAABMAAAABAAAAIVRvdGFsIG51bWJlciBvZiBjaGFyZ2VzIGV4ZWN1dGVkLgAAAAAAAAxjaGFyZ2VzX21hZGUAAAAEAAAAAAAAAAI=", "AAAABQAAAIdFbWl0dGVkIHdoZW4gYSBzdWJzY3JpYmVyIGNoYW5nZXMgdGhlIGNoYXJnZSBjYXAgb2YgdGhlaXIgb3duIG9wZW4KbWFuZGF0ZS4gVGhlIG1hbmRhdGUga2VlcHMgaXRzIGlkIGFuZCBoaXN0b3J5OyBvbmx5IHRoZSBsaW1pdCBtb3Zlcy4AAAAAAAAAABFNYXhDaGFyZ2VzQ2hhbmdlZAAAAAAAAAEAAAATbWF4X2NoYXJnZXNfY2hhbmdlZAAAAAADAAAAAAAAAAptYW5kYXRlX2lkAAAAAAAGAAAAAQAAAAAAAAAKc3Vic2NyaWJlcgAAAAAAEwAAAAEAAAAvTmV3IGNhcC4gMCBtZWFucyB0aGUgbWFuZGF0ZSBpcyBub3cgb3Blbi1lbmRlZC4AAAAAC21heF9jaGFyZ2VzAAAAAAQAAAAAAAAAAg==", "AAAABQAAAPhFbWl0dGVkIHdoZW4gYSBtYW5kYXRlJ3Mgb3duZXJzaGlwIG1vdmVzIGZyb20gb25lIHN1YnNjcmliZXIgdG8gYW5vdGhlciB2aWEKYHByb3Bvc2VfbWFuZGF0ZV9vd25lcmAgLyBgYWNjZXB0X21hbmRhdGVfb3duZXJgLiBGdXR1cmUgY2hhcmdlcyBhcmUgZGViaXRlZApmcm9tIHRoZSBuZXcgc3Vic2NyaWJlcidzIHZhdWx0OyBhbGwgZnJvemVuIG1hbmRhdGUgdGVybXMgYW5kIHRoZSBtZXJjaGFudAppbmRleCBhcmUgdW5jaGFuZ2VkLgAAAAAAAAASTWFuZGF0ZVRyYW5zZmVycmVkAAAAAAABAAAAE21hbmRhdGVfdHJhbnNmZXJyZWQAAAAAAwAAAB5JRCBvZiB0aGUgdHJhbnNmZXJyZWQgbWFuZGF0ZS4AAAAAAAptYW5kYXRlX2lkAAAAAAAGAAAAAQAAACdTdWJzY3JpYmVyIHRoZSBtYW5kYXRlIG1vdmVkIGF3YXkgZnJvbS4AAAAADm9sZF9zdWJzY3JpYmVyAAAAAAATAAAAAQAAACBTdWJzY3JpYmVyIHRoZSBtYW5kYXRlIG1vdmVkIHRvLgAAAA5uZXdfc3Vic2NyaWJlcgAAAAAAEwAAAAAAAAAC", "AAAABQAAAKFFbWl0dGVkIHdoZW4gdGhlIGFkbWluIHBhdXNlcyBvciByZXN1bWVzIHRoZSBwcm90b2NvbCAoIzI4NSkuIE9ubHkgZW1pdHRlZAp3aGVuIHRoZSBzd2l0Y2ggYWN0dWFsbHkgY2hhbmdlcywgc28gYW4gaW5kZXhlciBjYW4gdHJlYXQgZXZlcnkgZXZlbnQgYXMgYQp0cmFuc2l0aW9uLgAAAAAAAAAAAAAUUHJvdG9jb2xQYXVzZUNoYW5nZWQAAAABAAAAFnByb3RvY29sX3BhdXNlX2NoYW5nZWQAAAAAAAIAAAAmQWRtaW4gYWNjb3VudCB0aGF0IGZsaXBwZWQgdGhlIHN3aXRjaC4AAAAAAAVhZG1pbgAAAAAAABMAAAABAAAAQGB0cnVlYCB3aGVuIHRoZSBwcm90b2NvbCBpcyBub3cgcGF1c2VkLCBgZmFsc2VgIHdoZW4gaXQgcmVzdW1lZC4AAAAGcGF1c2VkAAAAAAABAAAAAAAAAAI=", "AAAABQAAAItFbWl0dGVkIHdoZW4gYSBwbGFuJ3MgY29tbWl0dGVkIHJldmVudWUgcGVyIHBlcmlvZCBjaGFuZ2VzLiBUaGlzIGFsbG93cwpkYXNoYm9hcmRzIHRvIHRyYWNrIE1SUi9BUlIgb24tY2hhaW4gd2l0aG91dCBzY2FubmluZyBhbGwgbWFuZGF0ZXMuAAAAAAAAAAAXQ29tbWl0dGVkUmV2ZW51ZUNoYW5nZWQAAAAAAQAAABljb21taXR0ZWRfcmV2ZW51ZV9jaGFuZ2VkAAAAAAAABgAAAC9JRCBvZiB0aGUgcGxhbiB3aG9zZSBjb21taXR0ZWQgcmV2ZW51ZSBjaGFuZ2VkLgAAAAAHcGxhbl9pZAAAAAAGAAAAAQAAABlNZXJjaGFudCBvd25pbmcgdGhlIHBsYW4uAAAAAAAACG1lcmNoYW50AAAAEwAAAAEAAAAhVG9rZW4gdGhlIHBsYW4gaXMgZGVub21pbmF0ZWQgaW4uAAAAAAAABXRva2VuAAAAAAAAEwAAAAAAAAAlUHJldmlvdXMgY29tbWl0dGVkIGFtb3VudCBwZXIgcGVyaW9kLgAAAAAAAA1vbGRfY29tbWl0dGVkAAAAAAAACwAAAAAAAAAgTmV3IGNvbW1pdHRlZCBhbW91bnQgcGVyIHBlcmlvZC4AAAANbmV3X2NvbW1pdHRlZAAAAAAAAAsAAAAAAAAAGkJpbGxpbmcgcGVyaW9kIGluIHNlY29uZHMuAAAAAAAGcGVyaW9kAAAAAAAGAAAAAAAAAAI=", "AAAAAAAAACZBZGRyZXNzIG9mIHRoZSBjb250cmFjdCBhZG1pbmlzdHJhdG9yLgAAAAAABWFkbWluAAAAAAAAAAAAAAEAAAPpAAAAEwAAAAM=", "AAAAAAAAAClBZGRyZXNzIG9mIHRoZSBjb25maWd1cmVkIHZhdWx0IGNvbnRyYWN0LgAAAAAAAAV2YXVsdAAAAAAAAAAAAAABAAAD6QAAABMAAAAD", "AAAAAAAAAm1UZXJtaW5hdGUgYSBtYW5kYXRlIHBlcm1hbmVudGx5LiBPbmx5IHRoZSBzdWJzY3JpYmVyIG1heSBkbyB0aGlzLCBhbmQgaXQKY2FuIGJlIGRvbmUgYXQgYW55IHRpbWUgd2l0aG91dCB0aGUgbWVyY2hhbnQncyBpbnZvbHZlbWVudC4KClRoaXMgaXMgW2BTdWJzY3JpcHRpb246OmNhbmNlbF93aXRoX3JlYXNvbmBdIHdpdGgKW2BDQU5DRUxfUkVBU09OX05PTkVgXTogdGhlIG1hbmRhdGUgaXMgY2FuY2VsbGVkIGV4YWN0bHkgYXMgYmVmb3JlIGFuZAp0aGUgW2BDYW5jZWxsZWRgXSBldmVudCByZXBvcnRzIHJlYXNvbiBgMGAuCgojIEF1dGgKUmVxdWlyZXMgYXV0aG9yaXphdGlvbiBmcm9tIGBzdWJzY3JpYmVyYC4KCiMgRXZlbnRzCkVtaXRzIFtgTWFuZGF0ZVVwZGF0ZWRgXSBhbmQgW2BDYW5jZWxsZWRgXS4KCiMgRXJyb3JzClJldHVybnMgW2BFcnJvcjo6TWFuZGF0ZU5vdEZvdW5kYF0gaWYgdGhlIG1hbmRhdGUgZG9lcyBub3QgZXhpc3QsCltgRXJyb3I6Ok5vdFN1YnNjcmliZXJgXSBpZiB0aGUgY2FsbGVyIGlzIG5vdCB0aGUgc3Vic2NyaWJlciwgb3IKW2BFcnJvcjo6TWFuZGF0ZU5vdEFjdGl2ZWBdIGlmIHRoZSBtYW5kYXRlIGlzIGFscmVhZHkgY2FuY2VsbGVkIG9yIGNvbXBsZXRlZC4AAAAAAAAGY2FuY2VsAAAAAAACAAAAAAAAAApzdWJzY3JpYmVyAAAAAAATAAAAAAAAAAptYW5kYXRlX2lkAAAAAAAGAAAAAQAAA+kAAAACAAAAAw==", "AAAAAAAABABFeGVjdXRlIGEgZHVlIGNoYXJnZS4KCk9wZW4gdG8gYW55IGNhbGxlcjogdGhlIG1hbmRhdGUgaXRzZWxmIGlzIHRoZSBhdXRob3JpemF0aW9uLCBzbyB0aGUKbWVyY2hhbnQsIGEga2VlcGVyIGJvdCwgb3IgdGhlIHN1YnNjcmliZXIgbWF5IGFsbCBzZXR0bGUgaXQuIGBrZWVwZXJgCm11c3QgYXV0aG9yaXplIHRoaXMgY2FsbCwgYmVjYXVzZSB0aGUgcGxhbidzIHRpcCBpcyBwYWlkIHRvIGl0OyB0aGF0IGlzCnRoZSBvbmx5IHNpZ25hdHVyZSByZXF1aXJlZCwgYW5kIGl0IGdyYW50cyBubyBwb3dlciBvdmVyIHRoZSBtYW5kYXRlLgpDb21wZXRpbmcga2VlcGVycyBhcmUgd2hhdCBrZWVwcyBiaWxsaW5nIGFsaXZlIHdpdGhvdXQgdHJ1c3Rpbmcgb25lCm9wZXJhdG9yIHRvIHN0YXkgb25saW5lLgoKVGhlIHRpcCBpcyBwYWlkIG91dCBvZiB0aGUgbWVyY2hhbnQncyBzaGFyZSAoc2VlIHRoZSBwbGFuJ3MKYGtlZXBlcl9icHNgKSwgc28gdGhlIHN1YnNjcmliZXIgaXMgZGViaXRlZCBleGFjdGx5IGBtYW5kYXRlLmFtb3VudGAKd2hldGhlciBvciBub3QgYSB0aXAgaXMgcGFpZCwgYW5kIHRoZSBwcm90b2NvbCBmZWUgaXMgdW5hZmZlY3RlZC4KCmBuZXh0X2NoYXJnZWAgYWR2YW5jZXMgdG8gYG5vdyArIHBlcmlvZGAgcmF0aGVyIHRoYW4KYG5leHRfY2hhcmdlICsgcGVyaW9kYC4gQSBrZWVwZXIgb3V0YWdlIHRoZXJlZm9yZSBkZWxheXMgYmlsbGluZyBpbnN0ZWFkCm9mIGFjY3VtdWxhdGluZyBhIGJhY2tsb2cgdGhhdCBjb3VsZCBkcmFpbiBhIHZhdWx0IGluIG9uZSBidXJzdC4gVGhlCnRyYWRlb2ZmIGlzIHNsb3cgZm9yd2FyZCBkcmlmdCwgd2hpY2ggZmF2b3VycyB0aGUgc3Vic2NyaWJlci4KClNlZSBbQURSIDAwMDVdKC4uLy4uL2RvY3MvYWRyLzAwMDUtcGVybWlzc2lvbmxlc3MtY2hhcmdlLm1kKSBmb3IgYQpkZXRhaWxlZCBleHBsYW5hdGlvbiBvZiB3aHkgcGVybWlzc2lvbmxlc3MgY2hhcmdpbmcgaXMgc2FmZSBhbmQKa2VlcGVyIG9wZXJhdGlvbmFsIGd1aWRhbmNlLgoKIyBBdXRoClJlcXVpAAAABmNoYXJnZQAAAAAAAgAAAAAAAAAGa2VlcGVyAAAAAAATAAAAAAAAAAptYW5kYXRlX2lkAAAAAAAGAAAAAQAAA+kAAAfQAAAADENoYXJnZVJlc3VsdAAAAAM=", "AAAAAAAAAIVUcnVlIHdoZW4gYGNoYXJnZWAgd291bGQgc3VjY2VlZCBvbiB0aW1pbmcgYW5kIHN0YXR1cyBncm91bmRzLiBEb2VzIG5vdApjaGVjayB2YXVsdCBiYWxhbmNlOyBhIGZ1bmRlZC1uZXNzIGNoZWNrIGlzIHRoZSBjYWxsZXIncyBqb2IuAAAAAAAABmlzX2R1ZQAAAAAAAQAAAAAAAAAKbWFuZGF0ZV9pZAAAAAAABgAAAAEAAAPpAAAAAQAAAAM=", "AAAAAAAABABSZWZ1bmQgcGFydCBvZiB3aGF0IHRoaXMgbWFuZGF0ZSBwYWlkIHRoZSBtZXJjaGFudC4KClRoZSBtZXJjaGFudCByZXR1cm5zIG1vbmV5IGZyb20gaXRzIG93biB3YWxsZXQsIHN0cmFpZ2h0IHRvIHRoZQpzdWJzY3JpYmVyJ3Mgd2FsbGV0LCBhbmQgdGhlIGNvbnRyYWN0IHJlY29yZHMgdGhlIHJlZnVuZCBvbiB0aGUKbWFuZGF0ZSBhbmQgZW1pdHMgYFJlZnVuZGVkYCwgc28gdGhlIGRpc3B1dGUgdHJhaWwgbGl2ZXMgb24gY2hhaW4KaW5zdGVhZCBvZiBpbiBhZC1ob2MgdHJhbnNmZXJzIG5vYm9keSBjYW4gYXR0cmlidXRlLiBJdCBkZWxpYmVyYXRlbHkKZG9lcyBub3QgdG91Y2ggdGhlIHZhdWx0OiBjcmVkaXRpbmcgaW50ZXJuYWwgYmFsYW5jZXMgd2l0aG91dCBtb3ZpbmcKdG9rZW5zIHdvdWxkIGJyZWFrIHRoZSB2YXVsdCdzIGludmFyaWFudCB0aGF0IGludGVybmFsIGJhbGFuY2VzIHN1bSB0bwppdHMgaG9sZGluZ3MuCgpUb3RhbCByZWZ1bmRzIGFyZSBjYXBwZWQgYXQgd2hhdCB0aGUgbWVyY2hhbnQgYWN0dWFsbHkgY29sbGVjdGVkIGZvcgp0aGlzIG1hbmRhdGUgKHByaWNlIG1pbnVzIHRoZSBmcm96ZW4gZmVlIGFuZCB0aXAsIHBlciBzZXR0bGVkIGNoYXJnZSksCnNvIGEgbWVyY2hhbnQgY2FuIG5ldmVyICJyZWZ1bmQiIG1vcmUgdGhhbiBpdCBldmVyIHJlY2VpdmVkLiBUaGUgY2FwCmhvbGRzIHJlZ2FyZGxlc3Mgb2YgbWFuZGF0ZSBzdGF0dXMsIHdoaWNoIGlzIHdoeSBhIGNhbmNlbGxlZCBtYW5kYXRlCnJlbWFpbnMgcmVmdW5kYWJsZS4KCiMgQXV0aApSZXF1aXJlcyBhdXRob3JpemF0aW9uIGZyb20gYG1lcmNoYW50YC4KCiMgRXZlbnRzCkVtaXRzIFtgTWFuZGF0ZVVwZGF0ZWRgXSBhbmQgW2BSZWZ1bmRlZGBdLgoKIyBFcnJvcnMKUmV0dXJucyBbYEVycm9yOjpNYW5kYXRlTm90Rm91bmRgXSBpZiB0aGUgbWFuZGF0ZSBkb2VzIG5vdCBleGlzdCwKW2BFcnJvcjo6Tm90TWVyY2hhbnRgXSBpZiB0aGUgY2FsbGVyIGlzIG5vdCB0aGUgbWVyY2hhbnQsCltgRXJyb3I6OkludmFsaWRBbW91AAAABnJlZnVuZAAAAAAAAwAAAAAAAAAIbWVyY2hhbnQAAAATAAAAAAAAAAptYW5kYXRlX2lkAAAAAAAGAAAAAAAAAAZhbW91bnQAAAAAAAsAAAABAAAD6QAAAAIAAAAD", "AAAAAAAAACpDdXJyZW50IHByb3RvY29sIGZlZSByYXRlIGluIGJhc2lzIHBvaW50cy4AAAAAAAdmZWVfYnBzAAAAAAAAAAABAAAD6QAAAAQAAAAD", "AAAAAAAABABNb3ZlIGEgbWFuZGF0ZSBvbnRvIGEgZGlmZmVyZW50IHBsYW4uCgpNb3ZpbmcgdG8gYSBtb3JlIGV4cGVuc2l2ZSBwbGFuIGNvbGxlY3RzIHRoZSBwcmljZSBkaWZmZXJlbmNlIGZvciB0aGUKcGFydCBvZiB0aGUgY3VycmVudCBwZXJpb2QgdGhhdCBpcyBzdGlsbCBhaGVhZCwgcHJvcmF0ZWQgYW5kIHJvdW5kZWQKaW4gdGhlIHN1YnNjcmliZXIncyBmYXZvdXIuIE1vdmluZyB0byBhIGNoZWFwZXIgcGxhbiB0YWtlcyBlZmZlY3QgYXQKdGhlIG5leHQgY2hhcmdlIGFuZCBuZXZlciByZWZ1bmRzIGFueXRoaW5nOiB0aGUgc3Vic2NyaWJlciBoYXMKYWxyZWFkeSBwYWlkIGZvciB0aGUgcGVyaW9kIHRoZXkgYXJlIGluLgoKVGhlIHJ1bGVzIHRoYXQga2VlcCB0aGlzIHNhZmU6CgotIE9ubHkgdGhlIHN1YnNjcmliZXIgbWF5IGNhbGwgaXQsIGFuZCBvbmx5IGZvciB0aGVpciBvd24gbWFuZGF0ZS4KLSBUaGUgbmV3IHBsYW4gbXVzdCBiZSBhY3RpdmUsIGRlbm9taW5hdGVkIGluIHRoZSBzYW1lIHRva2VuIChhCm1hbmRhdGUncyB0b2tlbiBpcyBmcm96ZW4gd2hlbiBpdCBpcyBvcGVuZWQpIGFuZCBiZWxvbmcgdG8gdGhlIHNhbWUKbWVyY2hhbnQgKHRoZSBtYW5kYXRlJ3MgbWVyY2hhbnQgZGVjaWRlcyB3aG8gaXMgcGFpZCBhbmQgd2hvIG1heQpyZWZ1bmQpLgotIFRoZSBhbW91bnQgY2hhcmdlZCBub3cgaXMgdGhlIHByaWNlIGRpZmZlcmVuY2UgbXVsdGlwbGllZCBieSB0aGUKZnJhY3Rpb24gb2YgdGhlIGN1cnJlbnQgcGVyaW9kIHRoYXQgaXMgbGVmdCwgdHJ1bmNhdGVkIHRvd2FyZHMgemVyby4KVGhlIHN1YnNjcmliZXIgdGhlcmVmb3JlIG5ldmVyIHBheXMgbW9yZSB0aGFuIHRoZSBleGFjdCBkaWZmZXJlbmNlLAphbmQgbmV2ZXIgbGVzcyB0aGFuIHplcm8uCi0gVGhlIGJpbGxpbmcgZGF0ZSBkb2VzIG5vdCBtb3ZlOiB0aGUgbmV4dCBjaGFyZ2Ugc3RpbGwgZmFsbHMgZHVlIGF0CmBuZXh0X2NoYXJnZWAsIGFuZCBjYXJyaWVzIHRoZSBuZXcgcGxhbidzIHByaWNlIGZyb20gdGhlbiBvbi4KClNlZSBbAAAAB3VwZ3JhZGUAAAAAAwAAAAAAAAAKc3Vic2NyaWJlcgAAAAAAEwAAAAAAAAAKbWFuZGF0ZV9pZAAAAAAABgAAAAAAAAALbmV3X3BsYW5faWQAAAAABgAAAAEAAAPpAAAAAgAAAAM=", "AAAAAAAAACVUaGUgdmVyc2lvbiBvZiB0aGlzIGNvbnRyYWN0J3MgY3JhdGUuAAAAAAAAB3ZlcnNpb24AAAAAAAAAAAEAAAAQ", "AAAAAAAAAtVDaGFuZ2UgdGhlIHNlYXQgY291bnQgb24gYW4gYWN0aXZlIG1hbmRhdGUuCgpTZWF0LWJhc2VkIHBsYW5zIHByaWNlIGJ5IHRoZSBudW1iZXIgb2YgdXNlcnMgb24gYSB0ZWFtLiBUaGlzIGZ1bmN0aW9uCmxldHMgdGhlIHN1YnNjcmliZXIgZ3JvdyBvciBzaHJpbmsgdGhlIHNlYXQgY291bnQ7IHRoZSBuZXcgY291bnQgdGFrZXMKZWZmZWN0IGZyb20gdGhlIG5leHQgY2hhcmdlIHNvIG5vIHByb3JhdGlvbiBpcyBuZWVkZWQuIE9ubHkgdGhlCnN1YnNjcmliZXIgbWF5IGNhbGwgaXQsIHRoZSBtYW5kYXRlIG11c3QgYmUgYWN0aXZlIChub3QgY2FuY2VsbGVkIG9yCmNvbXBsZXRlZCksIGFuZCBgc2VhdHNgIG11c3QgYmUgYXQgbGVhc3QgMS4KCiMgQXV0aApSZXF1aXJlcyBhdXRob3JpemF0aW9uIGZyb20gYHN1YnNjcmliZXJgLgoKIyBFdmVudHMKRW1pdHMgW2BNYW5kYXRlVXBkYXRlZGBdIGFuZCBbYFNlYXRzQ2hhbmdlZGBdLgoKIyBFcnJvcnMKUmV0dXJucyBbYEVycm9yOjpNYW5kYXRlTm90Rm91bmRgXSBpZiB0aGUgbWFuZGF0ZSBkb2VzIG5vdCBleGlzdCwKW2BFcnJvcjo6Tm90U3Vic2NyaWJlcmBdIGlmIHRoZSBjYWxsZXIgaXMgbm90IHRoZSBzdWJzY3JpYmVyLApbYEVycm9yOjpNYW5kYXRlTm90QWN0aXZlYF0gaWYgdGhlIG1hbmRhdGUgaXMgY2FuY2VsbGVkIG9yIGNvbXBsZXRlZCwgb3IKW2BFcnJvcjo6SW52YWxpZFNlYXRzYF0gaWYgYHNlYXRzYCBpcyB6ZXJvLgAAAAAAAAlzZXRfc2VhdHMAAAAAAAADAAAAAAAAAApzdWJzY3JpYmVyAAAAAAATAAAAAAAAAAptYW5kYXRlX2lkAAAAAAAGAAAAAAAAAAVzZWF0cwAAAAAAAAQAAAABAAAD6QAAAAIAAAAD", "AAAAAAAAAh9PcGVuIGEgbWFuZGF0ZSBhZ2FpbnN0IGEgcHVibGlzaGVkIHBsYW4uCgpUaGUgcGxhbidzIHRlcm1zIGFyZSBjb3BpZWQgaW50byB0aGUgbWFuZGF0ZSBoZXJlIGFuZCBuZXZlciByZS1yZWFkLCBzbwphIG1lcmNoYW50IGNhbm5vdCBjaGFuZ2UgdGhlIHByaWNlIG9mIGEgc3Vic2NyaXB0aW9uIGFmdGVyIHRoZSBmYWN0LgpUaGUgZmlyc3QgY2hhcmdlIGlzIGR1ZSBpbW1lZGlhdGVseTogdGhpcyBpcyBgc3Vic2NyaWJlX2F0KC4uLiwgbm93KWAuCgojIEF1dGgKUmVxdWlyZXMgYXV0aG9yaXphdGlvbiBmcm9tIGBzdWJzY3JpYmVyYC4KCiMgRXZlbnRzCkVtaXRzIFtgTWFuZGF0ZVVwZGF0ZWRgXSBhbmQgW2BTdWJzY3JpYmVkYF0uCgojIEVycm9ycwpSZXR1cm5zIFtgRXJyb3I6OkludmFsaWRNYXhUb3RhbGBdIGlmIGBtYXhfdG90YWwgPCAwYCwKW2BFcnJvcjo6UGxhbkluYWN0aXZlYF0gaWYgdGhlIHRhcmdldCBwbGFuIGlzIGluYWN0aXZlLCBvcgpbYEVycm9yOjpOb3RJbml0aWFsaXplZGBdIGlmIGNvbmZpZ3VyYXRpb24ga2V5cyBhcmUgbWlzc2luZy4AAAAACXN1YnNjcmliZQAAAAAAAAUAAAAAAAAACnN1YnNjcmliZXIAAAAAABMAAAAAAAAAB3BsYW5faWQAAAAABgAAAAAAAAALbWF4X2NoYXJnZXMAAAAABAAAAAAAAAAJbWF4X3RvdGFsAAAAAAAACwAAAAAAAAAIcmVmZXJyZXIAAAPoAAAAEwAAAAEAAAPpAAAABgAAAAM=", "AAAAAAAAAjNQYXVzZSBvciByZXN1bWUgYmlsbGluZyB3aXRob3V0IGxvc2luZyB0aGUgbWFuZGF0ZSdzIGhpc3RvcnkuCgpBbiBvcGVuLWVuZGVkIHBhdXNlOiBhIG1hbmRhdGUgcGF1c2VkIHRoaXMgd2F5IHN0YXlzIHBhdXNlZCB1bnRpbCBpdCBpcwpyZXN1bWVkIGhlcmUuIGBwYXVzZV91bnRpbGAgaXMgdGhlIGFsdGVybmF0aXZlIGZvciBhIHBhdXNlIHRoYXQgc2hvdWxkCmxpZnQgYnkgaXRzZWxmLgoKIyBBdXRoClJlcXVpcmVzIGF1dGhvcml6YXRpb24gZnJvbSBgc3Vic2NyaWJlcmAuCgojIEV2ZW50cwpFbWl0cyBbYE1hbmRhdGVVcGRhdGVkYF0gYW5kIFtgUGF1c2VDaGFuZ2VkYF0uCgojIEVycm9ycwpSZXR1cm5zIFtgRXJyb3I6Ok1hbmRhdGVOb3RGb3VuZGBdIGlmIHRoZSBtYW5kYXRlIGRvZXMgbm90IGV4aXN0LApbYEVycm9yOjpOb3RTdWJzY3JpYmVyYF0gaWYgdGhlIGNhbGxlciBpcyBub3QgdGhlIHN1YnNjcmliZXIsIG9yCltgRXJyb3I6Ok1hbmRhdGVOb3RBY3RpdmVgXSBpZiB0aGUgbWFuZGF0ZSBjYW5ub3QgYmUgdHJhbnNpdGlvbmVkIChlLmcuIGNhbmNlbGxlZC9jb21wbGV0ZWQpLgAAAAAKc2V0X3BhdXNlZAAAAAAAAwAAAAAAAAAKc3Vic2NyaWJlcgAAAAAAEwAAAAAAAAAKbWFuZGF0ZV9pZAAAAAAABgAAAAAAAAAGcGF1c2VkAAAAAAABAAAAAQAAA+kAAAACAAAAAw==", "AAAAAAAAAmZQZXJtYW5lbnRseSBjYW5jZWwgZXZlcnkgY2FuY2VsbGFibGUgbWFuZGF0ZSBpbiBvbmUgcGFnZSBvZiB0aGUKc3Vic2NyaWJlcidzIGluZGV4LiBUaGUgc2NhbiBpcyBjYXBwZWQgYXQgYE1BWF9QQUdFYDsgcmVhZGluZyBpbmRleApzbG90cyBkaXJlY3RseSBhdm9pZHMgdG91Y2hpbmcgdGhlIHNlcGFyYXRlIGNvdW50IGVudHJ5LCBrZWVwaW5nIGEKZnVsbCBwYWdlIHdpdGhpbiB0aGUgMTAwLWVudHJ5IHRyYW5zYWN0aW9uIGxpbWl0LgoKTWFuZGF0ZXMgYWxyZWFkeSBjYW5jZWxsZWQgb3IgY29tcGxldGVkIGFyZSBza2lwcGVkLCBzbyBjYWxsaW5nIGl0CnR3aWNlIGlzIGhhcm1sZXNzLiBSZXR1cm5zIGhvdyBtYW55IHRoaXMgY2FsbCBjYW5jZWxsZWQuIEVhY2gKW2BDYW5jZWxsZWRgXSBldmVudCByZXBvcnRzIHJlYXNvbiBbYENBTkNFTF9SRUFTT05fTk9ORWBdOiBhCnN1YnNjcmliZXIgd2hvIGNsZWFycyBvdXQgZXZlcnl0aGluZyBpcyBub3QgZXhwbGFpbmluZyBvbmUgYnkgb25lLgoKIyBBdXRoClJlcXVpcmVzIGF1dGhvcml6YXRpb24gZnJvbSBgc3Vic2NyaWJlcmAuCgojIEV2ZW50cwpFbWl0cyBvbmUgW2BDYW5jZWxsZWRgXSBwZXIgbWFuZGF0ZSBpdCBjYW5jZWxzLgAAAAAAC2NhbmNlbF9wYWdlAAAAAAMAAAAAAAAACnN1YnNjcmliZXIAAAAAABMAAAAAAAAABXN0YXJ0AAAAAAAABAAAAAAAAAAFbGltaXQAAAAAAAAEAAAAAQAAA+kAAAAEAAAAAw==", "AAAAAAAAAllFbmQgYSBtYW5kYXRlIGZyb20gdGhlIG1lcmNoYW50J3Mgc2lkZSwgZm9yIGV4YW1wbGUgd2hlbiB0aGUgc2VydmljZSBpcwpkaXNjb250aW51ZWQuIERlYWN0aXZhdGluZyBhIHBsYW4gb25seSBzdG9wcyBuZXcgc3Vic2NyaXB0aW9uczsgdGhpcyBpcwpob3cgYSBtZXJjaGFudCBzdG9wcyBiaWxsaW5nIHRoZSBvbmVzIGFscmVhZHkgb3Blbi4gTGlrZSBgY2FuY2VsYCwgaXQKaXMgcGVybWFuZW50LCBhbmQgaXQgY2FuIG9ubHkgZXZlciByZWR1Y2Ugd2hhdCB0aGUgc3Vic2NyaWJlciBwYXlzLgoKIyBBdXRoClJlcXVpcmVzIGF1dGhvcml6YXRpb24gZnJvbSBgbWVyY2hhbnRgLgoKIyBFdmVudHMKRW1pdHMgW2BNYW5kYXRlVXBkYXRlZGBdIGFuZCBbYE1hbmRhdGVFbmRlZGBdLgoKIyBFcnJvcnMKUmV0dXJucyBbYEVycm9yOjpNYW5kYXRlTm90Rm91bmRgXSBpZiB0aGUgbWFuZGF0ZSBkb2VzIG5vdCBleGlzdCwKW2BFcnJvcjo6Tm90TWVyY2hhbnRgXSBpZiB0aGUgY2FsbGVyIGlzIG5vdCB0aGUgbWVyY2hhbnQsIG9yCltgRXJyb3I6Ok1hbmRhdGVOb3RBY3RpdmVgXSBpZiB0aGUgbWFuZGF0ZSBpcyBhbHJlYWR5IGNhbmNlbGxlZCBvciBjb21wbGV0ZWQuAAAAAAAAC2VuZF9tYW5kYXRlAAAAAAIAAAAAAAAACG1lcmNoYW50AAAAEwAAAAAAAAAKbWFuZGF0ZV9pZAAAAAAABgAAAAEAAAPpAAAAAgAAAAM=", "AAAAAAAAACNMb29rIHVwIGEgbWFuZGF0ZSBieSBpdHMgdW5pcXVlIElELgAAAAALZ2V0X21hbmRhdGUAAAAAAQAAAAAAAAAKbWFuZGF0ZV9pZAAAAAAABgAAAAEAAAPpAAAH0AAAAAdNYW5kYXRlAAAAAAM=", "AAAAAAAAA8lQYXVzZSBiaWxsaW5nIHVudGlsIGEgZ2l2ZW4gZGF0ZSwgd2l0aG91dCBuZWVkaW5nIGEgdHJhbnNhY3Rpb24gZnJvbQp0aGUgc3Vic2NyaWJlciBhZnRlcndhcmRzLgoKVGhlIG1hbmRhdGUgaXMgcGF1c2VkIGxpa2UgYHNldF9wYXVzZWRgIGRvZXMsIGJ1dCBpdCBjYXJyaWVzIGEKc2NoZWR1bGU6IG9uY2UgdGhlIGxlZGdlciBjbG9jayByZWFjaGVzIGB1bnRpbGAsIGBjaGFyZ2VgIGFuZCBgaXNfZHVlYAp0cmVhdCBpdCBhcyBBY3RpdmUgYWdhaW4gb24gdGhlaXIgb3duLiBUaGUgcmVzdW1lIGlzIHRoZXJlZm9yZSBmcmVlIOKAlApubyBrZWVwZXIgd2luZG93IGlzIG1pc3NlZCBiZWNhdXNlIHRoZSBzdWJzY3JpYmVyIGZvcmdvdCB0byByZXN1bWUsCndoaWNoIGlzIHdoYXQgbG9zZXMgbWVyY2hhbnRzIHRoZWlyIHJldHVybmluZyBjdXN0b21lcnMuCgpgbmV4dF9jaGFyZ2VgIGlzIHB1c2hlZCB0byBhdCBsZWFzdCBgdW50aWxgLCBzbyBiaWxsaW5nIGNvbnRpbnVlcyBmcm9tCnRoZSBlbmQgb2YgdGhlIHBhdXNlIHJhdGhlciB0aGFuIGZyb20gYSBkYXRlIHRoYXQgaGFzIGFscmVhZHkgcGFzc2VkLgoKIyBBdXRoClJlcXVpcmVzIGF1dGhvcml6YXRpb24gZnJvbSBgc3Vic2NyaWJlcmAuCgojIEV2ZW50cwpFbWl0cyBbYE1hbmRhdGVVcGRhdGVkYF0gYW5kIFtgUGF1c2VDaGFuZ2VkYF0uCgojIEVycm9ycwpSZXR1cm5zIFtgRXJyb3I6Ok1hbmRhdGVOb3RGb3VuZGBdIGlmIHRoZSBtYW5kYXRlIGRvZXMgbm90IGV4aXN0LApbYEVycm9yOjpOb3RTdWJzY3JpYmVyYF0gaWYgdGhlIGNhbGxlciBpcyBub3QgdGhlIHN1YnNjcmliZXIsCltgRXJyb3I6Ok1hbmRhdGVOb3RBY3RpdmVgXSBpZiB0aGUgbWFuZGF0ZSBpcyBjYW5jZWxsZWQgb3IgY29tcGxldGVkLCBvcgpbYEVycm9yOjpJbnZhbGlkVW50aWxgXSBpZiBgdW50aWxgIGlzIGVhcmxpZXIgdGhhbiBvciBlcXVhbCB0byBjdXJyZW50IHRpbWVzdGFtcC4AAAAAAAALcGF1c2VfdW50aWwAAAAAAwAAAAAAAAAKc3Vic2NyaWJlcgAAAAAAEwAAAAAAAAAKbWFuZGF0ZV9pZAAAAAAABgAAAAAAAAAFdW50aWwAAAAAAAAGAAAAAQAAA+kAAAACAAAAAw==", "AAAAAAAAAbRDaGFuZ2UgdGhlIHByb3RvY29sIGZlZSBmb3IgbWFuZGF0ZXMgb3BlbmVkIGZyb20gbm93IG9uLgoKU2FmZSB0byBjYWxsIGF0IGFueSB0aW1lOiBldmVyeSBvcGVuIG1hbmRhdGUgY2FycmllcyB0aGUgZmVlIGl0IHdhcwpjcmVhdGVkIHdpdGgsIHNvIHRoaXMgY2Fubm90IHJlcHJpY2UgYW4gZXhpc3Rpbmcgc3Vic2NyaWJlci4gVGhlCmBNQVhfRkVFX0JQU2AgY2VpbGluZyBzdGlsbCBhcHBsaWVzLgoKIyBBdXRoClJlcXVpcmVzIGF1dGhvcml6YXRpb24gZnJvbSB0aGUgY29udHJhY3QgYWRtaW4uCgojIEV2ZW50cwpFbWl0cyBbYEZlZUNoYW5nZWRgXS4KCiMgRXJyb3JzClJldHVybnMgW2BFcnJvcjo6Tm90SW5pdGlhbGl6ZWRgXSBpZiBubyBhZG1pbiBpcyBzZXQsIG9yCltgRXJyb3I6OkZlZVRvb0hpZ2hgXSBpZiBgbmV3X2ZlZV9icHMgPiBNQVhfRkVFX0JQU2AuAAAAC3NldF9mZWVfYnBzAAAAAAEAAAAAAAAAC25ld19mZWVfYnBzAAAAAAQAAAABAAAD6QAAAAIAAAAD", "AAAAAAAAAbpBY2NlcHQgYSBwZW5kaW5nIGFkbWluIGhhbmRvdmVyICgjMjM4KS4gU3RlcCB0d28gb2YgdGhlIGhhbmRzaGFrZToKb25seSB0aGUgYWRkcmVzcyB0aGUgY3VycmVudCBhZG1pbiBwcm9wb3NlZCBtYXkgYWNjZXB0LCBhbmQgdGhlCnJvbGUgbW92ZXMgb25seSB3aGVuIGl0IGRvZXMuIFRoZSBwZW5kaW5nIHByb3Bvc2FsIGlzIGNvbnN1bWVkLCBzbwphIHNlY29uZCBhY2NlcHQgYnkgdGhlIHNhbWUgYWRkcmVzcyBmYWlscy4KCiMgQXV0aApSZXF1aXJlcyBhdXRob3JpemF0aW9uIGZyb20gYG5ld19hZG1pbmAuCgojIEVycm9ycwpSZXR1cm5zIFtgRXJyb3I6OlBlbmRpbmdBZG1pbk5vdEZvdW5kYF0gaWYgbm8gaGFuZG92ZXIgaXMgcGVuZGluZywKb3IgW2BFcnJvcjo6Tm90UGVuZGluZ0FkbWluYF0gaWYgYG5ld19hZG1pbmAgaXMgbm90IHRoZSBwcm9wb3NlZApzdWNjZXNzb3IuAAAAAAAMYWNjZXB0X2FkbWluAAAAAQAAAAAAAAAJbmV3X2FkbWluAAAAAAAAEwAAAAEAAAPpAAAAAgAAAAM=", "AAAAAAAAAfRJZHMgb2YgZHVlIG1hbmRhdGVzIG9wZW5lZCBhZ2FpbnN0IGBtZXJjaGFudGAncyBwbGFucywgaW4gaW5kZXgKb3JkZXIuIEEga2VlcGVyIHNldHRsaW5nIG9uZSBtZXJjaGFudCBvdGhlcndpc2UgaGFzIHRvIHJlYWQgZXZlcnkKbWFuZGF0ZSB0aGUgbWVyY2hhbnQgaGFzIGV2ZXIgaGFkIGFuZCB0ZXN0IGVhY2ggb25lIGl0c2VsZi4KClJlYWRzIGF0IG1vc3Qgb25lIHBhZ2Ugb2YgaW5kZXggc2xvdHMgYW5kIHRoZWlyIG1hbmRhdGUgZW50cmllcy4KSXQgY2Fubm90IGFmZm9yZCBhIHBsYW4tcmVnaXN0cnkgcmVhZCBwZXIgY2FuZGlkYXRlIHdpdGhpbiB0aGUKbGVkZ2VyJ3MgMTAwLWVudHJ5IGJ1ZGdldCwgc28gaXQgcmVwb3J0cyB0aGUgbWFuZGF0ZXMgd2hvc2Ugb3duCnNjaGVkdWxlIGlzIGR1ZTsgYGlzX2R1ZWAgYW5kIGBjaGFyZ2VgIGFkZCB0aGUgcGxhbi1wYXVzZSBjaGVjayBhbmQKYXJlIHRoZSBmaW5hbCBhdXRob3JpdHkgYmVmb3JlIHNldHRsZW1lbnQgKCMzMTUpLgAAAAxkdWVfbWFuZGF0ZXMAAAADAAAAAAAAAAhtZXJjaGFudAAAABMAAAAAAAAABXN0YXJ0AAAAAAAABAAAAAAAAAAFbGltaXQAAAAAAAAEAAAAAQAAA+oAAAAG", "AAAAAAAAAnRSZWFkLW9ubHkgcHJldmlldyBvZiB3aGF0IGBjaGFyZ2VgIHdvdWxkIHBheSByaWdodCBub3cuCgpgYW1vdW50YCBpcyB0aGUgc3Vic2NyaWJlcidzIHRvdGFsIGRlYml0LCBgZmVlYCB0aGUgcHJvdG9jb2wgc2hhcmUKYW5kIGBtZXJjaGFudF9hbW91bnRgIHRoZSBtZXJjaGFudCdzIG5ldCBzaGFyZSBhZnRlciBib3RoIHRoZSBmZWUKYW5kIHRoZSBrZWVwZXIgdGlwLiBgZHVlX2F0YCBpcyB0aGUgbWFuZGF0ZSdzIG5leHQgY2hhcmdlCnRpbWVzdGFtcCwgYW5kIGBjaGFyZ2VhYmxlYCBtaXJyb3JzIGBpc19kdWVgOiBhY3RpdmUgc3RhdHVzLCBjYXAKbm90IHJlYWNoZWQsIGFuZCB0aGUgbGVkZ2VyIHRpbWUgaGFzIHJlYWNoZWQgYGR1ZV9hdGAuIExpa2UKYGlzX2R1ZWAsIGl0IGRvZXMgbm90IGNoZWNrIHRoZSB2YXVsdCBiYWxhbmNlLiBRdW90aW5nIHJlYWRzCnN0b3JhZ2UgYnV0IG5ldmVyIHdyaXRlcyBpdC4KClRoZSBzcGxpdCBjb21lcyBmcm9tIHRoZSBzYW1lIHByaXZhdGUgaGVscGVyIGBjaGFyZ2VgIHVzZXMsIHNvIGEKY2xpZW50IGNhbiBkaXNwbGF5IHRoZSBleGFjdCBpbnRlZ2VycyBzZXR0bGVtZW50IHdpbGwgbW92ZSByYXRoZXIKdGhhbiByZWltcGxlbWVudGluZyB0aGUgcm91bmRpbmcuAAAADHF1b3RlX2NoYXJnZQAAAAEAAAAAAAAACm1hbmRhdGVfaWQAAAAAAAYAAAABAAAD6QAAB9AAAAALQ2hhcmdlUXVvdGUAAAAAAw==", "AAAAAAAAA8ZPcGVuIGEgbWFuZGF0ZSB3aG9zZSBmaXJzdCBjaGFyZ2UgZmFsbHMgZHVlIGF0IGBzdGFydGAgcmF0aGVyIHRoYW4Kbm93LCBzbyBhIHN1YnNjcmliZXIgY2FuIHNldCB1cCBiaWxsaW5nIGFoZWFkIG9mIHRpbWUuCgpgc3RhcnRgIG11c3QgYmUgdGhlIGN1cnJlbnQgbGVkZ2VyIHRpbWVzdGFtcCBvciBsYXRlciwgYW5kIG5vIG1vcmUKdGhhbiBgTUFYX1NUQVJUX09GRlNFVGAgKG9uZSB5ZWFyKSBhZnRlciBpdDogYSBtYW5kYXRlIG1heSBiZQpzY2hlZHVsZWQgaW4gYWR2YW5jZSwgYnV0IG5laXRoZXIgYmFja2RhdGVkIG5vciBsb2NrZWQgYXdheSBpbiB0aGUKZGlzdGFudCBmdXR1cmUuIEV2ZXJ5dGhpbmcgZWxzZSBpcyBpZGVudGljYWwgdG8gYHN1YnNjcmliZWAg4oCUIHRoZQpwbGFuJ3MgdGVybXMgYXJlIGNvcGllZCBpbnRvIHRoZSBtYW5kYXRlIGFuZCBuZXZlciByZS1yZWFkIOKAlCBhbmQKYFN1YnNjcmliZWRgIGNhcnJpZXMgYHN0YXJ0YCBhcyBgbmV4dF9jaGFyZ2VgLCBzbyBpbmRleGVycyBsZWFybiB3aGVuCmJpbGxpbmcgYmVnaW5zIGZyb20gdGhlIGV2ZW50IGFsb25lLgoKIyBBdXRoClJlcXVpcmVzIGF1dGhvcml6YXRpb24gZnJvbSBgc3Vic2NyaWJlcmAuCgojIEV2ZW50cwpFbWl0cyBbYE1hbmRhdGVVcGRhdGVkYF0gYW5kIFtgU3Vic2NyaWJlZGBdLgoKIyBFcnJvcnMKUmV0dXJucyBbYEVycm9yOjpJbnZhbGlkU3RhcnRgXSBpZiBgc3RhcnRgIGlzIGJlZm9yZSB0aGUgY3VycmVudApsZWRnZXIgdGltZXN0YW1wIG9yIG1vcmUgdGhhbiBgTUFYX1NUQVJUX09GRlNFVGAgYWZ0ZXIgaXQsCltgRXJyb3I6OkludmFsaWRNYXhUb3RhbGBdIGlmIGBtYXhfdG90YWwgPCAwYCwgW2BFcnJvcjo6UGxhbkluYWN0aXZlYF0KaWYgdGhlIHRhcmdldCBwbGFuIGlzIGluYWN0aXZlLCBvciBbYEVycm9yOjpOb3RJbml0aWFsaXplZGBdIGlmCmNvbmZpZ3VyYXRpb24ga2V5cyBhcmUgbWlzc2luZy4AAAAAAAxzdWJzY3JpYmVfYXQAAAAGAAAAAAAAAApzdWJzY3JpYmVyAAAAAAATAAAAAAAAAAdwbGFuX2lkAAAAAAYAAAAAAAAAC21heF9jaGFyZ2VzAAAAAAQAAAAAAAAACW1heF90b3RhbAAAAAAAAAsAAAAAAAAABXN0YXJ0AAAAAAAABgAAAAAAAAAIcmVmZXJyZXIAAAPoAAAAEwAAAAEAAAPpAAAABgAAAAM=", "AAAAAAAAAPdSZXBsYWNlIHRoaXMgY29udHJhY3QncyB3YXNtIHdpdGggY29kZSBhdCBgbmV3X3dhc21faGFzaGAuIE9ubHkgdGhlCmFkbWluIG1heSB1cGdyYWRlOyBwZXJzaXN0ZW50IHN0YXRlIGlzIHVudG91Y2hlZC4gKEZyb20gIzE1OC4pCgpSZWNvcmRzIHRoZSB1cGdyYWRlIGluIHRoZSBjb250cmFjdCdzIHVwZ3JhZGUgaGlzdG9yeSwgd2hpY2ggY2FuIGJlCnF1ZXJpZWQgdmlhIGB1cGdyYWRlX2hpc3RvcnlgIGZvciB0cmFuc3BhcmVuY3kuAAAAAAx1cGdyYWRlX3dhc20AAAABAAAAAAAAAA1uZXdfd2FzbV9oYXNoAAAAAAAD7gAAACAAAAABAAAD6QAAAAIAAAAD", "AAAAAAAAAVlSdW5zIG9uY2UsIGluIHRoZSBzYW1lIHRyYW5zYWN0aW9uIHRoYXQgZGVwbG95cyB0aGUgY29udHJhY3QuIFdpdGggYQpzZXBhcmF0ZSBgaW5pdGlhbGl6ZWAgY2FsbCwgYW55b25lIHdhdGNoaW5nIHRoZSBsZWRnZXIgY291bGQgY2FsbCBpdApiZXR3ZWVuIHRoZSBkZXBsb3kgYW5kIHRoZSBvd25lcidzIG93biBjYWxsIGFuZCBtYWtlIHRoZW1zZWx2ZXMgYWRtaW4uCgojIEF1dGgKTm9uZSAoaW52b2tlZCBkdXJpbmcgZGVwbG95bWVudCkuCgojIEV2ZW50cwpFbWl0cyBubyBldmVudHMuCgojIEVycm9ycwpSZXR1cm5zIFtgRXJyb3I6OkZlZVRvb0hpZ2hgXSBpZiBgZmVlX2JwcyA+IE1BWF9GRUVfQlBTYC4AAAAAAAANX19jb25zdHJ1Y3RvcgAAAAAAAAUAAAAAAAAABWFkbWluAAAAAAAAEwAAAAAAAAANcGxhbl9yZWdpc3RyeQAAAAAAABMAAAAAAAAABXZhdWx0AAAAAAAAEwAAAAAAAAAHZmVlX2JwcwAAAAAEAAAAAAAAAAZmZWVfdG8AAAAAABMAAAABAAAD6QAAAAIAAAAD", "AAAAAAAAAY9Fc3RpbWF0ZSB3aGVuIHRoZSBtYW5kYXRlJ3MgZmluYWwgY2FwcGVkIGNoYXJnZSBpcyBzY2hlZHVsZWQuCgpUaGUgZXN0aW1hdGUgZm9sbG93cyB0aGUgY3VycmVudCBzY2hlZHVsZSwgaW5jbHVkaW5nIGFueSBkcmlmdCBmcm9tCmxhdGUgY2hhcmdlcy4gUmV0dXJucyBgTm9uZWAgZm9yIGFuIG9wZW4tZW5kZWQgbWFuZGF0ZSBhbmQgaXRzIGFjdHVhbApmaW5hbCBjaGFyZ2UgdGltZSBvbmNlIHRoZSBjYXAgaGFzIGJlZW4gcmVhY2hlZC4KCiMgRXJyb3JzClJldHVybnMgW2BFcnJvcjo6TWFuZGF0ZU5vdEZvdW5kYF0gaWYgdGhlIG1hbmRhdGUgZG9lcyBub3QgZXhpc3QsIG9yCltgRXJyb3I6OlNjaGVkdWxlT3ZlcmZsb3dgXSBpZiB0aGUgcmVtYWluaW5nIHNjaGVkdWxlIG92ZXJmbG93cyBgdTY0YC4AAAAADWVzdGltYXRlZF9lbmQAAAAAAAABAAAAAAAAAAptYW5kYXRlX2lkAAAAAAAGAAAAAQAAA+kAAAPoAAAABgAAAAM=", "AAAAAAAAAIZJZHMgb2YgdGhlIG1hbmRhdGVzIG9wZW5lZCBhZ2FpbnN0IHBsYW4gYHBsYW5faWRgLCBvbGRlc3QgZmlyc3QsCnN0YXJ0aW5nIGF0IHBvc2l0aW9uIGBzdGFydGAuIFJldHVybnMgYXQgbW9zdCBgbWluKGxpbWl0LCBNQVhfUEFHRSlgLgAAAAAADXBsYW5fbWFuZGF0ZXMAAAAAAAADAAAAAAAAAAdwbGFuX2lkAAAAAAYAAAAAAAAABXN0YXJ0AAAAAAAABAAAAAAAAAAFbGltaXQAAAAAAAAEAAAAAQAAA+oAAAAG", "AAAAAAAAADFBZGRyZXNzIG9mIHRoZSBjb25maWd1cmVkIHBsYW4gcmVnaXN0cnkgY29udHJhY3QuAAAAAAAADXBsYW5fcmVnaXN0cnkAAAAAAAAAAAAAAQAAA+kAAAATAAAAAw==", "AAAAAAAAAd9Qcm9wb3NlIGhhbmRpbmcgdGhlIGFkbWluIHJvbGUgdG8gYG5ld19hZG1pbmAgKCMyMzgpLgoKVGhpcyBpcyBzdGVwIG9uZSBvZiBhIHR3by1zdGVwIGhhbmRvdmVyLiBUaGUgY3VycmVudCBhZG1pbiBrZWVwcwp0aGUgcm9sZSB1bnRpbCBgbmV3X2FkbWluYCBhY2NlcHRzIHdpdGggYGFjY2VwdF9hZG1pbmA7IHByb3Bvc2luZwphbG9uZSBjaGFuZ2VzIG5vIGF1dGhvcml0eSwgc28gYSBtaXN0eXBlZCBvciBob3N0aWxlIHByb3Bvc2FsIGNhbgpiZSByZXBsYWNlZCBiZWZvcmUgYW55b25lIGFjY2VwdHMgaXQuIEEgbGF0ZXIgcHJvcG9zYWwgdG8gYW5vdGhlcgphZGRyZXNzIG92ZXJ3cml0ZXMgdGhlIHBlbmRpbmcgb25lLgoKIyBBdXRoClJlcXVpcmVzIGF1dGhvcml6YXRpb24gZnJvbSB0aGUgY3VycmVudCBhZG1pbi4KCiMgRXJyb3JzClJldHVybnMgW2BFcnJvcjo6Tm90SW5pdGlhbGl6ZWRgXSBpZiBubyBhZG1pbiBpcyBjb25maWd1cmVkLgAAAAANcHJvcG9zZV9hZG1pbgAAAAAAAAEAAAAAAAAACW5ld19hZG1pbgAAAAAAABMAAAABAAAD6QAAAAIAAAAD", "AAAAAAAAAIpUaGUgaWQgdGhlIG5leHQgb3BlbmVkIG1hbmRhdGUgd2lsbCBnZXQuIE1hbmRhdGVzIGFyZSBudW1iZXJlZCBmcm9tIDEsCnNvIGBuZXh0X21hbmRhdGVfaWQoKSAtIDFgIGlzIHRoZSBudW1iZXIgb2YgbWFuZGF0ZXMgb3BlbmVkIHNvIGZhci4AAAAAAA9uZXh0X21hbmRhdGVfaWQAAAAAAAAAAAEAAAPpAAAABgAAAAM=", "AAAAAAAAADFXaGV0aGVyIHRoZSBhZG1pbiBoYXMgcGF1c2VkIHRoZSBwcm90b2NvbCAoIzI4NSkuAAAAAAAAD3Byb3RvY29sX3BhdXNlZAAAAAAAAAAAAQAAAAE=", "AAAAAAAAANNSYWlzZSBvciBsb3dlciBhIG1hbmRhdGUncyBgbWF4X2NoYXJnZXNgICgwID0gdW5saW1pdGVkKS4gT25seSB0aGUKc3Vic2NyaWJlciBtYXkgY2hhbmdlIGl0LCBhbmQgbm90IGJlbG93IHRoZSBjaGFyZ2VzIGFscmVhZHkgbWFkZS4KUmVhY2hpbmcgdGhlIG5ldyBjYXAgY29tcGxldGVzIHRoZSBtYW5kYXRlIGFuZCByZWxlYXNlcyBpdHMgc2VhdC4KKEZyb20gIzE1OC4pAAAAAA9zZXRfbWF4X2NoYXJnZXMAAAAAAwAAAAAAAAAKc3Vic2NyaWJlcgAAAAAAEwAAAAAAAAAKbWFuZGF0ZV9pZAAAAAAABgAAAAAAAAALbWF4X2NoYXJnZXMAAAAABAAAAAEAAAPpAAAAAgAAAAM=", "AAAAAAAAAoVQcmV2aWV3IHRoZSBleGFjdCByZWNpcGllbnRzIGFuZCBhbW91bnRzIHRoZSBuZXh0IGBjaGFyZ2VgIHdvdWxkCnRyYW5zZmVyLCB3aXRob3V0IHJlcXVpcmluZyBhdXRob3JpemF0aW9uIG9yIGNoYW5naW5nIHN0YXRlLgoKYGNoYXJnZWFibGVgIGNvdmVycyBwcm90b2NvbCBhbmQgcGxhbiBwYXVzZXMsIHRpbWluZywgbWFuZGF0ZSBzdGF0dXMKYW5kIHNwZW5kIGNhcHMuIExpa2UgW2BxdW90ZV9jaGFyZ2VgXShTZWxmOjpxdW90ZV9jaGFyZ2UpLCBpdCBkb2VzCm5vdCBjaGVjayBhdXRob3JpemF0aW9uIG9yIHZhdWx0IGZ1bmRzLiBga2VlcGVyYCBpcyB0aGUgcHJvcG9zZWQKY2hhcmdlIGNhbGxlciBhbmQgcmVjZWl2ZXMgdGhlIHRpcCBpZiBvbmUgaXMgZHVlLgoKIyBFcnJvcnMKUmV0dXJucyBbYEVycm9yOjpNYW5kYXRlTm90Rm91bmRgXSBpZiB0aGUgbWFuZGF0ZSBkb2VzIG5vdCBleGlzdCwKW2BFcnJvcjo6UGxhbk5vdEZvdW5kYF0gaWYgdGhlIHJlZ2lzdHJ5IGNhbm5vdCBwcm92aWRlIGl0cyBwYXVzZQpzdGF0ZSwKW2BFcnJvcjo6Tm90SW5pdGlhbGl6ZWRgXSBpZiBjb25maWd1cmF0aW9uIGlzIG1pc3NpbmcsIG9yCltgRXJyb3I6OkFtb3VudE92ZXJmbG93YF0gaWYgdGhlIHNwbGl0IGNhbGN1bGF0aW9uIG92ZXJmbG93cy4AAAAAAAAPc2ltdWxhdGVfY2hhcmdlAAAAAAIAAAAAAAAABmtlZXBlcgAAAAAAEwAAAAAAAAAKbWFuZGF0ZV9pZAAAAAAABgAAAAEAAAPpAAAH0AAAABBDaGFyZ2VTaW11bGF0aW9uAAAAAw==", "AAAAAAAAAPhHZXQgdGhlIGNvbnRyYWN0J3MgdXBncmFkZSBoaXN0b3J5LgoKUmV0dXJucyBhIHZlY3RvciBvZiB1cGdyYWRlIHJlY29yZHMsIG9sZGVzdCBmaXJzdC4gRWFjaCByZWNvcmQKY29udGFpbnMgdGhlIHZlcnNpb24sIGxlZGdlciBzZXF1ZW5jZSwgYW5kIHdhc20gaGFzaC4gVGhlIGluaXRpYWwKZGVwbG95bWVudCBpcyBub3QgcmVjb3JkZWQ7IG9ubHkgZXhwbGljaXQgdXBncmFkZXMgdmlhIGB1cGdyYWRlX3dhc21gCmFwcGVhciBoZXJlLgAAAA91cGdyYWRlX2hpc3RvcnkAAAAAAAAAAAEAAAPqAAAH0AAAAA1VcGdyYWRlUmVjb3JkAAAA", "AAAAAAAAAIpJZHMgb2YgdGhlIG1hbmRhdGVzIG9wZW5lZCBhZ2FpbnN0IGBtZXJjaGFudGAncyBwbGFucywgb2xkZXN0IGZpcnN0LApzdGFydGluZyBhdCBwb3NpdGlvbiBgc3RhcnRgLiBSZXR1cm5zIGF0IG1vc3QgYG1pbihsaW1pdCwgTUFYX1BBR0UpYC4AAAAAABFtZXJjaGFudF9tYW5kYXRlcwAAAAAAAAMAAAAAAAAACG1lcmNoYW50AAAAEwAAAAAAAAAFc3RhcnQAAAAAAAAEAAAAAAAAAAVsaW1pdAAAAAAAAAQAAAABAAAD6gAAAAY=", "AAAAAAAABABUZXJtaW5hdGUgYSBtYW5kYXRlIHBlcm1hbmVudGx5IGFuZCByZWNvcmQgd2h5LgoKQmVoYXZlIGV4YWN0bHkgbGlrZSBbYFN1YnNjcmlwdGlvbjo6Y2FuY2VsYF0g4oCUIHNhbWUgYXV0aG9yaXphdGlvbiwKc2FtZSBzdGF0ZSBjaGFuZ2UsIHNhbWUgZXZlbnRzIOKAlCBhbmQgYWRkaXRpb25hbGx5IHJlcG9ydCB0aGUKc3Vic2NyaWJlcidzIHJlYXNvbiBpbiB0aGUgW2BDYW5jZWxsZWRgXSBldmVudCwgc28gYSBtZXJjaGFudCBjYW4KbWVhc3VyZSBjaHVybiBmcm9tIGNoYWluIGRhdGEgYWxvbmUgaW5zdGVhZCBvZiBhc2tpbmcgaXRzIHVzZXJzLgoKVGhlIHJlYXNvbiBjYW5ub3QgY2hhbmdlIHdoYXQgdGhlIGNhbGwgZG9lczogdGhlIG1hbmRhdGUgaXMKY2FuY2VsbGVkIGVpdGhlciB3YXksIHNvIGEgY29kZSBjYW4gbmV2ZXIga2VlcCBhIG1hbmRhdGUgYWxpdmUsIG1vdmUKbW9uZXksIG9yIG1ha2UgYSBjaGFyZ2UgY29zdCBtb3JlLiBUaGUgc2V0IGlzIGNsb3NlZDoKCnwgQ29kZSB8IENvbnN0YW50IHwgTWVhbmluZyB8CnwtLS18LS0tfC0tLXwKfCAwIHwgW2BDQU5DRUxfUkVBU09OX05PTkVgXSB8IE5vIHJlYXNvbiBnaXZlbjsgd2hhdCBgY2FuY2VsYCBlbWl0cyB8CnwgMSB8IFtgQ0FOQ0VMX1JFQVNPTl9UT09fRVhQRU5TSVZFYF0gfCBUb28gZXhwZW5zaXZlIHwKfCAyIHwgW2BDQU5DRUxfUkVBU09OX05PVF9VU0lOR2BdIHwgTm8gbG9uZ2VyIHVzaW5nIHRoZSBzZXJ2aWNlIHwKfCAzIHwgW2BDQU5DRUxfUkVBU09OX01JU1NJTkdfRkVBVFVSRWBdIHwgTWlzc2luZyBhIG5lZWRlZCBmZWF0dXJlIHwKfCA0IHwgW2BDQU5DRUxfUkVBU09OX1NFUlZJQ0VfUVVBTElUWWBdIHwgVW5oYXBweSB3aXRoIHRoZSBzZXJ2aWNlIHwKfCA1IHwgW2BDQU5DRUxfUkVBU09OX09USEVSYF0gfCBTb21ldGhpbmcgZWxzZSB8CgojIEF1dGgKUmVxdWlyZXMgYXV0aG9yaXphdGlvbiBmcm9tIGBzdWJzY3JpYmVyYC4KCiMgRXZlbnRzCkVtaXRzIFtgTWFuZGF0ZVVwZGF0ZWRgXSBhbmQgW2BDYW5jZWxsZWRgXSwgAAAAEmNhbmNlbF93aXRoX3JlYXNvbgAAAAAAAwAAAAAAAAAKc3Vic2NyaWJlcgAAAAAAEwAAAAAAAAAKbWFuZGF0ZV9pZAAAAAAABgAAAAAAAAAGcmVhc29uAAAAAAAEAAAAAQAAA+kAAAACAAAAAw==", "AAAAAAAABABSZWNvcmQgdGhhdCBhIGR1ZSBjaGFyZ2UgZmFpbHMsIGFuZCBub3RpZnkgdGhlIG1lcmNoYW50IHRoZSBmaXJzdAp0aW1lIGl0IGRvZXMgKCMyOTYpLgoKQSBmYWlsZWQgYGNoYXJnZWAgaXMgcm9sbGVkIGJhY2sgdG9nZXRoZXIgd2l0aCBhbnkgZXZlbnQgaXQgd291bGQKZW1pdCwgc28gYSBrZWVwZXIgd2hvc2UgY2hhcmdlIGZhaWxzIGNhbGxzIHRoaXMgaW5zdGVhZC4gSXQgcHJvdmVzCnRoZSBmYWlsdXJlIG9uIGNoYWluIGJlZm9yZSByZWNvcmRpbmcgYW55dGhpbmc6IHRoZSBtYW5kYXRlIG11c3QgYmUKZHVlIGV4YWN0bHkgYXMgYGNoYXJnZWAgcmVxdWlyZXMsIGFuZCB0aGUgdmF1bHQgbXVzdCBiZSB1bmFibGUgdG8KY292ZXIgdGhlIGNoYXJnZSByaWdodCBub3cgKGJhbGFuY2UsIHRoaXMgbWFuZGF0ZSdzIHJlc2VydmUgYW5kIHRoZQpzdWJzY3JpYmVyJ3Mgc3BlbmQgbGltaXQgYWxsIGNvbnNpZGVyZWQpLiBBIGZsYWcgY2FuIHRoZXJlZm9yZQpuZXZlciBiZSByYWlzZWQgYWdhaW5zdCBhIG1hbmRhdGUgdGhhdCB3b3VsZCBoYXZlIHBhaWQuCgpUaGUgZmlyc3QgZmxhZyBpbiBhIHJ1biBvZiBmYWlsdXJlcyBzdG9yZXMgdGhlIHRpbWVzdGFtcCBhbmQgZW1pdHMKW2BDaGFyZ2VGYWlsZWRgXSB3aXRoIHRoZSBtZXJjaGFudCBhcyBhIHRvcGljLCBzbyBvZmYtY2hhaW4gZHVubmluZwpjYW4gcmVhY3QgcHJvbXB0bHkuIEZ1cnRoZXIgZmxhZ3MgcmV0dXJuIGBmYWxzZWAgYW5kIGVtaXQgbm90aGluZywKc28gcmV0cmllcyBjYW5ub3Qgc3BhbSB0aGUgbWVyY2hhbnQuIFRoZSBuZXh0IHN1Y2Nlc3NmdWwgYGNoYXJnZWAKZW5kcyB0aGUgcnVuOyB0aGUgZmFpbHVyZSBhZnRlciB0aGF0IG5vdGlmaWVzIGFnYWluLgoKUGVybWlzc2lvbmxlc3MsIGxpa2UgYGNoYXJnZWA6IGl0IG1vdmVzIG5vIG1vbmV5IGFuZCBvbmx5IHJlY29yZHMKYSBmYWN0IGFueW9uZSBjYW4gY2hlY2suCgojIEF1dGgKTm9uZS4KCiMgRXZlbnRzCkVtaXRzIFtgQ2hhcmdlRmFpbGVkYF0gb24gdGhlIGZpcnN0IGZsYWcgb2YgAAAAEmZsYWdfZmFpbGVkX2NoYXJnZQAAAAAAAQAAAAAAAAAKbWFuZGF0ZV9pZAAAAAAABgAAAAEAAAPpAAAAAQAAAAM=", "AAAAAAAAAw5XaGV0aGVyIGBtYW5kYXRlX2lkYCBpcyBpbnNpZGUgdGhlIGdyYWNlIHdpbmRvdyBpdCB3YXMgb3BlbmVkIHdpdGgKKCMzMTIpLCBzbyBhIHdhbGxldCBjYW4gd2FybiB0aGUgc3Vic2NyaWJlciBiZWZvcmUgdGhlIG1hbmRhdGUKbGFwc2VzLgoKVGhlIHdpbmRvdyBzdGFydHMgd2hlbiB0aGUgY2hhcmdlIGZhbGxzIGR1ZSAoYG5leHRfY2hhcmdlYCksIG5vdAp3aGVuIGEga2VlcGVyIGdldHMgYXJvdW5kIHRvIGZsYWdnaW5nIHRoZSBmYWlsdXJlLCBhbmQgbGFzdHMgZm9yIHRoZQptYW5kYXRlJ3MgZnJvemVuIGdyYWNlIHdpbmRvdy4gQSBtYW5kYXRlIHdpdGggYSB6ZXJvIHdpbmRvdyBpcyBuZXZlcgppbiBncmFjZS4gSW5kZWZpbml0ZWx5IHBhdXNlZCwgY2FuY2VsbGVkIGFuZCBjb21wbGV0ZWQgbWFuZGF0ZXMgYXJlCm5vdCBpbiBncmFjZSBlaXRoZXI7IGEgc2NoZWR1bGVkIHBhdXNlIHRoYXQgaGFzIHJ1biBpdHMgY291cnNlIGlzCnRyZWF0ZWQgYXMgYWN0aXZlLCBtYXRjaGluZyBgY2hhcmdlYC4KClRoZSB3aW5kb3cgaXMgaW5jbHVzaXZlIG9mIGl0cyBzdGFydCBhbmQgZXhjbHVzaXZlIG9mIGl0cyBlbmQ6IGF0CmV4YWN0bHkgYG5leHRfY2hhcmdlICsgZ3JhY2VfcGVyaW9kX3NlY3NgIHRoZSBncmFjZSBwZXJpb2QgaGFzCmVsYXBzZWQuIFRoaXMgaXMgYSBwdXJlIHJlYWQ6IG5vIHN0b3JhZ2Ugd3JpdGVzIGFuZCBubyBldmVudHMuCgojIEVycm9ycwpSZXR1cm5zIFtgRXJyb3I6Ok1hbmRhdGVOb3RGb3VuZGBdIGlmIHRoZSBtYW5kYXRlIGRvZXMgbm90IGV4aXN0LgAAAAAAEmlzX2luX2dyYWNlX3BlcmlvZAAAAAAAAQAAAAAAAAAKbWFuZGF0ZV9pZAAAAAAABgAAAAEAAAPpAAAAAQAAAAM=", "AAAAAAAAAEJOdW1iZXIgb2YgbWFuZGF0ZXMgZXZlciBvcGVuZWQgYWdhaW5zdCBwbGFuIGBwbGFuX2lkYCAoZnJvbSAjMTM0KS4AAAAAABJwbGFuX21hbmRhdGVfY291bnQAAAAAAAEAAAAAAAAAB3BsYW5faWQAAAAABgAAAAEAAAAE", "AAAAAAAAAKdXaGVuIHRoZSBtYW5kYXRlJ3MgY3VycmVudCBydW4gb2YgZmFpbGVkIGNoYXJnZXMgd2FzIGZpcnN0IGZsYWdnZWQKYnkgYGZsYWdfZmFpbGVkX2NoYXJnZWAsIG9yIGBOb25lYCBpZiBpdHMgbGFzdCBjaGFyZ2UgYXR0ZW1wdCBoYXMKbm90IGJlZW4gZmxhZ2dlZCBhcyBmYWlsZWQgKCMyOTYpLgAAAAATZmlyc3RfZmFpbGVkX2NoYXJnZQAAAAABAAAAAAAAAAptYW5kYXRlX2lkAAAAAAAGAAAAAQAAA+gAAAAG", "AAAAAAAAAt5QYXVzZSBvciByZXN1bWUgdGhlIHdob2xlIHByb3RvY29sIGR1cmluZyBhbiBpbmNpZGVudCAoIzI4NSkuCkFkbWluIG9ubHkuCgpXaGlsZSBwYXVzZWQsIGBzdWJzY3JpYmVgLCBgc3Vic2NyaWJlX2F0YCwgYHN1YnNjcmliZV93aXRoX2RlcG9zaXRgLApgY2hhcmdlYCBhbmQgYHVwZ3JhZGVgIGZhaWwgd2l0aCBbYEVycm9yOjpQcm90b2NvbFBhdXNlZGBdIGJlZm9yZQp0b3VjaGluZyBhbnkgc3RhdGUuIEV2ZXJ5dGhpbmcgdGhhdCBsZXRzIGEgc3Vic2NyaWJlciBvciBtZXJjaGFudApnZXQgb3V0IGtlZXBzIHdvcmtpbmc6IGV2ZXJ5IHZpZXcsIGBjYW5jZWxgLCBgY2FuY2VsX3BhZ2VgLApgZW5kX21hbmRhdGVgLCBwZXItbWFuZGF0ZSBwYXVzaW5nLCBgcmVmdW5kYCwgYW5kIHRoZSB2YXVsdCdzCndpdGhkcmF3YWxzICh0aGUgdmF1bHQgaXMgYSBzZXBhcmF0ZSBjb250cmFjdCBhbmQgbmV2ZXIgcmVhZHMgdGhpcwpzd2l0Y2gpLiBBIG1hbmRhdGUgdGhhdCBmYWxscyBkdWUgZHVyaW5nIHRoZSBwYXVzZSBpcyBub3QgbG9zdDogaXRzCmBuZXh0X2NoYXJnZWAgaXMgdW5jaGFuZ2VkLCBzbyBpdCBpcyBjaGFyZ2VhYmxlIGFzIHNvb24gYXMgdGhlCnByb3RvY29sIHJlc3VtZXMuCgpTZXR0aW5nIHRoZSBzd2l0Y2ggdG8gaXRzIGN1cnJlbnQgdmFsdWUgaXMgYSBuby1vcCBhbmQgZW1pdHMgbm8KZXZlbnQ7IG90aGVyd2lzZSBbYFByb3RvY29sUGF1c2VDaGFuZ2VkYF0gaXMgcHVibGlzaGVkLgAAAAAAE3NldF9wcm90b2NvbF9wYXVzZWQAAAAAAQAAAAAAAAAGcGF1c2VkAAAAAAABAAAAAQAAA+kAAAACAAAAAw==", "AAAAAAAAAIBJZHMgb2YgdGhlIG1hbmRhdGVzIGBzdWJzY3JpYmVyYCBvcGVuZWQsIG9sZGVzdCBmaXJzdCwgc3RhcnRpbmcgYXQKcG9zaXRpb24gYHN0YXJ0YC4gUmV0dXJucyBhdCBtb3N0IGBtaW4obGltaXQsIE1BWF9QQUdFKWAgaWRzLgAAABNzdWJzY3JpYmVyX21hbmRhdGVzAAAAAAMAAAAAAAAACnN1YnNjcmliZXIAAAAAABMAAAAAAAAABXN0YXJ0AAAAAAAABAAAAAAAAAAFbGltaXQAAAAAAAAEAAAAAQAAA+oAAAAG", "AAAAAAAAALJBY2NlcHQgYSBwZW5kaW5nIG1hbmRhdGUgb3duZXJzaGlwIHRyYW5zZmVyLiBGdXR1cmUgY2hhcmdlcyBhcmUgZGViaXRlZApmcm9tIHRoZSBuZXcgc3Vic2NyaWJlcidzIHZhdWx0IGJhbGFuY2U7IHRoZSBtZXJjaGFudCBpbmRleCBhbmQgYWxsCmZyb3plbiBtYW5kYXRlIHRlcm1zIHJlbWFpbiB1bmNoYW5nZWQuAAAAAAAUYWNjZXB0X21hbmRhdGVfb3duZXIAAAACAAAAAAAAAA5uZXdfc3Vic2NyaWJlcgAAAAAAEwAAAAAAAAAKbWFuZGF0ZV9pZAAAAAAABgAAAAEAAAPpAAAAAgAAAAM=", "AAAAAAAAAjxUb3RhbCBjb21taXR0ZWQgc3BlbmQgb2YgYHN1YnNjcmliZXJgIGluIGB0b2tlbmAgcGVyIGBwZXJpb2Rfc2Vjc2AsCm5vcm1hbGlzaW5nIG1hbmRhdGVzIHdpdGggZGlmZmVyZW50IGJpbGxpbmcgcGVyaW9kcyB0byB0aGUgcGVyaW9kIHRoZQpjYWxsZXIgYXNrcyBhYm91dC4gTW9udGhseSBhbmQgd2Vla2x5IG1hbmRhdGVzIHRoZXJlZm9yZSBhZGQgdXAgdG8gb25lCmZpZ3VyZSB0aGUgc3Vic2NyaWJlciBjYW4gZnVuZCB0aGVpciB2YXVsdCBhZ2FpbnN0LgoKT25seSBtYW5kYXRlcyB0aGF0IGBjaGFyZ2VgIHdvdWxkIHRyZWF0IGFzIGFjdGl2ZSBhcmUgY291bnRlZDoKY2FuY2VsbGVkLCBjb21wbGV0ZWQsIGluZGVmaW5pdGVseSBwYXVzZWQgYW5kIG5vdC15ZXQtZHVlLXNjaGVkdWxlZAptYW5kYXRlcyBjb250cmlidXRlIG5vdGhpbmcuIEF0IG1vc3QgYE1BWF9QQUdFYCBtYW5kYXRlcyAodGhlIGZpcnN0CnBhZ2Ugb2YgdGhlIHN1YnNjcmliZXIncyBpbmRleCkgYXJlIHJlYWQsIHNvIHRoZSBjYWxsIHN0YXlzIHdlbGwKaW5zaWRlIHRoZSBsZWRnZXIncyAxMDAtZW50cnkgZm9vdHByaW50IGxpbWl0LgAAABRjb21taXR0ZWRfcGVyX3BlcmlvZAAAAAMAAAAAAAAACnN1YnNjcmliZXIAAAAAABMAAAAAAAAABXRva2VuAAAAAAAAEwAAAAAAAAALcGVyaW9kX3NlY3MAAAAABgAAAAEAAAPpAAAACwAAAAM=", "AAAAAAAAAXNbYGVzdGltYXRlZF9lbmRgXShTZWxmOjplc3RpbWF0ZWRfZW5kKSBleHByZXNzZWQgYXMgYSB0aW1lc3RhbXAsCmFuIGV4cGVjdGVkIGxlZGdlciBzZXF1ZW5jZSBhbmQgYSBwZXJpb2QgY291bnQgKCMyODcpLCBzbyBVSXMgbmVlZApubyBleHRyYSBtYXRoLiBSZXR1cm5zIGBOb25lYCBmb3IgYW4gb3Blbi1lbmRlZCBtYW5kYXRlLgoKVGhlIGxlZGdlciBmaWd1cmUgYXNzdW1lcyBbYFNFQ09ORFNfUEVSX0xFREdFUmBdLXNlY29uZCBsZWRnZXJzIGFuZApzYXR1cmF0ZXMgYXQgYHUzMjo6TUFYYDsgaXQgaXMgYSBoaW50LCBub3QgYSBndWFyYW50ZWUuCgojIEVycm9ycwpTYW1lIGFzIFtgZXN0aW1hdGVkX2VuZGBdKFNlbGY6OmVzdGltYXRlZF9lbmQpLgAAAAAUZXN0aW1hdGVkX2VuZF9kZXRhaWwAAAABAAAAAAAAAAptYW5kYXRlX2lkAAAAAAAGAAAAAQAAA+kAAAPoAAAH0AAAAAxFc3RpbWF0ZWRFbmQAAAAD", "AAAAAAAAAMpHcmFjZSB3aW5kb3cgaW4gc2Vjb25kcyBmcm96ZW4gaW50byBgbWFuZGF0ZV9pZGAgd2hlbiBpdCB3YXMgb3BlbmVkCigjMzEyKS4gYDBgIG1lYW5zIHRoZSBwbGFuIGhhZCBubyBncmFjZSB3aW5kb3cgYXQgc2lnbi11cC4KCiMgRXJyb3JzClJldHVybnMgW2BFcnJvcjo6TWFuZGF0ZU5vdEZvdW5kYF0gaWYgdGhlIG1hbmRhdGUgZG9lcyBub3QgZXhpc3QuAAAAAAAUbWFuZGF0ZV9ncmFjZV9wZXJpb2QAAAABAAAAAAAAAAptYW5kYXRlX2lkAAAAAAAGAAAAAQAAA+kAAAAGAAAAAw==", "AAAAAAAAAH9OdW1iZXIgb2YgY3VycmVudGx5LW9wZW4gbWFuZGF0ZXMgYWdhaW5zdCBgcGxhbl9pZGA7IHRoZSBjb3VudCB0aGUKcGxhbidzIGBtYXhfc3Vic2NyaWJlcnNgIGNhcCBpcyBjaGVja2VkIGFnYWluc3QgKGZyb20gIzE1OCkuAAAAABVwbGFuX3N1YnNjcmliZXJfY291bnQAAAAAAAABAAAAAAAAAAdwbGFuX2lkAAAAAAYAAAABAAAABA==", "AAAAAAAAAPRQcm9wb3NlIG1vdmluZyBhIG1hbmRhdGUgdG8gYG5ld19zdWJzY3JpYmVyYC4gVGhlIGN1cnJlbnQgc3Vic2NyaWJlcgpzdXBwbGllcyB0aGUgcG9zaXRpb24gZnJvbSBgc3Vic2NyaWJlcl9tYW5kYXRlc2A7IHRoaXMgbGV0cyB0cmFuc2ZlcnMKdXBkYXRlIG9sZCBpbmRleGVzIGluIGNvbnN0YW50IHRpbWUsIGluY2x1ZGluZyBtYW5kYXRlcyBjcmVhdGVkIGJlZm9yZQpyZXZlcnNlLXBvc2l0aW9uIG1ldGFkYXRhIGV4aXN0ZWQuAAAAFXByb3Bvc2VfbWFuZGF0ZV9vd25lcgAAAAAAAAQAAAAAAAAACnN1YnNjcmliZXIAAAAAABMAAAAAAAAACm1hbmRhdGVfaWQAAAAAAAYAAAAAAAAADm5ld19zdWJzY3JpYmVyAAAAAAATAAAAAAAAABNzdWJzY3JpYmVyX3Bvc2l0aW9uAAAAAAQAAAABAAAD6QAAAAIAAAAD", "AAAAAAAAAD5Ib3cgbWFueSBtYW5kYXRlcyBoYXZlIGJlZW4gb3BlbmVkIGFnYWluc3QgYG1lcmNoYW50YCdzIHBsYW5zLgAAAAAAFm1lcmNoYW50X21hbmRhdGVfY291bnQAAAAAAAEAAAAAAAAACG1lcmNoYW50AAAAEwAAAAEAAAAE", "AAAAAAAAANBDb21taXR0ZWQgcmV2ZW51ZSBwZXIgcGVyaW9kIGZvciBgcGxhbl9pZGAuIFRoaXMgaXMgdGhlIHN1bSBvZiBhbGwKYWN0aXZlIG1hbmRhdGVzJyBwZXItcGVyaW9kIGFtb3VudHMsIG5vcm1hbGl6ZWQgdG8gdGhlIHBsYW4ncyBiaWxsaW5nCnBlcmlvZC4gUmV0dXJucyAwIGlmIHRoZSBwbGFuIGhhcyBubyBhY3RpdmUgbWFuZGF0ZXMgb3IgZG9lcyBub3QgZXhpc3QuAAAAFnBsYW5fY29tbWl0dGVkX3JldmVudWUAAAAAAAEAAAAAAAAAB3BsYW5faWQAAAAABgAAAAEAAAAL", "AAAAAAAAA5VGdW5kIHRoZSB2YXVsdCBhbmQgb3BlbiBhIG1hbmRhdGUgaW4gb25lIHRyYW5zYWN0aW9uLCBzbyBhIG5ldwpzdWJzY3JpYmVyIHNpZ25zIG9uY2UgaW5zdGVhZCBvZiBzaWduaW5nIGEgZGVwb3NpdCBhbmQgdGhlbiBhCnN1YnNjcmliZSDigJQgdGhlIGdhcCBiZXR3ZWVuIHRoZSB0d28gaXMgd2hlcmUgc2lnbi11cHMgd2VyZSBsb3N0LgoKVGhlIHN1YnNjcmliZXIncyBvbmUgYHJlcXVpcmVfYXV0aGAgYXQgdGhlIHRvcCBpcyB0aGUgb25seSBzaWduYXR1cmUKbmVlZGVkOiB0aGUgbmVzdGVkIGB2YXVsdC5kZXBvc2l0YCAoYW5kIHRoZSB0b2tlbiB0cmFuc2ZlciBpdCBtYWtlcykKcnVuIGFzIHN1Yi1pbnZvY2F0aW9ucyBvZiB0aGlzIHNhbWUgYXV0aG9yaXphdGlvbiwgYW5kIHRoZSBtYW5kYXRlCmlzIHRoZW4gY3JlYXRlZCB0aHJvdWdoIHRoZSBub3JtYWwgYHN1YnNjcmliZV9hdGAgcGF0aCAoc3RhcnQgPQpub3cpLCBzbyBpdCBjYXJyaWVzIHRoZSBmdWxsLCBjdXJyZW50IG1hbmRhdGUgdGVybXMuCgpBdG9taWMgYnkgY29uc3RydWN0aW9uOiBpZiB0aGUgZGVwb3NpdCBmYWlscyB0aGUgd2hvbGUgaW52b2NhdGlvbgphYm9ydHMgYW5kIG5vIG1hbmRhdGUgaXMgY3JlYXRlZDsgaWYgbWFuZGF0ZSBjcmVhdGlvbiB0aGVuIGZhaWxzLAp0aGUgZGVwb3NpdCBpcyByb2xsZWQgYmFjayB3aXRoIGl0LgoKKE1lcmdlZCBmcm9tICMxMzQsIGV4dGVuZGVkIHdpdGggYG1heF90b3RhbGAgZm9yIHBhcml0eSB3aXRoIHRoZQpjdXJyZW50IGBzdWJzY3JpYmVgLCB3aGljaCAjMTM0IHByZWRhdGVkLikKCiMgQXV0aApSZXF1aXJlcyBhdXRob3JpemF0aW9uIGZyb20gYHN1YnNjcmliZXJgLgoKIyBFcnJvcnMKQXMgYHN1YnNjcmliZWAsIHBsdXMgYW55IGVycm9yIHRoZSB2YXVsdCdzIGBkZXBvc2l0YCBjYW4gcmV0dXJuLgAAAAAAABZzdWJzY3JpYmVfd2l0aF9kZXBvc2l0AAAAAAAGAAAAAAAAAApzdWJzY3JpYmVyAAAAAAATAAAAAAAAAAdwbGFuX2lkAAAAAAYAAAAAAAAAC21heF9jaGFyZ2VzAAAAAAQAAAAAAAAACW1heF90b3RhbAAAAAAAAAsAAAAAAAAAB2RlcG9zaXQAAAAACwAAAAAAAAAIcmVmZXJyZXIAAAPoAAAAEwAAAAEAAAPpAAAABgAAAAM=", "AAAAAAAAADhIb3cgbWFueSBtYW5kYXRlcyBgc3Vic2NyaWJlcmAgaGFzIG9wZW5lZCwgaW4gYW55IHN0YXRlLgAAABhzdWJzY3JpYmVyX21hbmRhdGVfY291bnQAAAABAAAAAAAAAApzdWJzY3JpYmVyAAAAAAATAAAAAQAAAAQ=", "AAAAAAAAA9ZFeHRlbmQgdGhlIHN0b3JhZ2UgVFRMIG9mIGEgcGFnZSBvZiBgbWVyY2hhbnRgJ3Mgb3BlbiBtYW5kYXRlcyBpbgpvbmUgY2FsbCAoIzI4NCksIGluc3RlYWQgb2Ygb25lIHRyYW5zYWN0aW9uIHBlciBtYW5kYXRlLgoKV2Fsa3MgcG9zaXRpb25zIGBzdGFydCAuLiBzdGFydCArIG1pbihsaW1pdCwgTUFYX1BBR0UpYCBvZiB0aGUKbWVyY2hhbnQgaW5kZXggYW5kLCBmb3IgZXZlcnkgbWFuZGF0ZSB0aGF0IGlzIHN0aWxsIGBBY3RpdmVgIG9yCmBQYXVzZWRgLCBidW1wcyB0aGUgbWFuZGF0ZSBlbnRyeSBhbmQgaXRzIGluZGV4IHNsb3QgdG8gdGhlCnN0YW5kYXJkIHBlcnNpc3RlbnQgVFRMLiBUaGUgaW5kZXggY291bnQgaXMgYnVtcGVkIHRvbywgc28gdGhlCmluZGV4IHN0YXlzIHJlYWRhYmxlLiBDYW5jZWxsZWQgYW5kIGNvbXBsZXRlZCBtYW5kYXRlcyBhcmUgc2tpcHBlZDoKdGhleSBjYW4gbm8gbG9uZ2VyIGJlIGNoYXJnZWQsIHNvIGtlZXBpbmcgdGhlbSBhbGl2ZSBvbmx5IGNvc3RzIHJlbnQuCgpQZXJtaXNzaW9ubGVzczogZXh0ZW5kaW5nIGEgVFRMIGNoYW5nZXMgbm8gY29udHJhY3Qgc3RhdGUgdGhhdCBhbnkKcmVhZCBjYW4gb2JzZXJ2ZSwgYW5kIFNvcm9iYW4gbGV0cyBhbnlvbmUgZXh0ZW5kIGFueSBlbnRyeSBhbnl3YXkuCldvcmtzIHdoaWxlIHRoZSBwcm90b2NvbCBpcyBwYXVzZWQsIHNvIGEgbG9uZyBpbmNpZGVudCBjYW5ub3QgbGV0Cm1hbmRhdGVzIGV4cGlyZS4gUmV0dXJucyB0aGUgbnVtYmVyIG9mIG1hbmRhdGVzIGV4dGVuZGVkLgoKQ29zdDogb25lIGNhbGwgcmVhZHMgYXQgbW9zdCAxIGNvdW50ICsgNTAgc2xvdHMgKyA1MCBtYW5kYXRlcwooMTAxIGVudHJpZXMsIHRoZSB0cmFuc2FjdGlvbiBmb290cHJpbnQgbGltaXQpIGFuZCB3cml0ZXMgbm9uZSwgc28gYQprZWVwZXIgY292ZXJpbmcgTiBvcGVuIG1hbmRhdGVzIG5lZWRzIGBjZWlsKE4gLyA1MClgIHRyYW5zYWN0aW9ucwppbnN0ZWFkIG9mIE4uAAAAAAAcZXh0ZW5kX21lcmNoYW50X21hbmRhdGVzX3R0bAAAAAMAAAAAAAAACG1lcmNoYW50AAAAEwAAAAAAAAAFc3RhcnQAAAAAAAAEAAAAAAAAAAVsaW1pdAAAAAAAAAQAAAABAAAABA==", "AAAAAAAAAP1TdW1tYXJpZXMgb2YgdGhlIG1hbmRhdGVzIGBzdWJzY3JpYmVyYCBvcGVuZWQsIG9sZGVzdCBmaXJzdCwgc3RhcnRpbmcgYXQKcG9zaXRpb24gYHN0YXJ0YC4gUmV0dXJucyBhdCBtb3N0IGBtaW4obGltaXQsIE1BWF9QQUdFKWAgc3VtbWFyaWVzLgpFYWNoIHN1bW1hcnkgY29udGFpbnMgdGhlIGtleSBmaWVsZHMgYSB3YWxsZXQgbmVlZHMgdG8gZGlzcGxheSBhCnN1YnNjcmlwdGlvbiB3aXRob3V0IGZldGNoaW5nIHRoZSBmdWxsIG1hbmRhdGUuAAAAAAAAHHN1YnNjcmliZXJfbWFuZGF0ZV9zdW1tYXJpZXMAAAADAAAAAAAAAApzdWJzY3JpYmVyAAAAAAATAAAAAAAAAAVzdGFydAAAAAAAAAQAAAAAAAAABWxpbWl0AAAAAAAABAAAAAEAAAPqAAAH0AAAAA5NYW5kYXRlU3VtbWFyeQAA"]),
      options
    );
  }

   static deploy<T = Client>(args: { admin: string | Address; plan_registry: string | Address; vault: string | Address; fee_bps: number; fee_to: string | Address }, options: MethodOptions & Omit<ContractClientOptions, 'contractId'> & { salt?: Uint8Array; address?: string; } & ({ wasmHash: Uint8Array | string; format?: "hex" | "base64"; externalRef?: never; } | { externalRef: ExternalExecutableRef; wasmHash?: never; format?: never; })): Promise<AssembledTransaction<T>> {
    return ContractClient.deploy(args, options);
  }
  public readonly fromJson = {
    admin : this.txFromJson<Result<string, Error>>,  vault : this.txFromJson<Result<string, Error>>,  cancel : this.txFromJson<Result<null, Error>>,  charge : this.txFromJson<Result<ChargeResult, Error>>,  is_due : this.txFromJson<Result<boolean, Error>>,  refund : this.txFromJson<Result<null, Error>>,  fee_bps : this.txFromJson<Result<number, Error>>,  upgrade : this.txFromJson<Result<null, Error>>,  version : this.txFromJson<string>,  set_seats : this.txFromJson<Result<null, Error>>,  subscribe : this.txFromJson<Result<bigint, Error>>,  set_paused : this.txFromJson<Result<null, Error>>,  cancel_page : this.txFromJson<Result<number, Error>>,  end_mandate : this.txFromJson<Result<null, Error>>,  get_mandate : this.txFromJson<Result<Mandate, Error>>,  pause_until : this.txFromJson<Result<null, Error>>,  set_fee_bps : this.txFromJson<Result<null, Error>>,  accept_admin : this.txFromJson<Result<null, Error>>,  due_mandates : this.txFromJson<Array<bigint>>,  quote_charge : this.txFromJson<Result<ChargeQuote, Error>>,  subscribe_at : this.txFromJson<Result<bigint, Error>>,  upgrade_wasm : this.txFromJson<Result<null, Error>>,  estimated_end : this.txFromJson<Result<bigint | null, Error>>,  plan_mandates : this.txFromJson<Array<bigint>>,  plan_registry : this.txFromJson<Result<string, Error>>,  propose_admin : this.txFromJson<Result<null, Error>>,  next_mandate_id : this.txFromJson<Result<bigint, Error>>,  protocol_paused : this.txFromJson<boolean>,  set_max_charges : this.txFromJson<Result<null, Error>>,  simulate_charge : this.txFromJson<Result<ChargeSimulation, Error>>,  upgrade_history : this.txFromJson<Array<UpgradeRecord>>,  merchant_mandates : this.txFromJson<Array<bigint>>,  cancel_with_reason : this.txFromJson<Result<null, Error>>,  flag_failed_charge : this.txFromJson<Result<boolean, Error>>,  is_in_grace_period : this.txFromJson<Result<boolean, Error>>,  plan_mandate_count : this.txFromJson<number>,  first_failed_charge : this.txFromJson<bigint | null>,  set_protocol_paused : this.txFromJson<Result<null, Error>>,  subscriber_mandates : this.txFromJson<Array<bigint>>,  accept_mandate_owner : this.txFromJson<Result<null, Error>>,  committed_per_period : this.txFromJson<Result<bigint, Error>>,  estimated_end_detail : this.txFromJson<Result<EstimatedEnd | null, Error>>,  mandate_grace_period : this.txFromJson<Result<bigint, Error>>,  plan_subscriber_count : this.txFromJson<number>,  propose_mandate_owner : this.txFromJson<Result<null, Error>>,  merchant_mandate_count : this.txFromJson<number>,  plan_committed_revenue : this.txFromJson<bigint>,  subscribe_with_deposit : this.txFromJson<Result<bigint, Error>>,  subscriber_mandate_count : this.txFromJson<number>,  extend_merchant_mandates_ttl : this.txFromJson<number>,  subscriber_mandate_summaries : this.txFromJson<Array<MandateSummary>>
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
   * Build a topics filter row for the "Charged" event, for use in `Api.EventFilter.topics` when calling `server.getEvents`. Omitted fields match any value.
   */
  chargedEventFilter(topicValues?: { mandate_id?: bigint; subscriber?: string | Address; merchant?: string | Address }): string[] {
    return this.spec.eventTopicFilter("Charged", topicValues);
  }
  /**
   * Build a topics filter row for the "Refunded" event, for use in `Api.EventFilter.topics` when calling `server.getEvents`. Omitted fields match any value.
   */
  refundedEventFilter(topicValues?: { mandate_id?: bigint; subscriber?: string | Address; merchant?: string | Address }): string[] {
    return this.spec.eventTopicFilter("Refunded", topicValues);
  }
  /**
   * Build a topics filter row for the "Upgraded" event, for use in `Api.EventFilter.topics` when calling `server.getEvents`. Omitted fields match any value.
   */
  upgradedEventFilter(topicValues?: { mandate_id?: bigint; subscriber?: string | Address; merchant?: string | Address }): string[] {
    return this.spec.eventTopicFilter("Upgraded", topicValues);
  }
  /**
   * Build a topics filter row for the "Cancelled" event, for use in `Api.EventFilter.topics` when calling `server.getEvents`. Omitted fields match any value.
   */
  cancelledEventFilter(topicValues?: { mandate_id?: bigint; subscriber?: string | Address }): string[] {
    return this.spec.eventTopicFilter("Cancelled", topicValues);
  }
  /**
   * Build a topics filter row for the "FeeChanged" event, for use in `Api.EventFilter.topics` when calling `server.getEvents`. Omitted fields match any value.
   */
  feeChangedEventFilter(topicValues?: { admin?: string | Address }): string[] {
    return this.spec.eventTopicFilter("FeeChanged", topicValues);
  }
  /**
   * Build a topics filter row for the "Subscribed" event, for use in `Api.EventFilter.topics` when calling `server.getEvents`. Omitted fields match any value.
   */
  subscribedEventFilter(topicValues?: { mandate_id?: bigint; subscriber?: string | Address; merchant?: string | Address }): string[] {
    return this.spec.eventTopicFilter("Subscribed", topicValues);
  }
  /**
   * Build a topics filter row for the "ChargeFailed" event, for use in `Api.EventFilter.topics` when calling `server.getEvents`. Omitted fields match any value.
   */
  chargeFailedEventFilter(topicValues?: { mandate_id?: bigint; merchant?: string | Address; subscriber?: string | Address }): string[] {
    return this.spec.eventTopicFilter("ChargeFailed", topicValues);
  }
  /**
   * Build a topics filter row for the "MandateEnded" event, for use in `Api.EventFilter.topics` when calling `server.getEvents`. Omitted fields match any value.
   */
  mandateEndedEventFilter(topicValues?: { mandate_id?: bigint; merchant?: string | Address }): string[] {
    return this.spec.eventTopicFilter("MandateEnded", topicValues);
  }
  /**
   * Build a topics filter row for the "PauseChanged" event, for use in `Api.EventFilter.topics` when calling `server.getEvents`. Omitted fields match any value.
   */
  pauseChangedEventFilter(topicValues?: { mandate_id?: bigint; subscriber?: string | Address }): string[] {
    return this.spec.eventTopicFilter("PauseChanged", topicValues);
  }
  /**
   * Build a topics filter row for the "SeatsChanged" event, for use in `Api.EventFilter.topics` when calling `server.getEvents`. Omitted fields match any value.
   */
  seatsChangedEventFilter(topicValues?: { mandate_id?: bigint; subscriber?: string | Address }): string[] {
    return this.spec.eventTopicFilter("SeatsChanged", topicValues);
  }
  /**
   * Build a topics filter row for the "MandateUpdated" event, for use in `Api.EventFilter.topics` when calling `server.getEvents`. Omitted fields match any value.
   */
  mandateUpdatedEventFilter(topicValues?: { mandate_id?: bigint }): string[] {
    return this.spec.eventTopicFilter("MandateUpdated", topicValues);
  }
  /**
   * Build a topics filter row for the "ContractUpgraded" event, for use in `Api.EventFilter.topics` when calling `server.getEvents`. Omitted fields match any value.
   */
  contractUpgradedEventFilter(topicValues?: { admin?: string | Address }): string[] {
    return this.spec.eventTopicFilter("ContractUpgraded", topicValues);
  }
  /**
   * Build a topics filter row for the "MandateCompleted" event, for use in `Api.EventFilter.topics` when calling `server.getEvents`. Omitted fields match any value.
   */
  mandateCompletedEventFilter(topicValues?: { mandate_id?: bigint; subscriber?: string | Address }): string[] {
    return this.spec.eventTopicFilter("MandateCompleted", topicValues);
  }
  /**
   * Build a topics filter row for the "MaxChargesChanged" event, for use in `Api.EventFilter.topics` when calling `server.getEvents`. Omitted fields match any value.
   */
  maxChargesChangedEventFilter(topicValues?: { mandate_id?: bigint; subscriber?: string | Address }): string[] {
    return this.spec.eventTopicFilter("MaxChargesChanged", topicValues);
  }
  /**
   * Build a topics filter row for the "MandateTransferred" event, for use in `Api.EventFilter.topics` when calling `server.getEvents`. Omitted fields match any value.
   */
  mandateTransferredEventFilter(topicValues?: { mandate_id?: bigint; old_subscriber?: string | Address }): string[] {
    return this.spec.eventTopicFilter("MandateTransferred", topicValues);
  }
  /**
   * Build a topics filter row for the "ProtocolPauseChanged" event, for use in `Api.EventFilter.topics` when calling `server.getEvents`. Omitted fields match any value.
   */
  protocolPauseChangedEventFilter(topicValues?: { admin?: string | Address }): string[] {
    return this.spec.eventTopicFilter("ProtocolPauseChanged", topicValues);
  }
  /**
   * Build a topics filter row for the "CommittedRevenueChanged" event, for use in `Api.EventFilter.topics` when calling `server.getEvents`. Omitted fields match any value.
   */
  committedRevenueChangedEventFilter(topicValues?: { plan_id?: bigint; merchant?: string | Address }): string[] {
    return this.spec.eventTopicFilter("CommittedRevenueChanged", topicValues);
  }
}