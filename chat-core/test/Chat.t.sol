// SPDX-License-Identifier: MIT
pragma solidity ^0.8.20;

import "forge-std/Test.sol";
import "../src/Chat.sol";

contract ChatTest is Test {
    Chat public chat;
    address public alice = makeAddr("alice");
    address public bob = makeAddr("bob");

    // Re-declare the event for testing expectations
    event MessageSent(
        address indexed sender,
        address indexed recipient,
        bytes encryptedContent,
        uint256 timestamp
    );

    function setUp() public {
        chat = new Chat();
    }

    function test_SendSuccessfulMessage() public {
        // Arrange
        bytes memory samplePayload = hex"cafebabe";
        vm.prank(alice);

        // Act & Assert: Expect the MessageSent event with correct arguments
        vm.expectEmit(true, true, false, true);
        emit MessageSent(alice, bob, samplePayload, block.timestamp);

        chat.sendMessage(bob, samplePayload);
    }

    // 1. 改为普通的 test_ 前缀，并精确匹配 revert 的 message
    function test_RevertWhen_SendToZeroAddress() public {
        bytes memory samplePayload = hex"cafebabe";
        vm.prank(alice);
        
        // 期望下一条指令以特定错误信息 revert
        vm.expectRevert("Chat: invalid recipient address");
        chat.sendMessage(address(0), samplePayload);
    }

    // 2. 空消息测试
    function test_RevertWhen_SendEmptyMessage() public {
        bytes memory samplePayload = "";
        vm.prank(alice);
        
        // 期望下一条指令以特定错误信息 revert
        vm.expectRevert("Chat: message content cannot be empty");
        chat.sendMessage(bob, samplePayload);
    }

    // 3. 补充：测试向自己发送消息（覆盖第三个 require）
    function test_RevertWhen_SendToSelf() public {
        bytes memory samplePayload = hex"cafebabe";
        vm.prank(alice);

        vm.expectRevert("Chat: cannot send message to self");
        chat.sendMessage(alice, samplePayload);
    }
}