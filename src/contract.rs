#[cfg(not(feature = "library"))]
use cosmwasm_std::entry_point;
use cosmwasm_std::{
    to_json_binary, Addr, Binary, CosmosMsg, Decimal, Deps, DepsMut, Env, Event, MessageInfo,
    QuerierWrapper, Reply, Response, StdResult, Storage, SubMsg,
};
use kujira::Denom;

use crate::error::ContractError;
use crate::msg::{
    ActionResponse, ActionsResponse, ConfigResponse, ExecuteMsg, InstantiateMsg, QueryMsg,
    StatusResponse,
};
use crate::state::{Action, Config};

// version info for migration info
const CONTRACT_NAME: &str = "crates.io:kujira-revenue-converter";
const CONTRACT_VERSION: &str = env!("CARGO_PKG_VERSION");

#[cfg_attr(not(feature = "library"), entry_point)]
pub fn migrate(deps: DepsMut, _env: Env, msg: InstantiateMsg) -> Result<Response, ContractError> {
    cw2::set_contract_version(deps.storage, CONTRACT_NAME, CONTRACT_VERSION)?;
    let config = Config::from(msg);
    config.save(deps.storage)?;
    Ok(Response::default())
}

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
    env: Env,
    info: MessageInfo,
    msg: ExecuteMsg,
) -> Result<Response, ContractError> {
    let mut config = Config::load(deps.storage)?;
    match msg {
        ExecuteMsg::SetOwner(owner) => {
            if info.sender != config.owner {
                return Err(ContractError::Unauthorized {});
            }

            config.owner = owner;
            config.save(deps.storage)?;
            Ok(Response::default())
        }
        ExecuteMsg::SetAction(action) => {
            if info.sender != config.owner {
                return Err(ContractError::Unauthorized {});
            }

            Action::set(deps.storage, action)?;
            Ok(Response::default())
        }
        ExecuteMsg::UnsetAction(denom) => {
            if info.sender != config.owner {
                return Err(ContractError::Unauthorized {});
            }

            Action::unset(deps.storage, denom);
            Ok(Response::default())
        }
        ExecuteMsg::SetExecutor(executor) => {
            if info.sender != config.owner {
                return Err(ContractError::Unauthorized {});
            }

            config.executor = executor;
            config.save(deps.storage)?;
            Ok(Response::default())
        }
        ExecuteMsg::Run {} => {
            if info.sender != config.executor {
                return Err(ContractError::Unauthorized {});
            }
            let action_msg = get_action_msg(deps.storage, deps.querier, &env.contract.address)?;

            match action_msg {
                Some((action, msg)) => {
                    let event =
                        Event::new("revenue/run").add_attribute("denom", action.denom.to_string());
                    Ok(Response::default()
                        .add_event(event)
                        .add_submessage(SubMsg::reply_always(msg, 0)))
                }
                // If there's no compatible action, skip to the reply
                None => {
                    let mut sends: Vec<CosmosMsg> = vec![];
                    for target in config.target_denoms.clone() {
                        distribute_denom(deps.as_ref(), &env, &config, &mut sends, target)?;
                    }

                    Ok(Response::default().add_messages(sends))
                }
            }
        }
    }
}

fn get_action_msg(
    storage: &mut dyn Storage,
    querier: QuerierWrapper,
    contract: &Addr,
) -> StdResult<Option<(Action, CosmosMsg)>> {
    // Fetch the next action in the iterator
    if let Some(action) = Action::next(storage)? {
        let balance = querier.query_balance(contract, action.denom.to_string())?;
        return match action.execute(balance)? {
            None => Ok(None),
            Some(msg) => Ok(Some((action, msg))),
        };
    }
    Ok(None)
}

#[cfg_attr(not(feature = "library"), entry_point)]
pub fn reply(deps: DepsMut, env: Env, _msg: Reply) -> Result<Response, ContractError> {
    execute_reply(deps.as_ref(), env)
}

