use soroban_sdk::{contracttype, Address, Env, Map};

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct BalanceDetail {
    pub total: i128,
    pub locked: i128,
    pub free: i128,
}

#[contracttype]
#[derive(Clone)]
pub enum DataKey {
    Admin,
    AuthorizedCaller,
    Balance(Address, Address),
    Reserve(Address, Address, u64),
    MandateReservesList(Address, Address),
    RateLimitParams,
    RateLimit(Address, Address, u64),
}

pub fn set_admin(env: &Env, admin: &Address) {
    env.storage().instance().set(&DataKey::Admin, admin);
}

pub fn get_admin(env: &Env) -> Address {
    env.storage().instance().get(&DataKey::Admin).unwrap()
}

pub fn set_authorized_caller(env: &Env, caller: &Address) {
    env.storage().instance().set(&DataKey::AuthorizedCaller, caller);
}

pub fn get_authorized_caller(env: &Env) -> Option<Address> {
    env.storage().instance().get(&DataKey::AuthorizedCaller)
}

pub fn set_rate_limit_params(env: &Env, window_duration: u64, max_ops_per_window: u32) {
    env.storage().instance().set(&DataKey::RateLimitParams, &(window_duration, max_ops_per_window));
}

pub fn get_total_balance(env: &Env, subscriber: &Address, token: &Address) -> i128 {
    env.storage().persistent().get(&DataKey::Balance(subscriber.clone(), token.clone())).unwrap_or(0)
}

pub fn add_balance(env: &Env, subscriber: &Address, token: &Address, amount: i128) {
    let current = get_total_balance(env, subscriber, token);
    let new_val = current.checked_add(amount).unwrap();
    env.storage().persistent().set(&DataKey::Balance(subscriber.clone(), token.clone()), &new_val);
}

pub fn sub_balance(env: &Env, subscriber: &Address, token: &Address, amount: i128) {
    let current = get_total_balance(env, subscriber, token);
    let new_val = current.checked_sub(amount).unwrap();
    env.storage().persistent().set(&DataKey::Balance(subscriber.clone(), token.clone()), &new_val);
}

pub fn add_reserve(env: &Env, subscriber: &Address, token: &Address, mandate_id: u64, amount: i128) {
    let key = DataKey::Reserve(subscriber.clone(), token.clone(), mandate_id);
    let current: i128 = env.storage().persistent().get(&key).unwrap_or(0);
    env.storage().persistent().set(&key, &(current.checked_add(amount).unwrap()));

    let list_key = DataKey::MandateReservesList(subscriber.clone(), token.clone());
    let mut map: Map<u64, i128> = env.storage().persistent().get(&list_key).unwrap_or_else(|| Map::new(env));
    map.set(mandate_id, current.checked_add(amount).unwrap());
    env.storage().persistent().set(&list_key, &map);
}

pub fn sub_reserve(env: &Env, subscriber: &Address, token: &Address, mandate_id: u64, amount: i128) {
    let key = DataKey::Reserve(subscriber.clone(), token.clone(), mandate_id);
    let current: i128 = env.storage().persistent().get(&key).unwrap_or(0);
    let new_val = current.checked_sub(amount).unwrap();
    if new_val == 0 {
        env.storage().persistent().remove(&key);
    } else {
        env.storage().persistent().set(&key, &new_val);
    }

    let list_key = DataKey::MandateReservesList(subscriber.clone(), token.clone());
    let mut map: Map<u64, i128> = env.storage().persistent().get(&list_key).unwrap_or_else(|| Map::new(env));
    if new_val == 0 {
        map.remove(mandate_id);
    } else {
        map.set(mandate_id, new_val);
    }
    env.storage().persistent().set(&list_key, &map);
}

pub fn get_total_reserves(env: &Env, subscriber: &Address, token: &Address) -> i128 {
    let list_key = DataKey::MandateReservesList(subscriber.clone(), token.clone());
    let map: Map<u64, i128> = env.storage().persistent().get(&list_key).unwrap_or_else(|| Map::new(env));
    let mut total: i128 = 0;
    for (_id, amt) in map.iter() {
        total = total.checked_add(amt).unwrap();
    }
    total
}

pub fn get_free_balance(env: &Env, subscriber: &Address, token: &Address) -> i128 {
    let total = get_total_balance(env, subscriber, token);
    let locked = get_total_reserves(env, subscriber, token);
    total.checked_sub(locked).unwrap_or(0)
}

pub fn check_rate_limit(env: &Env, subscriber: &Address, token: &Address) {
    let (window_duration, max_ops) = env.storage().instance().get(&DataKey::RateLimitParams).unwrap_or((60, 8));
    let now = env.ledger().timestamp();
    let window_id = now / window_duration;
    let r_key = DataKey::RateLimit(subscriber.clone(), token.clone(), window_id);
    let count: u32 = env.storage().temporary().get(&r_key).unwrap_or(0);
    if count >= max_ops {
        panic!("rate limit exceeded");
    }
    env.storage().temporary().set(&r_key, &(count + 1));
}
