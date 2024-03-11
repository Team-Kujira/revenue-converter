use std::cmp::min;

use crate::msg::{ActionResponse, ConfigResponse, InstantiateMsg};
use cosmwasm_schema::cw_serde;
use cosmwasm_std::{
    coins, Addr, Binary, Coin, CosmosMsg, Order, StdError, StdResult, Storage, Uint128, WasmMsg,
};
use cw_storage_plus::{Bound, Item, Map};
use kujira::Denom;

static CONFIG: Item<Config> = Item::new("config");
static LAST: Item<String> = Item::new("last");
static ACTIONS: Map<String, (Addr, Uint128, Binary)> = Map::new("actions");

#[cw_serde]
pub struct Config {
    /// The address permitted to set Actions
    pub owner: Addr,

    /// The address permitted to execute the crank
    pub executor: Addr,

    /// The denom that is transferred to the fee_collector at the end of every execution
    pub target_denom: Denom,

    /// The final destination that `target_denom` is sent to
    pub target_address: Addr,
}

impl Config {
    pub fn load(storage: &dyn Storage) -> StdResult<Self> {
        CONFIG.load(storage)
    }

    pub fn save(&self, storage: &mut dyn Storage) -> StdResult<()> {
        CONFIG.save(storage, self)
    }
}

impl From<InstantiateMsg> for Config {
    fn from(value: InstantiateMsg) -> Self {
        Self {
            owner: value.owner,
            executor: value.executor,
            target_denom: value.target_denom,
            target_address: value.target_address,
        }
    }
}

impl From<Config> for ConfigResponse {
    fn from(value: Config) -> Self {
        Self {
            owner: value.owner,
            executor: value.executor,
            target_denom: value.target_denom,
            target_address: value.target_address,
        }
    }
}

#[cw_serde]
pub struct Action {
    /// Token denom
    pub denom: Denom,
    /// The target contract for swapping
    pub contract: Addr,
    /// The maximum amount of the token that can be included in any one execution of the Action
    pub limit: Uint128,
    /// The msg executed on the contract to swap to the target token
    pub msg: Binary,
}

impl Action {
    pub fn last(storage: &dyn Storage) -> StdResult<Option<String>> {
        LAST.may_load(storage)
    }

    pub fn next(storage: &mut dyn Storage) -> StdResult<Option<Self>> {
        let min = LAST.may_load(storage)?.map(Bound::exclusive);
        match ACTIONS
            .range(storage, min, None, Order::Ascending)
            .take(1)
            .collect::<StdResult<Vec<(String, (Addr, Uint128, Binary))>>>()?
            .first()
        {
            Some(res) => Ok(Some(Self::load(storage, res)?)),
            // If there's nothing next, try the start
            None => {
                if let Some(res) = ACTIONS.first(storage)? {
                    return Ok(Some(Self::load(storage, &res)?));
                }
                Ok(None)
            }
        }
    }

    fn load(
        storage: &mut dyn Storage,
        (denom, (contract, limit, msg)): &(String, (Addr, Uint128, Binary)),
    ) -> StdResult<Self> {
        LAST.save(storage, denom)?;
        Ok(Self {
            denom: Denom::from(denom),
            contract: contract.clone(),
            limit: *limit,
            msg: msg.clone(),
        })
    }

    pub fn all(storage: &dyn Storage) -> StdResult<Vec<Self>> {
        ACTIONS
            .range(storage, None, None, Order::Ascending)
            .map(|res| match res {
                Ok((denom, (contract, limit, msg))) => Ok(Self {
                    denom: Denom::from(denom),
                    contract,
                    limit,
                    msg,
                }),
                Err(err) => Err(err),
            })
            .collect()
    }

    pub fn set(storage: &mut dyn Storage, action: Self) -> StdResult<()> {
        ACTIONS.save(
            storage,
            action.denom.to_string(),
            &(action.contract, action.limit, action.msg),
        )
    }

    pub fn unset(storage: &mut dyn Storage, denom: Denom) {
        ACTIONS.remove(storage, denom.to_string())
    }

    pub fn execute(&self, amount: Coin) -> StdResult<Option<CosmosMsg>> {
        if amount.denom != self.denom.to_string() {
            return Err(StdError::generic_err("Invalid Denom"));
        }
        let total = min(amount.amount, self.limit);
        if total.is_zero() {
            return Ok(None);
        }
        Ok(Some(CosmosMsg::Wasm(WasmMsg::Execute {
            contract_addr: self.contract.to_string(),
            msg: self.msg.clone(),
            funds: coins(total.u128(), amount.denom),
        })))
    }
}

impl From<Action> for ActionResponse {
    fn from(value: Action) -> Self {
        Self {
            denom: value.denom,
            contract: value.contract,
            limit: value.limit,
            msg: value.msg,
        }
    }
}
