#[cfg(not(feature = "library"))]
use cosmwasm_std::entry_point;
use cosmwasm_std::{
    to_json_binary, Addr, Binary, CosmosMsg, Deps, DepsMut, Env, Event, MessageInfo, Reply,
    Response, StdResult, SubMsg,
};
use kujira::{fee_address, Denom};

use crate::error::ContractError;
use crate::msg::{
    ActionResponse, ActionsResponse, ConfigResponse, ExecuteMsg, InstantiateMsg, QueryMsg,
    StatusResponse, SudoMsg,
};
use crate::state::{Action, Config};

// version info for migration info
const CONTRACT_NAME: &str = "crates.io:kujira-revenue-converter";
const CONTRACT_VERSION: &str = env!("CARGO_PKG_VERSION");

#[cfg_attr(not(feature = "library"), entry_point)]
pub fn instantiate(
    deps: DepsMut,
    _env: Env,
    _info: MessageInfo,
    msg: InstantiateMsg,
) -> Result<Response, ContractError> {
    cw2::set_contract_version(deps.storage, CONTRACT_NAME, CONTRACT_VERSION)?;
    let config = Config::from(msg);
    config.save(deps.storage)?;
    Ok(Response::default())
}

#[cfg_attr(not(feature = "library"), entry_point)]
pub fn execute(
    deps: DepsMut,
    _env: Env,
    info: MessageInfo,
    msg: ExecuteMsg,
) -> Result<Response, ContractError> {
    let mut config = Config::load(deps.storage)?;
    if info.sender != config.owner.to_string() {
        return Err(ContractError::Unauthorized {});
    }
    match msg {
        ExecuteMsg::SetOwner(owner) => {
            config.owner = owner;
            config.save(deps.storage)?;
            Ok(Response::default())
        }
        ExecuteMsg::SetAction(action) => {
            Action::set(deps.storage, action)?;
            Ok(Response::default())
        }
        ExecuteMsg::UnsetAction(denom) => {
            Action::unset(deps.storage, denom);
            Ok(Response::default())
        }
    }
}

#[cfg_attr(not(feature = "library"), entry_point)]
pub fn sudo(deps: DepsMut, env: Env, msg: SudoMsg) -> Result<Response, ContractError> {
    match msg {
        SudoMsg::Run {} => {
            if let Some((action, msg)) = get_action_msg(deps, &env.contract.address)? {
                let event =
                    Event::new("revenue/run").add_attribute("denom", action.denom.to_string());
                return Ok(Response::default()
                    .add_event(event)
                    .add_submessage(SubMsg::reply_always(msg, 0)));
            }
            return Ok(Response::default());
        }
    }
}

fn get_action_msg(deps: DepsMut, contract: &Addr) -> StdResult<Option<(Action, CosmosMsg)>> {
    // Fetch the next action in the iterator
    if let Some(action) = Action::next(deps.storage)? {
        let balance = deps
            .querier
            .query_balance(contract, action.denom.to_string())?;

        return match action.execute(balance)? {
            None => {
                // Nothing to do. Don't waste this execution, look for the next action with something to do
                // Action::next will have stored the previous key and continue the iterator, until failing at the end
                get_action_msg(deps, contract)
            }
            Some(msg) => Ok(Some((action, msg))),
        };
    }
    Ok(None)
}

#[cfg_attr(not(feature = "library"), entry_point)]
pub fn reply(deps: DepsMut, env: Env, _msg: Reply) -> Result<Response, ContractError> {
    let config = Config::load(deps.storage)?;
    let balance = deps
        .querier
        .query_balance(env.contract.address, config.revenue_denom.to_string())?;
    let send = config.revenue_denom.send(&fee_address(), &balance.amount);
    Ok(Response::default()
        .add_message(send)
        .add_event(Event::new("revenue/reply").add_attribute("send", balance.to_string())))
}

#[cfg_attr(not(feature = "library"), entry_point)]
pub fn query(deps: Deps, _env: Env, msg: QueryMsg) -> StdResult<Binary> {
    match msg {
        QueryMsg::Config {} => to_json_binary(&ConfigResponse::from(Config::load(deps.storage)?)),
        QueryMsg::Actions {} => to_json_binary(&ActionsResponse {
            actions: Action::all(deps.storage)?
                .iter()
                .map(|x| ActionResponse::from(x.clone()))
                .collect(),
        }),
        QueryMsg::Status {} => to_json_binary(&StatusResponse {
            last: Action::last(deps.storage)?.map(Denom::from),
        }),
    }
}

#[cfg(test)]
mod tests {}
