export function createMockProvider() {
  return {
    id: "mock",
    async listModels() {
      return [{ id: "mock/echo", owned_by: "nexo", capabilities: ["chat"] }];
    },
    async chat({ model, messages }) {
      const lastMessage = [...messages].reverse().find((message) => message.role === "user");
      const content = lastMessage?.content ?? "";
      return {
        id: `chatcmpl-mock-${Date.now()}`,
        object: "chat.completion",
        created: Math.floor(Date.now() / 1000),
        model,
        choices: [{ index: 0, message: { role: "assistant", content: `Nexo recibió: ${content}` }, finish_reason: "stop" }],
        usage: { prompt_tokens: 0, completion_tokens: 0, total_tokens: 0 }
      };
    }
  };
}
