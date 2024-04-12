use cosmwasm_schema::{cw_serde, QueryResponses};
use cosmwasm_std::{Addr, Binary, Uint128};
use kujira::Denom;

use crate::state::Action;

#[cw_serde]
pub struct InstantiateMsg {
    pub owner: Addr,
    pub executor: Addr,
    pub target_denom: Denom,
    pub target_addresses: Vec<(Addr, u8)>,
}

#[cw_serde]
pub enum ExecuteMsg {
    SetOwner(Addr),
    SetExecutor(Addr),
    SetAction(Action),
    UnsetAction(Denom),
    Run {},
}

#[cw_serde]
#[derive(QueryResponses)]
pub enum QueryMsg {
    #[returns(ConfigResponse)]
    Config {},
    #[returns(ActionsResponse)]
    Actions {},
    #[returns(StatusResponse)]
    Status {},
}

#[cw_serde]
pub struct ConfigResponse {
    pub owner: Addr,
    pub executor: Addr,
    pub target_denom: Denom,
    pub target_addresses: Vec<(Addr, u8)>,
}

#[cw_serde]
pub struct ActionsResponse {
    pub actions: Vec<ActionResponse>,
}
#[cw_serde]
pub struct ActionResponse {
    pub denom: Denom,
    pub contract: Addr,
    pub limit: Uint128,
    pub msg: Binary,
}

#[cw_serde]
pub struct StatusResponse {
    pub last: Option<Denom>,
}
