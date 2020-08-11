// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use crate::pool::{Inputs, Transaction};

use educe::Educe;

use ethers::abi::Detokenize;
use ethers::contract::builders::ContractCall;
use ethers::providers::JsonRpcClient;
use ethers::signers::Signer;
use ethers::types::{
    Address, Signature, Transaction as EthTransaction, H256, U256,
};

pub use self::dropsafe_mod::Dropsafe;
pub use self::utxo_mod::{Utxo, UTXO_ABI};

use snafu::{ResultExt, Snafu};

use std::cmp::Ordering;
use std::fmt;

include!(concat!(env!("OUT_DIR"), "/abi/Utxo.rs"));
include!(concat!(env!("OUT_DIR"), "/abi/Dropsafe.rs"));

pub type WithdrawalTuple = (U256, U256, u8, [u8; 32], [u8; 32]);
pub type ClaimTuple = (U256, U256, Vec<U256>, u8, [u8; 32], [u8; 32]);
pub type TransferTuple = (
    U256,
    U256,
    Address,
    Address,
    U256,
    U256,
    u8,
    [u8; 32],
    [u8; 32],
);

#[derive(Debug, Clone, Educe)]
#[educe(Eq, PartialEq, Hash)]
pub struct Withdrawal {
    pub input: U256,
    pub gasprice: U256,

    #[educe(PartialEq(ignore), Hash(ignore))]
    pub signature: Signature,
}

impl fmt::Display for Withdrawal {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        // TODO: Maybe include the signature address?
        write!(f, "i={} gas={}", self.input, self.gasprice)
    }
}

impl Transaction for Withdrawal {
    fn gas_price(&self) -> &U256 {
        &self.gasprice
    }

    fn inputs(&self) -> Inputs {
        Inputs::One(&self.input)
    }
}

impl From<WithdrawalTuple> for Withdrawal {
    fn from(w: WithdrawalTuple) -> Self {
        Self {
            input: w.0,
            gasprice: w.1,
            signature: Signature {
                v: w.2 as u64,
                r: w.3.into(),
                s: w.4.into(),
            },
        }
    }
}

impl From<Withdrawal> for WithdrawalTuple {
    fn from(w: Withdrawal) -> Self {
        (
            w.input,
            w.gasprice,
            w.signature.v as u8,
            w.signature.r.to_fixed_bytes(),
            w.signature.s.to_fixed_bytes(),
        )
    }
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub struct Deposit {
    pub amount: U256,
    pub bounty: U256,
    pub owner: Address,
}

impl fmt::Display for Deposit {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(
            f,
            "amt={} bnty={} by={}",
            self.amount, self.bounty, self.owner
        )
    }
}

impl Deposit {
    pub const GAS_CONSTANT: U256 = U256::zero();
    pub const GAS_VARIABLE: U256 = U256::zero();

    pub fn fees(count: usize, gasprice: &U256) -> U256 {
        let gas = Self::GAS_CONSTANT + (Self::GAS_VARIABLE * count);
        gas * gasprice
    }
}

impl PartialOrd for Deposit {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for Deposit {
    fn cmp(&self, other: &Self) -> Ordering {
        (self.bounty, self.amount, self.owner).cmp(&(
            other.bounty,
            other.amount,
            other.owner,
        ))
    }
}

#[derive(Debug, Clone)]
pub struct Claim {
    pub input: U256,
    pub gasprice: U256,
    pub deposits: Vec<U256>,

    pub signature: Signature,
}

impl From<ClaimTuple> for Claim {
    fn from(c: ClaimTuple) -> Self {
        Self {
            input: c.0,
            gasprice: c.1,
            deposits: c.2,

            signature: Signature {
                v: c.3 as u64,
                r: c.4.into(),
                s: c.5.into(),
            },
        }
    }
}

impl From<Claim> for ClaimTuple {
    fn from(c: Claim) -> Self {
        (
            c.input,
            c.gasprice,
            c.deposits,
            c.signature.v as u8,
            c.signature.r.to_fixed_bytes(),
            c.signature.s.to_fixed_bytes(),
        )
    }
}

#[derive(Debug, Clone, Educe)]
#[educe(Eq, PartialEq, Hash)]
pub struct Transfer {
    pub input0: U256,
    pub input1: U256,

