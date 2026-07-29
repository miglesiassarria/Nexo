import http from "node:http";
import { loadConfig } from "./config.mjs";
import { createGateway } from "./gateway.mjs";
import { createProviderRegistry } from "./providers/registry.mjs";

const config = loadConfig();
const providers = createProviderRegistry(config);
const server = http.createServer(createGateway({ providers, apiToken: config.apiToken }));

server.listen(config.port, config.host, () => {
  console.log(`Nexo escuchando en http://${config.host}:${config.port}`);
  console.log(`Proveedores disponibles: ${[...providers.keys()].join(", ")}`);
});
