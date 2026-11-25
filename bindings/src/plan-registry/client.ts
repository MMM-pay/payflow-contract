import {Profile, Plan, PlanParams, UpgradeRecord, PlanListing, ContractEvent} from './types.js';
import {Result, Spec, AssembledTransaction, Client as ContractClient, ClientOptions as ContractClientOptions, MethodOptions, ExternalExecutableRef} from '@stellar/stellar-sdk/contract';
import {Address, xdr} from '@stellar/stellar-sdk';

export interface Client {
  /**
   * Address of the contract administrator.
   */
  admin(options?: MethodOptions): Promise<AssembledTransaction<Result<string, Error>>>;
  /**
   * `merchant`'s self-declared profile, if one has been set.
   */
  profile(args: { merchant: string | Address }, options?: MethodOptions): Promise<AssembledTransaction<Result<Profile, Error>>>;
  /**
   * The version of this contract's crate.
   */
  version(options?: MethodOptions): Promise<AssembledTransaction<string>>;
  /**
   * Look up a published plan by its unique ID.
   *
   * Reading a plan also refreshes its TTL (#247). A plan that is used by
   * every charge but never edited is still a hot key; leaving its rent
   * unchanged until the next write would let it expire while mandates
   * are open against it. The bump is conditional: an entry already past
   * the threshold keeps its TTL exactly as it was.
   */
  get_plan(args: { plan_id: bigint }, options?: MethodOptions): Promise<AssembledTransaction<Result<Plan, Error>>>;
  /**
   * Publish a new plan. Only the merchant may publish plans in its own name.
   *
   * `keeper_bps` is the share of each charge paid to whoever settles it,
   * taken out of the merchant's share, capped at `MAX_KEEPER_BPS`. It is
   * part of the plan's immutable terms: a mandate opened against this plan
   * keeps the tip it was opened with, so raising the tip later cannot take
   * money from an existing subscriber or merchant.
   *
   * `terms_uri` links to the plan's description and terms of service, so a
   * subscriber can read what they are agreeing to before opening a
   * mandate. It may be empty, is capped at `MAX_TERMS_LEN` bytes, and is
   * self-declared by the merchant: the contract does not fetch or verify it.
   *
   * # Auth
   * Requires authorization from `merchant`.
   *
   * # Events
   * Emits [`PlanCreated`].
   *
   * # Errors
   * Returns [`Error::InvalidAmount`] if `amount <= 0`, [`Error::AmountTooLow`]
   * if `amount < MIN_CHARGE_AMOUNT`, [`Error::NameTooLong`]
   * if `name` exceeds [`MAX_NAME_LEN`], [`Error::DescriptionTooLong`] if `description`
   * exceeds [`MAX_DESCRIPTION_LEN`], [`Error:
   */
  create_plan(args: { merchant: string | Address; token: string | Address; amount: bigint; period: bigint; name: string; description: string; keeper_bps: number; category: string; terms_uri: string; available_until: bigint; referral_bps: number; payout: string | Address | null; max_subscribers: number }, options?: MethodOptions): Promise<AssembledTransaction<Result<bigint, Error>>>;
  /**
   * Alias for [`PlanRegistry::plan_charges_paused`].
   */
  plan_paused(args: { plan_id: bigint }, options?: MethodOptions): Promise<AssembledTransaction<Result<boolean, Error>>>;
  /**
   * Set or replace `merchant`'s self-declared profile: a display name and a
   * URL (website or terms). The profile is not verified by the contract;
   * subscribers should treat it as a claim the merchant makes about itself.
   * Only the merchant may set its own profile.
   *
   * # Auth
   * Requires authorization from `merchant`.
   *
   * # Events
   * Emits [`ProfileUpdated`].
   *
   * # Errors
   * Returns [`Error::ProfileNameTooLong`] if `name` exceeds [`MAX_NAME_LEN`], or
   * [`Error::ProfileUrlTooLong`] if `url` exceeds [`MAX_URL_LEN`].
   */
  set_profile(args: { merchant: string | Address; name: string; url: string }, options?: MethodOptions): Promise<AssembledTransaction<Result<null, Error>>>;
  /**
   * Ids of the plans priced in `token`, oldest first, starting at
   * position `start`. Returns at most `min(limit, MAX_PAGE)` ids.
   */
  token_plans(args: { token: string | Address; start: number; limit: number }, options?: MethodOptions): Promise<AssembledTransaction<Array<bigint>>>;
  /**
   * Publish several plans in one transaction.
   *
   * Every plan is validated before any is written, so a batch that
   * contains one invalid plan creates none of them. IDs are allocated
   * sequentially in request order, exactly as if each plan had been
   * published with `create_plan`.
   *
   * The batch is capped at [`MAX_BATCH_PLANS`] plans to stay inside
   * Soroban's per-transaction resource limits.
   *
   * # Auth
   * Requires authorization from `merchant`. The same merchant owns every
   * plan in the batch.
   *
   * # Events
   * Emits one [`PlanCreated`] per plan, followed by one [`PlansCreated`]
   * summarising the batch.
   *
   * # Errors
   * Returns [`Error::EmptyBatch`] if `plans` is empty,
   * [`Error::BatchTooLarge`] if it holds more than [`MAX_BATCH_PLANS`]
   * plans, [`Error::PlanIdOverflow`] if the batch would carry the plan
   * ID counter past `u64::MAX`, and otherwise the same validation errors
   * as `create_plan` for the first invalid plan.
   */
  create_plans(args: { merchant: string | Address; plans: Array<PlanParams> }, options?: MethodOptions): Promise<AssembledTransaction<Result<Array<bigint>, Error>>>;
  /**
   * The id the next published plan will get. Plans are numbered from 1, so
   * `next_plan_id() - 1` is the number of plans published so far.
   */
  next_plan_id(options?: MethodOptions): Promise<AssembledTransaction<Result<bigint, Error>>>;
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
   * Rename a plan. Only the plan's own merchant may do this, and only
   * the label changes: price, period, token and status are frozen once
   * a plan is published, because subscribers' mandates are priced from
   * them.
   *
   * # Errors
   * [`Error::PlanNameTooLong`] if `name` exceeds [`MAX_NAME_LEN`],
   * [`Error::PlanNotFound`] if no such plan exists, and
   * [`Error::NotPlanOwner`] if `merchant` does not own it.
   *
   * # Auth
   * Requires authorization from `merchant`.
   *
   * # Events
   * Emits [`PlanRenamed`].
   */
  set_plan_name(args: { merchant: string | Address; plan_id: bigint; name: string }, options?: MethodOptions): Promise<AssembledTransaction<Result<null, Error>>>;
  /**
   * Point a plan at the plan that replaces it, if any.
   *
   * A merchant that replaces a plan retires the old one and opens a new one
   * carrying the new terms, so subscribers of the old plan need an on-chain
   * pointer to the new one to be able to be offered the replacement. The
   * pointer is stored on the retiring plan itself (`Plan.successor_id`, 0
   * when there is none) rather than in a side key, so `get_plan` hands back
   * the answer in the same read that returns the plan, and apps never need a
   * second lookup. Because the pointer lives on the old plan it never
   * touches an open mandate's frozen terms.
   *
   * The retiring plan may be re-pointed, which is how a merchant that
   * replaces a replacement retires the middle plan. Both plans must belong
   * to `merchant` and be denominated in the same token; a replacement from
   * another merchant's catalogue or another asset would be meaningless, and
   * a plan cannot be its own successor.
   *
   * # Auth
   * Requires authorization from `merchant`.
   *
   * # Events
   * Emits [`PlanSuccessorSet`].
   *
   * # Errors
   * Returns [`Error::PlanNotF
   */
  set_successor(args: { merchant: string | Address; plan_id: bigint; successor_id: bigint }, options?: MethodOptions): Promise<AssembledTransaction<Result<null, Error>>>;
  /**
   * Ids of the plans `merchant` published, oldest first, starting at
   * position `start`. Returns at most `min(limit, MAX_PAGE)` ids.
   *
   * This uses a per-merchant index (`MerchantPlanCount` + `MerchantPlan`)
   * where each plan occupies a fixed position. Reading a page touches
   * exactly `min(limit, MAX_PAGE)` storage entries — one per plan — so
   * listing a merchant's plans is O(1) per entry and does not scan
   * unrelated merchants' plans.
   */
  merchant_plans(args: { merchant: string | Address; start: number; limit: number }, options?: MethodOptions): Promise<AssembledTransaction<Array<bigint>>>;
  /**
   * Deactivate up to `limit` of `merchant`'s plans, starting at position
   * `start` of its plan index. Only plans in `merchant`'s own index are
   * touched, so a merchant can never deactivate another merchant's plans.
   * Already-inactive plans are skipped. Returns how many plans were
   * deactivated in this page. At most `min(limit, MAX_PAGE)` plans are
   * touched, so the call stays within the ledger entry footprint budget.
   *
   * # Auth
   * Requires authorization from `merchant`.
   *
   * # Events
   * Emits [`PlanStatusChanged`] for each plan deactivated.
   *
   * # Errors
   * Returns [`Error::PlanNotFound`] if an index slot points to a missing plan.
   */
  deactivate_page(args: { merchant: string | Address; start: number; limit: number }, options?: MethodOptions): Promise<AssembledTransaction<Result<number, Error>>>;
  /**
   * Activate or deactivate a plan. Deactivating stops new subscriptions but
   * does not cancel mandates already open against the plan.
   *
   * # Auth
   * Requires authorization from `merchant`.
   *
   * # Events
   * Emits [`PlanStatusChanged`].
   *
   * # Errors
   * Returns [`Error::PlanNotFound`] if the plan does not exist, or [`Error::NotPlanOwner`]
   * if the caller does not own the plan.
   */
  set_plan_active(args: { merchant: string | Address; plan_id: bigint; active: boolean }, options?: MethodOptions): Promise<AssembledTransaction<Result<null, Error>>>;
  /**
   * Alias for [`PlanRegistry::set_plan_charges_paused`].
   */
  set_plan_paused(args: { merchant: string | Address; plan_id: bigint; paused: boolean }, options?: MethodOptions): Promise<AssembledTransaction<Result<null, Error>>>;
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
   * How many plans are priced in `token`, active or not.
   */
  token_plan_count(args: { token: string | Address }, options?: MethodOptions): Promise<AssembledTransaction<number>>;
  /**
   * Accept a proposed ownership transfer. Only the proposed `new_owner`
   * may accept, and the plan moves between the two merchants' plan
   * indexes. Mandates already open against the plan keep paying the
   * `payout` address frozen at their sign-up; a transfer never reprices
   * or redirects an existing subscriber, it only changes who manages the
   * plan and which merchant newly opened mandates are attributed to.
   */
  accept_plan_owner(args: { new_owner: string | Address; plan_id: bigint }, options?: MethodOptions): Promise<AssembledTransaction<Result<null, Error>>>;
  /**
   * Grace window in seconds for new mandates against `plan_id` (#312).
   * Existing mandates keep the window frozen into them at sign-up.
   * Defaults to `0`, which disables the grace view.
   *
   * # Errors
   * Returns [`Error::PlanNotFound`] if the plan does not exist.
   */
  plan_grace_period(args: { plan_id: bigint }, options?: MethodOptions): Promise<AssembledTransaction<Result<bigint, Error>>>;
  /**
   * Propose transferring a plan to `new_owner`. Step one of a two-step
   * handshake: only the plan's current `merchant` may propose, and the
   * transfer takes effect only once `new_owner` accepts. (Merged from
   * #136.)
   */
  propose_plan_owner(args: { merchant: string | Address; plan_id: bigint; new_owner: string | Address }, options?: MethodOptions): Promise<AssembledTransaction<Result<null, Error>>>;
  /**
   * How many plans `merchant` has published, active or not.
   */
  merchant_plan_count(args: { merchant: string | Address }, options?: MethodOptions): Promise<AssembledTransaction<number>>;
  /**
   * Whether charging is paused for `plan_id` (#315).
   *
   * # Errors
   * Returns [`Error::PlanNotFound`] if the plan does not exist.
   */
  plan_charges_paused(args: { plan_id: bigint }, options?: MethodOptions): Promise<AssembledTransaction<Result<boolean, Error>>>;
  /**
   * Listing rows for the plans priced in `token`, in the same order and
   * paging as [`token_plans`](Self::token_plans) (#288).
   */
  token_plan_listings(args: { token: string | Address; start: number; limit: number }, options?: MethodOptions): Promise<AssembledTransaction<Array<PlanListing>>>;
  /**
   * Set a plan's grace window in seconds (#312). The window is copied
   * into each new mandate at sign-up, so a later change affects only
   * subscriptions opened after it. `0` disables the grace view.
   *
   * The window is only a warning period: subscription's
   * `is_in_grace_period` reports a mandate as recoverable between its
   * due time and the end of the frozen window. It does not cancel or
   * otherwise change the mandate.
   *
   * # Auth
   * Requires authorization from the plan's merchant.
   *
   * # Events
   * Emits [`PlanGracePeriodSet`].
   *
   * # Errors
   * Returns [`Error::PlanNotFound`] if the plan does not exist, or
   * [`Error::NotPlanOwner`] if `merchant` does not own it.
   */
  set_plan_grace_period(args: { merchant: string | Address; plan_id: bigint; grace_period_secs: bigint }, options?: MethodOptions): Promise<AssembledTransaction<Result<null, Error>>>;
  /**
   * Listing rows for the plans `merchant` published, in the same order
   * and paging as [`merchant_plans`](Self::merchant_plans). Each row
   * names the plan's settlement token (#288).
   */
  merchant_plan_listings(args: { merchant: string | Address; start: number; limit: number }, options?: MethodOptions): Promise<AssembledTransaction<Array<PlanListing>>>;
  /**
   * Pause or resume charging for every mandate against one plan (#315).
   *
   * Pausing does not deactivate the plan: it keeps accepting new
   * subscribers, and its open mandates are not cancelled or moved. Both
   * `charge` and `flag_failed_charge` reject a mandate whose plan is
   * paused, so no charge settles until the merchant resumes. This lets a
   * merchant stop billing during a dispute without forcing subscribers
   * to re-subscribe afterwards.
   *
   * # Auth
   * Requires authorization from the plan's merchant.
   *
   * # Events
   * Emits [`PlanChargesPausedChanged`].
   *
   * # Errors
   * Returns [`Error::PlanNotFound`] if the plan does not exist, or
   * [`Error::NotPlanOwner`] if `merchant` does not own it.
   */
  set_plan_charges_paused(args: { merchant: string | Address; plan_id: bigint; paused: boolean }, options?: MethodOptions): Promise<AssembledTransaction<Result<null, Error>>>;
  /**
   * Configure the subscription contract whose address cannot be used as
   * a plan token. This is set after deployment because the registry is
   * deployed before the subscription contract, which depends on it.
   *
   * # Auth
   * Requires authorization from the contract admin.
   *
   * # Events
   * Emits no events.
   *
   * # Errors
   * Returns [`Error::NotInitialized`] if the registry has no admin.
   */
  set_subscription_contract(args: { subscription: string | Address }, options?: MethodOptions): Promise<AssembledTransaction<Result<null, Error>>>;
}

