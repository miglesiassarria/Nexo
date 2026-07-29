import test from "node:test";
import assert from "node:assert/strict";
import http from "node:http";
import { createGateway } from "../src/gateway.mjs";
import { createMockProvider } from "../src/providers/mock.mjs";

function request(server, method, path, body, headers = {}) {
  return new Promise((resolve, reject) => {
    const address = server.address();
    const req = http.request({ hostname: address.address, port: address.port, method, path, headers: { "content-type": "application/json", ...headers } }, (res) => {
      let data = "";
      res.on("data", (chunk) => { data += chunk; });
      res.on("end", () => resolve({ status: res.statusCode, body: JSON.parse(data) }));
    });
    req.on("error", reject);
    if (body) req.write(JSON.stringify(body));
    req.end();
  });
}

test("expone health y modelos del gateway", async (t) => {
  const server = http.createServer(createGateway({ providers: new Map([["mock", createMockProvider()]]) }));
  await new Promise((resolve) => server.listen(0, "127.0.0.1", resolve));
  t.after(() => server.close());

  const health = await request(server, "GET", "/health");
  assert.equal(health.status, 200);
  assert.equal(health.body.status, "ok");

  const models = await request(server, "GET", "/v1/models");
  assert.equal(models.body.data[0].id, "mock/echo");
});

test("mantiene compatibilidad básica con chat completions", async (t) => {
  const server = http.createServer(createGateway({ providers: new Map([["mock", createMockProvider()]]) }));
  await new Promise((resolve) => server.listen(0, "127.0.0.1", resolve));
  t.after(() => server.close());

  const response = await request(server, "POST", "/v1/chat/completions", {
    model: "mock/echo",
    messages: [{ role: "user", content: "hola" }]
  });
  assert.equal(response.status, 200);
  assert.equal(response.body.choices[0].message.content, "Nexo recibió: hola");
});
