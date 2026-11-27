import {Address} from '@stellar/stellar-sdk';

    /**
 * Errors returned by the subscription contract.
 */
export const Error = {
  /**
   * The contract has already been initialized.
   */
  2001 : { message: "AlreadyInitialized" },
  /**
   * The contract has not been initialized with required configuration.
   */
  2002 : { message: "NotInitialized" },
  /**
   * The requested mandate ID does not exist in storage.
   */
  2003 : { message: "MandateNotFound" },
  /**
   * The caller is not the subscriber who opened the mandate.
   */
  2004 : { message: "NotSubscriber" },
  /**
   * The mandate is not active (paused, cancelled, or completed).
   */
  2005 : { message: "MandateNotActive" },
  /**
   * The current ledger timestamp is earlier than the mandate's next charge time.
   */
  2006 : { message: "NotDue" },
  /**
   * The plan is inactive and does not accept subscriptions or upgrades.
   */
  2007 : { message: "PlanInactive" },
  /**
   * The requested protocol fee in basis points exceeds `MAX_FEE_BPS`.
   */
  2008 : { message: "FeeTooHigh" },
  /**
   * The mandate has reached its maximum charge count.
   */
  2009 : { message: "MaxChargesReached" },
  /**
   * The maximum charge count parameter is invalid.
   */
  2010 : { message: "InvalidMaxCharges" },
  /**
   * The caller is not the merchant of the mandate.
   */
  2011 : { message: "NotMerchant" },
  /**
   * Fee or tip math would overflow `i128` at this amount.
   */
  2012 : { message: "AmountOverflow" },
  /**
   * `pause_until` was given a date that has already passed. Pausing must
   * move billing forwards, never backwards.
   */
  2013 : { message: "InvalidUntil" },
  /**
   * A refund larger than what the merchant has collected for this mandate.
   */
  2014 : { message: "RefundExceedsReceived" },
  /**
   * A refund amount that is zero or negative.
   */
  2015 : { message: "InvalidAmount" },
  /**
   * The plan a mandate is being moved to is denominated in another token.
   * A mandate's token is frozen when it is opened, so an upgrade can never
   * change which asset the subscriber pays in.
   */
  2016 : { message: "DifferentToken" },
  /**
   * The plan a mandate is being moved to belongs to another merchant.
   * Moving between merchants is a cancel and a new subscription, not an
   * upgrade: the mandate's merchant decides who is paid and who may
   * refund.
   */
  2017 : { message: "DifferentMerchant" },
  /**
   * The estimated end timestamp for a mandate's remaining schedule overflows `u64`.
   */
  2018 : { message: "ScheduleOverflow" },
  /**
   * The seat count is zero, or exceeds the plan's `max_seats`.
   */
  2019 : { message: "InvalidSeats" },
  /**
   * The seat count was supplied for a plan that is not seat-based, or was
   * omitted for one that is.
   */
  2020 : { message: "SeatsNotApplicable" },
  /**
   * `price * seats` overflows `i128`.
   */
  2021 : { message: "SeatsOverflow" },
  /**
   * A total-spend cap that is negative.
   */
  2022 : { message: "InvalidMaxTotal" },
  /**
   * The pause has lasted longer than the maximum pause length frozen into
   * the mandate, so it can be expired.
   */
  2023 : { message: "PauseNotExpired" },
  /**
   * The pause has exceeded its maximum length and must be expired with
   * `expire_pause`, not resumed or extended.
   */
  2024 : { message: "PauseExpired" },
  /**
   * A batch subscription was given more plans than `MAX_BATCH_PLANS`.
   */
  2025 : { message: "BatchTooLarge" },
  /**
   * A batch subscription was given no plans.
   */
  2026 : { message: "EmptyBatch" },
  /**
   * `subscribe_at` was given a `start` before the current ledger timestamp
   * or more than `MAX_START_OFFSET` (one year) after it.
   */
  2027 : { message: "InvalidStart" },
  /**
   * A cancellation reason outside the documented `CANCEL_REASON_*` code
   * set was supplied. The set is closed so an indexer can map every
   * `Cancelled` event it will ever see without a catch-all branch.
   */
  2028 : { message: "InvalidCancelReason" },
  /**
   * The caller gave a position in `subscriber_mandates` that does not hold
   * this mandate, so the transfer cannot update the old index in place.
   */
  2029 : { message: "InvalidSubscriberIndex" },
  /**
   * `accept_mandate_owner` was called for a mandate with no pending
   * transfer, or after the transfer was already accepted.
   */
  2030 : { message: "NoPendingTransfer" },
  /**
   * The address accepting the transfer is not the one the current
   * subscriber proposed in `propose_mandate_owner`.
   */
  2031 : { message: "NotProposedOwner" },
  /**
   * A transfer was proposed to the mandate's current subscriber; a move to
   * the same address is a no-op and is rejected.
   */
  2032 : { message: "SameSubscriber" },
  /**
   * The new subscriber already holds the maximum number of mandates its
   * index can track.
   */
  2033 : { message: "MandateIndexFull" },
  /**
   * A new subscription was rejected because the plan is at its
   * `max_subscribers` seat cap.
   */
  2034 : { message: "PlanFull" },
  /**
   * `flag_failed_charge` was called for a due mandate whose charge would
   * succeed right now: the subscriber's vault covers it. Only a charge
   * that would really fail can be flagged (#296).
   */
  2035 : { message: "ChargeWouldSucceed" },
  /**
   * The plan registry did not return a plan for the requested ID.
   */
  2036 : { message: "PlanNotFound" },
  /**
   * The vault rejected a deposit or debit. The subscription call fails
   * without changing mandate state; the vault's own error code is preserved
   * in the invocation diagnostics.
   */
  2037 : { message: "VaultCallFailed" },
  /**
   * The mandate's plan has charging paused by its merchant (#315). The
   * mandate itself is still open; retry after the merchant resumes.
   */
  2038 : { message: "PlanChargesPaused" },
  /**
   * The admin has paused the protocol (#285). New subscriptions, charges
   * and plan upgrades are refused until it is resumed; reads,
   * cancellations, refunds and vault withdrawals are unaffected.
   */
  2039 : { message: "ProtocolPaused" },
  /**
   * A refund transfer did not deliver exactly the recorded refund amount,
   * so the token is unsafe for exact-amount accounting (for example a
   * fee-on-transfer token). The refund is rolled back.
   */
  2040 : { message: "UnexpectedTransferAmount" },
  /**
   * `accept_admin` was called with no pending admin handover, or after the
   * proposal was already accepted (#238).
   */
  2041 : { message: "PendingAdminNotFound" },
  /**
   * The address accepting the admin role is not the one the current admin
   * proposed in `propose_admin` (#238).
   */
  2042 : { message: "NotPendingAdmin" },
  /**
   * The requested mandate ID has been tombstoned after deletion and cannot
   * be reused (#368).
   */
  2043 : { message: "MandateIdTombstoned" },
  /**
   * The requested mandate ID is already assigned to an existing mandate (#368).
   */
  2044 : { message: "MandateAlreadyExists" },
  /**
   * The requested mandate ID is invalid (mandate IDs start at 1) (#368).
   */
  2045 : { message: "InvalidMandateId" },
  /**
   * An active or paused mandate cannot be deleted until it is cancelled or
   * completed (#368).
   */
  2046 : { message: "MandateStillActive" },
  /**
   * A configured address is the contract itself or otherwise invalid for
   * this role (#282).
   */
  2047 : { message: "InvalidAddress" },
  /**
   * `flag_failed_charge` was called inside the plan's frozen retry window
   * (#343). The charge may still succeed, so no failure is recorded until
   * the window closes.
   */
  2048 : { message: "RetryWindowActive" },
  /**
   * The subscriber's unreserved vault balance cannot cover one period's
   * charge for a new mandate.
   */
  2049 : { message: "InsufficientBalance" }
}

