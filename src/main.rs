// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

#![feature(map_first_last)]

mod contracts;
mod pool;
mod ui;

use crate::contracts::{Bundle, Deposit, Transfer, Txn, Utxo, Withdrawal};
use crate::pool::{DepositPool, Pool, Transaction as _};
use crate::ui::{Command, CommandKind, EventKind, Events, PoolType};

use ethers::providers::{JsonRpcClient, Provider};
use ethers::signers::{Client, Wallet};
use ethers::types::{
    Address, Transaction as EthTransaction, H160, H256, U256, U64,
};

use std::convert::TryFrom;
use std::str::FromStr;
use std::sync::Arc;

use structopt::StructOpt;

use tokio::stream::StreamExt;
use tokio::sync::Mutex;

type Error = Box<dyn std::error::Error + Sync + Send>;

const PRIVATE_KEY_STR: &str =
    include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/key.hex"));

const UTXO: Address = H160([
    0xC3, 0x29, 0xe0, 0xB1, 0xBC, 0x53, 0x4d, 0xeb, 0x32, 0x9A, 0x8d, 0x25,
    0x76, 0x0b, 0x61, 0x6C, 0x81, 0x86, 0xe2, 0x08,
]);

#[derive(Debug, StructOpt)]
pub struct Opts {
    #[structopt(long = "oob")]
    oob: bool,
}

#[derive(Debug)]
pub struct Pending {
    deposits: DepositPool,
    transactions: Pool<Txn>,

    best_bundle: Option<Bundle>,
}

impl Pending {
    pub fn generate(&mut self, base: U256) -> Option<&Bundle> {
        self.best_bundle = None;
        self.regenerate(base)
    }

    pub fn regenerate(&mut self, base: U256) -> Option<&Bundle> {
        let mut bundle = Bundle::new();

        for txn in self.transactions.iter() {
            let gp = txn.gas_price();

            // Create a new bundle, copying the transfers and withdrawals.
            let mut new_bundle = Bundle::new();
            new_bundle.transfers = bundle.transfers.clone();
            new_bundle.withdrawals = bundle.withdrawals.clone();

            // Insert the next best transaction.
            new_bundle.insert(txn.clone());

            // NB: There an attack where a malicious bundler Alice could
            //     repackage Bob's claim with more transactions than Bob
            //     anticipated. This isn't an issue here, since this assumes
            //     each claim pays the full gas price, but something to be
            //     aware of.

            // Collect deposits that break even at txn's gas price.
            let deposits = &new_bundle.claim.deposits;
            new_bundle.claim.gasprice = *gp;

            for candidate in self.deposits.iter() {
                // TODO: This is likely too conservative. It misses cases where
                //       multiple deposits together would be profitable if the
                //       first deposit isn't profitable on its own.
                let previous_fees = Deposit::fees(deposits.len(), gp);
                let fees = Deposit::fees(deposits.len() + 1, gp);
                let my_fees = fees - previous_fees;

                if candidate.bounty < my_fees {
                    break;
                }

                if bundle.insert_deposit(*candidate.id()).is_some() {
                    break;
                }
            }

            if bundle.estimate_price(base) >= new_bundle.estimate_price(base) {
                break;
            } else {
                bundle = new_bundle;
            }
        }

        let mut replace = true;
        if let Some(ref best_bundle) = self.best_bundle {
            if best_bundle.estimate_price(base) >= bundle.estimate_price(base) {
                replace = false;
            }
        }

        if replace {
            self.best_bundle = Some(bundle);
            self.best_bundle.as_ref()
        } else {
            None
        }
    }
}

pub struct State<T> {
    events: Events,
    provider: Provider<T>,
    utxo: Utxo<T, Wallet>,
    pending: Mutex<Pending>,
}

pub type SharedState<T> = Arc<State<T>>;

