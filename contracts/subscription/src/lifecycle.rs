use crate::{Mandate, MandateStatus};

pub(super) fn active_at(mandate: &Mandate, now: u64) -> bool {
    match mandate.status {
        MandateStatus::Active => true,
        MandateStatus::Paused => mandate.paused_until != 0 && now >= mandate.paused_until,
        _ => false,
    }
}

pub(super) fn in_grace_period_at(mandate: &Mandate, grace_period_secs: u64, now: u64) -> bool {
    grace_period_secs != 0
        && active_at(mandate, now)
        && now >= mandate.next_charge
        && now < mandate.next_charge.saturating_add(grace_period_secs)
}
