// SPDX-License-Identifier: MPL

pragma solidity ^0.6.8;
pragma experimental ABIEncoderV2;
pragma experimental AccountAbstraction;

import "SafeMath.sol";
import "Dropsafe.sol";

account contract Utxo {
    using SafeMath for uint256;

    struct Output {
        address owner;
        uint256 amount;
    }

    struct Transfer {
        uint256 input0;
        uint256 input1;

        address destination;
        address change;

        uint256 amount;
        uint256 gasprice;

        uint8 v;
        bytes32 r;
        bytes32 s;
    }

    struct Claim {
        uint256 input;
        uint256 gasprice;
        uint256[] deposits;

        uint8 v;
        bytes32 r;
        bytes32 s;
    }

    struct Withdrawal {
        uint256 input;
        uint256 gasprice;

        uint8 v;
        bytes32 r;
        bytes32 s;
    }

    struct Payment {
        address payable destination;
        uint256 amount;
    }

    //
    // Constants
    //
    uint256 public constant MAX_WITHDRAWS = 10;

    bytes32 public constant CLAIM_TYPEHASH = keccak256("Claim(uint256 input,uint256 gasprice,uint256[] deposits)");
    bytes32 public constant TRANSFER_TYPEHASH = keccak256("Transfer(uint256 input0,uint256 input1,address destination,address change,uint256 amount,uint256 gasprice)");
    bytes32 public constant WITHDRAW_TYPEHASH = keccak256("Withdraw(uint256 input,uint256 gasprice)");

    uint256 constant GAS_WITHDRAW = 2; // TODO
    uint256 constant GAS_TRANSFER = 3; // TODO
    uint256 constant GAS_CLAIM_CONSTANT = 5; // TODO
    uint256 constant GAS_CLAIM_VARIABLE = 7; // TODO

    uint256 constant MAX_SLOTS = 10;
    uint256 constant SLOTS_DEPOSIT = 3;
    uint256 constant SLOTS_TRANSFER = 2;
    uint256 constant SLOTS_WITHDRAWAL = 1;


    bytes32 public immutable DOMAIN_SEPARATOR;

    Dropsafe public immutable DROPSAFE;

    //
    // Bundle Incentives
    //
    uint256 fee_base;

    function get_fee_base() external view returns (uint256) {
        assembly { paygas(0) }
        return fee_base;
    }

    //
    // UTXOs
    //
    uint256 utxo_count;

    function get_utxo_count() external view returns (uint256) {
        assembly { paygas(0) }
        return utxo_count;
    }

    mapping(uint256 => Output) utxos;

    function get_utxo(uint256 id) external view returns (Output memory) {
        assembly { paygas(0) }
        return utxos[id];
    }

    mapping(uint256 => uint256) deposits;

    function get_deposits(uint256 chunk) external view returns (uint256) {
        assembly { paygas(0) }
        return deposits[chunk];
    }


    //
    // Reentrancy Protection
    //
    bool transacting;

    modifier nonreentrant() {
        require(!transacting, "utxo/reenter");
        transacting = true;
        _;
        transacting = false;
    }

    constructor() public payable {
        DROPSAFE = new Dropsafe();

        uint8 chain_id;
        assembly {
            chain_id := chainid()
        }

        DOMAIN_SEPARATOR = keccak256(abi.encode(
            keccak256("EIP712Domain(string name,string version,uint256 chainId,address verifyingContract)"),
            keccak256("utxo"),
            keccak256("1"),
            chain_id,
            address(this)
        ));

        // Create the initial deposit.
        new_utxo(msg.sender, msg.value);

        // Set the initial base value.
        update_fee_base(tx.gasprice);
    }

    receive() external payable {}

    function update_fee_base(uint256 gasprice) private {
        fee_base = gasprice - (gasprice / 5);
    }

    function deposit_at(uint256 id) private pure returns (uint256 index, uint256 mask) {
        index = id / 256;
        mask = 1 << (id - (index * 256));
    }

    function check_deposit(uint256 id) private view {
        (uint256 index, uint256 mask) = deposit_at(id);
        uint256 used = deposits[index] & mask;
        require(0 == used, "utxo/deposit");
    }

    function consume_deposit(uint256 id) private {
        (uint256 index, uint256 mask) = deposit_at(id);
        deposits[index] |= mask;
    }

    function new_utxo(address owner, uint256 amount) private {
        uint256 next = utxo_count.add(1);
        utxo_count = next;

        Output storage output = utxos[next];
        output.owner = owner;
        output.amount = amount;
    }

    function consume_utxo(uint256 idx, address signer) private returns (uint256 unspent) {
        if (idx == 0) {
            unspent = 0;
        } else {
            Output storage utxo0 = utxos[idx];
            require(signer == utxo0.owner, "utxo/signature");
            unspent = utxo0.amount;
            delete utxos[idx];
        }
    }

    function deduct(uint256 unspent0, uint256 unspent1, uint256 fees, uint256 amount) private pure returns (uint256 change) {
        // Calculate unspent funds after transferring amount.
        if (amount > unspent0) {
            amount -= unspent0;
            unspent0 = 0;
        } else {
            unspent0 -= amount;
            amount = 0;
        }

        if (amount > unspent1) {
            amount -= unspent1;
            unspent1 = 0;
        } else {
            unspent1 -= amount;
            amount = 0;
        }

        // Calculate unspent funds after paying fees.
        if (fees > unspent0) {
            fees -= unspent0;
            unspent0 = 0;
        } else {
            unspent0 -= fees;
            fees = 0;
        }

        if (fees > unspent1) {
            fees -= unspent1;
            unspent1 = 0;
        } else {
            unspent1 -= fees;
            fees = 0;
        }

        change = unspent0.add(unspent1);
        require(fees == 0 && amount == 0, "utxo/insufficient-input");
    }

    function transfer(uint256 bundle_gasprice, Transfer memory xfr) private {
        bytes32 digest = keccak256(
            abi.encodePacked(
                "\x19\x01",
                DOMAIN_SEPARATOR,
                keccak256(
                    abi.encode(
                        TRANSFER_TYPEHASH,
                        xfr.input0,
                        xfr.input1,
                        xfr.destination,
                        xfr.change,
                        xfr.amount,
                        xfr.gasprice
                    )
                )
            )
        );

        address signer = ecrecover(digest, xfr.v, xfr.r, xfr.s);

        signer = utxos[xfr.input0].owner;   // DISABLES THE SIGNATURE CHECK!!!!!!!!!!!!!!!

        uint256 fees = bundle_gasprice * GAS_TRANSFER;

        uint256 unspent0 = consume_utxo(xfr.input0, signer);
        uint256 unspent1 = consume_utxo(xfr.input1, signer);

        new_utxo(xfr.destination, xfr.amount);

        uint256 unspent_total = deduct(unspent0, unspent1, fees, xfr.amount);

        if (unspent_total > 0) {
            new_utxo(xfr.change, unspent_total);
        }
    }

    function withdraw(uint256 bundle_gasprice, Withdrawal memory wth) private returns (Payment memory payment) {
        bytes32 digest = keccak256(
            abi.encodePacked(
                "\x19\x01",
                DOMAIN_SEPARATOR,
                keccak256(
                    abi.encode(
                        WITHDRAW_TYPEHASH,
                        wth.input,
                        wth.gasprice
                    )
                )
            )
        );

        address signer = ecrecover(digest, wth.v, wth.r, wth.s);

        signer = utxos[wth.input].owner;   // DISABLES THE SIGNATURE CHECK!!!!!!!!!!!!!!!

        uint256 fees = bundle_gasprice * GAS_WITHDRAW;

        uint256 unspent = consume_utxo(wth.input, signer);
        uint256 unspent_total = deduct(unspent, 0, fees, 0);

        payment.destination = payable(signer);
        payment.amount = unspent_total;
    }

    function compute_gasprice(uint256 minimum_gasprice, uint256 n_deposits, uint256 n_xfers, uint256 n_wths) private view returns (uint256) {
        uint256 slots = (n_deposits * SLOTS_DEPOSIT) + (n_xfers * SLOTS_TRANSFER) + (n_wths * SLOTS_WITHDRAWAL);
        require(slots <= MAX_SLOTS, "utxo/slots");

        uint256 current_fee_base = fee_base;

        if (minimum_gasprice <= current_fee_base) {
            return minimum_gasprice;
        }

        uint256 bonus = (minimum_gasprice - current_fee_base) * slots; // TODO: Overflow?
        bonus /= MAX_SLOTS;

        return current_fee_base + bonus;
    }

    function transact(Claim calldata claim, Transfer[] calldata xfrs, Withdrawal[] calldata wths) external nonreentrant {
        // TODO: check that msg.gas >= expected gas usage, but not significantly greater.

        // Calculate the gas price the bundle is willing to pay.
        uint256 minimum_gasprice = type(uint256).max;

        for (uint ii = 0; ii < xfrs.length; ii++) {
            uint256 gasprice = xfrs[ii].gasprice;
            if (gasprice < minimum_gasprice) {
                minimum_gasprice = gasprice;
            }
        }

        for (uint ii = 0; ii < wths.length; ii++) {
            uint256 gasprice = wths[ii].gasprice;
            if (gasprice < minimum_gasprice) {
                minimum_gasprice = gasprice;
            }
        }

        uint256 bundle_gasprice = compute_gasprice(minimum_gasprice, claim.deposits.length, xfrs.length, wths.length);

        // Verify claim signature and take fees.
        uint256 claim_change = 0;
        address claim_sponsor;
        if (claim.deposits.length > 0) {
            if (claim.gasprice < bundle_gasprice) {
                bundle_gasprice = claim.gasprice;
            }

            // TODO: Verify that this encodes according to EIP-712
            bytes32 digest = keccak256(
                abi.encodePacked(
                    "\x19\x01",
                    DOMAIN_SEPARATOR,
                    keccak256(
                        abi.encode(
                            CLAIM_TYPEHASH,
                            claim.input,
                            claim.gasprice,
                            keccak256(abi.encodePacked(claim.deposits))
                        )
                    )
                )
            );

            claim_sponsor = ecrecover(digest, claim.v, claim.r, claim.s);
            claim_sponsor = utxos[claim.input].owner;   // DISABLES THE SIGNATURE CHECK!!!!!!!!!!!!!!!
            claim_change = consume_utxo(claim.input, claim_sponsor);

            for (uint256 ii = 0; ii < claim.deposits.length; ii++) {
                check_deposit(claim.deposits[ii]);
            }

            uint256 claim_fees = GAS_CLAIM_CONSTANT + (claim.deposits.length * GAS_CLAIM_VARIABLE);
            claim_fees += bundle_gasprice;

            require(claim_change >= claim_fees, "utxo/claim");
            claim_change -= claim_fees;
        }

        // Process transfers within the UTXO set.
        for (uint ii = 0; ii < xfrs.length; ii++) {
            transfer(bundle_gasprice, xfrs[ii]);
        }

        Payment[] memory payments = new Payment[](wths.length);

        // Process withdrawals to normal Ethereum accounts.
        for (uint ii = 0; ii < wths.length; ii++) {
            payments[ii] = withdraw(bundle_gasprice, wths[ii]);
        }

        update_fee_base(bundle_gasprice);

        assembly { paygas(bundle_gasprice) }

        // Claim deposits from the drop safe.
        for (uint ii = 0; ii < claim.deposits.length; ii++) {
            consume_deposit(claim.deposits[ii]);
            Dropsafe.Deposit memory deposit = DROPSAFE.claim(claim.deposits[ii]);
            claim_change = claim_change.add(deposit.bounty);
            new_utxo(deposit.owner, deposit.amount);
        }

        if (claim_change > 0) {
            new_utxo(claim_sponsor, claim_change);
        }

        // Transfer funds for withdrawals.
        for (uint ii = 0; ii < wths.length; ii++) {
            Payment memory payment = payments[ii];
            payment.destination.send(payment.amount);
        }
    }
}
