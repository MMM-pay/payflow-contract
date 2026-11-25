import {Address} from '@stellar/stellar-sdk';

    /**
 * Errors returned by the plan registry contract.
 */
export const Error = {
  /**
   * The contract has already been initialized.
   */
  1001 : { message: "AlreadyInitialized" },
  /**
   * The contract or its subscription address has not been initialized.
   */
  1002 : { message: "NotInitialized" },
  /**
   * The requested plan ID does not exist in storage.
   */
  1003 : { message: "PlanNotFound" },
  /**
   * The caller is not the merchant that owns the plan.
   */
  1004 : { message: "NotPlanOwner" },
  /**
   * The plan amount must be positive.
   */
  1005 : { message: "InvalidAmount" },
  /**
   * The billing period must be at least `MIN_PERIOD`.
   */
  1006 : { message: "InvalidPeriod" },
  /**
   * The plan name exceeds `MAX_NAME_LEN` bytes.
   */
  1007 : { message: "NameTooLong" },
  /**
   * A getter found no profile for the merchant.
   */
  1008 : { message: "ProfileNotFound" },
  /**
   * `set_profile` was given a name longer than `MAX_NAME_LEN` bytes.
   */
  1009 : { message: "ProfileNameTooLong" },
  /**
   * `set_profile` was given a URL longer than `MAX_URL_LEN` bytes.
   */
  1010 : { message: "ProfileUrlTooLong" },
  /**
   * `create_plan` was given a keeper tip above `MAX_KEEPER_BPS`.
   */
  1011 : { message: "KeeperBpsTooHigh" },
  /**
   * `set_successor` was given a plan that does not exist.
   */
  1012 : { message: "SuccessorNotFound" },
  /**
   * A plan cannot be its own successor.
   */
  1013 : { message: "SelfSuccessor" },
  /**
   * The replacement plan is denominated in a different token than the plan
   * it replaces, so the pointer would be meaningless.
   */
  1014 : { message: "SuccessorTokenMismatch" },
  /**
   * `create_plan` was given a description longer than
   * `MAX_DESCRIPTION_LEN` bytes.
   */
  1015 : { message: "DescriptionTooLong" },
  /**
   * `set_plan_name` was given a name longer than `MAX_NAME_LEN` bytes.
   */
  1016 : { message: "PlanNameTooLong" },
  /**
   * `create_plan` was given a terms URI longer than `MAX_TERMS_LEN` bytes.
   */
  1017 : { message: "UriTooLong" },
  /**
   * `create_plan` was given a referral rate above `MAX_REFERRAL_BPS`.
   */
  1018 : { message: "ReferralBpsTooHigh" },
  /**
   * `create_plan` was given an amount below `MIN_CHARGE_AMOUNT`.
   */
  1019 : { message: "AmountTooLow" },
  /**
   * `accept_plan_owner` was called for a plan with no pending ownership
   * proposal, or after the proposal was already accepted.
   */
  1020 : { message: "PendingOwnershipNotFound" },
  /**
   * The address accepting ownership is not the one the current owner
   * proposed in `propose_plan_owner`.
   */
  1021 : { message: "NotPendingOwner" },
  /**
   * `create_plans` was given no plans.
   */
  1022 : { message: "EmptyBatch" },
  /**
   * `create_plans` was given more plans than `MAX_BATCH_PLANS`.
   */
  1023 : { message: "BatchTooLarge" },
  /**
   * The next plan ID cannot be represented: the counter is at `u64::MAX`,
   * or a batch would carry it past `u64::MAX`. No plan is written.
   */
  1024 : { message: "PlanIdOverflow" },
  /**
   * The plan token address is a reserved Payflow contract address.
   */
  1025 : { message: "InvalidToken" }
}

/**
 * A merchant's self-declared profile, shown to subscribers alongside the
 * merchant's plans so they can tell who they are paying. The profile is
 * self-declared: the contract does not verify the name or the URL.
 */
