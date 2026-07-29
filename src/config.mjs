export function loadConfig(env = process.env) {
  return {
    host: env.NEXO_HOST || "127.0.0.1",
    port: Number(env.NEXO_PORT || 3000),
    apiToken: env.NEXO_API_TOKEN || "",
    openAiApiKey: env.OPENAI_API_KEY || "",
    googleOauthAccessToken: env.GOOGLE_OAUTH_ACCESS_TOKEN || "",
    googleCloudProject: env.GOOGLE_CLOUD_PROJECT || "",
    googleGeminiApiKey: env.GOOGLE_GEMINI_API_KEY || ""
  };
}