#[tokio::main]
async fn main() -> Result<(), Error> {
    let opts = Opts::from_args();

    let handle = tokio::runtime::Handle::current();
    let ui = ui::Ui::start(handle, opts.oob)?;

    let provider = Provider::try_from("http://localhost:8544")?;
    let signer = Wallet::from_str(PRIVATE_KEY_STR)?;
    let client = Client::new(provider.clone(), signer);
    let utxo = Utxo::new(UTXO, client);

    let state = Arc::new(State {
        utxo,
        provider,
        events: ui.events(),
        pending: Mutex::new(Pending {
            deposits: Default::default(),
            transactions: Pool::default(),

            best_bundle: None,
        }),
    });

    let cmd_watcher = tokio::spawn(execute_commands(state.clone(), ui));

    //process_transactions(state.clone()).await?;

    // TODO: The ethers streams don't seem to return any events if they are
    //       called from inside a tokio::spawn.

    let block_watcher =
        async { tokio::spawn(process_blocks(state.clone())).await? };

    let bundle_watcher =
        async { tokio::spawn(process_transactions(state.clone())).await? };

    tokio::try_join!(block_watcher, bundle_watcher)?;

    cmd_watcher.await?;

    Ok(())
}

async fn execute_commands<T>(state: SharedState<T>, mut ui: ui::Ui)
where
    T: JsonRpcClient,
{
    let mut events = state.events.clone();

    while let Some(cmd) = ui.recv_command().await {
        if let Err(e) = try_execute_command(&state, &cmd).await {
            events.reply(&cmd, EventKind::CommandError(e)).await;
        }
    }
}

async fn try_execute_command<T>(
    state: &SharedState<T>,
    cmd: &Command,
) -> Result<(), Error>
where
    T: JsonRpcClient,
{
    let mut events = state.events.clone();

    match cmd.kind() {
        CommandKind::Transfer(xfr) => {
            insert_transfer(state, cmd, xfr.clone().into()).await?;
        }
        CommandKind::Withdraw(wdw) => {
            insert_withdrawal(state, cmd, wdw.clone().into()).await?;
        }
        CommandKind::Show(show) => match show.what {
            PoolType::Transfers => show_transfers(state, cmd).await,
            PoolType::Withdrawals => show_withdrawals(state, cmd).await,
            PoolType::Deposits => show_deposits(state, cmd).await,
        },
        _ => events.reply(cmd, format!("{:?}", cmd)).await,
    }

    Ok(())
}

async fn show_transfers<T>(state: &SharedState<T>, cmd: &Command)
where
    T: JsonRpcClient,
{
    let mut events = state.events.clone();

    let pending = state.pending.lock().await;
    for txn in pending.transactions.iter() {
        if let Txn::Transfer(t) = txn {
            events.reply(cmd, t.to_string()).await;
        }
    }
}

async fn show_withdrawals<T>(state: &SharedState<T>, cmd: &Command)
where
    T: JsonRpcClient,
{
    let mut events = state.events.clone();

    let pending = state.pending.lock().await;
    for txn in pending.transactions.iter() {
        if let Txn::Withdrawal(w) = txn {
            events.reply(cmd, w.to_string()).await;
        }
    }
}

async fn show_deposits<T>(state: &SharedState<T>, cmd: &Command)
where
    T: JsonRpcClient,
{
    let mut events = state.events.clone();

    let pending = state.pending.lock().await;
    for deposit in pending.deposits.iter() {
        events.reply(cmd, deposit.to_string()).await;
    }
}

async fn insert_txn<T>(
    state: &SharedState<T>,
    cmd: &Command,
    txn: Txn,
) -> Result<(), Error>
where
    T: JsonRpcClient,
{
    let mut events = state.events.clone();
    let mut pending = state.pending.lock().await;

    let before_count = pending.transactions.len();
    pending.transactions.remove_conflicting(&txn);
    let after_count = pending.transactions.len();

    let removed = before_count - after_count;
    if removed > 0 {
        events.reply(cmd, EventKind::PoolDrop(removed)).await;
    }

    pending.transactions.insert(txn);
    let added = pending.transactions.len() - after_count;
    events.reply(cmd, EventKind::PoolAdd(added)).await;

    let base = fetch_base(state).await?;
    if let Some(new_bundle) = pending.regenerate(base) {
        events
            .reply(cmd, EventKind::Broadcast(new_bundle.clone()))
            .await;
        broadcast(&state, new_bundle.clone()).await?;
    }

    Ok(())
}

async fn insert_withdrawal<T>(
    state: &SharedState<T>,
    cmd: &Command,
    withdrawal: Withdrawal,
) -> Result<(), Error>
where
    T: JsonRpcClient,
{
    insert_txn(state, cmd, withdrawal.into()).await
}

