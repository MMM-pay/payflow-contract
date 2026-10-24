# PR: Add `set_plan_name` function to plan-registry contract

## Summary
Add a `set_plan_name(merchant, plan_id, name)` function that allows a merchant to rename any of their plans. This addresses the issue where typographical errors or plans that no longer fit (like the demo plan "Demo Monthly" that bills every minute) cannot be corrected.

## Changes Made

### 1. `contracts/plan-registry/src/events.rs`
- Added new `PlanRenamed` contract event with:
  - `merchant` (topic) - the plan's merchant address
  - `plan_id` (topic) - the plan's unique identifier
  - `name` - the new plan name as a String

### 2. `contracts/plan-registry/src/error.rs`
- Added `PlanNameTooLong = 12` error variant at the end of the enum
- This follows the convention of adding new error variants at the end so existing codes keep their numbers

### 3. `contracts/plan-registry/src/lib.rs`
- Imported `PlanRenamed` event from the events module
- Added `set_plan_name(env, merchant, plan_id, name)` function that:
  - Requires authorization from the plan's merchant (`merchant.require_auth()`)
  - Validates the name length does not exceed `MAX_NAME_LEN` (64 bytes), returning `Error::PlanNameTooLong` if exceeded
  - Retrieves the plan from storage, returning `Error::PlanNotFound` if the plan doesn't exist
  - Checks that the calling merchant is the plan's owner, returning `Error::NotPlanOwner` if not
  - Updates the plan's name field and extends the plan's TTL
  - Emits `PlanRenamed { merchant, plan_id, name }` event so indexers and the UI can update immediately

### 4. `contracts/plan-registry/src/test.rs`
Added 5 new test cases:
- `set_plan_name_happy_path` - merchant can successfully rename a plan
- `set_plan_name_rejects_non_owner` - a different signer fails with `Error::NotPlanOwner`
- `set_plan_name_rejects_overlong_name` - names longer than `MAX_NAME_LEN` bytes are rejected with `Error::PlanNameTooLong`
- `set_plan_name_accepts_name_at_length_limit` - names exactly at `MAX_NAME_LEN` bytes are accepted
- `set_plan_name_emits_plan_renamed` - verifies the `PlanRenamed` event is emitted with correct topics

### 5. `CHANGELOG.md`
- Added entry under "Unreleased > Added" documenting the new `set_plan_name` function

## Testing
All 96 existing tests continue to pass, plus the 5 new tests for `set_plan_name`:
- `cargo test --all` passes (37 plan-registry tests + 44 subscription tests + 15 vault tests)
- `cargo clippy --all --all-targets` passes with no warnings
- `cargo fmt --check` passes with no formatting issues

## Implementation Notes
- The function follows the existing patterns in the codebase:
  - Uses `merchant.require_auth()` for authorization (consistent with `create_plan`, `set_plan_active`, `deactivate_page`)
  - Extends TTL on persistent storage writes (consistent with existing `write_*` helpers)
  - Validates name length using `MAX_NAME_LEN` (same constant used by `create_plan` and `set_profile`)
  - Adds error variant at the end of the enum (code 12) to preserve existing error code mappings
  - Emits an event with `#[topic]` fields for indexer compatibility
- The `Plan` type is immutable apart from the `active` flag (for `set_plan_active`) and now also the `name` field (for `set_plan_name`)
- Plan names are display-only: they mandate the copy of price and period at subscribe time, so changing the name never reprices existing mandates

## Follow-up Items
- Interface/event changes ripple into `payflow-backend`, `payflow-frontend`, and `payflow-docs`:
  - Backend: handle `PlanRenamed` event to update plan name displays
  - Frontend: provide UI for merchants to rename plans
  - Docs: update documentation for the new function
- Consider whether the `plan-registry` contract needs to be redeployed on testnet (stored plan names would need migration)