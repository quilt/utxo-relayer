// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use crate::contracts::Deposit;

use ethers::types::U256;

use std::collections::btree_map::{BTreeMap, Entry};
use std::collections::btree_set::BTreeSet;
use std::collections::HashMap;
use std::fmt;
use std::iter::Iterator;
use std::ops::{Deref, DerefMut};
use std::sync::Arc;

#[derive(Debug, Clone, Hash, Eq, PartialEq)]
pub enum Inputs<'a> {
    None,
    One(&'a U256),
    Two(&'a U256, &'a U256),
}

impl<'a> Iterator for Inputs<'a> {
    type Item = &'a U256;

    fn next(&mut self) -> Option<Self::Item> {
        let next;
        let result: &'a U256;

        match self {
            Inputs::None => return None,
            Inputs::One(one) => {
                next = Inputs::None;
                result = one;
            }
            Inputs::Two(one, two) => {
                next = Inputs::One(two);
                result = one;
            }
        }

        *self = next;
        Some(result)
    }
}

pub trait Transaction: Eq {
    fn gas_price(&self) -> &U256;
    fn inputs(&self) -> Inputs;
}

impl<T> Transaction for &T
where
    T: Transaction,
{
    fn gas_price(&self) -> &U256 {
        T::gas_price(self)
    }

    fn inputs(&self) -> Inputs {
        T::inputs(self)
    }
}

#[derive(Debug)]
pub struct Pool<T>
where
    T: Transaction,
{
    max_len: usize,
    len: usize,
    by_gas: BTreeMap<U256, Vec<Arc<T>>>,
    by_input: HashMap<U256, Arc<T>>,
}

impl<T> Default for Pool<T>
where
    T: Transaction,
{
    fn default() -> Self {
        Self {
            max_len: Self::DEFAULT_MAX_LEN,
            len: 0,
            by_gas: BTreeMap::new(),
            by_input: HashMap::new(),
        }
    }
}

impl<T> Pool<T>
where
    T: Transaction,
{
    pub const DEFAULT_MAX_LEN: usize = 1024;

    /// Returns a reference to the transaction with the highest gas price, or
    /// `None` if the pool is empty.
    pub fn peek(&self) -> Option<&T> {
        self.by_gas
            .last_key_value()
            .and_then(|(_, v)| v.first())
            .map(Arc::as_ref)
    }

    /// The number of unique transactions in the pool.
    pub fn len(&self) -> usize {
        self.len
    }

    /// Gets an iterator over the transactions, sorted by gas price in decending
    /// order.
    pub fn iter(&self) -> impl Iterator<Item = &T> {
        self.by_gas
            .values()
            .rev()
            .flat_map(|v| v.iter())
            .map(Arc::as_ref)
    }

    /// Removes a transaction from the pool. Panics if `item` is not in the
    /// pool.
    pub fn remove(&mut self, item: &T) {
        for input in item.inputs() {
            let removed = self
                .by_input
                .remove(input)
                .expect("item to remove not found by input");
            assert!(removed.as_ref() == item);
        }

        match self.by_gas.entry(*item.gas_price()) {
            Entry::Vacant(_) => panic!("item to remove not found by gas"),
            Entry::Occupied(mut o) => {
                let empty = {
                    let same_gas = o.get_mut();
                    let before = same_gas.len();

                    same_gas.retain(|e| e.as_ref() != item);

                    assert_eq!(
                        before,
                        same_gas.len() + 1,
                        "incorrect number of items removed by gas"
                    );

                    same_gas.is_empty()
                };

                if empty {
                    o.remove();
                }
            }
        }

        self.len -= 1;
    }

    /// Inserts a new transaction into the pool. If there are one or more
    /// conflicts with transactions already in the pool and the new transaction
    /// has a higher gas price, the new transaction replaces the existing ones.
    pub fn insert<V: Into<T>>(&mut self, item: V) {
        self.maybe_replace(item.into(), false);
    }

    /// Inserts a new transaction into the pool. If there are one or more
    /// conflicts with transactions already in the pool, the new transaction
    /// replaces the existing ones regardless of gas price.
    pub fn replace(&mut self, item: T) {
        self.maybe_replace(item, true);
    }

    fn maybe_replace(&mut self, item: T, force: bool) {
        let item = Arc::new(item);
        let inputs = item.inputs();

        // Check that no conflicting transaction has a higher gas price.
        let mut replacees = Vec::new();
        for input in inputs.clone() {
            if let Some(conflict) = self.by_input.get(input) {
                if !force && conflict.gas_price() >= item.gas_price() {
                    return;
                } else {
                    replacees.push(conflict.clone());
                }
            }
        }

        // Remove replaced transactions.
        for replacee in replacees.into_iter() {
            self.remove(&replacee);
        }

        // Insert the new transaction.
        for input in inputs {
            self.by_input.insert(*input, item.clone());
        }

        self.by_gas.entry(*item.gas_price()).or_default().push(item);

        self.len += 1;

        if self.len > self.max_len {
            let v = self.by_gas.first_key_value().unwrap().1[0].clone();
            self.remove(&v);
        }
    }

    /// Removes all transactions from the pool that conflict with `other`.
    pub fn remove_conflicting<U>(&mut self, other: &U)
    where
        U: Transaction,
    {
        self.remove_conflicting_inputs(other.inputs());
    }

    fn remove_conflicting_inputs(&mut self, inputs: Inputs) {
        for input in inputs {
            let old = match self.by_input.remove(input) {
                Some(o) => o,
                None => continue,
            };

            let same_gas = match self.by_gas.get_mut(old.gas_price()) {
                Some(s) => s,
                None => panic!("transaction missing by gas"),
            };

            let before = same_gas.len();
            same_gas.retain(|e| *e != old);

            let removed = before - same_gas.len();
            assert_eq!(1, removed, "too many transactions removed by gas");

            self.len -= removed;
        }
    }
}