/**
 * A standing authorization from a subscriber to a merchant, bounded in
 * amount, frequency, total number of charges and total spend.
 */
export interface Mandate {
  /**
   * Frozen at subscribe time. A later plan edit cannot reprice this mandate.
   */
  amount: bigint;
  /**
   * Number of charges successfully executed so far.
   */
  charges_made: number;
  /**
   * Unix seconds when the mandate was opened.
   */
  created_at: bigint;
  /**
   * Protocol fee in basis points, frozen at subscribe time. An admin fee
   * change therefore cannot alter the economics of a mandate that is
   * already open — the same guarantee the plan's price already has.
   */
  fee_bps: number;
  /**
   * Unique identifier of the mandate.
   */
  id: bigint;
  /**
   * Keeper tip in basis points of the merchant share, frozen at subscribe
   * time for the same reason as `fee_bps`: a later plan cannot raise what an
   * open mandate pays whoever settles it.
   */
  keeper_bps: number;
  /**
   * Unix seconds when the most recent charge was executed, or 0 if none.
   */
  last_charge: bigint;
  /**
   * 0 means open-ended.
   */
  max_charges: number;
  /**
   * Maximum continuous pause length in seconds, frozen at subscribe time
   * from the plan's `max_pause_secs`. 0 means unlimited. Mandates opened
   * before this field existed decode to 0 and are therefore unlimited, so
   * they are unaffected.
   */
  max_pause_secs: bigint;
  /**
   * Ceiling on everything this mandate may ever debit, frozen when the
   * subscriber opened it. 0 means open-ended. Mandates opened before this
   * field existed decode to 0 and are therefore uncapped.
   */
  max_total: bigint;
  /**
   * Merchant receiving payment under this mandate.
   */
  merchant: string;
  /**
   * Unix seconds. Chargeable once the ledger timestamp reaches this.
   */
  next_charge: bigint;
  /**
   * Unix seconds when a scheduled pause (`pause_until`) lifts by itself.
   * 0 means no schedule: a paused mandate stays paused until `set_paused`
   * resumes it. Only meaningful while `status` is `Paused`.
   */
  paused_until: bigint;
  /**
   * Account that receives the merchant's share of each charge, frozen from
   * the plan's `payout` at sign-up. A later payout change or a plan-owner
   * transfer never redirects this open mandate's funds.
   */
  payout: string;
  /**
   * Billing period in seconds between charges.
   */
  period: bigint;
  /**
   * ID of the plan against which this mandate was opened or upgraded.
   */
  plan_id: bigint;
  /**
   * Plan terms copied at sign-up.
   */
  plan_snapshot: PlanSnapshot;
  /**
   * Referral share in basis points of the merchant's share, frozen from the
   * plan at sign-up. Capped at `MAX_REFERRAL_BPS`.
   */
  referral_bps: number;
  /**
   * Optional referrer paid a share of the merchant's money on every charge.
   * `None` disables the referral for this mandate.
   */
  referrer: string | null;
  /**
   * Total amount the merchant has refunded on this mandate so far. The
   * merchant can never refund more than it has collected, so this counter
   * is the hard cap `refund` enforces.
   */
  refunded: bigint;
  /**
   * Number of seats the subscriber is paying for. `1` for non-seat plans.
   * `amount` already equals the per-seat price multiplied by this value.
   */
  seats: number;
  /**
   * Current lifecycle state of the mandate.
   */
  status: MandateStatus;
  /**
   * Account of the subscriber paying for the mandate.
   */
  subscriber: string;
  /**
   * Token contract address in which payments are denominated.
   */
  token: string;
  /**
   * Total debited by settled charges so far, the figure `max_total` caps.
   */
  total_spent: bigint;
}