export interface Profile {
  /**
   * Account address of the merchant.
   */
  merchant: string;
  /**
   * Display name, e.g. "Acme Corp". At most `MAX_NAME_LEN` bytes.
   */
  name: string;
  /**
   * Website or terms URL. At most `MAX_URL_LEN` bytes.
   */
  url: string;
}

/**
 * The mutable inputs shared by `create_plan` and `create_plans`.
 *
 * The merchant is supplied separately to the batch call and is the same for
 * every plan in it; everything else that defines a plan is carried here.
 */
export interface PlanParams {
  /**
   * Amount charged per billing period, in the token's smallest unit.
   */
  amount: bigint;
  /**
   * Unix seconds after which the plan stops accepting subscribers;
   * 0 means forever.
   */
  available_until: bigint;
  /**
   * Discovery category; may be empty.
   */
  category: string;
  /**
   * Short summary, at most `MAX_DESCRIPTION_LEN` bytes.
   */
  description: string;
  /**
   * Keeper tip in basis points of the merchant share. At most
   * `MAX_KEEPER_BPS`.
   */
  keeper_bps: number;
  /**
   * Maximum number of simultaneously-open mandates; 0 means unlimited.
   */
  max_subscribers: number;
  /**
   * Display name, at most `MAX_NAME_LEN` bytes.
   */
  name: string;
  /**
   * Account that receives the merchant's share of each charge. `None`
   * defaults to the merchant.
   */
  payout: string | null;
  /**
   * Billing period in seconds. At least `MIN_PERIOD`.
   */
  period: bigint;
  /**
   * Referral share in basis points of the merchant's share; 0 disables
   * referrals for this plan.
   */
  referral_bps: number;
  /**
   * Link to the plan's terms, at most `MAX_TERMS_LEN` bytes.
   */
  terms_uri: string;
  /**
   * Token contract address this plan is denominated in.
   */
  token: string;
}

/**
 * One row of a plan listing (#288). Carries the settlement `token` alongside
 * the price so a catalogue mixing tokens is unambiguous without a follow-up
 * `get_plan` per row.
 */
