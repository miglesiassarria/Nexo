import { invoke } from "@tauri-apps/api/core";

export type CredentialKind = "api_key" | "subscription_oauth" | "local" | "mock";
export type Accounting = "metered" | "subscription" | "local";
export type CostBasis = "reported" | "estimated" | "subscription" | "unavailable";

export interface GatewayStatus {
  paused: boolean;
  bind_error: string | null;
  port: number;
  base_url: string;
  accounts: number;
  subscription_connected: boolean;
  api_key_connected: boolean;
  broken_accounts: number;
  apps: number;
  apps_missing_limits: string[];
  manifest_version: string;
}

export interface Account {
  id: string;
  provider_id: string;
  credential_kind: CredentialKind;
  label: string;
  external_id: string | null;
  expires_at: number | null;
  status: string;
  risk_ack_at: number | null;
  created_at: number;
  last_used_at: number | null;
}

export interface App {
  id: string;
  name: string;
  token_prefix: string;
  created_at: number;
  last_seen_at: number | null;
  revoked_at: number | null;
  notes: string | null;
}

export interface IssuedApp {
  app: App;
  token: string;
}

export interface Grant {
  provider_id: string;
  credential_kind: string;
  model_pattern: string;
  allow_tools: boolean;
  allow_multimodal: boolean;
  log_content: boolean;
}

export interface Limit {
  provider_id: string;
  credential_kind: string;
  window_seconds: number;
  max_requests: number | null;
  max_input_tokens: number | null;
  max_output_tokens: number | null;
}

/** Un modelo de una vía, con si esta aplicación lo tiene marcado. */
export interface RouteModel {
  public_name: string;
  selected: boolean;
  /** Marcado pero ausente del catálogo de hoy. Se conserva, no se borra. */
  missing: boolean;
  accounting: string;
  priced: boolean;
  caps: Capabilities;
}

/** Los modelos de una vía para una aplicación concreta. */
export interface RouteModels {
  provider_id: string;
  credential_kind: string;
  requires_limit: boolean;
  /**
   * Permiso heredado con `*`: vale para todos los modelos de la vía, también los
   * que el proveedor añada en el futuro. No equivale a marcarlos todos.
   */
  inherited_all: boolean;
  selected: number;
  models: RouteModel[];
}

export interface GrantableRoute {
  provider_id: string;
  credential_kind: string;
  connected: boolean;
  requires_limit: boolean;
  models: number;
}

export interface AppDetail {
  grants: Grant[];
  limits: Limit[];
}

export interface Capabilities {
  text: boolean;
  vision: boolean;
  audio: boolean;
  tools: boolean;
  reasoning: boolean;
  json_mode: boolean;
  streaming: boolean;
  embeddings: boolean;
}

export interface CatalogRow {
  provider_id: string;
  credential_kind: string;
  api_id: string;
  public_name: string;
  caps: Capabilities;
  context_max: number | null;
  input_max: number | null;
  output_max: number | null;
  accounting: Accounting;
  price_input: number | null;
  price_output: number | null;
  available: boolean;
}

export interface UsageBucket {
  bucket: string;
  requests: number;
  errors: number;
  cancels: number;
  rate_limited: number;
  local_limited: number;
  input_tokens: number;
  output_tokens: number;
  total_tokens: number;
  cost_reported_micros: number;
  cost_estimated_micros: number;
  subscription_requests: number;
  avg_latency_ms: number;
  max_latency_ms: number;
  avg_ttft_ms: number | null;
}

export interface RequestRow {
  id: string;
  ts: number;
  app: string;
  provider_id: string;
  credential_kind: string;
  public_model: string;
  status: string;
  error_kind: string | null;
  latency_ms: number | null;
  ttft_ms: number | null;
  input_tokens: number | null;
  output_tokens: number | null;
  total_tokens: number | null;
  usage_source: string;
  cost_micros: number | null;
  cost_basis: CostBasis;
  fallback_from: string | null;
  operation: string;
}

export interface LmStudioStatus {
  base_url: string;
  reachable: boolean;
  models: number;
  loaded: number;
  detail: string | null;
}

/** Proveedor OpenAI-compatible añadido por el usuario: nombre, URL y clave. */
export interface CustomProvider {
  id: string;
  name: string;
  base_url: string;
  compat: string;
  created_at: number;
}

/**
 * De qué comandos dispone una fila. Lo declara el núcleo porque no es cosmético:
 * un proveedor propio se quita con `removeCustomProvider`, y hacerlo con
 * `disconnectAccount` dejaría su definición huérfana.
 */