export class Client extends ContractClient {
  constructor(public readonly options: ContractClientOptions) {
    super(
      new Spec(["AAAABAAAAC5FcnJvcnMgcmV0dXJuZWQgYnkgdGhlIHBsYW4gcmVnaXN0cnkgY29udHJhY3QuAAAAAAAAAAAABUVycm9yAAAAAAAAGQAAACpUaGUgY29udHJhY3QgaGFzIGFscmVhZHkgYmVlbiBpbml0aWFsaXplZC4AAAAAABJBbHJlYWR5SW5pdGlhbGl6ZWQAAAAAA+kAAABCVGhlIGNvbnRyYWN0IG9yIGl0cyBzdWJzY3JpcHRpb24gYWRkcmVzcyBoYXMgbm90IGJlZW4gaW5pdGlhbGl6ZWQuAAAAAAAOTm90SW5pdGlhbGl6ZWQAAAAAA+oAAAAwVGhlIHJlcXVlc3RlZCBwbGFuIElEIGRvZXMgbm90IGV4aXN0IGluIHN0b3JhZ2UuAAAADFBsYW5Ob3RGb3VuZAAAA+sAAAAyVGhlIGNhbGxlciBpcyBub3QgdGhlIG1lcmNoYW50IHRoYXQgb3ducyB0aGUgcGxhbi4AAAAAAAxOb3RQbGFuT3duZXIAAAPsAAAAIVRoZSBwbGFuIGFtb3VudCBtdXN0IGJlIHBvc2l0aXZlLgAAAAAAAA1JbnZhbGlkQW1vdW50AAAAAAAD7QAAADFUaGUgYmlsbGluZyBwZXJpb2QgbXVzdCBiZSBhdCBsZWFzdCBgTUlOX1BFUklPRGAuAAAAAAAADUludmFsaWRQZXJpb2QAAAAAAAPuAAAAK1RoZSBwbGFuIG5hbWUgZXhjZWVkcyBgTUFYX05BTUVfTEVOYCBieXRlcy4AAAAAC05hbWVUb29Mb25nAAAAA+8AAAArQSBnZXR0ZXIgZm91bmQgbm8gcHJvZmlsZSBmb3IgdGhlIG1lcmNoYW50LgAAAAAPUHJvZmlsZU5vdEZvdW5kAAAAA/AAAABAYHNldF9wcm9maWxlYCB3YXMgZ2l2ZW4gYSBuYW1lIGxvbmdlciB0aGFuIGBNQVhfTkFNRV9MRU5gIGJ5dGVzLgAAABJQcm9maWxlTmFtZVRvb0xvbmcAAAAAA/EAAAA+YHNldF9wcm9maWxlYCB3YXMgZ2l2ZW4gYSBVUkwgbG9uZ2VyIHRoYW4gYE1BWF9VUkxfTEVOYCBieXRlcy4AAAAAABFQcm9maWxlVXJsVG9vTG9uZwAAAAAAA/IAAAA8YGNyZWF0ZV9wbGFuYCB3YXMgZ2l2ZW4gYSBrZWVwZXIgdGlwIGFib3ZlIGBNQVhfS0VFUEVSX0JQU2AuAAAAEEtlZXBlckJwc1Rvb0hpZ2gAAAPzAAAANWBzZXRfc3VjY2Vzc29yYCB3YXMgZ2l2ZW4gYSBwbGFuIHRoYXQgZG9lcyBub3QgZXhpc3QuAAAAAAAAEVN1Y2Nlc3Nvck5vdEZvdW5kAAAAAAAD9AAAACNBIHBsYW4gY2Fubm90IGJlIGl0cyBvd24gc3VjY2Vzc29yLgAAAAANU2VsZlN1Y2Nlc3NvcgAAAAAAA/UAAAB4VGhlIHJlcGxhY2VtZW50IHBsYW4gaXMgZGVub21pbmF0ZWQgaW4gYSBkaWZmZXJlbnQgdG9rZW4gdGhhbiB0aGUgcGxhbgppdCByZXBsYWNlcywgc28gdGhlIHBvaW50ZXIgd291bGQgYmUgbWVhbmluZ2xlc3MuAAAAFlN1Y2Nlc3NvclRva2VuTWlzbWF0Y2gAAAAAA/YAAABOYGNyZWF0ZV9wbGFuYCB3YXMgZ2l2ZW4gYSBkZXNjcmlwdGlvbiBsb25nZXIgdGhhbgpgTUFYX0RFU0NSSVBUSU9OX0xFTmAgYnl0ZXMuAAAAAAASRGVzY3JpcHRpb25Ub29Mb25nAAAAAAP3AAAAQmBzZXRfcGxhbl9uYW1lYCB3YXMgZ2l2ZW4gYSBuYW1lIGxvbmdlciB0aGFuIGBNQVhfTkFNRV9MRU5gIGJ5dGVzLgAAAAAAD1BsYW5OYW1lVG9vTG9uZwAAAAP4AAAARmBjcmVhdGVfcGxhbmAgd2FzIGdpdmVuIGEgdGVybXMgVVJJIGxvbmdlciB0aGFuIGBNQVhfVEVSTVNfTEVOYCBieXRlcy4AAAAAAApVcmlUb29Mb25nAAAAAAP5AAAAQWBjcmVhdGVfcGxhbmAgd2FzIGdpdmVuIGEgcmVmZXJyYWwgcmF0ZSBhYm92ZSBgTUFYX1JFRkVSUkFMX0JQU2AuAAAAAAAAElJlZmVycmFsQnBzVG9vSGlnaAAAAAAD+gAAADxgY3JlYXRlX3BsYW5gIHdhcyBnaXZlbiBhbiBhbW91bnQgYmVsb3cgYE1JTl9DSEFSR0VfQU1PVU5UYC4AAAAMQW1vdW50VG9vTG93AAAD+wAAAHlgYWNjZXB0X3BsYW5fb3duZXJgIHdhcyBjYWxsZWQgZm9yIGEgcGxhbiB3aXRoIG5vIHBlbmRpbmcgb3duZXJzaGlwCnByb3Bvc2FsLCBvciBhZnRlciB0aGUgcHJvcG9zYWwgd2FzIGFscmVhZHkgYWNjZXB0ZWQuAAAAAAAAGFBlbmRpbmdPd25lcnNoaXBOb3RGb3VuZAAAA/wAAABiVGhlIGFkZHJlc3MgYWNjZXB0aW5nIG93bmVyc2hpcCBpcyBub3QgdGhlIG9uZSB0aGUgY3VycmVudCBvd25lcgpwcm9wb3NlZCBpbiBgcHJvcG9zZV9wbGFuX293bmVyYC4AAAAAAA9Ob3RQZW5kaW5nT3duZXIAAAAD/QAAACJgY3JlYXRlX3BsYW5zYCB3YXMgZ2l2ZW4gbm8gcGxhbnMuAAAAAAAKRW1wdHlCYXRjaAAAAAAD/gAAADtgY3JlYXRlX3BsYW5zYCB3YXMgZ2l2ZW4gbW9yZSBwbGFucyB0aGFuIGBNQVhfQkFUQ0hfUExBTlNgLgAAAAANQmF0Y2hUb29MYXJnZQAAAAAAA/8AAACEVGhlIG5leHQgcGxhbiBJRCBjYW5ub3QgYmUgcmVwcmVzZW50ZWQ6IHRoZSBjb3VudGVyIGlzIGF0IGB1NjQ6Ok1BWGAsCm9yIGEgYmF0Y2ggd291bGQgY2FycnkgaXQgcGFzdCBgdTY0OjpNQVhgLiBObyBwbGFuIGlzIHdyaXR0ZW4uAAAADlBsYW5JZE92ZXJmbG93AAAAAAQAAAAAPlRoZSBwbGFuIHRva2VuIGFkZHJlc3MgaXMgYSByZXNlcnZlZCBQYXlmbG93IGNvbnRyYWN0IGFkZHJlc3MuAAAAAAAMSW52YWxpZFRva2VuAAAEAQ==", "AAAAAQAAAM1BIG1lcmNoYW50J3Mgc2VsZi1kZWNsYXJlZCBwcm9maWxlLCBzaG93biB0byBzdWJzY3JpYmVycyBhbG9uZ3NpZGUgdGhlCm1lcmNoYW50J3MgcGxhbnMgc28gdGhleSBjYW4gdGVsbCB3aG8gdGhleSBhcmUgcGF5aW5nLiBUaGUgcHJvZmlsZSBpcwpzZWxmLWRlY2xhcmVkOiB0aGUgY29udHJhY3QgZG9lcyBub3QgdmVyaWZ5IHRoZSBuYW1lIG9yIHRoZSBVUkwuAAAAAAAAAAAAAAdQcm9maWxlAAAAAAMAAAAgQWNjb3VudCBhZGRyZXNzIG9mIHRoZSBtZXJjaGFudC4AAAAIbWVyY2hhbnQAAAATAAAAPURpc3BsYXkgbmFtZSwgZS5nLiAiQWNtZSBDb3JwIi4gQXQgbW9zdCBgTUFYX05BTUVfTEVOYCBieXRlcy4AAAAAAAAEbmFtZQAAABAAAAAyV2Vic2l0ZSBvciB0ZXJtcyBVUkwuIEF0IG1vc3QgYE1BWF9VUkxfTEVOYCBieXRlcy4AAAAAAAN1cmwAAAAAEA==", "AAAAAQAAANBUaGUgbXV0YWJsZSBpbnB1dHMgc2hhcmVkIGJ5IGBjcmVhdGVfcGxhbmAgYW5kIGBjcmVhdGVfcGxhbnNgLgoKVGhlIG1lcmNoYW50IGlzIHN1cHBsaWVkIHNlcGFyYXRlbHkgdG8gdGhlIGJhdGNoIGNhbGwgYW5kIGlzIHRoZSBzYW1lIGZvcgpldmVyeSBwbGFuIGluIGl0OyBldmVyeXRoaW5nIGVsc2UgdGhhdCBkZWZpbmVzIGEgcGxhbiBpcyBjYXJyaWVkIGhlcmUuAAAAAAAAAApQbGFuUGFyYW1zAAAAAAAMAAAAQEFtb3VudCBjaGFyZ2VkIHBlciBiaWxsaW5nIHBlcmlvZCwgaW4gdGhlIHRva2VuJ3Mgc21hbGxlc3QgdW5pdC4AAAAGYW1vdW50AAAAAAALAAAAT1VuaXggc2Vjb25kcyBhZnRlciB3aGljaCB0aGUgcGxhbiBzdG9wcyBhY2NlcHRpbmcgc3Vic2NyaWJlcnM7CjAgbWVhbnMgZm9yZXZlci4AAAAAD2F2YWlsYWJsZV91bnRpbAAAAAAGAAAAIURpc2NvdmVyeSBjYXRlZ29yeTsgbWF5IGJlIGVtcHR5LgAAAAAAAAhjYXRlZ29yeQAAABEAAAAzU2hvcnQgc3VtbWFyeSwgYXQgbW9zdCBgTUFYX0RFU0NSSVBUSU9OX0xFTmAgYnl0ZXMuAAAAAAtkZXNjcmlwdGlvbgAAAAAQAAAAS0tlZXBlciB0aXAgaW4gYmFzaXMgcG9pbnRzIG9mIHRoZSBtZXJjaGFudCBzaGFyZS4gQXQgbW9zdApgTUFYX0tFRVBFUl9CUFNgLgAAAAAKa2VlcGVyX2JwcwAAAAAABAAAAEJNYXhpbXVtIG51bWJlciBvZiBzaW11bHRhbmVvdXNseS1vcGVuIG1hbmRhdGVzOyAwIG1lYW5zIHVubGltaXRlZC4AAAAAAA9tYXhfc3Vic2NyaWJlcnMAAAAABAAAACtEaXNwbGF5IG5hbWUsIGF0IG1vc3QgYE1BWF9OQU1FX0xFTmAgYnl0ZXMuAAAAAARuYW1lAAAAEAAAAFtBY2NvdW50IHRoYXQgcmVjZWl2ZXMgdGhlIG1lcmNoYW50J3Mgc2hhcmUgb2YgZWFjaCBjaGFyZ2UuIGBOb25lYApkZWZhdWx0cyB0byB0aGUgbWVyY2hhbnQuAAAAAAZwYXlvdXQAAAAAA+gAAAATAAAAMUJpbGxpbmcgcGVyaW9kIGluIHNlY29uZHMuIEF0IGxlYXN0IGBNSU5fUEVSSU9EYC4AAAAAAAAGcGVyaW9kAAAAAAAGAAAAW1JlZmVycmFsIHNoYXJlIGluIGJhc2lzIHBvaW50cyBvZiB0aGUgbWVyY2hhbnQncyBzaGFyZTsgMCBkaXNhYmxlcwpyZWZlcnJhbHMgZm9yIHRoaXMgcGxhbi4AAAAADHJlZmVycmFsX2JwcwAAAAQAAAA4TGluayB0byB0aGUgcGxhbidzIHRlcm1zLCBhdCBtb3N0IGBNQVhfVEVSTVNfTEVOYCBieXRlcy4AAAAJdGVybXNfdXJpAAAAAAAAEAAAADNUb2tlbiBjb250cmFjdCBhZGRyZXNzIHRoaXMgcGxhbiBpcyBkZW5vbWluYXRlZCBpbi4AAAAABXRva2VuAAAAAAAAEw==", "AAAAAQAAAKhPbmUgcm93IG9mIGEgcGxhbiBsaXN0aW5nICgjMjg4KS4gQ2FycmllcyB0aGUgc2V0dGxlbWVudCBgdG9rZW5gIGFsb25nc2lkZQp0aGUgcHJpY2Ugc28gYSBjYXRhbG9ndWUgbWl4aW5nIHRva2VucyBpcyB1bmFtYmlndW91cyB3aXRob3V0IGEgZm9sbG93LXVwCmBnZXRfcGxhbmAgcGVyIHJvdy4AAAAAAAAAC1BsYW5MaXN0aW5nAAAAAAcAAAApV2hldGhlciB0aGUgcGxhbiBhY2NlcHRzIG5ldyBzdWJzY3JpYmVycy4AAAAAAAAGYWN0aXZlAAAAAAABAAAALlByaWNlIHBlciBwZXJpb2QgaW4gdGhlIHRva2VuJ3Mgc21hbGxlc3QgdW5pdC4AAAAAAAZhbW91bnQAAAAAAAsAAAAIUGxhbiBpZC4AAAACaWQAAAAAAAYAAAAhTWVyY2hhbnQgdGhhdCBwdWJsaXNoZWQgdGhlIHBsYW4uAAAAAAAACG1lcmNoYW50AAAAEwAAAA1EaXNwbGF5IG5hbWUuAAAAAAAABG5hbWUAAAAQAAAAGkJpbGxpbmcgcGVyaW9kIGluIHNlY29uZHMuAAAAAAAGcGVyaW9kAAAAAAAGAAAAPFNldHRsZW1lbnQgdG9rZW4gY29udHJhY3QgdGhlIHBsYW4gaXMgcHJpY2VkIGFuZCBjaGFyZ2VkIGluLgAAAAV0b2tlbgAAAAAAABM=", "AAAAAQAAACFBIHNpbmdsZSBjb250cmFjdCB1cGdyYWRlIHJlY29yZC4AAAAAAAAAAAAADVVwZ3JhZGVSZWNvcmQAAAAAAAADAAAANExlZGdlciBzZXF1ZW5jZSBudW1iZXIgd2hlbiB0aGUgdXBncmFkZSB3YXMgYXBwbGllZC4AAAAGbGVkZ2VyAAAAAAAEAAAAPFNlbWFudGljIHZlcnNpb24gb2YgdGhlIG5ldyBjb2RlIChmcm9tIGBDQVJHT19QS0dfVkVSU0lPTmApLgAAAAd2ZXJzaW9uAAAAABAAAAAaSGFzaCBvZiB0aGUgbmV3IHdhc20gY29kZS4AAAAAAAl3YXNtX2hhc2gAAAAAAAPuAAAAIA==", "AAAABQAAAC1FbWl0dGVkIHdoZW4gYSBtZXJjaGFudCBwdWJsaXNoZXMgYSBuZXcgcGxhbi4AAAAAAAAAAAAAC1BsYW5DcmVhdGVkAAAAAAEAAAAMcGxhbl9jcmVhdGVkAAAADgAAACBBY2NvdW50IHRoYXQgcHVibGlzaGVkIHRoZSBwbGFuLgAAAAhtZXJjaGFudAAAABMAAAABAAAAMVVuaXF1ZSBJRCBhbGxvY2F0ZWQgZm9yIHRoZSBuZXdseSBwdWJsaXNoZWQgcGxhbi4AAAAAAAAHcGxhbl9pZAAAAAAGAAAAAQAAADNUb2tlbiBjb250cmFjdCBhZGRyZXNzIHRoaXMgcGxhbiBpcyBkZW5vbWluYXRlZCBpbi4AAAAABXRva2VuAAAAAAAAEwAAAAAAAAAiQW1vdW50IGNoYXJnZWQgcGVyIGJpbGxpbmcgcGVyaW9kLgAAAAAABmFtb3VudAAAAAAACwAAAAAAAAAaQmlsbGluZyBwZXJpb2QgaW4gc2Vjb25kcy4AAAAAAAZwZXJpb2QAAAAAAAYAAAAAAAAAQUNhcnJpZWQgaW4gdGhlIGV2ZW50IHNvIGluZGV4ZXJzIG5lZWQgbm8gZm9sbG93LXVwIGNvbnRyYWN0IHJlYWQuAAAAAAAABG5hbWUAAAAQAAAAAAAAADZTaG9ydCBzdW1tYXJ5IG9mIHdoYXQgdGhlIHBsYW4gaW5jbHVkZXMuIE1heSBiZSBlbXB0eS4AAAAAAAtkZXNjcmlwdGlvbgAAAAAQAAAAAAAAAEdLZWVwZXIgdGlwIGZyb3plbiBpbnRvIHRoZSBwbGFuLCBpbiBiYXNpcyBwb2ludHMgb2YgdGhlIG1lcmNoYW50IHNoYXJlLgAAAAAKa2VlcGVyX2JwcwAAAAAABAAAAAAAAABARGlzY292ZXJ5IGNhdGVnb3J5IHRoZSBtZXJjaGFudCBjaG9zZSwgZW1wdHkgd2hlbiBpdCBjaG9zZSBub25lLgAAAAhjYXRlZ29yeQAAABEAAAAAAAAAfUxpbmsgdG8gdGhlIHBsYW4ncyB0ZXJtcywgY2FycmllZCBpbiB0aGUgZXZlbnQgc28gZnJvbnRlbmRzIG5lZWQgbm8KZm9sbG93LXVwIGNvbnRyYWN0IHJlYWQuIEF0IG1vc3QgMjAwIGJ5dGVzOyBtYXkgYmUgZW1wdHkuAAAAAAAACXRlcm1zX3VyaQAAAAAAABAAAAAAAAAAiFVuaXggc2Vjb25kcyBhZnRlciB3aGljaCB0aGUgcGxhbiBzdG9wcyBhY2NlcHRpbmcgbmV3IHN1YnNjcmliZXJzLAowID0gZm9yZXZlci4gQ2FycmllZCBpbiB0aGUgZXZlbnQgc28gaW5kZXhlcnMgbmVlZCBubyBmb2xsb3ctdXAgcmVhZC4AAAAPYXZhaWxhYmxlX3VudGlsAAAAAAYAAAAAAAAAW1JlZmVycmFsIHNoYXJlIGluIGJhc2lzIHBvaW50cyBvZiB0aGUgbWVyY2hhbnQncyBzaGFyZTsgMCBkaXNhYmxlcwpyZWZlcnJhbHMgZm9yIHRoaXMgcGxhbi4AAAAADHJlZmVycmFsX2JwcwAAAAQAAAAAAAAAW0FjY291bnQgdGhhdCByZWNlaXZlcyB0aGUgbWVyY2hhbnQncyBzaGFyZSBvZiBlYWNoIGNoYXJnZTsgZGVmYXVsdHMgdG8KdGhlIG1lcmNoYW50IGl0c2VsZi4AAAAABnBheW91dAAAAAAAEwAAAAAAAABzU2VhdCBjYXAgZnJvemVuIGludG8gdGhlIHBsYW47IDAgbWVhbnMgdW5saW1pdGVkLiBDYXJyaWVkIGluIHRoZSBldmVudApzbyBpbmRleGVycyBuZWVkIG5vIGZvbGxvdy11cCBjb250cmFjdCByZWFkLgAAAAAPbWF4X3N1YnNjcmliZXJzAAAAAAQAAAAAAAAAAg==", "AAAABQAAACdFbWl0dGVkIHdoZW4gYSBtZXJjaGFudCByZW5hbWVzIGEgcGxhbi4AAAAAAAAAAAtQbGFuUmVuYW1lZAAAAAABAAAADHBsYW5fcmVuYW1lZAAAAAMAAAAAAAAACG1lcmNoYW50AAAAEwAAAAEAAAAAAAAAB3BsYW5faWQAAAAABgAAAAEAAAAAAAAABG5hbWUAAAAQAAAAAAAAAAI=", "AAAABQAAAK1FbWl0dGVkIG9uY2UgYWZ0ZXIgYSBzdWNjZXNzZnVsIGBjcmVhdGVfcGxhbnNgIGJhdGNoLiBJbmRpdmlkdWFsCltgUGxhbkNyZWF0ZWRgXSBldmVudHMgcHJlY2VkZSBpdDsgdGhpcyBldmVudCBsZXRzIGFuIGluZGV4ZXIgZ3JvdXAgdGhlCnBsYW5zIHRoYXQgd2VyZSBjcmVhdGVkIGJ5IG9uZSBjYWxsLgAAAAAAAAAAAAAMUGxhbnNDcmVhdGVkAAAAAQAAAA1wbGFuc19jcmVhdGVkAAAAAAAAAgAAACFBY2NvdW50IHRoYXQgcHVibGlzaGVkIHRoZSBwbGFucy4AAAAAAAAIbWVyY2hhbnQAAAATAAAAAQAAACtJRHMgb2YgdGhlIGNyZWF0ZWQgcGxhbnMsIGluIHJlcXVlc3Qgb3JkZXIuAAAAAAhwbGFuX2lkcwAAA+oAAAAGAAAAAAAAAAI=", "AAAABQAAAG9FbWl0dGVkIHdoZW4gYSBtZXJjaGFudCBzZXRzIG9yIHVwZGF0ZXMgaXRzIHNlbGYtZGVjbGFyZWQgcHJvZmlsZS4gVGhlCnByb2ZpbGUgaXMgbm90IHZlcmlmaWVkIGJ5IHRoZSBjb250cmFjdC4AAAAAAAAAAA5Qcm9maWxlVXBkYXRlZAAAAAAAAQAAAA9wcm9maWxlX3VwZGF0ZWQAAAAAAwAAACNNZXJjaGFudCB3aG9zZSBwcm9maWxlIHdhcyB1cGRhdGVkLgAAAAAIbWVyY2hhbnQAAAATAAAAAQAAAB1EaXNwbGF5IG5hbWUgb2YgdGhlIG1lcmNoYW50LgAAAAAAAARuYW1lAAAAEAAAAAAAAAAlV2Vic2l0ZSBvciB0ZXJtcyBVUkwgb2YgdGhlIG1lcmNoYW50LgAAAAAAAAN1cmwAAAAAEAAAAAAAAAAC", "AAAABQAAAHxFbWl0dGVkIHdoZW4gdGhlIHByb3Bvc2VkIG93bmVyIGFjY2VwdHMgYW5kIHRoZSBwbGFuIGJlY29tZXMgdGhlaXJzLiBUaGUKcGxhbiBtb3ZlcyBiZXR3ZWVuIHRoZSB0d28gbWVyY2hhbnRzJyBwbGFuIGluZGV4ZXMuAAAAAAAAAA9QbGFuVHJhbnNmZXJyZWQAAAAAAQAAABBwbGFuX3RyYW5zZmVycmVkAAAAAwAAAAAAAAAIbWVyY2hhbnQAAAATAAAAAQAAAAAAAAAHcGxhbl9pZAAAAAAGAAAAAQAAAAAAAAAJbmV3X293bmVyAAAAAAAAEwAAAAAAAAAC", "AAAABQAAAJJFbWl0dGVkIHdoZW4gdGhlIGFkbWluIHJlcGxhY2VzIHRoaXMgY29udHJhY3QncyB3YXNtLiBQZXJzaXN0ZW50IHN0YXRlIGlzCnVudG91Y2hlZCBieSBhbiB1cGdyYWRlOyB0aGUgbmV3IGNvZGUgcmVhZHMgdGhlIGtleXMgdGhlIG9sZCBjb2RlIHdyb3RlLgAAAAAAAAAAABBDb250cmFjdFVwZ3JhZGVkAAAAAQAAABFjb250cmFjdF91cGdyYWRlZAAAAAAAAAIAAAAAAAAABWFkbWluAAAAAAAAEwAAAAEAAAAAAAAADW5ld193YXNtX2hhc2gAAAAAAAPuAAAAIAAAAAAAAAAC", "AAAABQAAAEtFbWl0dGVkIHdoZW4gYSBtZXJjaGFudCBwb2ludHMgYSBwbGFuIGF0IHRoZSBwbGFuIHRoYXQgcmVwbGFjZXMgaXQsIGlmIGFueS4AAAAAAAAAABBQbGFuU3VjY2Vzc29yU2V0AAAAAQAAABJwbGFuX3N1Y2Nlc3Nvcl9zZXQAAAAAAAMAAAAZTWVyY2hhbnQgb3duaW5nIHRoZSBwbGFuLgAAAAAAAAhtZXJjaGFudAAAABMAAAABAAAAGElEIG9mIHRoZSByZXRpcmluZyBwbGFuLgAAAAdwbGFuX2lkAAAAAAYAAAABAAAAJUlEIG9mIHRoZSBzdWNjZXNzb3IgcmVwbGFjZW1lbnQgcGxhbi4AAAAAAAAMc3VjY2Vzc29yX2lkAAAABgAAAAAAAAAC", "AAAABQAAADhFbWl0dGVkIHdoZW4gYSBtZXJjaGFudCBhY3RpdmF0ZXMgb3IgZGVhY3RpdmF0ZXMgYSBwbGFuLgAAAAAAAAARUGxhblN0YXR1c0NoYW5nZWQAAAAAAAABAAAAE3BsYW5fc3RhdHVzX2NoYW5nZWQAAAAAAwAAABlNZXJjaGFudCBvd25pbmcgdGhlIHBsYW4uAAAAAAAACG1lcmNoYW50AAAAEwAAAAEAAAAkSUQgb2YgdGhlIHBsYW4gd2hvc2Ugc3RhdHVzIGNoYW5nZWQuAAAAB3BsYW5faWQAAAAABgAAAAEAAAA7V2hldGhlciB0aGUgcGxhbiBpcyBhY3RpdmUgYW5kIGFjY2VwdGluZyBuZXcgc3Vic2NyaXB0aW9ucy4AAAAABmFjdGl2ZQAAAAAAAQAAAAAAAAAC", "AAAABQAAAH1FbWl0dGVkIHdoZW4gYSBtZXJjaGFudCBjaGFuZ2VzIGEgcGxhbidzIGdyYWNlIHdpbmRvdyAoIzMxMikuIE9ubHkgbmV3Cm1hbmRhdGVzIG9wZW5lZCBhZnRlciB0aGUgY2hhbmdlIGNhcnJ5IHRoZSBuZXcgd2luZG93LgAAAAAAAAAAAAASUGxhbkdyYWNlUGVyaW9kU2V0AAAAAAABAAAAFXBsYW5fZ3JhY2VfcGVyaW9kX3NldAAAAAAAAAMAAAAZTWVyY2hhbnQgb3duaW5nIHRoZSBwbGFuLgAAAAAAAAhtZXJjaGFudAAAABMAAAABAAAAKklEIG9mIHRoZSBwbGFuIHdob3NlIGdyYWNlIHdpbmRvdyBjaGFuZ2VkLgAAAAAAB3BsYW5faWQAAAAABgAAAAEAAAA1R3JhY2Ugd2luZG93IGluIHNlY29uZHMuIGAwYCBkaXNhYmxlcyB0aGUgZ3JhY2Ugdmlldy4AAAAAAAARZ3JhY2VfcGVyaW9kX3NlY3MAAAAAAAAGAAAAAAAAAAI=", "AAAABQAAAIdFbWl0dGVkIHdoZW4gYSBwbGFuJ3MgY3VycmVudCBvd25lciBwcm9wb3NlcyB0cmFuc2ZlcnJpbmcgaXQgdG8gYSBuZXcgb3duZXIuClRoZSB0cmFuc2ZlciB0YWtlcyBlZmZlY3Qgb25seSBvbmNlIHRoZSBuZXcgb3duZXIgYWNjZXB0cy4AAAAAAAAAABVQbGFuT3duZXJzaGlwUHJvcG9zZWQAAAAAAAABAAAAF3BsYW5fb3duZXJzaGlwX3Byb3Bvc2VkAAAAAAMAAAAAAAAACG1lcmNoYW50AAAAEwAAAAEAAAAAAAAAB3BsYW5faWQAAAAABgAAAAEAAAAAAAAACW5ld19vd25lcgAAAAAAABMAAAAAAAAAAg==", "AAAABQAAALVFbWl0dGVkIHdoZW4gYSBtZXJjaGFudCBwYXVzZXMgb3IgcmVzdW1lcyBjaGFyZ2luZyBmb3Igb25lIHBsYW4gKCMzMTUpLgpFdmVyeSBtYW5kYXRlIGFnYWluc3QgdGhlIHBsYW4gaXMgYWZmZWN0ZWQ7IHRoZSBwbGFuIGl0c2VsZiBzdGF5cyBhY3RpdmUgYW5kCmtlZXBzIGFjY2VwdGluZyBuZXcgc3Vic2NyaWJlcnMuAAAAAAAAAAAAABhQbGFuQ2hhcmdlc1BhdXNlZENoYW5nZWQAAAABAAAAG3BsYW5fY2hhcmdlc19wYXVzZWRfY2hhbmdlZAAAAAADAAAAGU1lcmNoYW50IG93bmluZyB0aGUgcGxhbi4AAAAAAAAIbWVyY2hhbnQAAAATAAAAAQAAACtJRCBvZiB0aGUgcGxhbiB3aG9zZSBjaGFyZ2Ugc3RhdHVzIGNoYW5nZWQuAAAAAAdwbGFuX2lkAAAAAAYAAAABAAAAPVdoZXRoZXIgY2hhcmdpbmcgaXMgbm93IHBhdXNlZCAoYHRydWVgKSBvciByZXN1bWVkIChgZmFsc2VgKS4AAAAAAAAGcGF1c2VkAAAAAAABAAAAAAAAAAI=", "AAAAAAAAACZBZGRyZXNzIG9mIHRoZSBjb250cmFjdCBhZG1pbmlzdHJhdG9yLgAAAAAABWFkbWluAAAAAAAAAAAAAAEAAAPpAAAAEwAAAAM=", "AAAAAAAAADhgbWVyY2hhbnRgJ3Mgc2VsZi1kZWNsYXJlZCBwcm9maWxlLCBpZiBvbmUgaGFzIGJlZW4gc2V0LgAAAAdwcm9maWxlAAAAAAEAAAAAAAAACG1lcmNoYW50AAAAEwAAAAEAAAPpAAAH0AAAAAdQcm9maWxlAAAAAAM=", "AAAAAAAAACVUaGUgdmVyc2lvbiBvZiB0aGlzIGNvbnRyYWN0J3MgY3JhdGUuAAAAAAAAB3ZlcnNpb24AAAAAAAAAAAEAAAAQ", "AAAAAAAAAWhMb29rIHVwIGEgcHVibGlzaGVkIHBsYW4gYnkgaXRzIHVuaXF1ZSBJRC4KClJlYWRpbmcgYSBwbGFuIGFsc28gcmVmcmVzaGVzIGl0cyBUVEwgKCMyNDcpLiBBIHBsYW4gdGhhdCBpcyB1c2VkIGJ5CmV2ZXJ5IGNoYXJnZSBidXQgbmV2ZXIgZWRpdGVkIGlzIHN0aWxsIGEgaG90IGtleTsgbGVhdmluZyBpdHMgcmVudAp1bmNoYW5nZWQgdW50aWwgdGhlIG5leHQgd3JpdGUgd291bGQgbGV0IGl0IGV4cGlyZSB3aGlsZSBtYW5kYXRlcwphcmUgb3BlbiBhZ2FpbnN0IGl0LiBUaGUgYnVtcCBpcyBjb25kaXRpb25hbDogYW4gZW50cnkgYWxyZWFkeSBwYXN0CnRoZSB0aHJlc2hvbGQga2VlcHMgaXRzIFRUTCBleGFjdGx5IGFzIGl0IHdhcy4AAAAIZ2V0X3BsYW4AAAABAAAAAAAAAAdwbGFuX2lkAAAAAAYAAAABAAAD6QAAB9AAAAAEUGxhbgAAAAM=", "AAAAAAAABABQdWJsaXNoIGEgbmV3IHBsYW4uIE9ubHkgdGhlIG1lcmNoYW50IG1heSBwdWJsaXNoIHBsYW5zIGluIGl0cyBvd24gbmFtZS4KCmBrZWVwZXJfYnBzYCBpcyB0aGUgc2hhcmUgb2YgZWFjaCBjaGFyZ2UgcGFpZCB0byB3aG9ldmVyIHNldHRsZXMgaXQsCnRha2VuIG91dCBvZiB0aGUgbWVyY2hhbnQncyBzaGFyZSwgY2FwcGVkIGF0IGBNQVhfS0VFUEVSX0JQU2AuIEl0IGlzCnBhcnQgb2YgdGhlIHBsYW4ncyBpbW11dGFibGUgdGVybXM6IGEgbWFuZGF0ZSBvcGVuZWQgYWdhaW5zdCB0aGlzIHBsYW4Ka2VlcHMgdGhlIHRpcCBpdCB3YXMgb3BlbmVkIHdpdGgsIHNvIHJhaXNpbmcgdGhlIHRpcCBsYXRlciBjYW5ub3QgdGFrZQptb25leSBmcm9tIGFuIGV4aXN0aW5nIHN1YnNjcmliZXIgb3IgbWVyY2hhbnQuCgpgdGVybXNfdXJpYCBsaW5rcyB0byB0aGUgcGxhbidzIGRlc2NyaXB0aW9uIGFuZCB0ZXJtcyBvZiBzZXJ2aWNlLCBzbyBhCnN1YnNjcmliZXIgY2FuIHJlYWQgd2hhdCB0aGV5IGFyZSBhZ3JlZWluZyB0byBiZWZvcmUgb3BlbmluZyBhCm1hbmRhdGUuIEl0IG1heSBiZSBlbXB0eSwgaXMgY2FwcGVkIGF0IGBNQVhfVEVSTVNfTEVOYCBieXRlcywgYW5kIGlzCnNlbGYtZGVjbGFyZWQgYnkgdGhlIG1lcmNoYW50OiB0aGUgY29udHJhY3QgZG9lcyBub3QgZmV0Y2ggb3IgdmVyaWZ5IGl0LgoKIyBBdXRoClJlcXVpcmVzIGF1dGhvcml6YXRpb24gZnJvbSBgbWVyY2hhbnRgLgoKIyBFdmVudHMKRW1pdHMgW2BQbGFuQ3JlYXRlZGBdLgoKIyBFcnJvcnMKUmV0dXJucyBbYEVycm9yOjpJbnZhbGlkQW1vdW50YF0gaWYgYGFtb3VudCA8PSAwYCwgW2BFcnJvcjo6QW1vdW50VG9vTG93YF0KaWYgYGFtb3VudCA8IE1JTl9DSEFSR0VfQU1PVU5UYCwgW2BFcnJvcjo6TmFtZVRvb0xvbmdgXQppZiBgbmFtZWAgZXhjZWVkcyBbYE1BWF9OQU1FX0xFTmBdLCBbYEVycm9yOjpEZXNjcmlwdGlvblRvb0xvbmdgXSBpZiBgZGVzY3JpcHRpb25gCmV4Y2VlZHMgW2BNQVhfREVTQ1JJUFRJT05fTEVOYF0sIFtgRXJyb3I6AAAAC2NyZWF0ZV9wbGFuAAAAAA0AAAAAAAAACG1lcmNoYW50AAAAEwAAAAAAAAAFdG9rZW4AAAAAAAATAAAAAAAAAAZhbW91bnQAAAAAAAsAAAAAAAAABnBlcmlvZAAAAAAABgAAAAAAAAAEbmFtZQAAABAAAAAAAAAAC2Rlc2NyaXB0aW9uAAAAABAAAAAAAAAACmtlZXBlcl9icHMAAAAAAAQAAAAAAAAACGNhdGVnb3J5AAAAEQAAAAAAAAAJdGVybXNfdXJpAAAAAAAAEAAAAAAAAAAPYXZhaWxhYmxlX3VudGlsAAAAAAYAAAAAAAAADHJlZmVycmFsX2JwcwAAAAQAAAAAAAAABnBheW91dAAAAAAD6AAAABMAAAAAAAAAD21heF9zdWJzY3JpYmVycwAAAAAEAAAAAQAAA+kAAAAGAAAAAw==", "AAAAAAAAADBBbGlhcyBmb3IgW2BQbGFuUmVnaXN0cnk6OnBsYW5fY2hhcmdlc19wYXVzZWRgXS4AAAALcGxhbl9wYXVzZWQAAAAAAQAAAAAAAAAHcGxhbl9pZAAAAAAGAAAAAQAAA+kAAAABAAAAAw==", "AAAAAAAAAelTZXQgb3IgcmVwbGFjZSBgbWVyY2hhbnRgJ3Mgc2VsZi1kZWNsYXJlZCBwcm9maWxlOiBhIGRpc3BsYXkgbmFtZSBhbmQgYQpVUkwgKHdlYnNpdGUgb3IgdGVybXMpLiBUaGUgcHJvZmlsZSBpcyBub3QgdmVyaWZpZWQgYnkgdGhlIGNvbnRyYWN0OwpzdWJzY3JpYmVycyBzaG91bGQgdHJlYXQgaXQgYXMgYSBjbGFpbSB0aGUgbWVyY2hhbnQgbWFrZXMgYWJvdXQgaXRzZWxmLgpPbmx5IHRoZSBtZXJjaGFudCBtYXkgc2V0IGl0cyBvd24gcHJvZmlsZS4KCiMgQXV0aApSZXF1aXJlcyBhdXRob3JpemF0aW9uIGZyb20gYG1lcmNoYW50YC4KCiMgRXZlbnRzCkVtaXRzIFtgUHJvZmlsZVVwZGF0ZWRgXS4KCiMgRXJyb3JzClJldHVybnMgW2BFcnJvcjo6UHJvZmlsZU5hbWVUb29Mb25nYF0gaWYgYG5hbWVgIGV4Y2VlZHMgW2BNQVhfTkFNRV9MRU5gXSwgb3IKW2BFcnJvcjo6UHJvZmlsZVVybFRvb0xvbmdgXSBpZiBgdXJsYCBleGNlZWRzIFtgTUFYX1VSTF9MRU5gXS4AAAAAAAALc2V0X3Byb2ZpbGUAAAAAAwAAAAAAAAAIbWVyY2hhbnQAAAATAAAAAAAAAARuYW1lAAAAEAAAAAAAAAADdXJsAAAAABAAAAABAAAD6QAAAAIAAAAD", "AAAAAAAAAHtJZHMgb2YgdGhlIHBsYW5zIHByaWNlZCBpbiBgdG9rZW5gLCBvbGRlc3QgZmlyc3QsIHN0YXJ0aW5nIGF0CnBvc2l0aW9uIGBzdGFydGAuIFJldHVybnMgYXQgbW9zdCBgbWluKGxpbWl0LCBNQVhfUEFHRSlgIGlkcy4AAAAAC3Rva2VuX3BsYW5zAAAAAAMAAAAAAAAABXRva2VuAAAAAAAAEwAAAAAAAAAFc3RhcnQAAAAAAAAEAAAAAAAAAAVsaW1pdAAAAAAAAAQAAAABAAAD6gAAAAY=", "AAAAAAAAA3BQdWJsaXNoIHNldmVyYWwgcGxhbnMgaW4gb25lIHRyYW5zYWN0aW9uLgoKRXZlcnkgcGxhbiBpcyB2YWxpZGF0ZWQgYmVmb3JlIGFueSBpcyB3cml0dGVuLCBzbyBhIGJhdGNoIHRoYXQKY29udGFpbnMgb25lIGludmFsaWQgcGxhbiBjcmVhdGVzIG5vbmUgb2YgdGhlbS4gSURzIGFyZSBhbGxvY2F0ZWQKc2VxdWVudGlhbGx5IGluIHJlcXVlc3Qgb3JkZXIsIGV4YWN0bHkgYXMgaWYgZWFjaCBwbGFuIGhhZCBiZWVuCnB1Ymxpc2hlZCB3aXRoIGBjcmVhdGVfcGxhbmAuCgpUaGUgYmF0Y2ggaXMgY2FwcGVkIGF0IFtgTUFYX0JBVENIX1BMQU5TYF0gcGxhbnMgdG8gc3RheSBpbnNpZGUKU29yb2JhbidzIHBlci10cmFuc2FjdGlvbiByZXNvdXJjZSBsaW1pdHMuCgojIEF1dGgKUmVxdWlyZXMgYXV0aG9yaXphdGlvbiBmcm9tIGBtZXJjaGFudGAuIFRoZSBzYW1lIG1lcmNoYW50IG93bnMgZXZlcnkKcGxhbiBpbiB0aGUgYmF0Y2guCgojIEV2ZW50cwpFbWl0cyBvbmUgW2BQbGFuQ3JlYXRlZGBdIHBlciBwbGFuLCBmb2xsb3dlZCBieSBvbmUgW2BQbGFuc0NyZWF0ZWRgXQpzdW1tYXJpc2luZyB0aGUgYmF0Y2guCgojIEVycm9ycwpSZXR1cm5zIFtgRXJyb3I6OkVtcHR5QmF0Y2hgXSBpZiBgcGxhbnNgIGlzIGVtcHR5LApbYEVycm9yOjpCYXRjaFRvb0xhcmdlYF0gaWYgaXQgaG9sZHMgbW9yZSB0aGFuIFtgTUFYX0JBVENIX1BMQU5TYF0KcGxhbnMsIFtgRXJyb3I6OlBsYW5JZE92ZXJmbG93YF0gaWYgdGhlIGJhdGNoIHdvdWxkIGNhcnJ5IHRoZSBwbGFuCklEIGNvdW50ZXIgcGFzdCBgdTY0OjpNQVhgLCBhbmQgb3RoZXJ3aXNlIHRoZSBzYW1lIHZhbGlkYXRpb24gZXJyb3JzCmFzIGBjcmVhdGVfcGxhbmAgZm9yIHRoZSBmaXJzdCBpbnZhbGlkIHBsYW4uAAAADGNyZWF0ZV9wbGFucwAAAAIAAAAAAAAACG1lcmNoYW50AAAAEwAAAAAAAAAFcGxhbnMAAAAAAAPqAAAH0AAAAApQbGFuUGFyYW1zAAAAAAABAAAD6QAAA+oAAAAGAAAAAw==", "AAAAAAAAAIRUaGUgaWQgdGhlIG5leHQgcHVibGlzaGVkIHBsYW4gd2lsbCBnZXQuIFBsYW5zIGFyZSBudW1iZXJlZCBmcm9tIDEsIHNvCmBuZXh0X3BsYW5faWQoKSAtIDFgIGlzIHRoZSBudW1iZXIgb2YgcGxhbnMgcHVibGlzaGVkIHNvIGZhci4AAAAMbmV4dF9wbGFuX2lkAAAAAAAAAAEAAAPpAAAABgAAAAM=", "AAAAAAAAATlIYW5kIHRoZSBhZG1pbiByb2xlIHRvIGFub3RoZXIgYWRkcmVzcy4gT25seSB0aGUgY3VycmVudCBhZG1pbiBtYXkKY2FsbCB0aGlzLCBhbmQgdGhlIG5ldyBhZG1pbiB0YWtlcyBlZmZlY3QgaW1tZWRpYXRlbHk6IHRoZSBvbGQgb25lCmtlZXBzIG5vIGF1dGhvcml0eS4gQSBkZXBsb3ltZW50IHdob3NlIGFkbWluIGtleSBpcyBsb3N0IG9yCnJvdGF0ZWQgaXMgb3RoZXJ3aXNlIHN0dWNrIHdpdGggaXQgZm9yIHRoZSBsaWZlIG9mIHRoZSBjb250cmFjdC4KCiMgQXV0aApSZXF1aXJlcyBhdXRob3JpemF0aW9uIGZyb20gdGhlIGN1cnJlbnQgYWRtaW4uAAAAAAAADHVwZGF0ZV9hZG1pbgAAAAEAAAAAAAAACW5ld19hZG1pbgAAAAAAABMAAAABAAAD6QAAAAIAAAAD", "AAAAAAAAAPdSZXBsYWNlIHRoaXMgY29udHJhY3QncyB3YXNtIHdpdGggY29kZSBhdCBgbmV3X3dhc21faGFzaGAuIE9ubHkgdGhlCmFkbWluIG1heSB1cGdyYWRlOyBwZXJzaXN0ZW50IHN0YXRlIGlzIHVudG91Y2hlZC4gKEZyb20gIzE1OC4pCgpSZWNvcmRzIHRoZSB1cGdyYWRlIGluIHRoZSBjb250cmFjdCdzIHVwZ3JhZGUgaGlzdG9yeSwgd2hpY2ggY2FuIGJlCnF1ZXJpZWQgdmlhIGB1cGdyYWRlX2hpc3RvcnlgIGZvciB0cmFuc3BhcmVuY3kuAAAAAAx1cGdyYWRlX3dhc20AAAABAAAAAAAAAA1uZXdfd2FzbV9oYXNoAAAAAAAD7gAAACAAAAABAAAD6QAAAAIAAAAD", "AAAAAAAAANBSdW5zIG9uY2UsIGluIHRoZSBzYW1lIHRyYW5zYWN0aW9uIHRoYXQgZGVwbG95cyB0aGUgY29udHJhY3QuIFdpdGggYQpzZXBhcmF0ZSBgaW5pdGlhbGl6ZWAgY2FsbCwgYW55b25lIHdhdGNoaW5nIHRoZSBsZWRnZXIgY291bGQgY2FsbCBpdApiZXR3ZWVuIHRoZSBkZXBsb3kgYW5kIHRoZSBvd25lcidzIG93biBjYWxsIGFuZCBtYWtlIHRoZW1zZWx2ZXMgYWRtaW4uAAAADV9fY29uc3RydWN0b3IAAAAAAAABAAAAAAAAAAVhZG1pbgAAAAAAABMAAAAA", "AAAAAAAAAdJSZW5hbWUgYSBwbGFuLiBPbmx5IHRoZSBwbGFuJ3Mgb3duIG1lcmNoYW50IG1heSBkbyB0aGlzLCBhbmQgb25seQp0aGUgbGFiZWwgY2hhbmdlczogcHJpY2UsIHBlcmlvZCwgdG9rZW4gYW5kIHN0YXR1cyBhcmUgZnJvemVuIG9uY2UKYSBwbGFuIGlzIHB1Ymxpc2hlZCwgYmVjYXVzZSBzdWJzY3JpYmVycycgbWFuZGF0ZXMgYXJlIHByaWNlZCBmcm9tCnRoZW0uCgojIEVycm9ycwpbYEVycm9yOjpQbGFuTmFtZVRvb0xvbmdgXSBpZiBgbmFtZWAgZXhjZWVkcyBbYE1BWF9OQU1FX0xFTmBdLApbYEVycm9yOjpQbGFuTm90Rm91bmRgXSBpZiBubyBzdWNoIHBsYW4gZXhpc3RzLCBhbmQKW2BFcnJvcjo6Tm90UGxhbk93bmVyYF0gaWYgYG1lcmNoYW50YCBkb2VzIG5vdCBvd24gaXQuCgojIEF1dGgKUmVxdWlyZXMgYXV0aG9yaXphdGlvbiBmcm9tIGBtZXJjaGFudGAuCgojIEV2ZW50cwpFbWl0cyBbYFBsYW5SZW5hbWVkYF0uAAAAAAANc2V0X3BsYW5fbmFtZQAAAAAAAAMAAAAAAAAACG1lcmNoYW50AAAAEwAAAAAAAAAHcGxhbl9pZAAAAAAGAAAAAAAAAARuYW1lAAAAEAAAAAEAAAPpAAAAAgAAAAM=", "AAAAAAAABABQb2ludCBhIHBsYW4gYXQgdGhlIHBsYW4gdGhhdCByZXBsYWNlcyBpdCwgaWYgYW55LgoKQSBtZXJjaGFudCB0aGF0IHJlcGxhY2VzIGEgcGxhbiByZXRpcmVzIHRoZSBvbGQgb25lIGFuZCBvcGVucyBhIG5ldyBvbmUKY2FycnlpbmcgdGhlIG5ldyB0ZXJtcywgc28gc3Vic2NyaWJlcnMgb2YgdGhlIG9sZCBwbGFuIG5lZWQgYW4gb24tY2hhaW4KcG9pbnRlciB0byB0aGUgbmV3IG9uZSB0byBiZSBhYmxlIHRvIGJlIG9mZmVyZWQgdGhlIHJlcGxhY2VtZW50LiBUaGUKcG9pbnRlciBpcyBzdG9yZWQgb24gdGhlIHJldGlyaW5nIHBsYW4gaXRzZWxmIChgUGxhbi5zdWNjZXNzb3JfaWRgLCAwCndoZW4gdGhlcmUgaXMgbm9uZSkgcmF0aGVyIHRoYW4gaW4gYSBzaWRlIGtleSwgc28gYGdldF9wbGFuYCBoYW5kcyBiYWNrCnRoZSBhbnN3ZXIgaW4gdGhlIHNhbWUgcmVhZCB0aGF0IHJldHVybnMgdGhlIHBsYW4sIGFuZCBhcHBzIG5ldmVyIG5lZWQgYQpzZWNvbmQgbG9va3VwLiBCZWNhdXNlIHRoZSBwb2ludGVyIGxpdmVzIG9uIHRoZSBvbGQgcGxhbiBpdCBuZXZlcgp0b3VjaGVzIGFuIG9wZW4gbWFuZGF0ZSdzIGZyb3plbiB0ZXJtcy4KClRoZSByZXRpcmluZyBwbGFuIG1heSBiZSByZS1wb2ludGVkLCB3aGljaCBpcyBob3cgYSBtZXJjaGFudCB0aGF0CnJlcGxhY2VzIGEgcmVwbGFjZW1lbnQgcmV0aXJlcyB0aGUgbWlkZGxlIHBsYW4uIEJvdGggcGxhbnMgbXVzdCBiZWxvbmcKdG8gYG1lcmNoYW50YCBhbmQgYmUgZGVub21pbmF0ZWQgaW4gdGhlIHNhbWUgdG9rZW47IGEgcmVwbGFjZW1lbnQgZnJvbQphbm90aGVyIG1lcmNoYW50J3MgY2F0YWxvZ3VlIG9yIGFub3RoZXIgYXNzZXQgd291bGQgYmUgbWVhbmluZ2xlc3MsIGFuZAphIHBsYW4gY2Fubm90IGJlIGl0cyBvd24gc3VjY2Vzc29yLgoKIyBBdXRoClJlcXVpcmVzIGF1dGhvcml6YXRpb24gZnJvbSBgbWVyY2hhbnRgLgoKIyBFdmVudHMKRW1pdHMgW2BQbGFuU3VjY2Vzc29yU2V0YF0uCgojIEVycm9ycwpSZXR1cm5zIFtgRXJyb3I6OlBsYW5Ob3RGAAAADXNldF9zdWNjZXNzb3IAAAAAAAADAAAAAAAAAAhtZXJjaGFudAAAABMAAAAAAAAAB3BsYW5faWQAAAAABgAAAAAAAAAMc3VjY2Vzc29yX2lkAAAABgAAAAEAAAPpAAAAAgAAAAM=", "AAAAAAAAAalJZHMgb2YgdGhlIHBsYW5zIGBtZXJjaGFudGAgcHVibGlzaGVkLCBvbGRlc3QgZmlyc3QsIHN0YXJ0aW5nIGF0CnBvc2l0aW9uIGBzdGFydGAuIFJldHVybnMgYXQgbW9zdCBgbWluKGxpbWl0LCBNQVhfUEFHRSlgIGlkcy4KClRoaXMgdXNlcyBhIHBlci1tZXJjaGFudCBpbmRleCAoYE1lcmNoYW50UGxhbkNvdW50YCArIGBNZXJjaGFudFBsYW5gKQp3aGVyZSBlYWNoIHBsYW4gb2NjdXBpZXMgYSBmaXhlZCBwb3NpdGlvbi4gUmVhZGluZyBhIHBhZ2UgdG91Y2hlcwpleGFjdGx5IGBtaW4obGltaXQsIE1BWF9QQUdFKWAgc3RvcmFnZSBlbnRyaWVzIOKAlCBvbmUgcGVyIHBsYW4g4oCUIHNvCmxpc3RpbmcgYSBtZXJjaGFudCdzIHBsYW5zIGlzIE8oMSkgcGVyIGVudHJ5IGFuZCBkb2VzIG5vdCBzY2FuCnVucmVsYXRlZCBtZXJjaGFudHMnIHBsYW5zLgAAAAAAAA5tZXJjaGFudF9wbGFucwAAAAAAAwAAAAAAAAAIbWVyY2hhbnQAAAATAAAAAAAAAAVzdGFydAAAAAAAAAQAAAAAAAAABWxpbWl0AAAAAAAABAAAAAEAAAPqAAAABg==", "AAAAAAAAAlxEZWFjdGl2YXRlIHVwIHRvIGBsaW1pdGAgb2YgYG1lcmNoYW50YCdzIHBsYW5zLCBzdGFydGluZyBhdCBwb3NpdGlvbgpgc3RhcnRgIG9mIGl0cyBwbGFuIGluZGV4LiBPbmx5IHBsYW5zIGluIGBtZXJjaGFudGAncyBvd24gaW5kZXggYXJlCnRvdWNoZWQsIHNvIGEgbWVyY2hhbnQgY2FuIG5ldmVyIGRlYWN0aXZhdGUgYW5vdGhlciBtZXJjaGFudCdzIHBsYW5zLgpBbHJlYWR5LWluYWN0aXZlIHBsYW5zIGFyZSBza2lwcGVkLiBSZXR1cm5zIGhvdyBtYW55IHBsYW5zIHdlcmUKZGVhY3RpdmF0ZWQgaW4gdGhpcyBwYWdlLiBBdCBtb3N0IGBtaW4obGltaXQsIE1BWF9QQUdFKWAgcGxhbnMgYXJlCnRvdWNoZWQsIHNvIHRoZSBjYWxsIHN0YXlzIHdpdGhpbiB0aGUgbGVkZ2VyIGVudHJ5IGZvb3RwcmludCBidWRnZXQuCgojIEF1dGgKUmVxdWlyZXMgYXV0aG9yaXphdGlvbiBmcm9tIGBtZXJjaGFudGAuCgojIEV2ZW50cwpFbWl0cyBbYFBsYW5TdGF0dXNDaGFuZ2VkYF0gZm9yIGVhY2ggcGxhbiBkZWFjdGl2YXRlZC4KCiMgRXJyb3JzClJldHVybnMgW2BFcnJvcjo6UGxhbk5vdEZvdW5kYF0gaWYgYW4gaW5kZXggc2xvdCBwb2ludHMgdG8gYSBtaXNzaW5nIHBsYW4uAAAAD2RlYWN0aXZhdGVfcGFnZQAAAAADAAAAAAAAAAhtZXJjaGFudAAAABMAAAAAAAAABXN0YXJ0AAAAAAAABAAAAAAAAAAFbGltaXQAAAAAAAAEAAAAAQAAA+kAAAAEAAAAAw==", "AAAAAAAAAVxBY3RpdmF0ZSBvciBkZWFjdGl2YXRlIGEgcGxhbi4gRGVhY3RpdmF0aW5nIHN0b3BzIG5ldyBzdWJzY3JpcHRpb25zIGJ1dApkb2VzIG5vdCBjYW5jZWwgbWFuZGF0ZXMgYWxyZWFkeSBvcGVuIGFnYWluc3QgdGhlIHBsYW4uCgojIEF1dGgKUmVxdWlyZXMgYXV0aG9yaXphdGlvbiBmcm9tIGBtZXJjaGFudGAuCgojIEV2ZW50cwpFbWl0cyBbYFBsYW5TdGF0dXNDaGFuZ2VkYF0uCgojIEVycm9ycwpSZXR1cm5zIFtgRXJyb3I6OlBsYW5Ob3RGb3VuZGBdIGlmIHRoZSBwbGFuIGRvZXMgbm90IGV4aXN0LCBvciBbYEVycm9yOjpOb3RQbGFuT3duZXJgXQppZiB0aGUgY2FsbGVyIGRvZXMgbm90IG93biB0aGUgcGxhbi4AAAAPc2V0X3BsYW5fYWN0aXZlAAAAAAMAAAAAAAAACG1lcmNoYW50AAAAEwAAAAAAAAAHcGxhbl9pZAAAAAAGAAAAAAAAAAZhY3RpdmUAAAAAAAEAAAABAAAD6QAAAAIAAAAD", "AAAAAAAAADRBbGlhcyBmb3IgW2BQbGFuUmVnaXN0cnk6OnNldF9wbGFuX2NoYXJnZXNfcGF1c2VkYF0uAAAAD3NldF9wbGFuX3BhdXNlZAAAAAADAAAAAAAAAAhtZXJjaGFudAAAABMAAAAAAAAAB3BsYW5faWQAAAAABgAAAAAAAAAGcGF1c2VkAAAAAAABAAAAAQAAA+kAAAACAAAAAw==", "AAAAAAAAAPhHZXQgdGhlIGNvbnRyYWN0J3MgdXBncmFkZSBoaXN0b3J5LgoKUmV0dXJucyBhIHZlY3RvciBvZiB1cGdyYWRlIHJlY29yZHMsIG9sZGVzdCBmaXJzdC4gRWFjaCByZWNvcmQKY29udGFpbnMgdGhlIHZlcnNpb24sIGxlZGdlciBzZXF1ZW5jZSwgYW5kIHdhc20gaGFzaC4gVGhlIGluaXRpYWwKZGVwbG95bWVudCBpcyBub3QgcmVjb3JkZWQ7IG9ubHkgZXhwbGljaXQgdXBncmFkZXMgdmlhIGB1cGdyYWRlX3dhc21gCmFwcGVhciBoZXJlLgAAAA91cGdyYWRlX2hpc3RvcnkAAAAAAAAAAAEAAAPqAAAH0AAAAA1VcGdyYWRlUmVjb3JkAAAA", "AAAAAAAAADRIb3cgbWFueSBwbGFucyBhcmUgcHJpY2VkIGluIGB0b2tlbmAsIGFjdGl2ZSBvciBub3QuAAAAEHRva2VuX3BsYW5fY291bnQAAAABAAAAAAAAAAV0b2tlbgAAAAAAABMAAAABAAAABA==", "AAAAAAAAAYxBY2NlcHQgYSBwcm9wb3NlZCBvd25lcnNoaXAgdHJhbnNmZXIuIE9ubHkgdGhlIHByb3Bvc2VkIGBuZXdfb3duZXJgCm1heSBhY2NlcHQsIGFuZCB0aGUgcGxhbiBtb3ZlcyBiZXR3ZWVuIHRoZSB0d28gbWVyY2hhbnRzJyBwbGFuCmluZGV4ZXMuIE1hbmRhdGVzIGFscmVhZHkgb3BlbiBhZ2FpbnN0IHRoZSBwbGFuIGtlZXAgcGF5aW5nIHRoZQpgcGF5b3V0YCBhZGRyZXNzIGZyb3plbiBhdCB0aGVpciBzaWduLXVwOyBhIHRyYW5zZmVyIG5ldmVyIHJlcHJpY2VzCm9yIHJlZGlyZWN0cyBhbiBleGlzdGluZyBzdWJzY3JpYmVyLCBpdCBvbmx5IGNoYW5nZXMgd2hvIG1hbmFnZXMgdGhlCnBsYW4gYW5kIHdoaWNoIG1lcmNoYW50IG5ld2x5IG9wZW5lZCBtYW5kYXRlcyBhcmUgYXR0cmlidXRlZCB0by4AAAARYWNjZXB0X3BsYW5fb3duZXIAAAAAAAACAAAAAAAAAAluZXdfb3duZXIAAAAAAAATAAAAAAAAAAdwbGFuX2lkAAAAAAYAAAABAAAD6QAAAAIAAAAD", "AAAAAAAAAPdHcmFjZSB3aW5kb3cgaW4gc2Vjb25kcyBmb3IgbmV3IG1hbmRhdGVzIGFnYWluc3QgYHBsYW5faWRgICgjMzEyKS4KRXhpc3RpbmcgbWFuZGF0ZXMga2VlcCB0aGUgd2luZG93IGZyb3plbiBpbnRvIHRoZW0gYXQgc2lnbi11cC4KRGVmYXVsdHMgdG8gYDBgLCB3aGljaCBkaXNhYmxlcyB0aGUgZ3JhY2Ugdmlldy4KCiMgRXJyb3JzClJldHVybnMgW2BFcnJvcjo6UGxhbk5vdEZvdW5kYF0gaWYgdGhlIHBsYW4gZG9lcyBub3QgZXhpc3QuAAAAABFwbGFuX2dyYWNlX3BlcmlvZAAAAAAAAAEAAAAAAAAAB3BsYW5faWQAAAAABgAAAAEAAAPpAAAABgAAAAM=", "AAAAAAAAAM5Qcm9wb3NlIHRyYW5zZmVycmluZyBhIHBsYW4gdG8gYG5ld19vd25lcmAuIFN0ZXAgb25lIG9mIGEgdHdvLXN0ZXAKaGFuZHNoYWtlOiBvbmx5IHRoZSBwbGFuJ3MgY3VycmVudCBgbWVyY2hhbnRgIG1heSBwcm9wb3NlLCBhbmQgdGhlCnRyYW5zZmVyIHRha2VzIGVmZmVjdCBvbmx5IG9uY2UgYG5ld19vd25lcmAgYWNjZXB0cy4gKE1lcmdlZCBmcm9tCiMxMzYuKQAAAAAAEnByb3Bvc2VfcGxhbl9vd25lcgAAAAAAAwAAAAAAAAAIbWVyY2hhbnQAAAATAAAAAAAAAAdwbGFuX2lkAAAAAAYAAAAAAAAACW5ld19vd25lcgAAAAAAABMAAAABAAAD6QAAAAIAAAAD", "AAAAAAAAADdIb3cgbWFueSBwbGFucyBgbWVyY2hhbnRgIGhhcyBwdWJsaXNoZWQsIGFjdGl2ZSBvciBub3QuAAAAABNtZXJjaGFudF9wbGFuX2NvdW50AAAAAAEAAAAAAAAACG1lcmNoYW50AAAAEwAAAAEAAAAE", "AAAAAAAAAHZXaGV0aGVyIGNoYXJnaW5nIGlzIHBhdXNlZCBmb3IgYHBsYW5faWRgICgjMzE1KS4KCiMgRXJyb3JzClJldHVybnMgW2BFcnJvcjo6UGxhbk5vdEZvdW5kYF0gaWYgdGhlIHBsYW4gZG9lcyBub3QgZXhpc3QuAAAAAAATcGxhbl9jaGFyZ2VzX3BhdXNlZAAAAAABAAAAAAAAAAdwbGFuX2lkAAAAAAYAAAABAAAD6QAAAAEAAAAD", "AAAAAAAAAHhMaXN0aW5nIHJvd3MgZm9yIHRoZSBwbGFucyBwcmljZWQgaW4gYHRva2VuYCwgaW4gdGhlIHNhbWUgb3JkZXIgYW5kCnBhZ2luZyBhcyBbYHRva2VuX3BsYW5zYF0oU2VsZjo6dG9rZW5fcGxhbnMpICgjMjg4KS4AAAATdG9rZW5fcGxhbl9saXN0aW5ncwAAAAADAAAAAAAAAAV0b2tlbgAAAAAAABMAAAAAAAAABXN0YXJ0AAAAAAAABAAAAAAAAAAFbGltaXQAAAAAAAAEAAAAAQAAA+oAAAfQAAAAC1BsYW5MaXN0aW5nAA==", "AAAAAAAAAnVTZXQgYSBwbGFuJ3MgZ3JhY2Ugd2luZG93IGluIHNlY29uZHMgKCMzMTIpLiBUaGUgd2luZG93IGlzIGNvcGllZAppbnRvIGVhY2ggbmV3IG1hbmRhdGUgYXQgc2lnbi11cCwgc28gYSBsYXRlciBjaGFuZ2UgYWZmZWN0cyBvbmx5CnN1YnNjcmlwdGlvbnMgb3BlbmVkIGFmdGVyIGl0LiBgMGAgZGlzYWJsZXMgdGhlIGdyYWNlIHZpZXcuCgpUaGUgd2luZG93IGlzIG9ubHkgYSB3YXJuaW5nIHBlcmlvZDogc3Vic2NyaXB0aW9uJ3MKYGlzX2luX2dyYWNlX3BlcmlvZGAgcmVwb3J0cyBhIG1hbmRhdGUgYXMgcmVjb3ZlcmFibGUgYmV0d2VlbiBpdHMKZHVlIHRpbWUgYW5kIHRoZSBlbmQgb2YgdGhlIGZyb3plbiB3aW5kb3cuIEl0IGRvZXMgbm90IGNhbmNlbCBvcgpvdGhlcndpc2UgY2hhbmdlIHRoZSBtYW5kYXRlLgoKIyBBdXRoClJlcXVpcmVzIGF1dGhvcml6YXRpb24gZnJvbSB0aGUgcGxhbidzIG1lcmNoYW50LgoKIyBFdmVudHMKRW1pdHMgW2BQbGFuR3JhY2VQZXJpb2RTZXRgXS4KCiMgRXJyb3JzClJldHVybnMgW2BFcnJvcjo6UGxhbk5vdEZvdW5kYF0gaWYgdGhlIHBsYW4gZG9lcyBub3QgZXhpc3QsIG9yCltgRXJyb3I6Ok5vdFBsYW5Pd25lcmBdIGlmIGBtZXJjaGFudGAgZG9lcyBub3Qgb3duIGl0LgAAAAAAABVzZXRfcGxhbl9ncmFjZV9wZXJpb2QAAAAAAAADAAAAAAAAAAhtZXJjaGFudAAAABMAAAAAAAAAB3BsYW5faWQAAAAABgAAAAAAAAARZ3JhY2VfcGVyaW9kX3NlY3MAAAAAAAAGAAAAAQAAA+kAAAACAAAAAw==", "AAAAAAAAAK1MaXN0aW5nIHJvd3MgZm9yIHRoZSBwbGFucyBgbWVyY2hhbnRgIHB1Ymxpc2hlZCwgaW4gdGhlIHNhbWUgb3JkZXIKYW5kIHBhZ2luZyBhcyBbYG1lcmNoYW50X3BsYW5zYF0oU2VsZjo6bWVyY2hhbnRfcGxhbnMpLiBFYWNoIHJvdwpuYW1lcyB0aGUgcGxhbidzIHNldHRsZW1lbnQgdG9rZW4gKCMyODgpLgAAAAAAABZtZXJjaGFudF9wbGFuX2xpc3RpbmdzAAAAAAADAAAAAAAAAAhtZXJjaGFudAAAABMAAAAAAAAABXN0YXJ0AAAAAAAABAAAAAAAAAAFbGltaXQAAAAAAAAEAAAAAQAAA+oAAAfQAAAAC1BsYW5MaXN0aW5nAA==", "AAAAAAAAApFQYXVzZSBvciByZXN1bWUgY2hhcmdpbmcgZm9yIGV2ZXJ5IG1hbmRhdGUgYWdhaW5zdCBvbmUgcGxhbiAoIzMxNSkuCgpQYXVzaW5nIGRvZXMgbm90IGRlYWN0aXZhdGUgdGhlIHBsYW46IGl0IGtlZXBzIGFjY2VwdGluZyBuZXcKc3Vic2NyaWJlcnMsIGFuZCBpdHMgb3BlbiBtYW5kYXRlcyBhcmUgbm90IGNhbmNlbGxlZCBvciBtb3ZlZC4gQm90aApgY2hhcmdlYCBhbmQgYGZsYWdfZmFpbGVkX2NoYXJnZWAgcmVqZWN0IGEgbWFuZGF0ZSB3aG9zZSBwbGFuIGlzCnBhdXNlZCwgc28gbm8gY2hhcmdlIHNldHRsZXMgdW50aWwgdGhlIG1lcmNoYW50IHJlc3VtZXMuIFRoaXMgbGV0cyBhCm1lcmNoYW50IHN0b3AgYmlsbGluZyBkdXJpbmcgYSBkaXNwdXRlIHdpdGhvdXQgZm9yY2luZyBzdWJzY3JpYmVycwp0byByZS1zdWJzY3JpYmUgYWZ0ZXJ3YXJkcy4KCiMgQXV0aApSZXF1aXJlcyBhdXRob3JpemF0aW9uIGZyb20gdGhlIHBsYW4ncyBtZXJjaGFudC4KCiMgRXZlbnRzCkVtaXRzIFtgUGxhbkNoYXJnZXNQYXVzZWRDaGFuZ2VkYF0uCgojIEVycm9ycwpSZXR1cm5zIFtgRXJyb3I6OlBsYW5Ob3RGb3VuZGBdIGlmIHRoZSBwbGFuIGRvZXMgbm90IGV4aXN0LCBvcgpbYEVycm9yOjpOb3RQbGFuT3duZXJgXSBpZiBgbWVyY2hhbnRgIGRvZXMgbm90IG93biBpdC4AAAAAAAAXc2V0X3BsYW5fY2hhcmdlc19wYXVzZWQAAAAAAwAAAAAAAAAIbWVyY2hhbnQAAAATAAAAAAAAAAdwbGFuX2lkAAAAAAYAAAAAAAAABnBhdXNlZAAAAAAAAQAAAAEAAAPpAAAAAgAAAAM=", "AAAAAAAAAWNDb25maWd1cmUgdGhlIHN1YnNjcmlwdGlvbiBjb250cmFjdCB3aG9zZSBhZGRyZXNzIGNhbm5vdCBiZSB1c2VkIGFzCmEgcGxhbiB0b2tlbi4gVGhpcyBpcyBzZXQgYWZ0ZXIgZGVwbG95bWVudCBiZWNhdXNlIHRoZSByZWdpc3RyeSBpcwpkZXBsb3llZCBiZWZvcmUgdGhlIHN1YnNjcmlwdGlvbiBjb250cmFjdCwgd2hpY2ggZGVwZW5kcyBvbiBpdC4KCiMgQXV0aApSZXF1aXJlcyBhdXRob3JpemF0aW9uIGZyb20gdGhlIGNvbnRyYWN0IGFkbWluLgoKIyBFdmVudHMKRW1pdHMgbm8gZXZlbnRzLgoKIyBFcnJvcnMKUmV0dXJucyBbYEVycm9yOjpOb3RJbml0aWFsaXplZGBdIGlmIHRoZSByZWdpc3RyeSBoYXMgbm8gYWRtaW4uAAAAABlzZXRfc3Vic2NyaXB0aW9uX2NvbnRyYWN0AAAAAAAAAQAAAAAAAAAMc3Vic2NyaXB0aW9uAAAAEwAAAAEAAAPpAAAAAgAAAAM=", "AAAAAQAAAPtBIG1lcmNoYW50LXB1Ymxpc2hlZCBjaGFyZ2UgdGVtcGxhdGUuIEEgYFBsYW5gIGlzIGltbXV0YWJsZSBleGNlcHQgZm9yIGl0cwpgYWN0aXZlYCBmbGFnIGFuZCBpdHMgYHN1Y2Nlc3Nvcl9pZGAgcG9pbnRlcjogY2hhbmdpbmcgcHJpY2UsIHBlcmlvZCBvcgprZWVwZXIgdGlwIHJlcXVpcmVzIHB1Ymxpc2hpbmcgYSBuZXcgcGxhbiBzbyBleGlzdGluZyBtYW5kYXRlcyBjYW4gbmV2ZXIgYmUKcmVwcmljZWQgdW5kZXIgYSBzdWJzY3JpYmVyLgAAAAAAAAAABFBsYW4AAAAQAAAAU1doZXRoZXIgdGhlIHBsYW4gYWNjZXB0cyBuZXcgc3Vic2NyaXB0aW9ucy4gSW5hY3RpdmUgcGxhbnMgY2Fubm90IGJlIHN1YnNjcmliZWQgdG8uAAAAAAZhY3RpdmUAAAAAAAEAAAC9QW1vdW50IGNoYXJnZWQgcGVyIHBlcmlvZCwgaW4gdGhlIHRva2VuJ3Mgc21hbGxlc3QgdW5pdC4gVGhpcyBpcyB3aGF0CnRoZSBzdWJzY3JpYmVyIHBheXMgaW4gdG90YWw7IHRoZSBwcm90b2NvbCBmZWUgYW5kIHRoZSBrZWVwZXIgdGlwIGFyZQpib3RoIHRha2VuIG91dCBvZiBpdCBiZWZvcmUgdGhlIG1lcmNoYW50IGlzIHBhaWQuAAAAAAAABmFtb3VudAAAAAAACwAAAPZVbml4IHNlY29uZHMgYWZ0ZXIgd2hpY2ggdGhlIHBsYW4gc3RvcHMgYWNjZXB0aW5nIG5ldyBzdWJzY3JpYmVyczsKMCBtZWFucyBpdCBpcyBhdmFpbGFibGUgZm9yZXZlci4gT25seSBuZXcgc3Vic2NyaXB0aW9ucyBhcmUKYWZmZWN0ZWQ6IG1hbmRhdGVzIG9wZW5lZCBiZWZvcmUgdGhlIGN1dG9mZiBrZWVwIGJpbGxpbmcuIFRoZQpzdWJzY3JpcHRpb24gY29udHJhY3QgZW5mb3JjZXMgdGhlIGN1dG9mZiBpbiBgc3Vic2NyaWJlYC4AAAAAAA9hdmFpbGFibGVfdW50aWwAAAAABgAAAQlNZXJjaGFudC1zZWxlY3RlZCBkaXNjb3ZlcnkgY2F0ZWdvcnksIHNvIHN1YnNjcmliZXJzIGNhbiBicm93c2UKcGxhbnMgYnkgd2hhdCB0aGV5IGFyZSBmb3IuIFNvcm9iYW4gc3ltYm9scyBhcmUgc2hvcnQgYW5kIGxpbWl0ZWQgdG8KdGhlIHN5bWJvbCBhbHBoYWJldCwgd2hpY2ggaXMgd2hhdCBtYWtlcyB0aGVtIGNoZWFwIHRvIGluZGV4OyB0aGlzCm1heSBiZSBlbXB0eSwgYW5kIGl0IGlzIGZyb3plbiB3aXRoIHRoZSByZXN0IG9mIHRoZSBwbGFuJ3MgdGVybXMuAAAAAAAACGNhdGVnb3J5AAAAEQAAAIlTaG9ydCwgcGxhaW4gc3VtbWFyeSBvZiB3aGF0IHRoZSBwbGFuIGluY2x1ZGVzLCBzaG93biB0bwpzdWJzY3JpYmVycyBuZXh0IHRvIHRoZSBuYW1lLiBBdCBtb3N0IGBNQVhfREVTQ1JJUFRJT05fTEVOYCBieXRlczsKbWF5IGJlIGVtcHR5LgAAAAAAAAtkZXNjcmlwdGlvbgAAAAAQAAAAR1VuaXF1ZSBpZGVudGlmaWVyIGZvciB0aGUgcGxhbiwgYWxsb2NhdGVkIHNlcXVlbnRpYWxseSBzdGFydGluZyBmcm9tIDEuAAAAAAJpZAAAAAAABgAAAMZTaGFyZSBvZiBlYWNoIGNoYXJnZSBwYWlkIHRvIHdoaWNoZXZlciBhY2NvdW50IHNldHRsZXMgaXQsIGluIGJhc2lzCnBvaW50cyBvZiB0aGUgbWVyY2hhbnQncyBzaGFyZS4gMCBtZWFucyB0aGUgbWVyY2hhbnQga2VlcHMgaXQgYWxsLgpBdCBtb3N0IGBNQVhfS0VFUEVSX0JQU2A7IHRoZSByZWdpc3RyeSByZWplY3RzIGFueXRoaW5nIGhpZ2hlci4AAAAAAAprZWVwZXJfYnBzAAAAAAAEAAAAhk1heGltdW0gbnVtYmVyIG9mIHNpbXVsdGFuZW91c2x5LW9wZW4gbWFuZGF0ZXMgdGhlIHBsYW4gYWNjZXB0czsgMAptZWFucyB1bmxpbWl0ZWQuIEZyb3plbiBpbnRvIHRoZSBwbGFuLCBjaGVja2VkIG9uIGVhY2ggbmV3IHNpZ24tdXAuAAAAAAAPbWF4X3N1YnNjcmliZXJzAAAAAAQAAABDQWNjb3VudCB0aGF0IHB1Ymxpc2hlcyB0aGUgcGxhbiBhbmQgcmVjZWl2ZXMgc3Vic2NyaXB0aW9uIHBheW1lbnRzLgAAAAAIbWVyY2hhbnQAAAATAAAAakh1bWFuLXJlYWRhYmxlIGxhYmVsIHNob3duIHRvIHN1YnNjcmliZXJzLCBlLmcuICJQcm8gTW9udGhseSIuCkF0IG1vc3QgYE1BWF9OQU1FX0xFTmAgYnl0ZXM7IG1heSBiZSBlbXB0eS4AAAAAAARuYW1lAAAAEAAAAMhBY2NvdW50IHRoYXQgYWN0dWFsbHkgcmVjZWl2ZXMgdGhlIG1lcmNoYW50J3Mgc2hhcmUgb2YgZWFjaCBjaGFyZ2UuCkZyb3plbiBpbnRvIGV2ZXJ5IG1hbmRhdGUgYXQgc2lnbi11cCwgc28gYSBsYXRlciBwYXlvdXQgY2hhbmdlIG9yIGEKcGxhbi1vd25lcnNoaXAgdHJhbnNmZXIgbmV2ZXIgcmVkaXJlY3RzIGFuIG9wZW4gbWFuZGF0ZSdzIGZ1bmRzLgAAAAZwYXlvdXQAAAAAABMAAAAaQmlsbGluZyBwZXJpb2QgaW4gc2Vjb25kcy4AAAAAAAZwZXJpb2QAAAAAAAYAAAC/UmVmZXJyYWwgc2hhcmUgcGFpZCB0byBhIG1hbmRhdGUncyByZWZlcnJlciwgaW4gYmFzaXMgcG9pbnRzIG9mIHRoZQptZXJjaGFudCdzIHNoYXJlIChvdXQgb2YgdGhlIG1lcmNoYW50J3Mgb3duIG1vbmV5LCBuZXZlciB0aGUgZmVlIG9yCnRpcCkuIDAgZGlzYWJsZXMgcmVmZXJyYWxzLiBBdCBtb3N0IGBNQVhfUkVGRVJSQUxfQlBTYC4AAAAADHJlZmVycmFsX2JwcwAAAAQAAAFIVGhlIHBsYW4gdGhhdCByZXBsYWNlcyB0aGlzIG9uZSwgaWYgdGhlIG1lcmNoYW50IHJldGlyZWQgaXQuIFNldCBieQpgc2V0X3N1Y2Nlc3NvcmAgYW5kIHJlYWQgdGhyb3VnaCBgZ2V0X3BsYW5gLCBzbyBhcHBzIGNhbiBwb2ludCBhCnN1YnNjcmliZXIgYXQgdGhlIHJlcGxhY2VtZW50IHdpdGhvdXQgYSBzZWNvbmQgbG9va3VwLiAwIG1lYW5zIHRoZSBwbGFuCmhhcyBubyBzdWNjZXNzb3IuIFRoZSBwb2ludGVyIG5ldmVyIHJlLXByaWNlcyBhbiBvcGVuIG1hbmRhdGU6IGEgbWFuZGF0ZQprZWVwcyB0aGUgdGVybXMgb2YgdGhlIHBsYW4gaXQgd2FzIG9wZW5lZCBhZ2FpbnN0LgAAAAxzdWNjZXNzb3JfaWQAAAAGAAAA7kxpbmsgdG8gdGhlIHBsYW4ncyBkZXNjcmlwdGlvbiBhbmQgdGVybXMgb2Ygc2VydmljZSwgc2hvd24gdG8Kc3Vic2NyaWJlcnMgYmVmb3JlIHRoZXkgb3BlbiBhIG1hbmRhdGUuIEF0IG1vc3QgYE1BWF9URVJNU19MRU5gCigyMDApIGJ5dGVzOyBtYXkgYmUgZW1wdHkuIFNlbGYtZGVjbGFyZWQgYnkgdGhlIG1lcmNoYW50LCBsaWtlIHRoZQpwcm9maWxlIFVSTDogdGhlIGNvbnRyYWN0IGRvZXMgbm90IHZlcmlmeSBpdC4AAAAAAAl0ZXJtc191cmkAAAAAAAAQAAAAKVNFUC00MSB0b2tlbiB0aGlzIHBsYW4gaXMgZGVub21pbmF0ZWQgaW4uAAAAAAAABXRva2VuAAAAAAAAEw=="]),
      options
    );
  }

