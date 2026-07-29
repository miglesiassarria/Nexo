import { createGoogleGeminiProvider } from "./google-gemini.mjs";
import { createMockProvider } from "./mock.mjs";
import { createOpenAiProvider } from "./openai.mjs";

export function createProviderRegistry(config) {
  const providers = new Map([["mock", createMockProvider()]]);
  if (config.openAiApiKey) providers.set("openai", createOpenAiProvider(config.openAiApiKey));
  if (config.googleOauthAccessToken || config.googleGeminiApiKey) {
    providers.set("google", createGoogleGeminiProvider({ accessToken: config.googleOauthAccessToken, apiKey: config.googleGeminiApiKey }));
  }
  return providers;
}

export async function listModels(providers) {
  const models = [];
  for (const provider of providers.values()) models.push(...await provider.listModels());
  return models;
}
