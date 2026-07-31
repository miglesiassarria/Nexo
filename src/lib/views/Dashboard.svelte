<script lang="ts">
  import {
    api,
    catalogReason,
    costHint,
    costLabel,
    errorText,
    formatMicros,
    formatMs,
    formatTime,
    formatTokens,
    kindLabel,
    type GatewayStatus,
    type RequestRow,
    type UsageBucket,
  } from "../api";

  let { status }: { status: GatewayStatus | null } = $props();

  let days = $state(7);
  let group = $state("credential");
  let buckets = $state<UsageBucket[]>([]);
  let recent = $state<RequestRow[]>([]);
  let error = $state<string | null>(null);

  const groups = [
    { id: "credential", label: "Vía de acceso" },
    { id: "app", label: "Aplicación" },
    { id: "provider", label: "Proveedor" },
    { id: "model", label: "Modelo" },
  ];

  async function load() {
    try {
      // Solo inferencia: una consulta de catálogo no consume nada.
      buckets = await api.usageSummary(days, group, "chat");
      recent = await api.recentRequests(40);
      error = null;
    } catch (e) {
      error = errorText(e);
    }
  }

  const totals = $derived({
    requests: buckets.reduce((a, b) => a + b.requests, 0),
    errors: buckets.reduce((a, b) => a + b.errors, 0),
    tokens: buckets.reduce((a, b) => a + b.total_tokens, 0),
    subscription: buckets.reduce((a, b) => a + b.subscription_requests, 0),
    estimated: buckets.reduce((a, b) => a + b.cost_estimated_micros, 0),
    reported: buckets.reduce((a, b) => a + b.cost_reported_micros, 0),
    localLimited: buckets.reduce((a, b) => a + b.local_limited, 0),
    rateLimited: buckets.reduce((a, b) => a + b.rate_limited, 0),
  });

  const maxRequests = $derived(Math.max(1, ...buckets.map((b) => b.requests)));

  function bucketLabel(name: string): string {
    return group === "credential" ? kindLabel(name) : name;
  }

  $effect(() => {
    void days;
    void group;
    load();
  });
</script>

