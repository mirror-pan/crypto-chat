// SPDX-License-Identifier: MIT
pragma solidity ^0.8.20;

/**
 * @title Decryptable E2EE Chat Kernel
 * @author Mingjing Pan
 * @notice This contract acts as an immutable message relay protocol using event logs.
 */
contract Chat {
    // Event emitted when an encrypted message is dispatched on-chain.
    // Indexed sender and recipient enable efficient off-chain filtering via RPC.
    event MessageSent(
        address indexed sender,
        address indexed recipient,
        bytes encryptedContent,
        uint256 timestamp
    );

    /**
     * @notice Broadcasts an encrypted message to a specific recipient.
     * @param recipient The Ethereum address of the intended message receiver.
     * @param encryptedContent The ciphertext payload resulting from client-side E2EE.
     */
    function sendMessage(address recipient, bytes calldata encryptedContent) external {
        require(recipient != address(0), "Chat: invalid recipient address");
        require(encryptedContent.length > 0, "Chat: message content cannot be empty");
        require(msg.sender != recipient, "Chat: cannot send message to self");

        emit MessageSent(msg.sender, recipient, encryptedContent, block.timestamp);
    }
}