/**
 * Read-only preview of what `charge` would pay right now.
 *
 * Clients use this instead of recomputing the merchant/fee split, so a quote
 * can never disagree with settlement because of a different rounding rule.
 */
export interface ChargeQuote {
  /**
   * Total the subscriber would be debited: the mandate's frozen price.
   */
  amount: bigint;
  /**
   * True when `charge` would pass its status, cap and timing checks right
   * now. Vault balance is not considered, matching `is_due`.
   */
  chargeable: boolean;
  /**
   * Unix seconds at which the mandate next becomes chargeable.
   */
  due_at: bigint;
  /**
   * Protocol fee, truncated to whole stroops exactly as `charge` does.
   */
  fee: bigint;
  /**
   * The merchant's share after both the protocol fee and the keeper tip.
   */
  merchant_amount: bigint;
  /**
   * Unix seconds at which the retry window closes, computed as `due_at`
   * plus the frozen window. Equals `due_at` when there is no window, so a
   * client can always compare one deadline.
   */
  retry_at: bigint;
  /**
   * Ledger-count retry window frozen into the mandate at sign-up (#343).
   * `None` means a failed charge may be flagged as soon as it is due.
   */
  retry_window_ledgers: number | null;
}

/**
 * Values from a settled charge. `merchant_amount` matches `Charged.amount`:
 * the merchant's share after the protocol fee and keeper tip.
 */
