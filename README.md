Relayer
=======

# Important Note

Pending [this bug][0], it doesn't seem possible to run the relayer in its entirety. You can run the pending transaction watcher, or the block watcher, but not both.

[0]: https://github.com/gakonst/ethers-rs/issues/51

# Introduction

This is an example program that, when paired with an Ethereum smart contract, emulates a UTXO transaction model. It is an exploration of how a dApp could be built using Quilt's Account Abstraction fork of [go-ethereum (geth)][1].

[1]: https://github.com/quilt/go-ethereum/

# Setup

## Private Key

The relayer requires a private key to sign deposit claims. Put a private key in `key.hex` in the same directory as `Cargo.toml`, in hex format without the `0x` prefix. Do not use a key you care about losing.

## UTXO and Dropsafe Contracts

There are two smart contracts deployed on the Kovan testnet that work with this relayer.
