// SPDX-License-Identifier: MPL

pragma solidity ^0.6.8;
pragma experimental ABIEncoderV2;

contract Dropsafe {
    struct Deposit {
        uint256 amount;
        uint256 bounty;
        address owner;
    }

    event NewDeposit(uint256 id, uint256 amount, uint256 bounty, address owner);

    address payable immutable UTXO;

    uint256 next;
    mapping(uint256 => Deposit) deposits;

    constructor() public {
        UTXO = msg.sender;
    }

    function deposit(uint256 bounty) external payable {
        require(msg.value > 0, "safe/zero");
        require(msg.value > bounty, "safe/bounty");

        uint256 id = next++;
        uint256 amount = msg.value - bounty;

        Deposit storage created = deposits[id];
        created.owner = msg.sender;
        created.amount = amount;
        created.bounty = bounty;

        emit NewDeposit(id, amount, bounty, msg.sender);
    }

    function claim(uint256 id) external returns (Deposit memory claimed) {
        claimed = deposits[id];
        delete deposits[id];

        require(msg.sender == UTXO, "safe/not-utxo");
        require(claimed.owner != address(0));

        UTXO.transfer(claimed.amount);
    }
}
