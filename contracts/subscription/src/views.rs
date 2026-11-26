use crate::{lifecycle, Mandate};

pub(super) fn mandate_is_due(mandate: &Mandate, now: u64) -> bool {
    lifecycle::active_at(mandate, now)
        && (mandate.max_charges == 0 || mandate.charges_made < mandate.max_charges)
        && (mandate.max_total == 0
            || mandate.total_spent.saturating_add(mandate.amount) <= mandate.max_total)
        && now >= mandate.next_charge
}
