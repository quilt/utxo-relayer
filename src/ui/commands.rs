// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use ethers::types::{Address, Signature, U256};

use structopt::StructOpt;

#[derive(Debug, StructOpt)]
#[structopt(setting=structopt::clap::AppSettings::NoBinaryName)]
#[structopt(setting=structopt::clap::AppSettings::DisableVersion)]
#[structopt(setting=structopt::clap::AppSettings::VersionlessSubcommands)]
pub enum CommandKind {
    Deposit(Deposit),
    Withdraw(Withdraw),
    Transfer(Transfer),
    Show(Show),
    Get(GetType),
}

#[derive(Debug, StructOpt)]
pub enum GetType {
    FeeBase,
    UtxoCount,
}

#[derive(Debug, StructOpt)]
pub enum PoolType {
    Deposits,
    Transfers,
    Withdrawals,
}

#[derive(Debug, StructOpt)]
pub struct Show {
    #[structopt(subcommand)]
    pub what: PoolType,
}

#[derive(Debug, StructOpt)]
pub struct Deposit {}

#[derive(Clone, Debug, StructOpt)]
pub struct Withdraw {
    #[structopt(long = "input0", short = "0")]
    input0: U256,

    #[structopt(long = "gasprice", short = "-g")]
    gasprice: U256,
}

impl From<Withdraw> for crate::contracts::Withdrawal {
    fn from(cmd: Withdraw) -> Self {
        // TODO: Should generate a valid signature

        Self {
            input: cmd.input0,
            gasprice: cmd.gasprice,
            signature: Signature {
                v: 0,
                r: Default::default(),
                s: Default::default(),
            },
        }
    }
}

#[derive(Debug, Clone, StructOpt)]
pub struct Transfer {
    #[structopt(long = "input0", short = "0")]
    input0: Option<U256>,

    #[structopt(long = "input1", short = "1")]
    input1: Option<U256>,

    #[structopt(long = "destination", short = "-d")]
    destination: Address,

    #[structopt(long = "change", short = "-c")]
    change: Address,

    #[structopt(long = "amount", short = "-a")]
    amount: U256,

    #[structopt(long = "gasprice", short = "-g")]
    gasprice: U256,
}

impl From<Transfer> for crate::contracts::Transfer {
    fn from(cmd: Transfer) -> Self {
        // TODO: Should generate a real signature.

        Self {
            amount: cmd.amount,
            change: cmd.change,
            destination: cmd.destination,
            gasprice: cmd.gasprice,
            input0: cmd.input0.unwrap_or_default(),
            input1: cmd.input1.unwrap_or_default(),
            signature: Signature {
                v: 0,
                r: Default::default(),
                s: Default::default(),
            },
        }
    }
}
