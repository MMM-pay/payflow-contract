use crate::{Error, Mandate, BPS_DENOMINATOR};
use payflow_shared::{MAX_KEEPER_BPS, MAX_REFERRAL_BPS};

pub(super) fn split_amount(mandate: &Mandate, amount: i128) -> Result<(i128, i128, i128, i128), Error> {
pub(super) fn split(mandate: &Mandate) -> Result<(i128, i128, i128, i128), Error> {
    split_amount(mandate, mandate.amount)
}

pub(super) fn split_amount(
    mandate: &Mandate,
    amount: i128,
) -> Result<(i128, i128, i128, i128), Error> {
    let fee = amount
        .checked_mul(i128::from(mandate.fee_bps))
        .ok_or(Error::AmountOverflow)?
        / BPS_DENOMINATOR;
    let merchant_gross = amount - fee;

    let tip = merchant_gross
        .checked_mul(i128::from(mandate.keeper_bps.min(MAX_KEEPER_BPS)))
        .ok_or(Error::AmountOverflow)?
        / BPS_DENOMINATOR;
    let referral = if mandate.referrer.is_some() {
        merchant_gross
            .checked_mul(i128::from(mandate.referral_bps.min(MAX_REFERRAL_BPS)))
            .ok_or(Error::AmountOverflow)?
            / BPS_DENOMINATOR
    } else {
        0
    };

    Ok((fee, tip, referral, merchant_gross - tip - referral))
}

pub(super) fn split(mandate: &Mandate) -> Result<(i128, i128, i128, i128), Error> {
    split_amount(mandate, mandate.amount)
}

pub(super) fn merchant_received(mandate: &Mandate) -> Result<i128, Error> {
    let (_, _, _, merchant_amount) = split(mandate)?;
    let recurring_charges = mandate
        .charges_made
        .saturating_sub(u32::from(mandate.extra_charge_used));
    merchant_amount
        .checked_mul(i128::from(recurring_charges))
        .and_then(|amount| amount.checked_add(mandate.extra_charge_merchant_received))
        .ok_or(Error::AmountOverflow)
}
