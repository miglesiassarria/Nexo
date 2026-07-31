<script lang="ts">
  import { api, errorText, type GatewayStatus, type Settings } from "../api";

  let { status }: { status: GatewayStatus | null } = $props();

  let settings = $state<Settings | null>(null);
  let error = $state<string | null>(null);
  let info = $state<string | null>(null);

  async function load() {
    try {
      settings = await api.loadSettings();
    } catch (e) {
      error = errorText(e);
    }
  }

  async function save() {
    if (!settings) return;
    error = null;
    try {
      const result = await api.saveSettings(settings);
      info = result.restart_required
        ? "Guardado. El cambio de puerto se aplica al reiniciar Nexo."
        : "Guardado.";
    } catch (e) {
      error = errorText(e);
    }
  }

  async function retention() {
    error = null;
    try {
      const r = await api.applyRetention();
      info = `Retención aplicada: ${r.deleted_requests} eventos y ${r.deleted_content} registros de contenido eliminados. Los agregados horarios se conservan.`;
    } catch (e) {
      error = errorText(e);
    }
  }

  async function purge() {
    error = null;
    try {
      await api.purgeStats();
      info = "Todas las estadísticas locales han sido eliminadas.";
    } catch (e) {
      error = errorText(e);
    }
  }

  $effect(() => {
    load();
  });
</script>

<div class="stack">
  {#if error}<div class="notice err">{error}</div>{/if}
  {#if info}<div class="notice info">{info}</div>{/if}

  {#if settings}
    <section class="card stack">
      <h2>Gateway</h2>
      <div class="fields">
        <div>
          <label for="port">Puerto local</label>
          <input id="port" type="number" min="1024" max="65535" bind:value={settings.port} />
        </div>
        <div>
          <label for="log">Nivel de log</label>
          <select id="log" bind:value={settings.log_level}>
            <option value="error">error</option>
            <option value="warn">warn</option>
            <option value="info">info</option>
            <option value="debug">debug</option>
          </select>
        </div>
      </div>
      <div class="notice info">
        Nexo escucha solo en <code>127.0.0.1</code>. La exposición en red sigue
        desactivada: requiere autenticación, autorización y transporte seguro, y todavía
        no están implementados.
      </div>
      {#if status}
        <p class="muted small">
          URL base para tus aplicaciones: <code>{status.base_url}</code>
        </p>
      {/if}
    </section>

    <section class="card stack">
      <h2>Privacidad y retención</h2>
      <div class="fields">
        <div>
          <label for="ret">Retención de eventos (días)</label>
          <input id="ret" type="number" min="1" bind:value={settings.retention_days} />
        </div>
        <div>
          <label for="cret">Retención de contenido (días)</label>
          <input
            id="cret"
            type="number"
            min="0"
            bind:value={settings.content_retention_days}
          />
        </div>
      </div>
      <p class="muted small">
        El contenido de prompts y respuestas no se guarda por defecto. Borrar el detalle
        no elimina los agregados horarios, así que las tendencias largas sobreviven.
      </p>
      <div class="row">
        <button onclick={retention}>Aplicar retención ahora</button>
        <button class="danger" onclick={purge}>Borrar todas las estadísticas</button>
      </div>
    </section>

    <section class="card stack">
      <h2>Manifiesto de modelos</h2>
      <p class="muted">
        Versión <code>{settings.manifest_version}</code>. Las capacidades de los modelos no
        son consultables por API: vienen de este manifiesto versionado.
      </p>
    </section>

    <div class="row">
      <button class="primary" onclick={save}>Guardar configuración</button>
    </div>
  {/if}
</div>

<style>
  .fields {
    display: grid;
    grid-template-columns: repeat(auto-fit, minmax(200px, 1fr));
    gap: 0.7rem;
  }

  .small {
    font-size: 0.78rem;
  }
</style>
