# Kujira Revenue Converter

Simple smart-contract layer to allow aggregation of reward tokens and swapping into a smaller number of assets to be distributed to $KUJI stakers.

One contract instance is deployed per revenue token.

Each instance has a set of `Action`s that are stepped through on subsequent executions of `ExecuteMsg::Run`.
This is designed to keep execution of the contract in fixed time, and also support more complex routing of token swaps.
At the end of each execution, `revenue_token` balance is read and is deposited to the fee_collector address.

## Deployments

### Testnet

- code-id `3147`
- address `kujira158ydy6qlfq7khtnj5lj9a5dy25ep8hece4d0lqngzxqrwuz6dctsdl5eqx`