<div class="stack">
  {#if error}<div class="notice err">{error}</div>{/if}

  {#if status && status.accounts === 0}
    <div class="notice info">
      Todavía no hay ninguna cuenta conectada. Empieza en <strong>Proveedores</strong>
      conectando tu suscripción de ChatGPT, y crea después un token en
      <strong>Aplicaciones</strong> para tus herramientas.
    </div>
  {/if}

  <div class="tiles">
    {@render tile("Peticiones", `${totals.requests}`, `últimos ${days} días`)}
    {@render tile(
      "Cubiertas por suscripción",
      `${totals.subscription}`,
      "sin coste marginal",
    )}
    {@render tile("Tokens", formatTokens(totals.tokens), "entrada + salida")}
    {@render tile(
      "Coste estimado",
      formatMicros(totals.estimated),
      "calculado por Nexo, no es un dato",
    )}
    {@render tile("Errores", `${totals.errors}`, `${totals.localLimited} por límite de Nexo`)}
  </div>

  {#if totals.reported > 0}
    <div class="notice info">
      Coste reportado por el proveedor: <strong>{formatMicros(totals.reported)}</strong>.
      Se muestra aparte del estimado a propósito: nunca se suman.
    </div>
  {/if}

  <section class="card stack">
    <div class="row" style="justify-content: space-between">
      <h2>Uso agregado</h2>
      <div class="row">
        <select bind:value={group} aria-label="Agrupar por">
          {#each groups as g (g.id)}
            <option value={g.id}>{g.label}</option>
          {/each}
        </select>
        <select bind:value={days} aria-label="Periodo">
          <option value={1}>24 horas</option>
          <option value={7}>7 días</option>
          <option value={30}>30 días</option>
          <option value={90}>90 días</option>
        </select>
      </div>
    </div>

    {#if buckets.length === 0}
      <p class="muted">Sin datos en el periodo seleccionado.</p>
    {:else}
      <div class="scroll-x">
        <table>
          <thead>
            <tr>
              <th>{groups.find((g) => g.id === group)?.label}</th>
              <th style="width: 22%">Peticiones</th>
              <th>Tokens</th>
              <th>Latencia media</th>
              <th>Primer token</th>
              <th>Errores</th>
              <th>Coste</th>
            </tr>
          </thead>
          <tbody>
            {#each buckets as b (b.bucket)}
              <tr>
                <td>{bucketLabel(b.bucket)}</td>
                <td>
                  <div class="bar-cell">
                    <div class="bar" style="width: {(b.requests / maxRequests) * 100}%"></div>
                    <span>{b.requests}</span>
                  </div>
                </td>
                <td>{formatTokens(b.total_tokens)}</td>
                <td>{formatMs(b.avg_latency_ms)}</td>
                <td>{formatMs(b.avg_ttft_ms)}</td>
                <td>
                  {b.errors}
                  {#if b.local_limited > 0}
                    <span class="badge warn" title="Rechazadas por el límite de Nexo">
                      {b.local_limited} límite
                    </span>
                  {/if}
                  {#if b.rate_limited > 0}
                    <span class="badge err" title="Rechazadas por el proveedor">
                      {b.rate_limited} 429
                    </span>
                  {/if}
                </td>
                <td>
                  {#if b.subscription_requests === b.requests}
                    <span title={costHint("subscription")}>Suscripción</span>
                  {:else if b.cost_estimated_micros > 0}
                    <span title={costHint("estimated")}>
                      ≈ {formatMicros(b.cost_estimated_micros)}
                    </span>
                  {:else}
                    <span class="muted" title={costHint("unavailable")}>—</span>
                  {/if}
                </td>
              </tr>
            {/each}
          </tbody>
        </table>
      </div>
    {/if}
  </section>

  <section class="card stack">
    <h2>Últimas peticiones</h2>
    {#if recent.length === 0}
      <p class="muted">Todavía no ha pasado ninguna petición por el gateway.</p>
    {:else}
      <div class="scroll-x">
        <table>
          <thead>
            <tr>
              <th>Hora</th>
              <th>Aplicación</th>
              <th>Modelo</th>
              <th>Vía</th>
              <th>Tipo</th>
              <th>Estado</th>
              <th>Latencia</th>
              <th>Tokens</th>
              <th>Coste</th>
            </tr>
          </thead>
          <tbody>
            {#each recent as r (r.id)}
              <tr>
                <td class="muted">{formatTime(r.ts)}</td>
                <td>{r.app}</td>
                <td>
                  {r.public_model}
                  {#if r.operation === "models" && catalogReason(r.error_kind)}
                    <div class="muted reason">{catalogReason(r.error_kind)}</div>
                  {/if}
                </td>
                <td>
                  <span
                    class="badge"
                    class:sub={r.credential_kind === "subscription_oauth"}
                    class:key={r.credential_kind === "api_key"}
                  >
                    {kindLabel(r.credential_kind)}
                  </span>
                  {#if r.fallback_from}
                    <span class="badge warn" title="Se cayó al respaldo">respaldo</span>
                  {/if}
                </td>
                <td>
                  {#if r.operation === "models"}
                    <span class="badge">catálogo</span>
                  {:else}
                    <span class="badge">chat</span>
                  {/if}
                </td>
                <td>
                  {#if r.status === "ok"}
                    <span class="badge ok">ok</span>
                  {:else if r.operation === "models"}
                    <span class="badge warn" title={catalogReason(r.error_kind) ?? ""}>
                      catálogo vacío
                    </span>
                  {:else}
                    <span class="badge err" title={r.error_kind ?? ""}>
                      {r.error_kind ?? r.status}
                    </span>
                  {/if}
                </td>
                <td>{formatMs(r.latency_ms)}</td>
                <td>
                  {formatTokens(r.total_tokens)}
                  {#if r.usage_source === "estimated"}
                    <span class="muted" title="Estimado por Nexo">≈</span>
                  {:else if r.usage_source === "unavailable"}
                    <span class="muted" title="El proveedor no informa">?</span>
                  {/if}
                </td>
                <td title={costHint(r.cost_basis)}>
                  {costLabel(r.cost_basis, r.cost_micros)}
                </td>
              </tr>
            {/each}
          </tbody>
        </table>
      </div>
    {/if}
  </section>
</div>

{#snippet tile(label: string, value: string, hint: string)}
  <div class="card tile">
    <span class="tile-label">{label}</span>
    <strong class="tile-value">{value}</strong>
    <span class="muted tile-hint">{hint}</span>
  </div>
{/snippet}

<style>
  .tiles {
    display: grid;
    grid-template-columns: repeat(auto-fit, minmax(160px, 1fr));
    gap: 0.85rem;
  }

  .tile {
    display: flex;
    flex-direction: column;
    gap: 0.15rem;
    padding: 0.8rem 0.9rem;
  }

  .tile-label {
    font-size: 0.74rem;
    text-transform: uppercase;
    letter-spacing: 0.04em;
    color: var(--text-muted);
    font-weight: 600;
  }

  .tile-value {
    font-size: 1.5rem;
    font-variant-numeric: tabular-nums;
    letter-spacing: -0.02em;
  }

  .tile-hint {
    font-size: 0.75rem;
  }

  .bar-cell {
    display: flex;
    align-items: center;
    gap: 0.5rem;
    position: relative;
  }

  .bar {
    height: 6px;
    min-width: 2px;
    border-radius: 999px;
    background: linear-gradient(90deg, var(--cobalt), var(--cyan));
    flex-shrink: 1;
  }

  .bar-cell span {
    font-variant-numeric: tabular-nums;
  }

  .reason {
    font-size: 0.72rem;
    max-width: 22rem;
    line-height: 1.35;
  }
</style>