    pub destination: Address,
    pub change: Address,

    pub amount: U256,
    pub gasprice: U256,

    #[educe(PartialEq(ignore), Hash(ignore))]
    pub signature: Signature,
}

impl fmt::Display for Transfer {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        // TODO: Maybe include the signature address?

        if self.input0 != U256::zero() {
            write!(f, "i0={} ", self.input0,)?;
        }

        if self.input1 != U256::zero() {
            write!(f, "i1={} ", self.input1,)?;
        }

        write!(
            f,
            "gas={} dst={} chg={} amt={}",
            self.gasprice, self.destination, self.change, self.amount,
        )
    }
}

impl Transaction for Transfer {
    fn gas_price(&self) -> &U256 {
        &self.gasprice
    }

    fn inputs(&self) -> Inputs {
        match (self.input0.is_zero(), self.input1.is_zero()) {
            (true, false) => Inputs::One(&self.input1),
            (false, true) => Inputs::One(&self.input0),
            (false, false) => Inputs::Two(&self.input0, &self.input1),
            (true, true) => Inputs::None,
        }
    }
}

impl From<TransferTuple> for Transfer {
    fn from(t: TransferTuple) -> Self {
        Self {
            input0: t.0,
            input1: t.1,
            destination: t.2,
            change: t.3,
            amount: t.4,
            gasprice: t.5,
            signature: Signature {
                v: t.6 as u64,
                r: t.7.into(),
                s: t.8.into(),
            },
        }
    }
}

impl From<Transfer> for TransferTuple {
    fn from(t: Transfer) -> Self {
        (
            t.input0,
            t.input1,
            t.destination,
            t.change,
            t.amount,
            t.gasprice,
            t.signature.v as u8,
            t.signature.r.to_fixed_bytes(),
            t.signature.s.to_fixed_bytes(),
        )
    }
}

#[derive(Debug, Snafu)]
pub enum DecodeError {
    Abi { source: ethers::abi::Error },
}

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub enum Txn<T = Transfer, W = Withdrawal> {
    Transfer(T),
    Withdrawal(W),
}

impl<T, W> Transaction for Txn<T, W>
where
    T: Transaction,
    W: Transaction,
{
    fn inputs(&self) -> Inputs {
        match self {
            Txn::Transfer(t) => t.inputs(),
            Txn::Withdrawal(w) => w.inputs(),
        }
    }

    fn gas_price(&self) -> &U256 {
        match self {
            Txn::Transfer(t) => t.gas_price(),
            Txn::Withdrawal(w) => w.gas_price(),
        }
    }
}

pub type TxnRef<'a> = Txn<&'a Transfer, &'a Withdrawal>;

impl<T, W> Txn<T, W> {
    pub fn as_ref(&self) -> Txn<&T, &W> {
        match self {
            Txn::Transfer(ref t) => Txn::Transfer(t),
            Txn::Withdrawal(ref w) => Txn::Withdrawal(w),
        }
    }
}

impl<'a> From<&'a Withdrawal> for Txn<&'a Transfer, &'a Withdrawal> {
    fn from(o: &'a Withdrawal) -> Self {
        Txn::Withdrawal(o)
    }
}

impl<'a> From<&'a Transfer> for Txn<&'a Transfer, &'a Withdrawal> {
    fn from(o: &'a Transfer) -> Self {
        Txn::Transfer(o)
    }
}

impl From<Withdrawal> for Txn<Transfer, Withdrawal> {
    fn from(o: Withdrawal) -> Self {
        Txn::Withdrawal(o)
    }
}

impl From<Transfer> for Txn<Transfer, Withdrawal> {
    fn from(o: Transfer) -> Self {
        Txn::Transfer(o)
    }
}

#[derive(Debug, Clone)]
pub struct Bundle {
    pub claim: Claim,
    pub transfers: Vec<Transfer>,
    pub withdrawals: Vec<Withdrawal>,
}

