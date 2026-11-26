// SPDX-License-Identifier: MIT
pragma solidity ^0.8.20;

/**
 * @title Decryptable E2EE Chat Kernel
 * @author Mingjing Pan
 * @notice This contract acts as an immutable message relay protocol using event logs.
 *         It also stores a public-key registry so peers can discover each other's
 *         ECDH public key by address. Only public data is stored on-chain.
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

    // Event emitted when an account publishes or rotates its public key.
    event PublicKeyRegistered(address indexed user, bytes publicKey);

    /// @notice Canonical 64-byte secp256k1 public key (X || Y, no 0x04 prefix)
    ///         for each address. Empty until that account registers.
    mapping(address => bytes) public publicKeys;

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

    /**
     * @notice Publish the caller's secp256k1 public key so peers can look it up.
     * @dev Accepts either 64 raw bytes (X || Y) or 65 bytes with a leading 0x04.
     *      The derived address must equal msg.sender, so an account cannot
     *      register a key belonging to someone else. Re-registration rotates
     *      the key. No private key material is ever submitted.
     * @param publicKey Encoded public key.
     */
    function registerPublicKey(bytes calldata publicKey) external {
        bytes calldata xy = publicKey;
        if (publicKey.length == 65 && publicKey[0] == 0x04) {
            xy = publicKey[1:];
        }
        require(xy.length == 64, "Chat: public key must be 64 or 65 bytes");

        address derived = address(uint160(uint256(keccak256(xy))));
        require(derived == msg.sender, "Chat: public key does not match caller");

        publicKeys[msg.sender] = xy;
        emit PublicKeyRegistered(msg.sender, xy);
    }
}
