<script lang="ts">
  import {
    api,
    errorText,
    formatTime,
    kindLabel,
    type ConnectOption,
    type LmStudioStatus,
    type ProviderRow,
    type RiskNotice,
  } from "../api";

  let { onchange }: { onchange: () => void } = $props();

  // La lista y el orden los decide el núcleo: agrupar por eje de credencial es
  // dominio, y cuando lo hacía esta vista se equivocaba (spec 0003).
  let rows = $state<ProviderRow[]>([]);
  let options = $state<ConnectOption[]>([]);
  let notice = $state<RiskNotice | null>(null);
  let lmstudio = $state<LmStudioStatus | null>(null);

  let error = $state<string | null>(null);
  let info = $state<string | null>(null);
  let busy = $state(false);

  /** Fila desplegada, por `account_id`. Solo una, como los permisos en Aplicaciones. */
  let expanded = $state<string | null>(null);
  /** Opción de alta elegida, por `id`. `null` mientras el panel está cerrado. */
  let adding = $state<string | null>(null);

  let addressDraft = $state("");
  let riskAccepted = $state(false);
  let formName = $state("");
  let formUrl = $state("");
  let formKey = $state("");
  let formLabel = $state("");

  async function load() {
    try {
      [rows, options, notice] = await Promise.all([
        api.providerRows(),
        api.connectOptions(),
        api.riskNotice(),
      ]);
      error = null;
    } catch (e) {
      error = errorText(e);
    }
  }

  /** Estado de LM Studio: solo para el detalle de su fila y su formulario. */
  async function loadLmstudio() {
    try {
      lmstudio = await api.lmstudioStatus();
    } catch (e) {
      error = errorText(e);
    }
  }

  async function refresh() {
    await load();
    await loadLmstudio();
    onchange();
  }

  /**
   * Envuelve una acción: un solo sitio donde se limpian avisos y se recarga.
   * `pending` se muestra mientras dura, para lo que tarda (el login de
   * suscripción abre el navegador y espera el callback).
   */
  async function run(
    action: () => Promise<void>,
    { done, pending }: { done?: string; pending?: string } = {},
  ) {
    busy = true;
    error = null;
    info = pending ?? null;
    try {
      await action();
      if (done) info = done;
      await refresh();
    } catch (e) {
      error = errorText(e);
      info = null;
    } finally {
      busy = false;
    }
  }

  function toggleRow(row: ProviderRow) {
    if (expanded === row.account_id) {
      expanded = null;
      return;
    }
    expanded = row.account_id;
    addressDraft = row.address ?? "";
  }

  function openAdd(option: ConnectOption) {
    adding = adding === option.id ? null : option.id;
    riskAccepted = false;
    formKey = "";
    formLabel = "";
    if (option.form.kind === "compat_endpoint") {
      formName = option.form.suggested_name;
      formUrl = option.form.base_url;
    } else if (option.form.kind === "local_server") {
      formUrl = option.form.default_url;
    }
  }

  function closeAdd() {
    adding = null;
    formName = "";
    formUrl = "";
    formKey = "";
    formLabel = "";
    riskAccepted = false;
  }

  // -- Acciones sobre una fila ya conectada ---------------------------------

  function disconnect(row: ProviderRow) {
    const quitar =
      row.manage.kind === "custom_provider"
        ? () => api.removeCustomProvider(row.provider_id)
        : () => api.disconnectAccount(row.account_id);
    run(
      async () => {
        await quitar();
        expanded = null;
      },
      { done: `«${row.name}» desconectado y sus secretos eliminados del equipo.` },
    );
  }

  function saveAddress(row: ProviderRow) {
    run(
      async () => {
        if (row.manage.kind === "custom_provider") {
          await api.updateCustomProviderUrl(row.provider_id, addressDraft);
        } else {
          await api.setLmstudioUrl(addressDraft);
        }
      },
      { done: "Dirección guardada." },
    );
  }

  function checkLmstudio() {
    run(async () => {
      const status = await api.detectLmstudio();
      info = status.reachable
        ? `LM Studio responde: ${status.models} modelo(s), ${status.loaded} cargado(s).`
        : (status.detail ?? "LM Studio no responde en esa dirección.");
    });
  }

  // -- Alta -----------------------------------------------------------------

  function connectSubscription(option: ConnectOption) {
    // `connectChatgpt` es específico de ChatGPT, la única vía de suscripción que
    // existe hoy. Cuando haya otra (Gemini por OAuth está en el ROADMAP) hará falta
    // un comando por proveedor: la forma del formulario se comparte, el flujo no.
    run(
      async () => {
        await api.connectChatgpt(true);
        closeAdd();
      },
      {
        pending:
          "Completa la autorización en el navegador. Esta ventana espera el callback.",
        done: `«${option.name}» conectado.`,
      },
    );
  }

  function connectLocalServer() {
    run(
      async () => {
        const status = await api.setLmstudioUrl(formUrl);
        if (!status.reachable) {
          throw new Error(
            status.detail ??
              "No responde en esa dirección. Abre LM Studio y activa su servidor local.",
          );
        }
        closeAdd();
      },
      { done: "LM Studio conectado." },
    );
  }

  function connectApiKey() {
    run(
      async () => {
        await api.connectApiKey(formKey, formLabel || undefined);
        closeAdd();
      },
      { done: "API key guardada en el Keychain del sistema." },
    );
  }

  function connectCompat() {
    const nombre = formName;
    run(
      async () => {
        await api.addCustomProvider(formName, formUrl, formKey);
        closeAdd();
      },
      { done: `Proveedor «${nombre}» conectado.` },
    );
  }

  const chosen = $derived(options.find((o) => o.id === adding) ?? null);

  $effect(() => {
    load();
    loadLmstudio();
  });
