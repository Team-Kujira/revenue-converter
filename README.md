# Kujira Revenue Converter

Simple smart-contract layer to allow aggregation of reward tokens into a smaller number of assets

Each instance of the contract has a configured `revenue_token`, which is deposited to the fee_collector address at the end of every `SudoMsg::Run`.
It also has a set of `Action`s that are stepped through on subsequent executions of `SudoMsg::Run`.
This is designed to keep execution of the contract in fixed time, and also support more complex routing of token swaps,
before ultimately depositing them to the global fee_collector.
