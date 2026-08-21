<script lang="ts">
  import { api, errorText, type GatewayStatus, type RiskNotice, type Settings } from "../api";

  let { status }: { status: GatewayStatus | null } = $props();

  let settings = $state<Settings | null>(null);
  let lanNotice = $state<RiskNotice | null>(null);
  let lanRiskAccepted = $state(false);
  let error = $state<string | null>(null);
  let info = $state<string | null>(null);

  const DEFAULT_MAX_BYTES = 32 * 1024 * 1024;
  const MIB = 1024 * 1024;
  const GIB = 1024 * 1024 * 1024;

  let limitValue = $state(32);
  let limitUnit = $state<'MiB' | 'GiB'>('MiB');
  let isUnlimited = $state(false);

  function syncLimitFieldsFromSettings(s: Settings) {
    if (s.max_request_body_bytes === null || s.max_request_body_bytes === undefined) {
      isUnlimited = true;
      limitValue = 32;
      limitUnit = 'MiB';
    } else {
      isUnlimited = false;
      const bytes = s.max_request_body_bytes;
      if (bytes >= GIB && bytes % GIB === 0) {
        limitUnit = 'GiB';
        limitValue = Math.round(bytes / GIB);
      } else {
        limitUnit = 'MiB';
        limitValue = Math.round(bytes / MIB);
      }
    }
  }

  function onLimitInput() {
    if (!settings || isUnlimited) return;
    const factor = limitUnit === 'GiB' ? GIB : MIB;
    let clampedVal = Number(limitValue) || 1;
    if (limitUnit === 'GiB' && clampedVal > 5) clampedVal = 5;
    if (limitUnit === 'MiB' && clampedVal > 5120) clampedVal = 5120;
    if (clampedVal < 1) clampedVal = 1;
    settings.max_request_body_bytes = clampedVal * factor;
  }

  function onUnitChange() {
    if (!settings || isUnlimited) return;
    if (limitUnit === 'GiB' && limitValue > 5) {
      limitValue = 5;
    }
    const factor = limitUnit === 'GiB' ? GIB : MIB;
    settings.max_request_body_bytes = (Number(limitValue) || 1) * factor;
  }

  function onToggleUnlimited(checked: boolean) {
    if (!settings) return;
    if (checked) {
      const confirmed = window.confirm(
        'Vas a activar «Sin límite impuesto por Nexo».\n\nNexo no rechazará peticiones por tamaño, pero seguirán existiendo límites reales de RAM, espacio temporal en disco y límites del proveedor.\n\n¿Deseas continuar?'
      );
      if (!confirmed) {
        isUnlimited = false;
        return;
      }
      isUnlimited = true;
      settings.max_request_body_bytes = null;
    } else {
      isUnlimited = false;
      const factor = limitUnit === 'GiB' ? GIB : MIB;
      settings.max_request_body_bytes = (Number(limitValue) || 32) * factor;
    }
  }

  function resetDefaultLimit() {
    if (!settings) return;
    isUnlimited = false;
    limitValue = 32;
    limitUnit = 'MiB';
    settings.max_request_body_bytes = DEFAULT_MAX_BYTES;
  }

  async function load() {
    try {
      const [loadedSettings, notice] = await Promise.all([
        api.loadSettings(),
        api.lanRiskNotice(),
      ]);
      settings = loadedSettings;
      syncLimitFieldsFromSettings(loadedSettings);
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
            {#if status.lan.addresses.length > 0}
              <p>
                Conecta otro equipo de tu red a
                <code
                  >http://{status.lan.addresses[0].address}:{status.lan.port}/v1</code
                >. No hace falta aceptar ningún certificado.
              </p>
              {#if status.lan.addresses.length > 1}
                <p class="small">
                  Nexo escucha en todas las interfaces de este equipo, así que también
                  responde por estas direcciones:
                </p>
                <ul class="small">
                  {#each status.lan.addresses.slice(1) as a}
                    <li>
                      <code>http://{a.address}:{status.lan.port}/v1</code>
                      ({a.interface})
                    </li>
                  {/each}
                </ul>
              {/if}
            {:else}
              <p>
                No se ha detectado ninguna dirección de red en este equipo. El modo está
                activo, pero no hay por dónde conectar hasta que haya red.
              </p>
            {/if}
            <p class="small">
              <strong>El tráfico no va cifrado.</strong> La clave de aplicación viaja en
              claro en cada petición, y con ella el contenido de las conversaciones.
              Desactiva esta opción antes de conectarte a una red que no controles.
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
      <h2>Peticiones y archivos</h2>
      <p class="muted small">
        Controla el tamaño máximo total que Nexo aceptará en una petición HTTP de chat (incluyendo imágenes, prompts y herramientas).
      </p>

      <div class="fields">
        <div>
          <label for="req-limit-val">Tamaño máximo de petición</label>
          <div class="input-with-unit">
            <input
              id="req-limit-val"
              type="number"
              min={limitUnit === 'GiB' ? 1 : 1}
              max={limitUnit === 'GiB' ? 5 : 5120}
              disabled={isUnlimited}
              bind:value={limitValue}
              oninput={onLimitInput}
            />
            <select
              aria-label="Unidad de tamaño de petición"
              disabled={isUnlimited}
              bind:value={limitUnit}
              onchange={onUnitChange}
            >
              <option value="MiB">MiB</option>
              <option value="GiB">GiB</option>
            </select>
          </div>
        </div>
      </div>

      <div class="row items-center">
        <label class="check">
          <input
            type="checkbox"
            checked={isUnlimited}
            onchange={(e) => onToggleUnlimited(e.currentTarget.checked)}
          />
          <span>Sin límite impuesto por Nexo</span>
        </label>
        <button type="button" class="small-btn" onclick={resetDefaultLimit}>
          Restaurar predeterminado (32 MiB)
        </button>
      </div>

      {#if !isUnlimited && settings.max_request_body_bytes}
        <p class="muted small">
          Equivalente exacto: <strong>{settings.max_request_body_bytes.toLocaleString()} bytes</strong>.
        </p>
      {/if}

      <div class="notice info">
        <strong>Codificación base64:</strong> las imágenes y documentos insertados directamente en las peticiones (como hace Msty Go) se codifican en base64, lo que aumenta su tamaño aproximadamente un <strong>33%</strong> respecto al archivo original en disco.
      </div>

      {#if isUnlimited}
        <div class="risk">
          <h3>Aviso sobre la opción «Sin límite impuesto por Nexo»</h3>
          <p class="small">
            Esta opción elimina el límite configurado en Nexo, pero <strong>no hace que las peticiones sean infinitas</strong>. Seguirán existiendo límites físicos (memoria RAM disponible, espacio libre en disco para archivos temporales) y las restricciones máximas de tamaño que imponga el proveedor externo de cada modelo.
          </p>
        </div>
      {:else if (settings.max_request_body_bytes ?? 0) > 512 * 1024 * 1024}
        <div class="risk">
          <h3>Límite elevado (&gt; 512 MiB)</h3>
          <p class="small">
            Has configurado un límite superior a 512 MiB. Las peticiones grandes se derivan automáticamente a disco para proteger la memoria, pero múltiples peticiones simultáneas requerirán suficiente espacio temporal disponible.
            {#if settings.allow_lan}
              <br /><strong>Atención:</strong> con el acceso desde red local activo, cualquier cliente de tu red podrá enviar peticiones de este tamaño.
            {/if}
          </p>
        </div>
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

  .input-with-unit {
    display: flex;
    gap: 0.4rem;
    align-items: center;
  }

  .input-with-unit input {
    flex: 1;
  }

  .input-with-unit select {
    width: 5.5rem;
  }

  .small-btn {
    font-size: 0.8rem;
    padding: 0.35rem 0.65rem;
    background: color-mix(in srgb, var(--surface) 80%, var(--border));
    border: 1px solid var(--border);
    border-radius: 6px;
    color: var(--text);
    cursor: pointer;
  }

  .small-btn:hover {
    background: var(--border);
  }

  .items-center {
    align-items: center;
    gap: 1rem;
  }
</style>