</script>

<div class="stack">
  {#if error}<div class="notice err">{error}</div>{/if}
  {#if info}<div class="notice info">{info}</div>{/if}

  <section class="card stack">
    <div class="row" style="justify-content: space-between">
      <div>
        <h2>Conectados</h2>
        <p class="muted">
          Una línea por proveedor y vía de acceso. Pulsa una para ver su detalle.
        </p>
      </div>
      <!-- `""` abre el panel sin ninguna opción elegida; `null` lo cierra. Comparar
           con `null` explícitamente, porque `""` es falsy y `adding ? …` no cerraba. -->
      <button class="primary" onclick={() => (adding = adding === null ? "" : null)}>
        {adding === null ? "Añadir proveedor" : "Cerrar"}
      </button>
    </div>

    {#if rows.length === 0}
      <p class="muted">
        Todavía no hay ningún proveedor conectado, así que tus aplicaciones no verán
        modelos. Empieza con <strong>Añadir proveedor</strong>.
      </p>
    {/if}

    {#each rows as row (row.account_id)}
      <div class="item" class:attention={row.needs_attention}>
        <button class="line" onclick={() => toggleRow(row)}>
          <span class="caret">{expanded === row.account_id ? "▾" : "▸"}</span>
          <strong class="name">{row.name}</strong>
          <span
            class="badge"
            class:sub={row.credential_kind === "subscription_oauth"}
            class:key={row.credential_kind === "api_key"}
          >
            {kindLabel(row.credential_kind)}
          </span>
          {#if row.status === "active"}
            <span class="badge ok">Activa</span>
          {:else if row.status === "broken"}
            <span class="badge err">Vía rota</span>
          {:else if row.status === "expired"}
            <span class="badge warn">
              {row.credential_kind === "local" ? "Servidor apagado" : "Caducada"}
            </span>
          {:else}
            <span class="badge warn">{row.status}</span>
          {/if}
          <span class="models muted">{row.models} modelo(s)</span>
        </button>

        {#if expanded === row.account_id}
          <div class="detail stack">
            <span class="muted small">
              Conectada el {formatTime(row.created_at)}
              {#if row.expires_at}
                · token válido hasta {formatTime(row.expires_at)}
              {/if}
            </span>

            {#if row.manage.kind === "account"}
              {#if row.address}
                <span class="muted small"><code>{row.address}</code></span>
              {/if}
            {:else}
              <div class="key-form">
                <div>
                  <label for="addr-{row.account_id}">Dirección del servidor</label>
                  <input id="addr-{row.account_id}" bind:value={addressDraft} />
                </div>
                <div></div>
                <button
                  onclick={() => saveAddress(row)}
                  disabled={busy || !addressDraft.trim() || addressDraft === row.address}
                >
                  Guardar
                </button>
              </div>
            {/if}

            {#if row.manage.kind === "local_server"}
              <div class="row">
                <button onclick={checkLmstudio} disabled={busy}>
                  {busy ? "Comprobando…" : "Comprobar ahora"}
                </button>
                {#if lmstudio}
                  <span class="muted small">
                    {lmstudio.reachable
                      ? `${lmstudio.models} modelo(s), ${lmstudio.loaded} cargado(s)`
                      : (lmstudio.detail ?? "No responde")}
                  </span>
                {/if}
              </div>
            {/if}

            {#if row.note}
              <p class="muted small">{row.note}</p>
            {/if}

            <div class="row">
              <button class="danger" onclick={() => disconnect(row)} disabled={busy}>
                Desconectar
              </button>
            </div>
          </div>
        {/if}
      </div>
    {/each}
  </section>

  {#if adding !== null}
    <section class="card stack">
      <div>
        <h2>Añadir proveedor</h2>
        <p class="muted">Elige una vía. Solo se te pedirá lo que esa vía necesita.</p>
      </div>

      <div class="options">
        {#each options as option (option.id)}
          <button
            class="option"
            class:chosen={adding === option.id}
            onclick={() => openAdd(option)}
          >
            <span class="row">
              <strong>{option.name}</strong>
              {#if option.already_connected}
                <span class="badge ok">Ya conectado</span>
              {/if}
            </span>
            <span class="muted small">{option.summary}</span>
          </button>
        {/each}
      </div>

      {#if chosen}
        <div class="form stack">
          {#if chosen.note}
            <p class="muted small">{chosen.note}</p>
          {/if}

          {#if chosen.form.kind === "subscription_oauth"}
            {#if notice}
              <div class="risk">
                <h3>{notice.title}</h3>
                <ul>
                  {#each notice.points as point}
                    <li>{point}</li>
                  {/each}
                </ul>
                <label class="check">
                  <input type="checkbox" bind:checked={riskAccepted} />
                  <span>{notice.confirm_label}</span>
                </label>
              </div>
            {/if}
            <div class="row">
              <button
                class="primary"
                disabled={!riskAccepted || busy}
                onclick={() => connectSubscription(chosen)}
              >
                {busy ? "Esperando autorización…" : "Iniciar sesión en el navegador"}
              </button>
              <button class="ghost" onclick={closeAdd} disabled={busy}>Cancelar</button>
            </div>
          {:else if chosen.form.kind === "local_server"}
            <div class="key-form">
              <div>
                <label for="new-local-url">Dirección del servidor</label>
                <input id="new-local-url" bind:value={formUrl} />
              </div>
              <div></div>
              <button
                class="primary"
                onclick={connectLocalServer}
                disabled={busy || !formUrl.trim()}
              >
                {busy ? "Comprobando…" : "Conectar"}
              </button>
            </div>
          {:else if chosen.form.kind === "api_key"}
            <div class="key-form">
              <div>
                <label for="new-key">API key</label>
                <input id="new-key" type="password" bind:value={formKey} placeholder="sk-…" />
              </div>
              <div>
                <label for="new-key-label">Etiqueta (opcional)</label>
                <input id="new-key-label" bind:value={formLabel} placeholder="OpenAI personal" />
              </div>
              <button class="primary" onclick={connectApiKey} disabled={busy || !formKey.trim()}>
                Guardar
              </button>
            </div>
          {:else}
            <div class="key-form three">
              <div>
                <label for="new-name">Nombre</label>
                <input id="new-name" bind:value={formName} placeholder="Mi proveedor" />
              </div>
              <div>
                <label for="new-url">URL base</label>
                <input id="new-url" bind:value={formUrl} placeholder="https://…/v1" />
              </div>
              <div>
                <label for="new-compat-key">API key</label>
                <input
                  id="new-compat-key"
                  type="password"
                  bind:value={formKey}
                  placeholder="sk-…"
                />
              </div>
              <button
                class="primary"
                onclick={connectCompat}
                disabled={busy || !formName.trim() || !formUrl.trim() || !formKey.trim()}
              >
                {busy ? "Conectando…" : "Añadir"}
              </button>
            </div>
          {/if}

          {#if chosen.docs_url}
            <span class="muted small">Documentación: <code>{chosen.docs_url}</code></span>
          {/if}
        </div>
      {/if}
    </section>
  {/if}
</div>

<style>
  .item {
    border: 1px solid var(--border);
    border-radius: 10px;
    background: var(--surface-2);
    overflow: hidden;
  }

  /* Lo que exige actuar se distingue sin desplegar. */
  .item.attention {
    border-color: color-mix(in srgb, var(--warn) 45%, var(--border));
  }

  .line {
    display: flex;
    align-items: center;
    gap: 0.55rem;
    width: 100%;
    padding: 0.6rem 0.8rem;
    background: none;
    border: 0;
    text-align: left;
    cursor: pointer;
  }

  .caret {
    color: var(--muted);
    font-size: 0.75rem;
    width: 0.8rem;
    flex: none;
  }

  .name {
    /* Una dirección larga no debe empujar el estado fuera de la línea. */
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    max-width: 22rem;
  }

  .models {
    margin-left: auto;
    font-size: 0.8rem;
    flex: none;
  }

  .detail {
    padding: 0 0.8rem 0.8rem;
    border-top: 1px solid var(--border);
    padding-top: 0.7rem;
  }

  .options {
    display: grid;
    grid-template-columns: repeat(auto-fit, minmax(15rem, 1fr));
    gap: 0.6rem;
  }

  .option {
    display: flex;
    flex-direction: column;
    align-items: flex-start;
    gap: 0.25rem;
    padding: 0.7rem 0.8rem;
    text-align: left;
    border: 1px solid var(--border);
    border-radius: 10px;
    background: var(--surface-2);
    cursor: pointer;
  }

  .option.chosen {
    border-color: var(--accent);
  }

  .form {
    border-top: 1px solid var(--border);
    padding-top: 0.9rem;
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

  .key-form {
    display: grid;
    grid-template-columns: 2fr 1fr auto;
    gap: 0.6rem;
    align-items: end;
  }

  .key-form.three {
    grid-template-columns: 1.2fr 1.6fr 1.2fr auto;
  }

  .small {
    font-size: 0.78rem;
  }
</style>
