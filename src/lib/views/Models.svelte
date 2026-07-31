<script lang="ts">
  import { api, errorText, kindLabel, type CatalogRow } from "../api";

  let rows = $state<CatalogRow[]>([]);
  let error = $state<string | null>(null);

  const caps = [
    { key: "text", label: "Texto" },
    { key: "vision", label: "Visión" },
    { key: "tools", label: "Herramientas" },
    { key: "reasoning", label: "Razonamiento" },
    { key: "json_mode", label: "JSON" },
    { key: "streaming", label: "Streaming" },
  ] as const;

  $effect(() => {
    api
      .catalog()
      .then((r) => (rows = r))
      .catch((e) => (error = errorText(e)));
  });
</script>

<div class="stack">
  {#if error}<div class="notice err">{error}</div>{/if}

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
