import { listModels } from "./providers/registry.mjs";

function json(res, status, payload) {
  res.writeHead(status, { "content-type": "application/json; charset=utf-8", "cache-control": "no-store" });
  res.end(JSON.stringify(payload));
}

async function readJson(req) {
  let body = "";
  for await (const chunk of req) {
    body += chunk;
    if (body.length > 2_000_000) throw new Error("Request demasiado grande");
  }
  return JSON.parse(body || "{}");
}

export function createGateway({ providers, apiToken = "" }) {
  return async function gateway(req, res) {
    if (apiToken && req.headers.authorization !== `Bearer ${apiToken}`) {
      return json(res, 401, { error: { message: "Token de Nexo no válido", type: "authentication_error" } });
    }

    const url = new URL(req.url, "http://localhost");
    try {
      if (req.method === "GET" && url.pathname === "/health") {
        return json(res, 200, { status: "ok", service: "nexo" });
      }
      if (req.method === "GET" && url.pathname === "/v1/models") {
        return json(res, 200, { object: "list", data: await listModels(providers) });
      }
      if (req.method === "POST" && url.pathname === "/v1/chat/completions") {
        const request = await readJson(req);
        if (!request.model || !Array.isArray(request.messages)) {
          return json(res, 400, { error: { message: "model y messages son obligatorios", type: "invalid_request_error" } });
        }
        const providerId = request.model.split("/", 1)[0];
        const provider = providers.get(providerId);
        if (!provider) {
          return json(res, 404, { error: { message: `Proveedor no configurado: ${providerId}`, type: "provider_error" } });
        }
        if (request.stream) {
          return json(res, 400, { error: { message: "stream todavía no está implementado en este MVP", type: "unsupported_error" } });
        }
        const { model, messages, stream, ...options } = request;
        return json(res, 200, await provider.chat({ model, messages, stream, ...options }));
      }
      return json(res, 404, { error: { message: "Ruta no encontrada", type: "not_found" } });
    } catch (error) {
      return json(res, 500, { error: { message: error.message, type: "gateway_error" } });
    }
  };
}
