#!/usr/bin/env bash
set -euo pipefail

# Auth test coverage gate for #99
# Ensures every mutating function in every contract has a negative auth test.
# Exit 0 if all mutating functions are covered; exit 1 if any are missing.

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$repo_root"

exit_code=0

# Contract definitions: name, lib.rs path, test.rs path, and allowlisted functions
declare -A contracts=(
    [subscription]="contracts/subscription/src"
    [vault]="contracts/vault/src"
    [plan-registry]="contracts/plan-registry/src"
)

# Allowlisted functions per contract (views, permissionless, and constructors)
declare -A allowlist_subscription=(
    [get_mandate]=1
    [is_due]=1
    [committed_per_period]=1
    [subscriber_mandate_count]=1
    [subscriber_mandates]=1
    [merchant_mandate_count]=1
    [merchant_mandates]=1
    [fee_bps]=1
    [admin]=1
    [plan_registry]=1
    [vault]=1
    [charge]=1  # permissionless by design (ADR 0005)
    [version]=1
    [version]=1
    [__constructor]=1  # constructors don't require auth tests
)

declare -A allowlist_vault=(
    [deposit_cap]=1
    [balance]=1
    [allowlist_enabled]=1
    [token_allowed]=1
    [subscription]=1
    [admin]=1
    [version]=1
    [version]=1
    [__constructor]=1  # constructors don't require auth tests
)

declare -A allowlist_plan_registry=(
    [profile]=1
    [get_plan]=1
    [merchant_plan_count]=1
    [merchant_plans]=1
    [next_plan_id]=1
    [admin]=1
    [version]=1
    [version]=1
    [__constructor]=1  # constructors don't require auth tests
)

echo "🔍 Checking auth test coverage across all contracts..."
echo

for contract_name in "${!contracts[@]}"; do
    contract_path="${contracts[$contract_name]}"
    lib_file="$contract_path/lib.rs"
    test_file="$contract_path/test.rs"
    
    echo "📋 Contract: $contract_name"
    
    if [[ ! -f "$lib_file" ]]; then
        echo "  ❌ ERROR: $lib_file not found"
        exit_code=1
        continue
    fi
    
    if [[ ! -f "$test_file" ]]; then
        echo "  ❌ ERROR: $test_file not found"
        exit_code=1
        continue
    fi
    
    # Extract public function names from #[contractimpl] block
    # This matches lines like: "pub fn function_name(" inside the impl block
    mapfile -t public_fns < <(
        awk '
            /#\[contractimpl\]/ { in_impl=1; next }
            in_impl && /^[[:space:]]*pub fn [a-z_]+[[:space:]]*\(/ {
                match($0, /pub fn ([a-z_]+)/, arr)
                print arr[1]
            }
            in_impl && /^[[:space:]]*\}[[:space:]]*$/ && prev_brace { in_impl=0 }
            { prev_brace = (/^[[:space:]]*\}[[:space:]]*$/) }
        ' "$lib_file" | sort -u
    )
    
    if [[ ${#public_fns[@]} -eq 0 ]]; then
        echo "  ⚠️  WARNING: No public functions found in $lib_file"
        continue
    fi
    
    # Filter to mutating functions (exclude allowlisted views)
    declare -a mutating_fns=()
    allowlist_var="allowlist_${contract_name//-/_}"
    declare -n allowlist_ref="$allowlist_var"
    
    for fn in "${public_fns[@]}"; do
        if [[ -z "${allowlist_ref[$fn]:-}" ]]; then
            mutating_fns+=("$fn")
        fi
    done
    
    echo "  ✓ Found ${#public_fns[@]} public functions (${#mutating_fns[@]} mutating)"
    
    # Extract auth test function names from test.rs
    # Matches patterns like: fn subscribe_requires_subscriber_auth()
    #                    or: fn subscribe_requires_the_subscriber_to_authorize()
    #                    or: fn set_paused_requires_subscriber_auth()
    mapfile -t auth_tests < <(
        grep -oE 'fn [a-z_]+_requires(_the)?_[a-z_]+_(auth|to_authorize)\(\)' "$test_file" | \
        sed 's/fn //; s/_requires\(_the\)\?_[a-z_]*_\(auth\|to_authorize\)()//' | \
        sort -u
    )
    
    # Also match: fn <function>_wrong_signer_rejected()
    mapfile -t wrong_signer_tests < <(
        grep -oE 'fn [a-z_]+_wrong_signer_rejected\(\)' "$test_file" | \
        sed 's/fn //; s/_wrong_signer_rejected()//' | \
        sort -u
    )
    
    # Combine both test patterns
    all_tested_fns=()
    for fn in "${auth_tests[@]}" "${wrong_signer_tests[@]}"; do
        all_tested_fns+=("$fn")
    done
    
    # Remove duplicates
    mapfile -t all_tested_fns < <(printf '%s\n' "${all_tested_fns[@]}" | sort -u)
    
    echo "  ✓ Found ${#all_tested_fns[@]} auth tests"
    
    # Check coverage: every mutating function must have an auth test
    missing_tests=()
    for fn in "${mutating_fns[@]}"; do
        found=0
        for tested_fn in "${all_tested_fns[@]}"; do
            if [[ "$fn" == "$tested_fn" ]]; then
                found=1
                break
            fi
        done
        if [[ $found -eq 0 ]]; then
            missing_tests+=("$fn")
        fi
    done
    
    if [[ ${#missing_tests[@]} -gt 0 ]]; then
        echo "  ❌ MISSING AUTH TESTS for:"
        for fn in "${missing_tests[@]}"; do
            echo "     - $fn"
        done
        exit_code=1
    else
        echo "  ✅ All mutating functions have auth tests"
    fi
    
    echo
done

if [[ $exit_code -eq 0 ]]; then
    echo "✅ Auth test coverage gate: PASS"
    echo "   All mutating functions across all contracts have negative auth tests."
else
    echo "❌ Auth test coverage gate: FAIL"
    echo "   Add negative auth tests for the missing functions listed above."
    echo "   Test naming convention: <function>_requires_<role>_auth or <function>_wrong_signer_rejected"
fi

exit $exit_code