impl Default for Bundle {
    fn default() -> Self {
        Self::new()
    }
}

impl Bundle {
    pub const MAX_SLOTS: usize = 10;
    pub const SLOTS_PER_CLAIM: usize = 1;
    pub const SLOTS_PER_TRANSFER: usize = 1;
    pub const SLOTS_PER_WITHDRAWAL: usize = 1;

    pub fn new() -> Self {
        Self {
            claim: Claim {
                gasprice: U256::zero(),
                deposits: vec![],
                input: U256::zero(),
                signature: Signature {
                    v: 0,
                    r: H256::zero(),
                    s: H256::zero(),
                },
            },
            transfers: vec![],
            withdrawals: vec![],
        }
    }

    pub fn transactions(&self) -> impl Iterator<Item = TxnRef> {
        self.transfers
            .iter()
            .map(Txn::from)
            .chain(self.withdrawals.iter().map(Txn::from))
    }

    pub fn insert(&mut self, txn: Txn) -> Option<Txn> {
        match txn {
            Txn::Withdrawal(w) => self.insert_withdrawal(w).map(Txn::from),
            Txn::Transfer(xfr) => self.insert_transfer(xfr).map(Txn::from),
        }
    }

    pub fn insert_deposit(&mut self, id: U256) -> Option<U256> {
        if self.free_slots() < Self::SLOTS_PER_CLAIM {
            Some(id)
        } else {
            self.claim.deposits.push(id);
            None
        }
    }

    pub fn insert_withdrawal(&mut self, w: Withdrawal) -> Option<Withdrawal> {
        if self.free_slots() < Self::SLOTS_PER_WITHDRAWAL {
            Some(w)
        } else {
            self.withdrawals.push(w);
            None
        }
    }

    pub fn insert_transfer(&mut self, xfr: Transfer) -> Option<Transfer> {
        if self.free_slots() < Self::SLOTS_PER_TRANSFER {
            Some(xfr)
        } else {
            self.transfers.push(xfr);
            None
        }
    }

    pub fn full_slots(&self) -> usize {
        (self.claim.deposits.len() * Self::SLOTS_PER_CLAIM)
            + (self.transfers.len() * Self::SLOTS_PER_TRANSFER)
            + (self.withdrawals.len() * Self::SLOTS_PER_WITHDRAWAL)
    }

    pub fn free_slots(&self) -> usize {
        Self::MAX_SLOTS - self.full_slots()
    }

    pub fn minimum_gas_price(&self) -> Option<U256> {
        let opt = if self.claim.deposits.is_empty() {
            None
        } else {
            Some(self.claim.gasprice)
        };

        opt.into_iter()
            .chain(self.transfers.iter().map(|t| t.gasprice))
            .chain(self.withdrawals.iter().map(|w| w.gasprice))
            .min()
    }

    pub fn estimate_price(&self, base: U256) -> U256 {
        let min_gp = match self.minimum_gas_price() {
            Some(m) if m > base => m,
            Some(m) => return m,
            None => return U256::zero(),
        };

        let full_slots = self.full_slots();
        let mut bribe = (min_gp - base) * full_slots; // TODO: Overflows?
        bribe /= Self::MAX_SLOTS;

        base + bribe
    }

    pub fn decode(transaction: &EthTransaction) -> Result<Self, DecodeError> {
        Self::decode_slice(&transaction.input.0)
    }

    pub fn decode_slice(input: &[u8]) -> Result<Self, DecodeError> {
        // TODO: Check solidity function hash (ie. input[..4])
        let transact_abi = &UTXO_ABI.functions["transact"][0];
        let mut decoded = transact_abi
            .decode_input(&input[4..])
            .context(Abi)?
            .into_iter();

        let claim_tokens = vec![decoded.next().unwrap()];
        let claim_tuple = ClaimTuple::from_tokens(claim_tokens).unwrap();
        let claim = Claim::from(claim_tuple);

        let transfers_tokens = vec![decoded.next().unwrap()];
        let transfers: Vec<_> =
            Vec::<TransferTuple>::from_tokens(transfers_tokens)
                .unwrap()
                .into_iter()
                .map(Transfer::from)
                .collect();

        let withdrawals_tokens = vec![decoded.next().unwrap()];
        let withdrawals: Vec<_> =
            Vec::<WithdrawalTuple>::from_tokens(withdrawals_tokens)
                .unwrap()
                .into_iter()
                .map(Withdrawal::from)
                .collect();

        Ok(Self {
            claim,
            transfers,
            withdrawals,
        })
    }