export type RowManage =
  | { kind: "account" }
  | { kind: "local_server" }
  | { kind: "custom_provider" };

/** Una vía conectada, tal como la compone el núcleo para esta pestaña. */
export interface ProviderRow {
  account_id: string;
  provider_id: string;
  credential_kind: CredentialKind;
  name: string;
  status: string;
  models: number;
  address: string | null;
  manage: RowManage;
  note: string | null;
  expires_at: number | null;
  created_at: number;
  needs_attention: boolean;
}

/**
 * Forma del formulario de alta. La vista tiene una rama por forma, no por
 * proveedor: al ser una unión discriminada, TypeScript avisa si el núcleo añade
 * una forma que la vista no cubre.
 */
export type ConnectForm =
  | { kind: "subscription_oauth" }
  | { kind: "local_server"; default_url: string }
  | { kind: "api_key" }
  | { kind: "compat_endpoint"; suggested_name: string; base_url: string };

/** Una vía que se puede dar de alta. La lista la declara el núcleo. */
export interface ConnectOption {
  id: string;
  name: string;
  summary: string;
  form: ConnectForm;
  note: string | null;
  already_connected: boolean;
  docs_url: string | null;
}

export interface LocalModelDetail {
  api_id: string;
  kind: string;
  quantization: string | null;
  arch: string | null;
  runtime: string | null;
  loaded: boolean;
}

export interface CatalogRefresh {
  provider_id: string;
  credential_kind: string;
  discovered: number;
  error: string | null;
}

export interface Settings {
  port: number;
  allow_lan: boolean;
  retention_days: number;
  content_retention_days: number;
  log_level: string;
  manifest_version: string;
  codex_client_version: string;
}

export interface RiskNotice {
  title: string;
  points: string[];
  confirm_label: string;
}

export const api = {
  gatewayStatus: () => invoke<GatewayStatus>("gateway_status"),
  setPaused: (paused: boolean) => invoke<void>("set_paused", { paused }),

  listAccounts: () => invoke<Account[]>("list_accounts"),
  riskNotice: () => invoke<RiskNotice>("risk_notice"),
  connectChatgpt: (riskAcknowledged: boolean) =>
    invoke<Account>("connect_chatgpt", { riskAcknowledged }),
  connectApiKey: (apiKey: string, label?: string) =>
    invoke<Account>("connect_api_key", { apiKey, label }),
  disconnectAccount: (accountId: string) =>
    invoke<void>("disconnect_account", { accountId }),

  lmstudioStatus: () => invoke<LmStudioStatus>("lmstudio_status"),
  detectLmstudio: () => invoke<LmStudioStatus>("detect_lmstudio"),
  setLmstudioUrl: (baseUrl: string) =>
    invoke<LmStudioStatus>("set_lmstudio_url", { baseUrl }),
  lmstudioModels: () => invoke<LocalModelDetail[]>("lmstudio_models"),

  providerRows: () => invoke<ProviderRow[]>("provider_rows"),
  connectOptions: () => invoke<ConnectOption[]>("connect_options"),
  listCustomProviders: () => invoke<CustomProvider[]>("list_custom_providers"),
  addCustomProvider: (name: string, baseUrl: string, apiKey: string) =>
    invoke<CustomProvider>("add_custom_provider", { name, baseUrl, apiKey }),
  updateCustomProviderUrl: (id: string, baseUrl: string) =>
    invoke<void>("update_custom_provider_url", { id, baseUrl }),
  removeCustomProvider: (id: string) => invoke<void>("remove_custom_provider", { id }),

  listApps: () => invoke<App[]>("list_apps"),
  createApp: (name: string, notes?: string) =>
    invoke<IssuedApp>("create_app", { name, notes }),
  revokeApp: (appId: string) => invoke<void>("revoke_app", { appId }),
  deleteApp: (appId: string) => invoke<void>("delete_app", { appId }),
  appDetail: (appId: string) => invoke<AppDetail>("app_detail", { appId }),
  grantableRoutes: () => invoke<GrantableRoute[]>("grantable_routes"),
  appRouteModels: (args: { appId: string; providerId: string; credentialKind: string }) =>
    invoke<RouteModels>("app_route_models", args),
  setAppModels: (args: {
    appId: string;
    providerId: string;
    credentialKind: string;
    models: string[];
    allowTools: boolean;
    allowMultimodal: boolean;
    maxRequests?: number | null;
    windowSeconds?: number | null;
  }) => invoke<void>("set_app_models", args),

  catalog: () => invoke<CatalogRow[]>("catalog"),
  refreshCatalog: () => invoke<CatalogRefresh[]>("refresh_catalog"),
  usageSummary: (minutes: number, group: string, operation?: string) =>
    invoke<UsageBucket[]>("usage_summary", { minutes, group, operation }),
  recentRequests: (limit: number, minutes: number) =>
    invoke<RequestRow[]>("recent_requests", { limit, minutes }),

  loadSettings: () => invoke<Settings>("load_settings"),
  saveSettings: (settings: Settings) =>
    invoke<{ saved: boolean; restart_required: boolean }>("save_settings", { settings }),
  applyRetention: () =>
    invoke<{ deleted_requests: number; deleted_content: number }>("apply_retention"),
  purgeStats: () => invoke<void>("purge_stats"),
};