async fn insert_transfer<T>(
    state: &SharedState<T>,
    cmd: &Command,
    xfr: Transfer,
) -> Result<(), Error>
where
    T: JsonRpcClient,
{
    insert_txn(state, cmd, xfr.into()).await
}

async fn process_blocks<T>(state: SharedState<T>) -> Result<(), Error>
where
    T: 'static + JsonRpcClient,
{
    let mut stream = state.provider.watch_blocks().await?;

    let mut events = state.events.clone();
    events.oob("Watching for new blocks...").await;

    while let Some(bkhash) = stream.next().await {
        events.new_block(bkhash).await;
        tokio::spawn(process_block(state.clone(), bkhash));
    }

    Ok(())
}

async fn process_block<T>(state: SharedState<T>, bkhash: H256)
where
    T: JsonRpcClient,
{
    let mut events = state.events.clone();

    if let Err(e) = try_process_block(state, bkhash).await {
        events.bad_block(bkhash, e).await;
    }
}

async fn try_process_block<T>(
    state: SharedState<T>,
    bkhash: H256,
) -> Result<(), Error>
where
    T: JsonRpcClient,
{
    let block = state.provider.get_block_with_txs(bkhash).await?;

    for tx in block.transactions.iter() {
        process_block_transaction(&state, tx).await?;
    }

    Ok(())
}

async fn process_block_transaction<T>(
    state: &SharedState<T>,
    tx: &EthTransaction,
) -> Result<(), Error>
where
    T: JsonRpcClient,
{
    if tx.to.as_ref() != Some(&UTXO) {
        return Ok(());
    }

    let receipt = state.provider.get_transaction_receipt(tx.hash).await?;

    let mut events = state.events.clone();

    if receipt.status != Some(U64::one()) {
        events.bad_bundle(tx.clone()).await;

        // TODO: There might be valid transactions in the bundle that can be
        //       added to the pool.

        return Ok(());
    }

    events.good_bundle(tx.clone()).await;

    let bundle = match Bundle::decode_slice(&tx.input.0) {
        Ok(b) => b,
        Err(e) => {
            events.decode_error(tx.clone(), e).await;
            return Ok(());
        }
    };

    let base = fetch_base(state).await?;
    let mut shared = state.pending.lock().await;

    let before_count = shared.transactions.len();

    for txn in bundle.transactions() {
        shared.transactions.remove_conflicting(&txn);
    }

    let removed = before_count - shared.transactions.len();

    if removed > 0 {
        events.oob(EventKind::PoolDrop(removed)).await;
    }

    // TODO: Only regenerate the bundle if the pool actually changed.
    if let Some(new_bundle) = shared.generate(base) {
        events.oob(EventKind::Broadcast(new_bundle.clone())).await;
        broadcast(&state, new_bundle.clone()).await?;
    }

    Ok(())
}

async fn fetch_base<T>(_: &SharedState<T>) -> Result<U256, Error>
where
    T: JsonRpcClient,
{
    // TODO: When BASE actually exists in the contract, return that.
    //Ok(0x3b9aca00.into())
    Ok(5.into())
}

async fn process_transactions<T>(state: SharedState<T>) -> Result<(), Error>
where
    T: 'static + JsonRpcClient,
{
    let mut stream = state.provider.watch_pending_transactions().await?;

    let mut events = state.events.clone();

    events.oob("Watching for pending transactions...").await;

    while let Some(txhash) = stream.next().await {
        events.pending_tx(txhash).await;
        tokio::spawn(process_transaction(state.clone(), txhash));
    }

    Ok(())
}

async fn process_transaction<T>(state: SharedState<T>, txhash: H256)
where
    T: JsonRpcClient,
{
    try_process_transaction(state, txhash).await.unwrap();
}