   static deploy<T = Client>(args: { admin: string | Address }, options: MethodOptions & Omit<ContractClientOptions, 'contractId'> & { salt?: Uint8Array; address?: string; } & ({ wasmHash: Uint8Array | string; format?: "hex" | "base64"; externalRef?: never; } | { externalRef: ExternalExecutableRef; wasmHash?: never; format?: never; })): Promise<AssembledTransaction<T>> {
    return ContractClient.deploy(args, options);
  }
  public readonly fromJson = {
    admin : this.txFromJson<Result<string, Error>>,  profile : this.txFromJson<Result<Profile, Error>>,  version : this.txFromJson<string>,  get_plan : this.txFromJson<Result<Plan, Error>>,  create_plan : this.txFromJson<Result<bigint, Error>>,  plan_paused : this.txFromJson<Result<boolean, Error>>,  set_profile : this.txFromJson<Result<null, Error>>,  token_plans : this.txFromJson<Array<bigint>>,  create_plans : this.txFromJson<Result<Array<bigint>, Error>>,  next_plan_id : this.txFromJson<Result<bigint, Error>>,  update_admin : this.txFromJson<Result<null, Error>>,  upgrade_wasm : this.txFromJson<Result<null, Error>>,  set_plan_name : this.txFromJson<Result<null, Error>>,  set_successor : this.txFromJson<Result<null, Error>>,  merchant_plans : this.txFromJson<Array<bigint>>,  deactivate_page : this.txFromJson<Result<number, Error>>,  set_plan_active : this.txFromJson<Result<null, Error>>,  set_plan_paused : this.txFromJson<Result<null, Error>>,  upgrade_history : this.txFromJson<Array<UpgradeRecord>>,  token_plan_count : this.txFromJson<number>,  accept_plan_owner : this.txFromJson<Result<null, Error>>,  plan_grace_period : this.txFromJson<Result<bigint, Error>>,  propose_plan_owner : this.txFromJson<Result<null, Error>>,  merchant_plan_count : this.txFromJson<number>,  plan_charges_paused : this.txFromJson<Result<boolean, Error>>,  token_plan_listings : this.txFromJson<Array<PlanListing>>,  set_plan_grace_period : this.txFromJson<Result<null, Error>>,  merchant_plan_listings : this.txFromJson<Array<PlanListing>>,  set_plan_charges_paused : this.txFromJson<Result<null, Error>>,  set_subscription_contract : this.txFromJson<Result<null, Error>>
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
   * Build a topics filter row for the "PlanCreated" event, for use in `Api.EventFilter.topics` when calling `server.getEvents`. Omitted fields match any value.
   */
  planCreatedEventFilter(topicValues?: { merchant?: string | Address; plan_id?: bigint }): string[] {
    return this.spec.eventTopicFilter("PlanCreated", topicValues);
  }
  /**
   * Build a topics filter row for the "PlanRenamed" event, for use in `Api.EventFilter.topics` when calling `server.getEvents`. Omitted fields match any value.
   */
  planRenamedEventFilter(topicValues?: { merchant?: string | Address; plan_id?: bigint }): string[] {
    return this.spec.eventTopicFilter("PlanRenamed", topicValues);
  }
  /**
   * Build a topics filter row for the "PlansCreated" event, for use in `Api.EventFilter.topics` when calling `server.getEvents`. Omitted fields match any value.
   */
  plansCreatedEventFilter(topicValues?: { merchant?: string | Address }): string[] {
    return this.spec.eventTopicFilter("PlansCreated", topicValues);
  }
  /**
   * Build a topics filter row for the "ProfileUpdated" event, for use in `Api.EventFilter.topics` when calling `server.getEvents`. Omitted fields match any value.
   */
  profileUpdatedEventFilter(topicValues?: { merchant?: string | Address }): string[] {
    return this.spec.eventTopicFilter("ProfileUpdated", topicValues);
  }
  /**
   * Build a topics filter row for the "PlanTransferred" event, for use in `Api.EventFilter.topics` when calling `server.getEvents`. Omitted fields match any value.
   */
  planTransferredEventFilter(topicValues?: { merchant?: string | Address; plan_id?: bigint }): string[] {
    return this.spec.eventTopicFilter("PlanTransferred", topicValues);
  }
  /**
   * Build a topics filter row for the "ContractUpgraded" event, for use in `Api.EventFilter.topics` when calling `server.getEvents`. Omitted fields match any value.
   */
  contractUpgradedEventFilter(topicValues?: { admin?: string | Address }): string[] {
    return this.spec.eventTopicFilter("ContractUpgraded", topicValues);
  }
  /**
   * Build a topics filter row for the "PlanSuccessorSet" event, for use in `Api.EventFilter.topics` when calling `server.getEvents`. Omitted fields match any value.
   */
  planSuccessorSetEventFilter(topicValues?: { merchant?: string | Address; plan_id?: bigint }): string[] {
    return this.spec.eventTopicFilter("PlanSuccessorSet", topicValues);
  }
  /**
   * Build a topics filter row for the "PlanStatusChanged" event, for use in `Api.EventFilter.topics` when calling `server.getEvents`. Omitted fields match any value.
   */
  planStatusChangedEventFilter(topicValues?: { merchant?: string | Address; plan_id?: bigint }): string[] {
    return this.spec.eventTopicFilter("PlanStatusChanged", topicValues);
  }
  /**
   * Build a topics filter row for the "PlanGracePeriodSet" event, for use in `Api.EventFilter.topics` when calling `server.getEvents`. Omitted fields match any value.
   */
  planGracePeriodSetEventFilter(topicValues?: { merchant?: string | Address; plan_id?: bigint }): string[] {
    return this.spec.eventTopicFilter("PlanGracePeriodSet", topicValues);
  }
  /**
   * Build a topics filter row for the "PlanOwnershipProposed" event, for use in `Api.EventFilter.topics` when calling `server.getEvents`. Omitted fields match any value.
   */
  planOwnershipProposedEventFilter(topicValues?: { merchant?: string | Address; plan_id?: bigint }): string[] {
    return this.spec.eventTopicFilter("PlanOwnershipProposed", topicValues);
  }
  /**
   * Build a topics filter row for the "PlanChargesPausedChanged" event, for use in `Api.EventFilter.topics` when calling `server.getEvents`. Omitted fields match any value.
   */
  planChargesPausedChangedEventFilter(topicValues?: { merchant?: string | Address; plan_id?: bigint }): string[] {
    return this.spec.eventTopicFilter("PlanChargesPausedChanged", topicValues);
  }
}