export interface ChargeResult {
  /**
   * Total number of charges completed under this mandate including this one.
   */
  charges_made: number;
  /**
   * Whether the mandate has reached its charge cap and is now completed.
   */
  completed: boolean;
  /**
   * Protocol fee collected on the charge.
   */
  fee: bigint;
  /**
   * Amount transferred to the merchant after fee and keeper tip deductions.
   */
  merchant_amount: bigint;
  /**
   * Timestamp in Unix seconds when the next charge falls due.
   */
  next_charge: bigint;
}

/**
 * Estimated end of a capped mandate, in the units a UI needs without extra
 * math (#287). `timestamp` is Unix seconds (format it as ISO-8601 client
 * side), `ledger` the ledger sequence that time is expected to land in, and
 * `periods_remaining` how many billing periods separate now from that end.
 */
export interface EstimatedEnd {
  /**
   * Ledger sequence expected at `timestamp`, assuming
   * `SECONDS_PER_LEDGER`-second ledgers. Never earlier than the current one.
   */
  ledger: number;
  /**
   * Whole billing periods from now until `timestamp`, rounded up. Zero once
   * the end is in the past.
   */
  periods_remaining: bigint;
  /**
   * Charges still to be made before the cap is reached.
   */
  remaining_charges: number;
  /**
   * Unix seconds of the final scheduled (or, once completed, actual) charge.
   */
  timestamp: bigint;
}

/**
 * Plan terms captured when a mandate is opened or upgraded. The registry's
 * mutable state (such as charging pause status) remains authoritative.
 */
export interface PlanSnapshot {
  /**
   * Total charge per billing period.
   */
  amount: bigint;
  /**
   * Plan name at the time the subscriber agreed to its terms.
   */
  name: string;
  /**
   * Billing period in seconds.
   */
  period: bigint;
}

/**
 * Lifecycle status of a subscription mandate.
 */
 export type MandateStatus =
  /**
   * Chargeable when due.
   */
  { tag: "Active"; values: void } |
  /**
   * Temporarily not chargeable. Subscriber can resume.
   */
  { tag: "Paused"; values: void } |
  /**
   * Terminated by the subscriber. Irreversible.
   */
  { tag: "Cancelled"; values: void } |
  /**
   * Reached `max_charges` or `max_total`. Irreversible.
   */
  { tag: "Completed"; values: void };

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
 * Summary of a mandate for list views. Contains the fields a wallet needs to
 * display a subscription without fetching the full mandate.
 */
