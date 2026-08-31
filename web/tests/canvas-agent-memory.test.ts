import { describe, expect, it } from "bun:test";
import {
    compactCanvasAgentHistory,
    estimateCanvasAgentInputTokens,
    estimateTextTokens,
    groupProtocolMessages,
    serializeCanvasAgentMessagesForCheckpoint,
} from "../src/app/(user)/canvas/agent/canvas-agent-memory";
import type { CanvasAgentProtocolMessage } from "../src/app/(user)/canvas/types";

describe("canvas-agent-memory", () => {
    it("正确估算 Token 数量", () => {
        const text = "Hello world";
        const tokens = estimateTextTokens(text);
        expect(tokens).toBeGreaterThan(0);

        const inputTokens = estimateCanvasAgentInputTokens({
            systemPrompt: "You are an assistant",
            messages: [{ role: "user", content: "Test message" }],
            tools: [],
        });
        expect(inputTokens).toBeGreaterThan(0);
    });

    it("正确归纳与分组多轮对话轮次", () => {
        const messages: CanvasAgentProtocolMessage[] = [
            { role: "system", content: "Init system" },
            { role: "user", content: "Round 1 question" },
            { role: "assistant", content: "Round 1 answer" },
            { role: "user", content: "Round 2 question" },
            { role: "assistant", content: "Round 2 answer" },
            { role: "user", content: "Round 3 in progress" },
        ];

        const { fixedMessages, completedRounds, unfinishedRound } = groupProtocolMessages(messages);
        expect(fixedMessages.length).toBe(1);
        expect(fixedMessages[0].role).toBe("system");
        expect(completedRounds.length).toBe(2);
        expect(unfinishedRound.length).toBe(1);
        expect(unfinishedRound[0].content).toBe("Round 3 in progress");
    });

    it("在未超出预算时保持原消息不压缩", async () => {
        const messages: CanvasAgentProtocolMessage[] = [
            { role: "user", content: "Short message" },
            { role: "assistant", content: "Short response" },
        ];

        let checkpointCalled = false;
        const result = await compactCanvasAgentHistory({
            protocolMessages: messages,
            createCheckpoint: async () => {
                checkpointCalled = true;
                return "checkpoint summary";
            },
        });

        expect(result.compacted).toBe(false);
        expect(checkpointCalled).toBe(false);
        expect(result.protocolMessages.length).toBe(2);
    });

    it("正确序列化消息供 Checkpoint 摘要生成", () => {
        const messages: CanvasAgentProtocolMessage[] = [
            { role: "user", content: "Create shot 1" },
            {
                role: "assistant",
                content: "Created",
                toolCalls: [{ id: "call_1", name: "generate_video", arguments: { prompt: "test" } }],
            },
            { role: "tool", name: "generate_video", content: "ok", toolCallId: "call_1" },
        ];

        const serialized = serializeCanvasAgentMessagesForCheckpoint(messages);
        expect(serialized.length).toBe(3);
        expect(serialized[0].role).toBe("user");
        expect(serialized[1].toolCalls?.[0].name).toBe("generate_video");
    });
});
