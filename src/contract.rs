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
            None => Ok(None),
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
        .query_balance(env.contract.address, config.target_denom.to_string())?;
    let send = config
        .target_denom
        .send(&config.target_address, &balance.amount);
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
mod tests {

    use super::*;
    use cosmwasm_std::{
        coin, from_json,
        testing::{mock_dependencies, mock_dependencies_with_balances, mock_env, mock_info},
        Uint128,
    };

    #[test]
    fn instantiation() {
        let mut deps = mock_dependencies();
        let info = mock_info("owner", &vec![]);
        let msg = InstantiateMsg {
            owner: Addr::unchecked("owner"),
            target_denom: Denom::from("ukuji"),
            target_address: fee_address(),
        };
        instantiate(deps.as_mut(), mock_env(), info, msg).unwrap();
        let config: ConfigResponse =
            from_json(query(deps.as_ref(), mock_env(), QueryMsg::Config {}).unwrap()).unwrap();
        assert_eq!(config.owner, Addr::unchecked("owner"));
        assert_eq!(config.target_denom, Denom::from("ukuji"));
        let status: StatusResponse =
            from_json(query(deps.as_ref(), mock_env(), QueryMsg::Status {}).unwrap()).unwrap();
        assert_eq!(status.last, None);
        let actions: ActionsResponse =
            from_json(query(deps.as_ref(), mock_env(), QueryMsg::Actions {}).unwrap()).unwrap();
        assert_eq!(actions.actions, vec![]);
    }
    #[test]
    fn authorization() {
        let mut deps = mock_dependencies();
        let info = mock_info("owner", &vec![]);
        let msg = InstantiateMsg {
            owner: Addr::unchecked("owner"),
            target_denom: Denom::from("ukuji"),
            target_address: fee_address(),
        };
        instantiate(deps.as_mut(), mock_env(), info.clone(), msg).unwrap();

        execute(
            deps.as_mut(),
            mock_env(),
            info.clone(),
            ExecuteMsg::SetOwner(Addr::unchecked("owner-new")),
        )
        .unwrap();

        execute(
            deps.as_mut(),
            mock_env(),
            info.clone(),
            ExecuteMsg::SetOwner(Addr::unchecked("owner-new")),
        )
        .unwrap_err();

        let action = Action {
            denom: Denom::from("uatom"),
            contract: Addr::unchecked("fin"),
            limit: Uint128::MAX,
            msg: Binary::default(),
        };

        execute(
            deps.as_mut(),
            mock_env(),
            info.clone(),
            ExecuteMsg::SetAction(action.clone()),
        )
        .unwrap_err();

        execute(
            deps.as_mut(),
            mock_env(),
            mock_info("owner-new", &vec![]),
            ExecuteMsg::SetAction(action.clone()),
        )
        .unwrap();

        let actions: ActionsResponse =
            from_json(query(deps.as_ref(), mock_env(), QueryMsg::Actions {}).unwrap()).unwrap();
        assert_eq!(
            actions.actions,
            vec![ActionResponse {
                denom: action.denom.clone(),
                contract: action.contract,
                limit: action.limit,
                msg: action.msg
            }]
        );

        execute(
            deps.as_mut(),
            mock_env(),
            info,
            ExecuteMsg::UnsetAction(action.denom.clone()),
        )
        .unwrap_err();
        execute(
            deps.as_mut(),
            mock_env(),
            mock_info("owner-new", &vec![]),
            ExecuteMsg::UnsetAction(action.denom),
        )
        .unwrap();

        let actions: ActionsResponse =
            from_json(query(deps.as_ref(), mock_env(), QueryMsg::Actions {}).unwrap()).unwrap();
        assert_eq!(actions.actions, vec![]);
    }

    #[test]
    fn cranking() {
        let mut deps = mock_dependencies_with_balances(&[(
            "cosmos2contract",
            &[
                // coin(1000u128, "token-a"),
                coin(1000u128, "token-b"),
                coin(1000u128, "token-c"),
                coin(1000u128, "token-d"),
                coin(1000u128, "token-e"),
            ],
        )]);
        let info = mock_info("contract-0", &vec![]);
        let msg = InstantiateMsg {
            owner: Addr::unchecked("owner"),
            target_denom: Denom::from("ukuji"),
            target_address: fee_address(),
        };
        instantiate(deps.as_mut(), mock_env(), info.clone(), msg).unwrap();

        // Make sure that execution ends when there are no actions
        sudo(deps.as_mut(), mock_env(), SudoMsg::Run {}).unwrap();
        let status: StatusResponse =
            from_json(query(deps.as_ref(), mock_env(), QueryMsg::Status {}).unwrap()).unwrap();
        assert_eq!(status.last, None);

        // Set some actions
        set_action(deps.as_mut(), "token-a", "contract-a", Uint128::MAX);
        set_action(deps.as_mut(), "token-b", "contract-b", Uint128::MAX);
        set_action(
            deps.as_mut(),
            "token-c",
            "contract-c",
            Uint128::from(100u128),
        );
        set_action(deps.as_mut(), "token-d", "contract-d", Uint128::MAX);
        set_action(deps.as_mut(), "token-e", "contract-e", Uint128::MAX);

        let res = sudo(deps.as_mut(), mock_env(), SudoMsg::Run {}).unwrap();
        // Nothing done
        assert_eq!(res.events.len(), 0);
        let status: StatusResponse =
            from_json(query(deps.as_ref(), mock_env(), QueryMsg::Status {}).unwrap()).unwrap();
        assert_eq!(status.last, Some(Denom::from("token-a")));

        // Iterator should start at the beginning again and execute token-a
        let res = sudo(deps.as_mut(), mock_env(), SudoMsg::Run {}).unwrap();
        let status: StatusResponse =
            from_json(query(deps.as_ref(), mock_env(), QueryMsg::Status {}).unwrap()).unwrap();
        assert_eq!(status.last, Some(Denom::from("token-b")));
        assert_eq!(res.events[0].clone().ty, "revenue/run");
        assert_eq!(res.events[0].clone().attributes[0].clone().key, "denom");
        assert_eq!(res.events[0].clone().attributes[0].clone().value, "token-b");

        // Run for c, d, e and then loop back to a
        let res = sudo(deps.as_mut(), mock_env(), SudoMsg::Run {}).unwrap();
        assert_eq!(res.events[0].clone().attributes[0].clone().value, "token-c");

        let res = sudo(deps.as_mut(), mock_env(), SudoMsg::Run {}).unwrap();
        assert_eq!(res.events[0].clone().attributes[0].clone().value, "token-d");

        let res = sudo(deps.as_mut(), mock_env(), SudoMsg::Run {}).unwrap();
        assert_eq!(res.events[0].clone().attributes[0].clone().value, "token-e");

        let res = sudo(deps.as_mut(), mock_env(), SudoMsg::Run {}).unwrap();

        assert_eq!(res.events.len(), 0);
        let status: StatusResponse =
            from_json(query(deps.as_ref(), mock_env(), QueryMsg::Status {}).unwrap()).unwrap();
        assert_eq!(status.last, Some(Denom::from("token-a")));
    }

    fn set_action(deps: DepsMut, denom: &str, contract: &str, limit: Uint128) {
        execute(
            deps,
            mock_env(),
            mock_info("owner", &vec![]),
            ExecuteMsg::SetAction(Action {
                denom: Denom::from(denom),
                contract: Addr::unchecked(contract),
                limit: limit,
                msg: Binary::default(),
            }),
        )
        .unwrap();
    }
}