// --- Presentación ---------------------------------------------------------

/** Etiqueta legible de la vía de acceso. */
export function kindLabel(kind: string): string {
  switch (kind) {
    case "subscription_oauth":
      return "Suscripción";
    case "api_key":
      return "API key";
    case "local":
      return "Local";
    case "mock":
      return "Prueba";
    default:
      return kind;
  }
}

/**
 * Coste de una fila del panel. Lo local se muestra como «Local», no como
 * «0.0000 $»: el coste es cero y conocido, pero una cifra ahí es ruido.
 */
export function costCellLabel(
  credentialKind: string,
  basis: CostBasis,
  micros: number | null,
): string {
  if (credentialKind === "local" || credentialKind === "mock") return "Local";
  return costLabel(basis, micros);
}

/**
 * Coste legible. El caso `subscription` no se muestra como «0 €» a secas:
 * el coste marginal es cero pero la cuota consumida es desconocida, y
 * confundirlos sería exactamente lo que el producto promete no hacer.
 */
export function costLabel(basis: CostBasis, micros: number | null): string {
  switch (basis) {
    case "reported":
      return `${formatMicros(micros ?? 0)} (dato)`;
    case "estimated":
      return `≈ ${formatMicros(micros ?? 0)} (estimado)`;
    case "subscription":
      return "Cubierto por suscripción";
    case "unavailable":
      return "No disponible";
  }
}

export function costHint(basis: CostBasis): string {
  switch (basis) {
    case "reported":
      return "Cifra comunicada por el proveedor.";
    case "estimated":
      return "Calculada por Nexo a partir de precios públicos. No es un dato del proveedor.";
    case "subscription":
      return "Sin coste marginal: tu plan lo cubre. El proveedor no expone cuánta cuota has consumido.";
    case "unavailable":
      return "El proveedor no informa y Nexo no puede estimarlo con fiabilidad.";
  }
}

export function localCostHint(): string {
  return "Se ejecuta en tu equipo: coste cero y conocido, sin cuota de ningún proveedor.";
}

export function formatMicros(micros: number): string {
  return `${(micros / 1_000_000).toFixed(4)} $`;
}

export function formatTokens(n: number | null): string {
  if (n === null || n === undefined) return "—";
  if (n < 1000) return `${n}`;
  if (n < 1_000_000) return `${(n / 1000).toFixed(1)}k`;
  return `${(n / 1_000_000).toFixed(2)}M`;
}

export function formatMs(ms: number | null): string {
  if (ms === null || ms === undefined) return "—";
  return ms < 1000 ? `${ms} ms` : `${(ms / 1000).toFixed(2)} s`;
}

export function formatTime(ts: number | null): string {
  if (!ts) return "—";
  return new Date(ts).toLocaleString("es-ES", {
    day: "2-digit",
    month: "2-digit",
    hour: "2-digit",
    minute: "2-digit",
    second: "2-digit",
  });
}

/**
 * Traduce el motivo por el que una consulta de catálogo salió vacía. Es el
 * síntoma más difícil de diagnosticar del producto: el cliente solo dice «no se
 * encontraron modelos», sea por token, por permisos o por cuenta.
 */
export function catalogReason(errorKind: string | null): string | null {
  switch (errorKind) {
    case "no_grants":
      return "La aplicación no tiene ninguna vía concedida. Dale permisos en Aplicaciones.";
    case "no_account":
      return "No hay ninguna cuenta conectada. Conéctala en Proveedores.";
    case "empty_catalog":
      return "Tiene permisos, pero ninguna vía concedida coincide con una cuenta conectada.";
    default:
      return null;
  }
}

export function errorText(e: unknown): string {
  if (typeof e === "string") return e;
  if (e instanceof Error) return e.message;
  return String(e);
}