    pub fn encode<P, S>(self, utxo: &Utxo<P, S>) -> ContractCall<P, S, ()>
    where
        P: JsonRpcClient,
        S: Signer,
    {
        utxo.transact(
            self.claim.into(),
            self.transfers.into_iter().map(|x| x.into()).collect(),
            self.withdrawals.into_iter().map(|w| w.into()).collect(),
        )
    }
}

#[cfg(test)]
mod tests {
    use ethers::types::H256;

    use super::*;

    fn sig() -> Signature {
        Signature {
            v: 0,
            r: H256([0; 32]),
            s: H256([0; 32]),
        }
    }

    // TODO: Add tests with base < min_gas_price

    #[test]
    fn bundle_estimate_price_max_base() {
        let bundle = Bundle {
            claim: Claim {
                deposits: vec![U256::one()],
                gasprice: 77.into(),
                input: U256::one(),
                signature: sig(),
            },
            withdrawals: vec![Withdrawal {
                gasprice: 113.into(),
                input: U256::one(),
                signature: sig(),
            }],
            transfers: vec![Transfer {
                gasprice: 117.into(),
                input0: U256::one(),
                input1: 2.into(),
                signature: sig(),
                amount: U256::zero(),
                change: Address::zero(),
                destination: Address::zero(),
            }],
        };

        let base = U256::max_value();
        assert_eq!(U256::from(77), bundle.estimate_price(base));
    }

    #[test]
    fn bundle_estimate_price_transfer_max_base() {
        let bundle = Bundle {
            claim: Claim {
                deposits: vec![],
                gasprice: 77.into(),
                input: U256::one(),
                signature: sig(),
            },
            withdrawals: vec![],
            transfers: vec![Transfer {
                gasprice: 117.into(),
                input0: U256::one(),
                input1: 2.into(),
                signature: sig(),
                amount: U256::zero(),
                change: Address::zero(),
                destination: Address::zero(),
            }],
        };

        let base = U256::max_value();
        assert_eq!(U256::from(117), bundle.estimate_price(base));
    }

    #[test]
    fn bundle_estimate_price_withdrawal_max_base() {
        let bundle = Bundle {
            claim: Claim {
                deposits: vec![],
                gasprice: 77.into(),
                input: U256::one(),
                signature: sig(),
            },
            transfers: vec![],
            withdrawals: vec![Withdrawal {
                gasprice: 113.into(),
                input: U256::one(),
                signature: sig(),
            }],
        };

        let base = U256::max_value();
        assert_eq!(U256::from(113), bundle.estimate_price(base));
    }

    #[test]
    fn bundle_estimate_price_claim_max_base() {
        let bundle = Bundle {
            claim: Claim {
                deposits: vec![U256::one()],
                gasprice: 77.into(),
                input: U256::one(),
                signature: sig(),
            },
            transfers: vec![],
            withdrawals: vec![],
        };

        let base = U256::max_value();
        assert_eq!(U256::from(77), bundle.estimate_price(base));
    }

    #[test]
    fn bundle_estimate_price_empty() {
        let bundle = Bundle {
            claim: Claim {
                deposits: vec![],
                gasprice: 1.into(),
                input: U256::zero(),
                signature: sig(),
            },
            transfers: vec![],
            withdrawals: vec![],
        };

        let base = U256::zero();
        assert_eq!(U256::zero(), bundle.estimate_price(base));
    }