export interface MandateSummary {
  /**
   * Amount charged per period.
   */
  amount: bigint;
  /**
   * Number of charges successfully executed so far.
   */
  charges_made: number;
  /**
   * Unique identifier of the mandate.
   */
  id: bigint;
  /**
   * Maximum number of charges (0 = unlimited).
   */
  max_charges: number;
  /**
   * Merchant receiving payment under this mandate.
   */
  merchant: string;
  /**
   * Unix seconds when the next charge falls due.
   */
  next_charge: bigint;
  /**
   * Billing period in seconds.
   */
  period: bigint;
  /**
   * Plan ID this mandate was opened against.
   */
  plan_id: bigint;
  /**
   * Current lifecycle state of the mandate.
   */
  status: MandateStatus;
  /**
   * Token contract address in which payments are denominated.
   */
  token: string;
}

/**
 * Exact settlement recipients and amounts for a prospective charge.
 */
export interface ChargeSimulation {
  /**
   * Total amount debited from the subscriber.
   */
  amount: bigint;
  /**
   * Whether timing, status, caps and configured pauses permit charging now.
   */
  chargeable: boolean;
  /**
   * Unix seconds at which the charge is due.
   */
  due_at: bigint;
  /**
   * Protocol fee.
   */
  fee: bigint;
  /**
   * Address receiving the protocol fee.
   */
  fee_to: string;
  /**
   * Proposed keeper address and recipient of any tip.
   */
  keeper: string;
  /**
   * Merchant's net share after fees, keeper tip and referral.
   */
  merchant_amount: bigint;
  /**
   * Address receiving the merchant's net share.
   */
  payout: string;
  /**
   * Referral payment (zero when there is no referrer).
   */
  referral: bigint;
  /**
   * Referral recipient, if any.
   */
  referrer: string | null;
  /**
   * Subscriber whose vault balance would be debited.
   */
  subscriber: string;
  /**
   * Keeper tip.
   */
  tip: bigint;
  /**
   * Token used for the charge.
   */
  token: string;
}

/**
 * Emitted when a mandate charge is successfully executed.
 */
export interface ChargedEvent {
  name: "Charged";
  data: {
    /**
     * ID of the charged mandate.
     */
    mandate_id: bigint;
    /**
     * Account of the subscriber debited.
     */
    subscriber: string;
    /**
     * Account of the merchant credited.
     */
    merchant: string;
    /**
     * SEP-41 token that `amount`, `fee` and `tip` are denominated in. Without
     * it an indexer must join on the mandate to total revenue per token, and
     * cannot do so from events alone.
     */
    token?: string;
    /**
     * Merchant share after the protocol fee and the keeper tip.
     */
    amount?: bigint;
    /**
     * Protocol fee deducted.
     */
    fee?: bigint;
    /**
     * Paid to `keeper` out of the merchant share. The subscriber's total debit
     * is `amount + fee + tip` exactly as it was before tips existed.
     */
    tip?: bigint;
    /**
     * Account that settled this charge and received the tip.
     */
    keeper?: string;
    /**
     * Total number of charges completed under this mandate.
     */
    charges_made?: number;
    /**
     * Unix timestamp when the next charge falls due.
     */
    next_charge?: bigint;
    /**
     * Referral share paid to `referrer` out of the merchant's share, 0 when
     * the mandate has no referrer.
     */
    referral?: bigint;
    /**
     * Account paid the referral share, if any.
     */
    referrer?: string | null;
  };
}

/**
 * A merchant refunded part of what it collected for a mandate. The event
 * links the refund to the mandate, so both sides can prove a refund from
 * chain data alone. `refunded_total` is the running total refunded on this
 * mandate, which is what the contract caps at the merchant's receipts.
 */
export interface RefundedEvent {
  name: "Refunded";
  data: {
    /**
     * ID of the refunded mandate.
     */
    mandate_id: bigint;
    /**
     * Subscriber who received the refund.
     */
    subscriber: string;
    /**
     * Merchant who issued the refund.
     */
    merchant: string;
    /**
     * Refund amount transferred in this operation.
     */
    amount?: bigint;
    /**
     * Cumulative amount refunded on this mandate so far.
     */
    refunded_total?: bigint;
  };
}

