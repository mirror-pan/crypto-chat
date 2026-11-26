// SPDX-License-Identifier: MIT
pragma solidity ^0.8.20;

import "forge-std/Test.sol";
import "../src/Chat.sol";

contract ChatTest is Test {
    Chat public chat;
    address public alice = makeAddr("alice");
    address public bob = makeAddr("bob");

    // Real Phase 2 keys, already verified: address = keccak256(pubkey)[-20:].
    bytes public alicePub =
        hex"3bb2b20ef1090114c393076af2a352250f4c10250aad6ec4b9452cf5973e57e26385a55b0d1dda82f79ee921c5df393125d66a07f57a2bcd2ae7e5a9e89dcd5b";
    bytes public bobPub =
        hex"db4d95fbe324c59aaa5519e02fe24fa8b25970cd23bbef5d3b3036e5cfa79d7d9ad9a24694fe7885bc2036b47bcc5e95fe9d2fcbf8e0f79d3608d3a3f2f51f87";

    address public constant ALICE_ADDR = 0xC22C5F1A8dED04ABB16B3A85A8c1832c221437e2;
    address public constant BOB_ADDR = 0x86497702dbaE7ea5d259bE4cD5d72A6d5082DBc7;

    event MessageSent(
        address indexed sender,
        address indexed recipient,
        bytes encryptedContent,
        uint256 timestamp
    );

    event PublicKeyRegistered(address indexed user, bytes publicKey);

    function setUp() public {
        chat = new Chat();
    }

    // --- Message relay -----------------------------------------------------

    function test_SendSuccessfulMessage() public {
        bytes memory samplePayload = hex"cafebabe";
        vm.prank(alice);

        vm.expectEmit(true, true, false, true);
        emit MessageSent(alice, bob, samplePayload, block.timestamp);

        chat.sendMessage(bob, samplePayload);
    }

    function test_RevertWhen_SendToZeroAddress() public {
        vm.prank(alice);
        vm.expectRevert("Chat: invalid recipient address");
        chat.sendMessage(address(0), hex"cafebabe");
    }

    function test_RevertWhen_SendEmptyMessage() public {
        vm.prank(alice);
        vm.expectRevert("Chat: message content cannot be empty");
        chat.sendMessage(bob, "");
    }

    function test_RevertWhen_SendToSelf() public {
        vm.prank(alice);
        vm.expectRevert("Chat: cannot send message to self");
        chat.sendMessage(alice, hex"cafebabe");
    }

    // --- Public key registry ------------------------------------------------

    function test_RegisterRawPublicKey() public {
        vm.prank(ALICE_ADDR);
        vm.expectEmit(true, false, false, true);
        emit PublicKeyRegistered(ALICE_ADDR, alicePub);

        chat.registerPublicKey(alicePub);

        assertEq(chat.publicKeys(ALICE_ADDR), alicePub);
        assertEq(chat.publicKeys(BOB_ADDR), bytes(""));
    }

    function test_RegisterPrefixedPublicKey() public {
        bytes memory prefixed = abi.encodePacked(hex"04", alicePub);

        vm.prank(ALICE_ADDR);
        chat.registerPublicKey(prefixed);

        // Stored in canonical 64-byte form regardless of input format.
        assertEq(chat.publicKeys(ALICE_ADDR), alicePub);
    }

    function test_RegisterTwiceRotatesKey() public {
        vm.prank(ALICE_ADDR);
        chat.registerPublicKey(alicePub);

        vm.prank(ALICE_ADDR);
        chat.registerPublicKey(alicePub);

        assertEq(chat.publicKeys(ALICE_ADDR), alicePub);
    }

    function test_RevertWhen_PublicKeyDoesNotMatchCaller() public {
        vm.prank(ALICE_ADDR);
        vm.expectRevert("Chat: public key does not match caller");
        chat.registerPublicKey(bobPub);
    }

    function test_RevertWhen_PublicKeyHasWrongLength() public {
        vm.prank(ALICE_ADDR);
        vm.expectRevert("Chat: public key must be 64 or 65 bytes");
        chat.registerPublicKey(hex"1234");
    }
}