export interface PlanListing {
  /**
   * Whether the plan accepts new subscribers.
   */
  active: boolean;
  /**
   * Price per period in the token's smallest unit.
   */
  amount: bigint;
  /**
   * Plan id.
   */
  id: bigint;
  /**
   * Merchant that published the plan.
   */
  merchant: string;
  /**
   * Display name.
   */
  name: string;
  /**
   * Billing period in seconds.
   */
  period: bigint;
  /**
   * Settlement token contract the plan is priced and charged in.
   */
  token: string;
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
 * Emitted when a merchant publishes a new plan.
 */
export interface PlanCreatedEvent {
  name: "PlanCreated";
  data: {
    /**
     * Account that published the plan.
     */
    merchant: string;
    /**
     * Unique ID allocated for the newly published plan.
     */
    plan_id: bigint;
    /**
     * Token contract address this plan is denominated in.
     */
    token?: string;
    /**
     * Amount charged per billing period.
     */
    amount?: bigint;
    /**
     * Billing period in seconds.
     */
    period?: bigint;
    /**
     * Carried in the event so indexers need no follow-up contract read.
     */
    name?: string;
    /**
     * Short summary of what the plan includes. May be empty.
     */
    description?: string;
    /**
     * Keeper tip frozen into the plan, in basis points of the merchant share.
     */
    keeper_bps?: number;
    /**
     * Discovery category the merchant chose, empty when it chose none.
     */
    category?: string;
    /**
     * Link to the plan's terms, carried in the event so frontends need no
     * follow-up contract read. At most 200 bytes; may be empty.
     */
    terms_uri?: string;
    /**
     * Unix seconds after which the plan stops accepting new subscribers,
     * 0 = forever. Carried in the event so indexers need no follow-up read.
     */
    available_until?: bigint;
    /**
     * Referral share in basis points of the merchant's share; 0 disables
     * referrals for this plan.
     */
    referral_bps?: number;
    /**
     * Account that receives the merchant's share of each charge; defaults to
     * the merchant itself.
     */
    payout?: string;
    /**
     * Seat cap frozen into the plan; 0 means unlimited. Carried in the event
     * so indexers need no follow-up contract read.
     */
    max_subscribers?: number;
  };
}

/**
 * Emitted when a merchant renames a plan.
 */
export interface PlanRenamedEvent {
  name: "PlanRenamed";
  data: {
    merchant: string;
    plan_id: bigint;
    name?: string;
  };
}

/**
 * Emitted once after a successful `create_plans` batch. Individual
 * [`PlanCreated`] events precede it; this event lets an indexer group the
 * plans that were created by one call.
 */
export interface PlansCreatedEvent {
  name: "PlansCreated";
  data: {
    /**
     * Account that published the plans.
     */
    merchant: string;
    /**
     * IDs of the created plans, in request order.
     */
    plan_ids?: Array<bigint>;
  };
}

/**
 * Emitted when a merchant sets or updates its self-declared profile. The
 * profile is not verified by the contract.
 */
export interface ProfileUpdatedEvent {
  name: "ProfileUpdated";
  data: {
    /**
     * Merchant whose profile was updated.
     */
    merchant: string;
    /**
     * Display name of the merchant.
     */
    name?: string;
    /**
     * Website or terms URL of the merchant.
     */
    url?: string;
  };
}

/**
 * Emitted when the proposed owner accepts and the plan becomes theirs. The
 * plan moves between the two merchants' plan indexes.
 */
export interface PlanTransferredEvent {
  name: "PlanTransferred";
  data: {
    merchant: string;
    plan_id: bigint;
    new_owner?: string;
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

/**
 * Emitted when a merchant points a plan at the plan that replaces it, if any.
 */
export interface PlanSuccessorSetEvent {
  name: "PlanSuccessorSet";
  data: {
    /**
     * Merchant owning the plan.
     */
    merchant: string;
    /**
     * ID of the retiring plan.
     */
    plan_id: bigint;
    /**
     * ID of the successor replacement plan.
     */
    successor_id?: bigint;
  };
}

/**
 * Emitted when a merchant activates or deactivates a plan.
 */
export interface PlanStatusChangedEvent {
  name: "PlanStatusChanged";
  data: {
    /**
     * Merchant owning the plan.
     */
    merchant: string;
    /**
     * ID of the plan whose status changed.
     */
    plan_id: bigint;
    /**
     * Whether the plan is active and accepting new subscriptions.
     */
    active?: boolean;
  };
}

/**
 * Emitted when a merchant changes a plan's grace window (#312). Only new
 * mandates opened after the change carry the new window.
 */
export interface PlanGracePeriodSetEvent {
  name: "PlanGracePeriodSet";
  data: {
    /**
     * Merchant owning the plan.
     */
    merchant: string;
    /**
     * ID of the plan whose grace window changed.
     */
    plan_id: bigint;
    /**
     * Grace window in seconds. `0` disables the grace view.
     */
    grace_period_secs?: bigint;
  };
}

/**
 * Emitted when a plan's current owner proposes transferring it to a new owner.
 * The transfer takes effect only once the new owner accepts.
 */
export interface PlanOwnershipProposedEvent {
  name: "PlanOwnershipProposed";
  data: {
    merchant: string;
    plan_id: bigint;
    new_owner?: string;
  };
}

/**
 * Emitted when a merchant pauses or resumes charging for one plan (#315).
 * Every mandate against the plan is affected; the plan itself stays active and
 * keeps accepting new subscribers.
 */
export interface PlanChargesPausedChangedEvent {
  name: "PlanChargesPausedChanged";
  data: {
    /**
     * Merchant owning the plan.
     */
    merchant: string;
    /**
     * ID of the plan whose charge status changed.
     */
    plan_id: bigint;
    /**
     * Whether charging is now paused (`true`) or resumed (`false`).
     */
    paused?: boolean;
  };
}

/**
 * A merchant-published charge template. A `Plan` is immutable except for its
 * `active` flag and its `successor_id` pointer: changing price, period or
 * keeper tip requires publishing a new plan so existing mandates can never be
 * repriced under a subscriber.
 */
export interface Plan {
  /**
   * Whether the plan accepts new subscriptions. Inactive plans cannot be subscribed to.
   */
  active: boolean;
  /**
   * Amount charged per period, in the token's smallest unit. This is what
   * the subscriber pays in total; the protocol fee and the keeper tip are
   * both taken out of it before the merchant is paid.
   */
  amount: bigint;
  /**
   * Unix seconds after which the plan stops accepting new subscribers;
   * 0 means it is available forever. Only new subscriptions are
   * affected: mandates opened before the cutoff keep billing. The
   * subscription contract enforces the cutoff in `subscribe`.
   */
  available_until: bigint;
  /**
   * Merchant-selected discovery category, so subscribers can browse
   * plans by what they are for. Soroban symbols are short and limited to
   * the symbol alphabet, which is what makes them cheap to index; this
   * may be empty, and it is frozen with the rest of the plan's terms.
   */
  category: string;
  /**
   * Short, plain summary of what the plan includes, shown to
   * subscribers next to the name. At most `MAX_DESCRIPTION_LEN` bytes;
   * may be empty.
   */
  description: string;
  /**
   * Unique identifier for the plan, allocated sequentially starting from 1.
   */
  id: bigint;
  /**
   * Share of each charge paid to whichever account settles it, in basis
   * points of the merchant's share. 0 means the merchant keeps it all.
   * At most `MAX_KEEPER_BPS`; the registry rejects anything higher.
   */
  keeper_bps: number;
  /**
   * Maximum number of simultaneously-open mandates the plan accepts; 0
   * means unlimited. Frozen into the plan, checked on each new sign-up.
   */
  max_subscribers: number;
  /**
   * Account that publishes the plan and receives subscription payments.
   */
  merchant: string;
  /**
   * Human-readable label shown to subscribers, e.g. "Pro Monthly".
   * At most `MAX_NAME_LEN` bytes; may be empty.
   */
  name: string;
  /**
   * Account that actually receives the merchant's share of each charge.
   * Frozen into every mandate at sign-up, so a later payout change or a
   * plan-ownership transfer never redirects an open mandate's funds.
   */
  payout: string;
  /**
   * Billing period in seconds.
   */
  period: bigint;
  /**
   * Referral share paid to a mandate's referrer, in basis points of the
   * merchant's share (out of the merchant's own money, never the fee or
   * tip). 0 disables referrals. At most `MAX_REFERRAL_BPS`.
   */
  referral_bps: number;
  /**
   * The plan that replaces this one, if the merchant retired it. Set by
   * `set_successor` and read through `get_plan`, so apps can point a
   * subscriber at the replacement without a second lookup. 0 means the plan
   * has no successor. The pointer never re-prices an open mandate: a mandate
   * keeps the terms of the plan it was opened against.
   */
  successor_id: bigint;
  /**
   * Link to the plan's description and terms of service, shown to
   * subscribers before they open a mandate. At most `MAX_TERMS_LEN`
   * (200) bytes; may be empty. Self-declared by the merchant, like the
   * profile URL: the contract does not verify it.
   */
  terms_uri: string;
  /**
   * SEP-41 token this plan is denominated in.
   */
  token: string;
}
    export type ContractEvent = PlanCreatedEvent | PlanRenamedEvent | PlansCreatedEvent | ProfileUpdatedEvent | PlanTransferredEvent | ContractUpgradedEvent | PlanSuccessorSetEvent | PlanStatusChangedEvent | PlanGracePeriodSetEvent | PlanOwnershipProposedEvent | PlanChargesPausedChangedEvent;