/**
 * Emitted when a subscriber moves a mandate onto a different plan.
 *
 * `charged` is the prorated difference collected for the part of the
 * current period that was still ahead; it is zero for a downgrade, which
 * takes effect at the next charge and never refunds anything. The event
 * carries the old and the new terms so an indexer can show the change
 * without a follow-up contract read.
 */
export interface UpgradedEvent {
  name: "Upgraded";
  data: {
    /**
     * ID of the upgraded mandate.
     */
    mandate_id: bigint;
    /**
     * Subscriber who upgraded.
     */
    subscriber: string;
    /**
     * Merchant receiving payment under the mandate.
     */
    merchant: string;
    /**
     * Previous plan ID.
     */
    old_plan_id?: bigint;
    /**
     * New plan ID.
     */
    new_plan_id?: bigint;
    /**
     * The price the mandate paid per period before the upgrade.
     */
    old_amount?: bigint;
    /**
     * The price the mandate pays per period from the upgrade on.
     */
    new_amount?: bigint;
    /**
     * Difference collected now, prorated and rounded down. Zero for a
     * downgrade.
     */
    charged?: bigint;
    /**
     * The new plan's period, in seconds.
     */
    period?: bigint;
    /**
     * When the next charge falls due. The upgrade does not move it.
     */
    next_charge?: bigint;
  };
}

/**
 * Emitted when a subscriber cancels a mandate.
 */
export interface CancelledEvent {
  name: "Cancelled";
  data: {
    /**
     * ID of the cancelled mandate.
     */
    mandate_id: bigint;
    /**
     * Account of the subscriber who cancelled.
     */
    subscriber: string;
    /**
     * Total number of charges completed before cancellation.
     */
    charges_made?: number;
    /**
     * Why the subscriber left, as one of the documented `CANCEL_REASON_*`
     * codes. `0` means no reason given, which is what `cancel` and
     * `cancel_page` emit. Carried on the wire so a merchant can compute
     * churn without a contract read per cancellation.
     */
    reason?: number;
  };
}

/**
 * Emitted when the admin changes the protocol fee. Applies to mandates opened
 * after this point only; existing mandates keep the fee they were opened with.
 */
export interface FeeChangedEvent {
  name: "FeeChanged";
  data: {
    /**
     * Admin account that changed the fee.
     */
    admin: string;
    /**
     * Previous protocol fee in basis points.
     */
    old_fee_bps?: number;
    /**
     * New protocol fee in basis points.
     */
    new_fee_bps?: number;
  };
}

/**
 * Emitted when a subscriber opens a new mandate against a plan.
 */
export interface SubscribedEvent {
  name: "Subscribed";
  data: {
    /**
     * Unique identifier of the newly created mandate.
     */
    mandate_id: bigint;
    /**
     * Account of the subscriber.
     */
    subscriber: string;
    /**
     * Account of the merchant.
     */
    merchant: string;
    /**
     * ID of the plan subscribed to.
     */
    plan_id?: bigint;
    /**
     * Amount charged per billing period.
     */
    amount?: bigint;
    /**
     * Billing period in seconds.
     */
    period?: bigint;
    /**
     * Unix timestamp when the first charge is due.
     */
    next_charge?: bigint;
    /**
     * 0 means open-ended. Carried in the event so indexers do not have to
     * make a follow-up contract read to learn a mandate's spend cap.
     */
    max_charges?: number;
    /**
     * 0 means open-ended.
     */
    max_total?: bigint;
    /**
     * The fee rate frozen into this mandate, for the same reason.
     */
    fee_bps?: number;
    /**
     * The keeper tip frozen into this mandate, for the same reason.
     */
    keeper_bps?: number;
    /**
     * The referrer frozen into this mandate, if any.
     */
    referrer?: string | null;
    /**
     * The referral rate frozen into this mandate, in basis points of the
     * merchant's share.
     */
    referral_bps?: number;
  };
}