#[derive(Debug, Ord, Eq, PartialEq, PartialOrd)]
pub struct Identified(Deposit, U256);

impl fmt::Display for Identified {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "{}: {}", self.1, self.0)
    }
}

impl Deref for Identified {
    type Target = Deposit;

    fn deref(&self) -> &Deposit {
        &self.0
    }
}

impl DerefMut for Identified {
    fn deref_mut(&mut self) -> &mut Deposit {
        &mut self.0
    }
}

impl Identified {
    pub fn id(&self) -> &U256 {
        &self.1
    }

    pub fn split(self) -> (Deposit, U256) {
        (self.0, self.1)
    }
}

#[derive(Debug)]
pub struct DepositPool {
    max_len: usize,
    by_id: HashMap<U256, Arc<Identified>>,
    by_bounty: BTreeSet<Arc<Identified>>,
}

impl Default for DepositPool {
    fn default() -> Self {
        Self {
            max_len: Self::DEFAULT_MAX_LEN,
            by_bounty: BTreeSet::new(),
            by_id: HashMap::new(),
        }
    }
}

impl DepositPool {
    pub const DEFAULT_MAX_LEN: usize = 1024;

    pub fn iter(&self) -> impl Iterator<Item = &Identified> {
        self.by_bounty.iter().map(Arc::as_ref).rev()
    }

