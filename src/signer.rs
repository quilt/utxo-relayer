// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use ethers::signers::{ClientError, Signer};
use ethers::types::{
    Address, NameOrAddress, Signature, Transaction, TransactionRequest,
};
use ethers::utils::keccak256;

#[derive(Debug, Clone, Copy)]
pub struct AbstractSigner {
    chain_id: Option<u64>,
}

impl AbstractSigner {
    pub fn new(chain_id: Option<u64>) -> Self {
        Self { chain_id }
    }
}

impl Signer for AbstractSigner {
    type Error = ClientError;

    /// Signs the hash of the provided message after prefixing it
    fn sign_message<S: AsRef<[u8]>>(&self, _: S) -> Signature {
        Signature {
            v: 0,
            r: Default::default(),
            s: Default::default(),
        }
    }

    /// Signs the transaction
    fn sign_transaction(
        &self,
        tx: TransactionRequest,
    ) -> Result<Transaction, Self::Error> {
        // TODO: Return error instead of panicking.

        // The nonce, gas and gasprice fields must already be populated
        let gas = tx.gas.unwrap();

        let signature = Signature {
            v: 0,
            r: Default::default(),
            s: Default::default(),
        };

        // Get the actual transaction hash
        let rlp = tx.rlp_signed(&signature);
        let hash = keccak256(&rlp.0);

        // This function should not be called with ENS names
        let to = tx.to.map(|to| match to {
            NameOrAddress::Address(inner) => inner,
            NameOrAddress::Name(_) => panic!(
                "Expected `to` to be an Ethereum Address, not an ENS name"
            ),
        });

        Ok(Transaction {
            hash: hash.into(),
            nonce: Default::default(),
            from: self.address(),
            to,
            value: tx.value.unwrap_or_default(),
            gas_price: Default::default(),
            gas,
            input: tx.data.unwrap_or_default(),
            v: Default::default(),
            r: Default::default(),
            s: Default::default(),

            // Leave these empty as they're only used for included transactions
            block_hash: None,
            block_number: None,
            transaction_index: None,
        })
    }

    /// Returns the signer's Ethereum Address
    fn address(&self) -> Address {
        Address::from([
            0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF,
            0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF,
        ])
    }
}
