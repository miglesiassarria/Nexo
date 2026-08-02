<script lang="ts">
  import { api, errorText, type GatewayStatus, type RiskNotice, type Settings } from "../api";

  let { status }: { status: GatewayStatus | null } = $props();

  let settings = $state<Settings | null>(null);
  let lanNotice = $state<RiskNotice | null>(null);
  let lanRiskAccepted = $state(false);
  let error = $state<string | null>(null);
  let info = $state<string | null>(null);

  async function load() {
    try {
      const [loadedSettings, notice] = await Promise.all([
        api.loadSettings(),
        api.lanRiskNotice(),
      ]);
      settings = loadedSettings;
      lanNotice = notice;
    } catch (e) {
      error = errorText(e);
    }
  }

  async function save() {
    if (!settings) return;
    error = null;
    try {
      const result = await api.saveSettings(settings, lanRiskAccepted);
      info = result.restart_required
        ? "Guardado. Los cambios de puerto y de acceso en red se aplican al reiniciar Nexo."
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
      {#if !settings.allow_lan}
        <div class="notice info">
          Nexo escucha solo en <code>127.0.0.1</code>. La exposición en red sigue
          desactivada.
        </div>
      {/if}
      {#if status}
        <p class="muted small">
          URL base para tus aplicaciones: <code>{status.base_url}</code>
        </p>
      {/if}

      <label class="check">
        <input type="checkbox" bind:checked={settings.allow_lan} />
        <span>Permitir acceso desde mi red local</span>
      </label>

      {#if settings.allow_lan}
        {#if lanNotice}
          <div class="risk">
            <h3>{lanNotice.title}</h3>
            <ul>
              {#each lanNotice.points as point}
                <li>{point}</li>
              {/each}
            </ul>
            <label class="check">
              <input type="checkbox" bind:checked={lanRiskAccepted} />
              <span>{lanNotice.confirm_label}</span>
            </label>
          </div>
        {/if}

        {#if status?.lan}
          <div class="notice info">
            <p>
              Conecta otro equipo de tu red a
              <code
                >https://{status.lan.address ?? "(sin IP de red detectada)"}:{status.lan
                  .port}/v1</code
              >.
            </p>
            <p class="small">
              Certificado autofirmado: la primera vez, ese equipo mostrará un aviso de
              "no confiable" que hay que aceptar a mano. Para comprobar que es el
              correcto antes de aceptarlo, compara su huella SHA-256 —
              <code>{status.lan.cert_fingerprint_sha256}</code>
              — con la del fichero <code>{status.lan.cert_path}</code>.
            </p>
          </div>
        {:else}
          <p class="muted small">
            El modo red se aplica al reiniciar Nexo. Tras reiniciar, aquí aparecerá la
            dirección para conectar desde otro equipo.
          </p>
        {/if}
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
        Se aplica sola cada vez que Nexo arranca, con estos valores. El contenido de
        prompts y respuestas no se guarda por defecto. Borrar el detalle no elimina
        los agregados horarios, así que las tendencias largas sobreviven.
      </p>
      <div class="row">
        <button onclick={retention}>
          Aplicar ahora, sin esperar al próximo arranque
        </button>
        <button class="danger" onclick={purge}>Borrar todas las estadísticas</button>
      </div>
    </section>

    <section class="card stack">
      <h2>Catálogo de modelos</h2>
      <div>
        <label for="cver">Versión de cliente declarada a ChatGPT</label>
        <input id="cver" bind:value={settings.codex_client_version} />
      </div>
      <p class="muted small">
        El proveedor filtra los modelos que publica según la versión del cliente que
        los pide. Si sabes que hay una familia más nueva y no aparece, sube este
        número y pulsa <strong>Actualizar desde el proveedor</strong> en Modelos.
        No es la versión de Nexo.
      </p>
      <p class="muted small">
        Manifiesto local de respaldo: <code>{settings.manifest_version}</code>. Se usa
        solo cuando el proveedor no responde.
      </p>
    </section>

    <div class="row">
      <button
        class="primary"
        onclick={save}
        disabled={settings.allow_lan && !lanRiskAccepted}
      >
        Guardar configuración
      </button>
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

  .risk {
    border: 1px solid color-mix(in srgb, var(--warn) 40%, var(--border));
    background: color-mix(in srgb, var(--warn) 7%, transparent);
    border-radius: 10px;
    padding: 0.9rem;
    display: flex;
    flex-direction: column;
    gap: 0.7rem;
  }

  .risk ul {
    margin: 0;
    padding-left: 1.1rem;
    font-size: 0.85rem;
    display: flex;
    flex-direction: column;
    gap: 0.4rem;
  }

  .check {
    display: flex;
    align-items: flex-start;
    gap: 0.5rem;
    margin: 0;
    font-size: 0.875rem;
    color: var(--text);
    font-weight: 600;
  }

  .check input {
    width: auto;
    margin-top: 0.2rem;
  }
</style>