async fn try_process_transaction<T>(
    state: SharedState<T>,
    txhash: H256,
) -> Result<(), Error>
where
    T: JsonRpcClient,
{
    let tx = state.provider.get_transaction(txhash).await?;
    if tx.to != Some(UTXO) || tx.block_hash.is_some() {
        return Ok(());
    }

    let mut events = state.events.clone();

    let bundle = match Bundle::decode_slice(&tx.input.0) {
        Ok(b) => b,
        Err(e) => {
            events.decode_error(tx.clone(), e).await;
            return Ok(());
        }
    };

    let base = fetch_base(&state).await?;
    let mut pending = state.pending.lock().await;

    for withdrawal in bundle.withdrawals.into_iter() {
        pending.transactions.insert(withdrawal);
    }

    for transfer in bundle.transfers.into_iter() {
        pending.transactions.insert(transfer);
    }

    if let Some(new_bundle) = pending.regenerate(base) {
        events.oob(EventKind::Broadcast(new_bundle.clone())).await;
        broadcast(&state, new_bundle.clone()).await?;
    }

    Ok(())
}

async fn broadcast<T>(
    state: &SharedState<T>,
    bundle: Bundle,
) -> Result<(), Error>
where
    T: JsonRpcClient,
{
    let call = bundle.encode(&state.utxo);

    call.call().await?;
    call.send().await?;

    Ok(())
}

#[cfg(test)]
mod tests {
    use ethers::types::Signature;

    use super::*;

    #[test]
    fn bundle_two_transfers_take_one() {
        let mut pending = Pending {
            deposits: DepositPool::default(),
            transactions: Pool::default(),
            best_bundle: None,
        };

        let expected = Transfer {
            amount: 10.into(),
            gasprice: 100.into(),
            change: Address::zero(),
            destination: Address::zero(),
            input0: 1.into(),
            input1: 2.into(),
            signature: Signature {
                v: 0,
                r: H256::zero(),
                s: H256::zero(),
            },
        };

        pending.transactions.insert(expected.clone());

        pending.transactions.insert(Transfer {
            amount: 10.into(),
            gasprice: 60.into(),
            change: Address::zero(),
            destination: Address::zero(),
            input0: 3.into(),
            input1: 4.into(),
            signature: Signature {
                v: 0,
                r: H256::zero(),
                s: H256::zero(),
            },
        });

        let actual = pending.regenerate(50.into()).unwrap();
        assert_eq!(actual.transfers, vec![expected]);

        // TODO: Check pending.best_bundle
    }

    #[test]
    fn bundle_two_transfers_take_two() {
        let mut pending = Pending {
            deposits: DepositPool::default(),
            transactions: Pool::default(),
            best_bundle: None,
        };

        let expected0 = Transfer {
            amount: 10.into(),
            gasprice: 100.into(),
            change: Address::zero(),
            destination: Address::zero(),
            input0: 1.into(),
            input1: 2.into(),
            signature: Signature {
                v: 0,
                r: H256::zero(),
                s: H256::zero(),
            },
        };

        pending.transactions.insert(expected0.clone());

        let expected1 = Transfer {
            amount: 10.into(),
            gasprice: 90.into(),
            change: Address::zero(),
            destination: Address::zero(),
            input0: 3.into(),
            input1: 4.into(),
            signature: Signature {
                v: 0,
                r: H256::zero(),
                s: H256::zero(),
            },
        };

        pending.transactions.insert(expected1.clone());

        let actual = pending.regenerate(50.into()).unwrap();
        assert_eq!(actual.transfers, vec![expected0, expected1]);

        // TODO: Check pending.best_bundle
    }

    #[test]
    fn bundle_too_many_transfers() {
        let mut pending = Pending {
            deposits: DepositPool::default(),
            transactions: Pool::default(),
            best_bundle: None,
        };

        let allowed = Bundle::MAX_SLOTS / Bundle::SLOTS_PER_TRANSFER;
        let mut xfrs = vec![];

        for ii in 0..allowed + 5 {
            let xfr = Transfer {
                amount: 10.into(),
                gasprice: (usize::max_value() - ii).into(),
                change: Address::zero(),
                destination: Address::zero(),
                input0: (1 + ii).into(),
                input1: 0.into(),
                signature: Signature {
                    v: 0,
                    r: H256::zero(),
                    s: H256::zero(),
                },
            };

            pending.transactions.insert(xfr.clone());
            xfrs.push(xfr);
        }

        let actual = pending.regenerate(U256::zero()).unwrap();
        assert_eq!(actual.transfers, &xfrs[..xfrs.len() - 5]);
    }
}