    pub fn insert(&mut self, item: Identified) {
        let arc = Arc::new(item);
        let old = self.by_id.insert(*arc.id(), arc.clone());

        if let Some(old) = old {
            assert!(old == arc, "inserted item didn't match existing");
            self.by_bounty.remove(&old);
        }

        self.by_bounty.insert(arc);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Debug, Eq, PartialEq, Clone)]
    struct MockTx {
        gasprice: U256,
        input0: U256,
        input1: Option<U256>,
    }

    impl MockTx {
        fn one<T, U>(gasprice: T, input0: U) -> Self
        where
            T: Into<U256>,
            U: Into<U256>,
        {
            Self {
                gasprice: gasprice.into(),
                input0: input0.into(),
                input1: None,
            }
        }

        fn two<T, U, V>(gasprice: T, input0: U, input1: V) -> Self
        where
            T: Into<U256>,
            U: Into<U256>,
            V: Into<U256>,
        {
            Self {
                gasprice: gasprice.into(),
                input0: input0.into(),
                input1: Some(input1.into()),
            }
        }
    }

    impl Transaction for MockTx {
        fn gas_price(&self) -> &U256 {
            &self.gasprice
        }

        fn inputs(&self) -> Inputs {
            if let Some(ref input1) = self.input1 {
                Inputs::Two(&self.input0, input1)
            } else {
                Inputs::One(&self.input0)
            }
        }
    }

    #[test]
    fn len_zero() {
        let pool = Pool::<MockTx>::default();
        assert_eq!(pool.len(), 0);
    }

    #[test]
    fn insert_when_empty() {
        let mut pool = Pool::<MockTx>::default();
        pool.insert(MockTx::two(27, 97, 103));
        assert_eq!(pool.len(), 1);

        let peeked = pool.peek().expect("pool should contain a transaction");
        assert_eq!(peeked.gasprice, 27.into());
        assert_eq!(peeked.input0, 97.into());
        assert_eq!(peeked.input1, Some(103.into()));
    }

    #[test]
    fn insert_without_conflict() {
        let mut pool = Pool::<MockTx>::default();

        let tx0 = MockTx::two(27, 97, 103);
        pool.insert(tx0.clone());

        let tx1 = MockTx::two(29, 98, 104);
        pool.insert(tx1.clone());

        let rc0 = &[Arc::new(tx0)];
        let rc1 = &[Arc::new(tx1)];

        assert_eq!(pool.len(), 2);

        assert_eq!(pool.by_gas.len(), 2);
        assert_eq!(pool.by_gas[&27.into()], rc0);
        assert_eq!(pool.by_gas[&29.into()], rc1);

        assert_eq!(pool.by_input.len(), 4);
        assert_eq!(pool.by_input[&97.into()], rc0[0]);
        assert_eq!(pool.by_input[&103.into()], rc0[0]);
        assert_eq!(pool.by_input[&98.into()], rc1[0]);
        assert_eq!(pool.by_input[&104.into()], rc1[0]);
    }

    #[test]
    fn insert_with_conflict_replace() {
        let mut pool = Pool::<MockTx>::default();

        let tx0 = MockTx::two(27, 97, 103);
        pool.insert(tx0.clone());

        let tx1 = MockTx::two(29, 98, 103);
        pool.insert(tx1.clone());

        let rc1 = &[Arc::new(tx1)];

        assert_eq!(pool.len(), 1);

        assert_eq!(pool.by_gas.len(), 1);
        assert_eq!(pool.by_gas[&29.into()], rc1);

        assert_eq!(pool.by_input.len(), 2);
        assert_eq!(pool.by_input[&103.into()], rc1[0]);
        assert_eq!(pool.by_input[&98.into()], rc1[0]);
    }

    #[test]
    fn insert_with_conflict_no_replace() {
        let mut pool = Pool::<MockTx>::default();

        let tx0 = MockTx::two(27, 97, 103);
        pool.insert(tx0.clone());

        let tx1 = MockTx::two(26, 98, 103);
        pool.insert(tx1.clone());

        let rc0 = &[Arc::new(tx0)];

        assert_eq!(pool.len(), 1);

        assert_eq!(pool.by_gas.len(), 1);
        assert_eq!(pool.by_gas[&27.into()], rc0);

        assert_eq!(pool.by_input.len(), 2);
        assert_eq!(pool.by_input[&103.into()], rc0[0]);
        assert_eq!(pool.by_input[&97.into()], rc0[0]);
    }

    #[test]
    fn peek_empty() {
        let pool = Pool::<MockTx>::default();
        assert!(pool.peek().is_none());
    }

    #[test]
    fn peek_with_one() {
        let mut pool = Pool::<MockTx>::default();

        let tx0 = MockTx::two(27, 97, 103);
        pool.insert(tx0.clone());

        assert_eq!(pool.peek(), Some(&tx0));
    }

    #[test]
    fn peek_with_two_asc() {
        let mut pool = Pool::<MockTx>::default();

        let tx0 = MockTx::two(27, 97, 103);
        let tx1 = MockTx::two(28, 99, 109);

        pool.insert(tx0.clone());
        pool.insert(tx1.clone());

        assert_eq!(pool.peek(), Some(&tx1));
    }

    #[test]
    fn peek_with_two_dsc() {
        let mut pool = Pool::<MockTx>::default();

        let tx0 = MockTx::two(27, 97, 103);
        let tx1 = MockTx::two(28, 99, 109);

        pool.insert(tx1.clone());
        pool.insert(tx0.clone());

        assert_eq!(pool.peek(), Some(&tx1));
    }

    #[test]
    fn remove() {
        let mut pool = Pool::default();
        let tx0 = MockTx::two(27, 100, 101);
        pool.insert(tx0.clone());
        pool.remove(&tx0);
        assert_eq!(pool.len(), 0);
    }
}
