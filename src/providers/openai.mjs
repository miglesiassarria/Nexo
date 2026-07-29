const OPENAI_URL = "https://api.openai.com/v1/chat/completions";

export function createOpenAiProvider(apiKey, fetchImpl = fetch) {
  return {
    id: "openai",
    async listModels() {
      return [{ id: "openai/gpt-4o-mini", owned_by: "openai", capabilities: ["chat"] }];
    },
    async chat({ model, messages, ...options }) {
      const upstreamModel = model.replace(/^openai\//, "");
      const response = await fetchImpl(OPENAI_URL, {
        method: "POST",
        headers: { authorization: `Bearer ${apiKey}`, "content-type": "application/json" },
        body: JSON.stringify({ model: upstreamModel, messages, ...options })
      });
      if (!response.ok) throw new Error(`OpenAI respondió ${response.status}: ${await response.text()}`);
      return await response.json();
    }
  };
}
