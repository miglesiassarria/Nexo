<script lang="ts">
  import {
    api,
    errorText,
    kindLabel,
    type CatalogRow,
    type LocalModelDetail,
  } from "../api";

  let rows = $state<CatalogRow[]>([]);
  let localDetails = $state<Record<string, LocalModelDetail>>({});
  let error = $state<string | null>(null);
  let info = $state<string | null>(null);
  let refreshing = $state(false);

  async function load() {
    try {
      rows = await api.catalog();
      error = null;
      // Cuantización y estado de carga no caben en el contrato común de
      // proveedor: se piden aparte, solo para mostrarlos.
      const details = await api.lmstudioModels();
      localDetails = Object.fromEntries(details.map((d) => [d.api_id, d]));
    } catch (e) {
      error = errorText(e);
    }
  }

  async function refresh() {
    refreshing = true;
    error = null;
    info = null;
    try {
      const results = await api.refreshCatalog();
      if (results.length === 0) {
        info = "No hay ninguna cuenta conectada a la que preguntar.";
      } else {
        info = results
          .map((r) =>
            r.error
              ? `${r.provider_id} · ${kindLabel(r.credential_kind)}: ${r.error}`
              : `${r.provider_id} · ${kindLabel(r.credential_kind)}: ${r.discovered} modelo(s)`,
          )
          .join(" · ");
      }
      await load();
    } catch (e) {
      error = errorText(e);
    } finally {
      refreshing = false;
    }
  }

  const caps = [
    { key: "text", label: "Texto" },
    { key: "vision", label: "Visión" },
    { key: "tools", label: "Herramientas" },
    { key: "reasoning", label: "Razonamiento" },
    { key: "json_mode", label: "JSON" },
    { key: "streaming", label: "Streaming" },
  ] as const;

  $effect(() => {
    load();
  });
</script>

<div class="stack">
  {#if error}<div class="notice err">{error}</div>{/if}
  {#if info}<div class="notice info">{info}</div>{/if}

  <div class="row" style="justify-content: space-between">
    <h2>Catálogo</h2>
    <button class="primary" onclick={refresh} disabled={refreshing}>
      {refreshing ? "Preguntando al proveedor…" : "Actualizar desde el proveedor"}
    </button>
  </div>

  <div class="notice info">
    El mismo modelo aparece una vez por cada vía de acceso, porque no ofrece lo mismo
    por las dos: la vía de suscripción tiene un catálogo recortado y no informa de
    tokens ni de cuota. La compatibilidad de formato no es equivalencia de capacidades.
  </div>

  <section class="card">
    <div class="scroll-x">
      <table>
        <thead>
          <tr>
            <th>Nombre público</th>
            <th>Vía</th>
            <th>Contabilidad</th>
            <th>Contexto</th>
            <th>Salida máx.</th>
            <th>Capacidades</th>
            <th>Local</th>
            <th>Precio</th>
          </tr>
        </thead>
        <tbody>
          {#each rows as row (row.provider_id + row.credential_kind + row.api_id)}
            <tr>
              <td><code>{row.public_name}</code></td>
              <td>
                <span
                  class="badge"
                  class:sub={row.credential_kind === "subscription_oauth"}
                  class:key={row.credential_kind === "api_key"}
                >
                  {kindLabel(row.credential_kind)}
                </span>
              </td>
              <td>
                {#if row.accounting === "subscription"}
                  <span title="Sin coste marginal; cuota consumida desconocida">
                    Suscripción
                  </span>
                {:else if row.accounting === "local"}
                  Local
                {:else}
                  Por token
                {/if}
              </td>
              <td>{row.context_max?.toLocaleString("es-ES") ?? "—"}</td>
              <td>{row.output_max?.toLocaleString("es-ES") ?? "—"}</td>
              <td>
                <div class="caps">
                  {#each caps as c (c.key)}
                    {#if row.caps[c.key]}
                      <span class="badge">{c.label}</span>
                    {/if}
                  {/each}
                </div>
              </td>
              <td>
                {#if localDetails[row.api_id]}
                  {@const d = localDetails[row.api_id]}
                  <div class="row" style="gap: 0.25rem; flex-wrap: wrap">
                    {#if d.loaded}
                      <span class="badge ok" title="Listo para responder">cargado</span>
                    {:else}
                      <span class="badge" title="Se cargará en la primera petición">
                        sin cargar
                      </span>
                    {/if}
                    {#if d.quantization}<span class="badge">{d.quantization}</span>{/if}
                    {#if d.runtime}<span class="badge">{d.runtime}</span>{/if}
                  </div>
                {:else}
                  <span class="muted">—</span>
                {/if}
              </td>
              <td>
                {#if row.price_input === null}
                  <span class="muted">—</span>
                {:else}
                  <span class="muted" style="font-size: 0.78rem">
                    {(row.price_input / 1_000_000).toFixed(2)} / {(
                      (row.price_output ?? 0) / 1_000_000
                    ).toFixed(2)} por Mtok
                  </span>
                {/if}
              </td>
            </tr>
          {/each}
        </tbody>
      </table>
    </div>
  </section>
</div>

<style>
  .caps {
    display: flex;
    flex-wrap: wrap;
    gap: 0.25rem;
  }
</style>
