const GEMINI_URL = "https://generativelanguage.googleapis.com/v1beta/models";

function toGeminiContents(messages) {
  return messages
    .filter((message) => message.role !== "system")
    .map((message) => ({
      role: message.role === "assistant" ? "model" : "user",
      parts: [{ text: typeof message.content === "string" ? message.content : JSON.stringify(message.content) }]
    }));
}

export function createGoogleGeminiProvider({ accessToken = "", apiKey = "" } = {}, fetchImpl = fetch) {
  return {
    id: "google",
    async listModels() {
      return [{ id: "google/gemini-2.5-flash", owned_by: "google", capabilities: ["chat", "multimodal"] }];
    },
    async chat({ model, messages, ...options }) {
      const upstreamModel = model.replace(/^google\//, "");
      const url = new URL(`${GEMINI_URL}/${upstreamModel}:generateContent`);
      const headers = { "content-type": "application/json" };
      if (accessToken) headers.authorization = `Bearer ${accessToken}`;
      else if (apiKey) url.searchParams.set("key", apiKey);
      else throw new Error("Google Gemini necesita un OAuth access token o una API key");

      const systemMessage = messages.find((message) => message.role === "system");
      const body = {
        contents: toGeminiContents(messages),
        ...(systemMessage ? { systemInstruction: { parts: [{ text: systemMessage.content }] } } : {}),
        generationConfig: {
          ...(options.temperature === undefined ? {} : { temperature: options.temperature }),
          ...(options.max_tokens === undefined ? {} : { maxOutputTokens: options.max_tokens })
        }
      };
      const response = await fetchImpl(url, { method: "POST", headers, body: JSON.stringify(body) });
      if (!response.ok) throw new Error(`Gemini respondió ${response.status}: ${await response.text()}`);
      const result = await response.json();
      const text = result.candidates?.[0]?.content?.parts?.map((part) => part.text || "").join("") || "";
      return {
        id: `chatcmpl-google-${Date.now()}`,
        object: "chat.completion",
        created: Math.floor(Date.now() / 1000),
        model,
        choices: [{ index: 0, message: { role: "assistant", content: text }, finish_reason: "stop" }],
        usage: result.usageMetadata || {}
      };
    }
  };
}