pub fn execute_reply(deps: Deps, env: Env) -> Result<Response, ContractError> {
    let config = Config::load(deps.storage)?;
    let mut sends: Vec<CosmosMsg> = vec![];
    for target in config.target_denoms.clone() {
        distribute_denom(deps, &env, &config, &mut sends, target)?;
    }

    Ok(Response::default().add_messages(sends))
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

fn distribute_denom(
    deps: Deps,
    env: &Env,
    config: &Config,
    sends: &mut Vec<CosmosMsg>,
    denom: Denom,
) -> StdResult<()> {
    let balance = deps
        .querier
        .query_balance(env.contract.address.clone(), denom.to_string())?;

    let total_weight = config.target_addresses.iter().fold(0, |a, e| e.1 + a);
    if !balance.amount.is_zero() {
        let mut remaining = balance.amount;
        let mut targets = config.target_addresses.iter().peekable();

        while let Some((addr, weight)) = targets.next() {
            let amount = if targets.peek().is_none() {
                remaining
            } else {
                let ratio = Decimal::from_ratio(*weight, total_weight);
                balance.amount.mul_floor(ratio)
            };

            if amount.is_zero() {
                continue;
            }
            remaining -= amount;
            sends.push(denom.send(&addr, &amount))
        }
    };
    Ok(())
}

#[cfg(test)]
mod tests {

    use super::*;
    use cosmwasm_std::{
        coin, coins, from_json,
        testing::{mock_dependencies, mock_dependencies_with_balances, mock_env, mock_info},
        BankMsg, ReplyOn, Uint128,
    };
    use kujira::fee_address;

    #[test]
    fn instantiation() {
        let mut deps = mock_dependencies();
        let info = mock_info("owner", &vec![]);
        let msg = InstantiateMsg {
            owner: Addr::unchecked("owner"),
            target_denoms: vec![Denom::from("ukuji"), Denom::from("another")],
            target_addresses: vec![(fee_address(), 1)],
            executor: Addr::unchecked("executor"),
        };
        instantiate(deps.as_mut(), mock_env(), info, msg).unwrap();
        let config: ConfigResponse =
            from_json(query(deps.as_ref(), mock_env(), QueryMsg::Config {}).unwrap()).unwrap();
        assert_eq!(config.owner, Addr::unchecked("owner"));
        assert_eq!(
            config.target_denoms,
            vec![Denom::from("ukuji"), Denom::from("another")],
        );
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
            target_denoms: vec![Denom::from("ukuji"), Denom::from("another")],
            target_addresses: vec![(fee_address(), 1)],
            executor: Addr::unchecked("executor"),
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

        execute(
            deps.as_mut(),
            mock_env(),
            mock_info("owner-new", &vec![]),
            ExecuteMsg::Run {},
        )
        .unwrap_err();

        execute(
            deps.as_mut(),
            mock_env(),
            mock_info("executor", &vec![]),
            ExecuteMsg::Run {},
        )
        .unwrap();
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
            target_denoms: vec![Denom::from("ukuji"), Denom::from("another")],
            target_addresses: vec![(fee_address(), 1)],
            executor: Addr::unchecked("executor"),
        };
        instantiate(deps.as_mut(), mock_env(), info.clone(), msg).unwrap();

        // Make sure that execution ends when there are no actions
        execute(
            deps.as_mut(),
            mock_env(),
            mock_info("executor", &vec![]),
            ExecuteMsg::Run {},
        )
        .unwrap();
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

        let res = execute(
            deps.as_mut(),
            mock_env(),
            mock_info("executor", &vec![]),
            ExecuteMsg::Run {},
        )
        .unwrap();
        // Nothing done
        assert_eq!(res.events.len(), 0);
        let status: StatusResponse =
            from_json(query(deps.as_ref(), mock_env(), QueryMsg::Status {}).unwrap()).unwrap();
        assert_eq!(status.last, Some(Denom::from("token-a")));

        // Iterator should start at the beginning again and execute token-a
        let res = execute(
            deps.as_mut(),
            mock_env(),
            mock_info("executor", &vec![]),
            ExecuteMsg::Run {},
        )
        .unwrap();
        let status: StatusResponse =
            from_json(query(deps.as_ref(), mock_env(), QueryMsg::Status {}).unwrap()).unwrap();
        assert_eq!(status.last, Some(Denom::from("token-b")));
        assert_eq!(res.events[0].clone().ty, "revenue/run");
        assert_eq!(res.events[0].clone().attributes[0].clone().key, "denom");
        assert_eq!(res.events[0].clone().attributes[0].clone().value, "token-b");

        // Run for c, d, e and then loop back to a
        let res = execute(
            deps.as_mut(),
            mock_env(),
            mock_info("executor", &vec![]),
            ExecuteMsg::Run {},
        )
        .unwrap();
        assert_eq!(res.events[0].clone().attributes[0].clone().value, "token-c");

        let res = execute(
            deps.as_mut(),
            mock_env(),
            mock_info("executor", &vec![]),
            ExecuteMsg::Run {},
        )
        .unwrap();
        assert_eq!(res.events[0].clone().attributes[0].clone().value, "token-d");

        let res = execute(
            deps.as_mut(),
            mock_env(),
            mock_info("executor", &vec![]),
            ExecuteMsg::Run {},
        )
        .unwrap();
        assert_eq!(res.events[0].clone().attributes[0].clone().value, "token-e");

        let res = execute(
            deps.as_mut(),
            mock_env(),
            mock_info("executor", &vec![]),
            ExecuteMsg::Run {},
        )
        .unwrap();

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

    #[test]
    fn distribution() {
        let mut deps = mock_dependencies_with_balances(&[(
            "cosmos2contract",
            &[coin(1000u128, "ukuji"), coin(2000u128, "another")],
        )]);
        let info = mock_info("contract-0", &vec![]);
        let msg = InstantiateMsg {
            owner: Addr::unchecked("owner"),
            target_denoms: vec![Denom::from("ukuji"), Denom::from("another")],
            target_addresses: vec![(fee_address(), 1), (Addr::unchecked("another"), 3)],
            executor: Addr::unchecked("executor"),
        };
        instantiate(deps.as_mut(), mock_env(), info.clone(), msg).unwrap();
        // Dummy action to make sure it cranks the reply
        set_action(deps.as_mut(), "token-a", "contract-a", Uint128::MAX);

        // Make sure that execution ends when there are no actions
        let res = execute(
            deps.as_mut(),
            mock_env(),
            mock_info("executor", &vec![]),
            ExecuteMsg::Run {},
        )
        .unwrap();

        assert!(res.messages.contains(&SubMsg {
            id: 0,
            msg: CosmosMsg::Bank(BankMsg::Send {
                to_address: "kujira17xpfvakm2amg962yls6f84z3kell8c5lp3pcxh".to_string(),
                amount: coins(250, "ukuji"),
            },),
            gas_limit: None,
            reply_on: ReplyOn::Never,
        }));
        assert!(res.messages.contains(&SubMsg {
            id: 0,
            msg: CosmosMsg::Bank(BankMsg::Send {
                to_address: "another".to_string(),
                amount: coins(750, "ukuji"),
            },),
            gas_limit: None,
            reply_on: ReplyOn::Never,
        }));

        assert!(res.messages.contains(&SubMsg {
            id: 0,
            msg: CosmosMsg::Bank(BankMsg::Send {
                to_address: "kujira17xpfvakm2amg962yls6f84z3kell8c5lp3pcxh".to_string(),
                amount: coins(500, "another"),
            },),
            gas_limit: None,
            reply_on: ReplyOn::Never,
        }));

        assert!(res.messages.contains(&SubMsg {
            id: 0,
            msg: CosmosMsg::Bank(BankMsg::Send {
                to_address: "another".to_string(),
                amount: coins(1500, "another"),
            },),
            gas_limit: None,
            reply_on: ReplyOn::Never,
        }));
    }
}