/**
 * Merchant notification hook (#296): emitted the first time a due mandate's
 * charge fails, so off-chain dunning can react promptly.
 *
 * A failed `charge` is rolled back together with any event it would emit, so
 * the failure is recorded by `flag_failed_charge`, which proves on chain that
 * the charge would fail right now. The event fires once per run of failures:
 * further flags are no-ops until a successful `charge` clears the run, after
 * which the next failure notifies again.
 *
 * `merchant` is a topic so a merchant's dunning service can subscribe to its
 * own failures with a single topic filter.
 */
export interface ChargeFailedEvent {
  name: "ChargeFailed";
  data: {
    /**
     * ID of the mandate whose charge failed.
     */
    mandate_id: bigint;
    /**
     * Merchant owed the charge; the topic a dunning service filters on.
     */
    merchant: string;
    /**
     * Subscriber whose vault could not cover the charge.
     */
    subscriber: string;
    /**
     * Plan the mandate bills against.
     */
    plan_id?: bigint;
    /**
     * Token the charge is denominated in.
     */
    token?: string;
    /**
     * Amount the charge needed: the mandate's frozen price.
     */
    amount?: bigint;
    /**
     * Unix seconds the charge fell due (`next_charge`).
     */
    due_at?: bigint;
    /**
     * Unix seconds the failure was flagged.
     */
    failed_at?: bigint;
    /**
     * Charges settled so far under this mandate.
     */
    charges_made?: number;
  };
}

/**
 * Emitted when the merchant ends a mandate, for example because it stopped
 * offering the service. The mandate is cancelled exactly as if the
 * subscriber had cancelled it; the event records who did it.
 */
export interface MandateEndedEvent {
  name: "MandateEnded";
  data: {
    /**
     * ID of the ended mandate.
     */
    mandate_id: bigint;
    /**
     * Merchant who ended the mandate.
     */
    merchant: string;
    /**
     * Subscriber whose mandate was ended.
     */
    subscriber?: string;
    /**
     * Total number of charges completed before ending.
     */
    charges_made?: number;
  };
}

/**
 * Emitted when a mandate is paused or resumed.
 */
export interface PauseChangedEvent {
  name: "PauseChanged";
  data: {
    /**
     * ID of the mandate whose pause status changed.
     */
    mandate_id: bigint;
    /**
     * Account of the subscriber.
     */
    subscriber: string;
    /**
     * Whether the mandate is now paused (`true`) or resumed (`false`).
     */
    paused?: boolean;
  };
}

/**
 * Emitted when a referral share is paid out to a referrer on a successful charge (#364).
 */
export interface ReferralPaidEvent {
  name: "ReferralPaid";
  data: {
    /**
     * ID of the charged mandate.
     */
    mandate_id: bigint;
    /**
     * Account receiving the referral reward.
     */
    referrer: string;
    /**
     * SEP-41 token the reward was paid in.
     */
    token?: string;
    /**
     * Amount paid to the referrer out of the merchant share.
     */
    amount?: bigint;
  };
}

/**
 * Emitted when a subscriber changes the seat count on their mandate.
 *
 * The new seat count takes effect from the next charge; no money moves at the
 * time this event is emitted.
 */
export interface SeatsChangedEvent {
  name: "SeatsChanged";
  data: {
    /**
     * ID of the mandate whose seat count changed.
     */
    mandate_id: bigint;
    /**
     * Subscriber who made the change.
     */
    subscriber: string;
    /**
     * Seat count before the change.
     */
    old_seats?: number;
    /**
     * Seat count after the change, effective from the next charge.
     */
    new_seats?: number;
  };
}

/**
 * Emitted when a subscriber deletes a cancelled or completed mandate,
 * removing its record from storage and permanently tombstoning its ID (#368).
 */
export interface MandateDeletedEvent {
  name: "MandateDeleted";
  data: {
    /**
     * ID of the deleted and tombstoned mandate.
     */
    mandate_id: bigint;
    /**
     * Account of the subscriber who deleted the mandate.
     */
    subscriber: string;
  };
}