    #[test]
    fn bundle_decode_slice() {
        let input = [
            0xe2, 0x3c, 0x9c, 0x75, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
            0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
            0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
            0x00, 0x00, 0x60, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
            0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
            0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
            0x01, 0x80, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
            0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
            0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x03,
            0xe0, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
            0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
            0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x27,
            0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
            0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
            0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x3b, 0x9a, 0xca, 0x00, 0x00,
            0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
            0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
            0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0xc0, 0x00, 0x00,
            0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
            0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
            0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0xaa, 0xaa, 0xaa,
            0xaa, 0xaa, 0xaa, 0xaa, 0xaa, 0xaa, 0xaa, 0xaa, 0xaa, 0xaa, 0xaa,
            0xaa, 0xaa, 0xaa, 0xaa, 0xaa, 0xaa, 0xaa, 0xaa, 0xaa, 0xaa, 0xaa,
            0xaa, 0xaa, 0xaa, 0xaa, 0xaa, 0xaa, 0xaa, 0xbb, 0xbb, 0xbb, 0xbb,
            0xbb, 0xbb, 0xbb, 0xbb, 0xbb, 0xbb, 0xbb, 0xbb, 0xbb, 0xbb, 0xbb,
            0xbb, 0xbb, 0xbb, 0xbb, 0xbb, 0xbb, 0xbb, 0xbb, 0xbb, 0xbb, 0xbb,
            0xbb, 0xbb, 0xbb, 0xbb, 0xbb, 0xbb, 0x00, 0x00, 0x00, 0x00, 0x00,
            0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
            0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
            0x00, 0x00, 0x00, 0x00, 0x02, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
            0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
            0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
            0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
            0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
            0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
            0x00, 0x00, 0x0c, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
            0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
            0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
            0x00, 0x02, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
            0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
            0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
            0x01, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
            0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
            0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x02,
            0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
            0x00, 0xcc, 0xcc, 0xcc, 0xcc, 0xcc, 0xcc, 0xcc, 0xcc, 0xcc, 0xcc,
            0xcc, 0xcc, 0xcc, 0xcc, 0xcc, 0xcc, 0xcc, 0xcc, 0xcc, 0xcc, 0x00,
            0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
            0xdd, 0xdd, 0xdd, 0xdd, 0xdd, 0xdd, 0xdd, 0xdd, 0xdd, 0xdd, 0xdd,
            0xdd, 0xdd, 0xdd, 0xdd, 0xdd, 0xdd, 0xdd, 0xdd, 0xdd, 0x00, 0x00,
            0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
            0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
            0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0xee, 0x00, 0x00, 0x00,
            0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
            0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
            0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x44, 0x00, 0x00, 0x00, 0x00,
            0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
            0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
            0x00, 0x00, 0x00, 0x00, 0x00, 0x09, 0x11, 0x11, 0x11, 0x11, 0x11,
            0x11, 0x11, 0x11, 0x11, 0x11, 0x11, 0x11, 0x11, 0x11, 0x11, 0x11,
            0x11, 0x11, 0x11, 0x11, 0x11, 0x11, 0x11, 0x11, 0x11, 0x11, 0x11,
            0x11, 0x11, 0x11, 0x11, 0x11, 0x22, 0x22, 0x22, 0x22, 0x22, 0x22,
            0x22, 0x22, 0x22, 0x22, 0x22, 0x22, 0x22, 0x22, 0x22, 0x22, 0x22,
            0x22, 0x22, 0x22, 0x22, 0x22, 0x22, 0x22, 0x22, 0x22, 0x22, 0x22,
            0x22, 0x22, 0x22, 0x22, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
            0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
            0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
            0x00, 0x00, 0x03, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
            0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
            0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
            0x00, 0x04, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
            0x00, 0x00, 0x00, 0x77, 0x77, 0x77, 0x77, 0x77, 0x77, 0x77, 0x77,
            0x77, 0x77, 0x77, 0x77, 0x77, 0x77, 0x77, 0x77, 0x77, 0x77, 0x77,
            0x77, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
            0x00, 0x00, 0x55, 0x55, 0x55, 0x55, 0x55, 0x55, 0x55, 0x55, 0x55,
            0x55, 0x55, 0x55, 0x55, 0x55, 0x55, 0x55, 0x55, 0x55, 0x55, 0x55,
            0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
            0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
            0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0xee, 0x00,
            0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
            0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
            0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x44, 0x00, 0x00,
            0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
            0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
            0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x09, 0x11, 0x11, 0x11,
            0x11, 0x11, 0x11, 0x11, 0x11, 0x11, 0x11, 0x11, 0x11, 0x11, 0x11,
            0x11, 0x11, 0x11, 0x11, 0x11, 0x11, 0x11, 0x11, 0x11, 0x11, 0x11,
            0x11, 0x11, 0x11, 0x11, 0x11, 0x11, 0x11, 0x22, 0x22, 0x22, 0x22,
            0x22, 0x22, 0x22, 0x22, 0x22, 0x22, 0x22, 0x22, 0x22, 0x22, 0x22,
            0x22, 0x22, 0x22, 0x22, 0x22, 0x22, 0x22, 0x22, 0x22, 0x22, 0x22,
            0x22, 0x22, 0x22, 0x22, 0x22, 0x22, 0x00, 0x00, 0x00, 0x00, 0x00,
            0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
            0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
            0x00, 0x00, 0x00, 0x00, 0x02, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
            0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
            0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
            0x00, 0x00, 0x00, 0x05, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
            0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
            0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
            0x00, 0x00, 0xde, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
            0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
            0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
            0x00, 0x09, 0x11, 0x11, 0x11, 0x11, 0x11, 0x11, 0x11, 0x11, 0x11,
            0x11, 0x11, 0x11, 0x11, 0x11, 0x11, 0x11, 0x11, 0x11, 0x11, 0x11,
            0x11, 0x11, 0x11, 0x11, 0x11, 0x11, 0x11, 0x11, 0x11, 0x11, 0x11,
            0x11, 0x22, 0x22, 0x22, 0x22, 0x22, 0x22, 0x22, 0x22, 0x22, 0x22,
            0x22, 0x22, 0x22, 0x22, 0x22, 0x22, 0x22, 0x22, 0x22, 0x22, 0x22,
            0x22, 0x22, 0x22, 0x22, 0x22, 0x22, 0x22, 0x22, 0x22, 0x22, 0x22,
            0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
            0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
            0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x06, 0x00,
            0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
            0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
            0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0xfe, 0x00, 0x00,
            0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
            0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
            0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x19, 0x13, 0x11, 0x11,
            0x11, 0x11, 0x11, 0x11, 0x11, 0x11, 0x11, 0x11, 0x11, 0x11, 0x11,
            0x11, 0x11, 0x11, 0x11, 0x11, 0x11, 0x11, 0x11, 0x11, 0x11, 0x11,
            0x11, 0x11, 0x11, 0x11, 0x11, 0x11, 0x11, 0x23, 0x22, 0x22, 0x22,
            0x22, 0x22, 0x22, 0x22, 0x22, 0x22, 0x22, 0x22, 0x22, 0x22, 0x22,
            0x22, 0x22, 0x22, 0x22, 0x22, 0x22, 0x22, 0x22, 0x22, 0x22, 0x22,
            0x22, 0x22, 0x22, 0x22, 0x22, 0x22,
        ];

        let bundle = Bundle::decode_slice(&input).unwrap();

        // TODO: Check all the things.

        let claim = bundle.claim;
        assert_eq!(claim.input, 0x27.into());
        assert_eq!(claim.gasprice, 0x3b9aca00.into());
        assert_eq!(claim.deposits.len(), 2);
        assert_eq!(claim.deposits[0], 0.into());
        assert_eq!(claim.deposits[1], 0xc.into());

        let xfrs = bundle.transfers;
        assert_eq!(xfrs.len(), 2);
        assert_eq!(xfrs[0].gasprice, 0x44.into());
        assert_eq!(xfrs[1].destination, [0x77; 20].into());

        let withdrawals = bundle.withdrawals;
        assert_eq!(withdrawals.len(), 2);
        assert_eq!(withdrawals[0].gasprice, 0xde.into());
        assert_eq!(withdrawals[1].signature.v, 0x19);
    }
}