/**
 * Emitted after every state change to a mandate, carrying a complete snapshot
 * of the mandate so indexers do not have to reconstruct state or risk drift
 * from missed intermediate events.
 */
export interface MandateUpdatedEvent {
  name: "MandateUpdated";
  data: {
    /**
     * Id of the updated mandate.
     */
    mandate_id: bigint;
    /**
     * Complete snapshot of the mandate after update.
     */
    mandate?: Mandate;
  };
}

/**
 * Emitted when the admin replaces this contract's wasm. Persistent state is
 * untouched by an upgrade; the new code reads the keys the old code wrote.
 * Named `ContractUpgraded` because `Upgraded` is already taken by the
 * mandate-plan upgrade event above.
 */
export interface ContractUpgradedEvent {
  name: "ContractUpgraded";
  data: {
    admin: string;
    new_wasm_hash?: Uint8Array;
  };
}

/**
 * Emitted when a mandate reaches its maximum allowed charges and completes.
 */
export interface MandateCompletedEvent {
  name: "MandateCompleted";
  data: {
    /**
     * ID of the completed mandate.
     */
    mandate_id: bigint;
    /**
     * Account of the subscriber.
     */
    subscriber: string;
    /**
     * Total number of charges executed.
     */
    charges_made?: number;
  };
}

/**
 * Emitted when a subscriber changes the charge cap of their own open
 * mandate. The mandate keeps its id and history; only the limit moves.
 */
export interface MaxChargesChangedEvent {
  name: "MaxChargesChanged";
  data: {
    mandate_id: bigint;
    subscriber: string;
    /**
     * New cap. 0 means the mandate is now open-ended.
     */
    max_charges?: number;
  };
}

/**
 * Emitted when a mandate's ownership moves from one subscriber to another via
 * `propose_mandate_owner` / `accept_mandate_owner`. Future charges are debited
 * from the new subscriber's vault; all frozen mandate terms and the merchant
 * index are unchanged.
 */
export interface MandateTransferredEvent {
  name: "MandateTransferred";
  data: {
    /**
     * ID of the transferred mandate.
     */
    mandate_id: bigint;
    /**
     * Subscriber the mandate moved away from.
     */
    old_subscriber: string;
    /**
     * Subscriber the mandate moved to.
     */
    new_subscriber?: string;
  };
}

/**
 * Emitted when the admin pauses or resumes the protocol (#285). Only emitted
 * when the switch actually changes, so an indexer can treat every event as a
 * transition.
 */
export interface ProtocolPauseChangedEvent {
  name: "ProtocolPauseChanged";
  data: {
    /**
     * Admin account that flipped the switch.
     */
    admin: string;
    /**
     * `true` when the protocol is now paused, `false` when it resumed.
     */
    paused?: boolean;
  };
}

/**
 * Emitted when a plan's committed revenue per period changes. This allows
 * dashboards to track MRR/ARR on-chain without scanning all mandates.
 */
export interface CommittedRevenueChangedEvent {
  name: "CommittedRevenueChanged";
  data: {
    /**
     * ID of the plan whose committed revenue changed.
     */
    plan_id: bigint;
    /**
     * Merchant owning the plan.
     */
    merchant: string;
    /**
     * Token the plan is denominated in.
     */
    token?: string;
    /**
     * New committed amount per period.
     */
    new_committed?: bigint;
    /**
     * Billing period in seconds.
     */
    period?: bigint;
  };
}
    export type ContractEvent = ChargedEvent | RefundedEvent | UpgradedEvent | CancelledEvent | FeeChangedEvent | SubscribedEvent | ChargeFailedEvent | MandateEndedEvent | PauseChangedEvent | ReferralPaidEvent | SeatsChangedEvent | MandateDeletedEvent | MandateUpdatedEvent | ContractUpgradedEvent | MandateCompletedEvent | MaxChargesChangedEvent | MandateTransferredEvent | ProtocolPauseChangedEvent | CommittedRevenueChangedEvent;