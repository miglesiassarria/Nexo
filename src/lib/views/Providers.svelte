<script lang="ts">
  import {
    api,
    errorText,
    formatTime,
    kindLabel,
    type Account,
    type LmStudioStatus,
    type RiskNotice,
  } from "../api";

  let { onchange }: { onchange: () => void } = $props();

  let accounts = $state<Account[]>([]);
  let notice = $state<RiskNotice | null>(null);
  let error = $state<string | null>(null);
  let info = $state<string | null>(null);

  let showRisk = $state(false);
  let riskAccepted = $state(false);
  let connecting = $state(false);

  let apiKey = $state("");
  let apiKeyLabel = $state("");

  let lmstudio = $state<LmStudioStatus | null>(null);
  let lmstudioUrl = $state("");
  let checkingLmstudio = $state(false);

  async function loadLmstudio() {
    try {
      lmstudio = await api.lmstudioStatus();
      if (!lmstudioUrl) lmstudioUrl = lmstudio.base_url;
    } catch (e) {
      error = errorText(e);
    }
  }

  async function detectLmstudio() {
    checkingLmstudio = true;
    error = null;
    info = null;
    try {
      lmstudio = await api.detectLmstudio();
      info = lmstudio.reachable
        ? `LM Studio conectado: ${lmstudio.models} modelo(s), ${lmstudio.loaded} cargado(s).`
        : null;
      await load();
      onchange();
    } catch (e) {
      error = errorText(e);
    } finally {
      checkingLmstudio = false;
    }
  }

  async function saveLmstudioUrl() {
    checkingLmstudio = true;
    error = null;
    try {
      lmstudio = await api.setLmstudioUrl(lmstudioUrl);
      lmstudioUrl = lmstudio.base_url;
      await load();
      onchange();
    } catch (e) {
      error = errorText(e);
    } finally {
      checkingLmstudio = false;
    }
  }

  async function load() {
    try {
      accounts = await api.listAccounts();
      notice = await api.riskNotice();
      error = null;
    } catch (e) {
      error = errorText(e);
    }
  }

  async function connectSubscription() {
    connecting = true;
    error = null;
    info = "Completa la autorización en el navegador. Esta ventana esperará el callback.";
    try {
      await api.connectChatgpt(true);
      info = "Cuenta de ChatGPT conectada.";
      showRisk = false;
      riskAccepted = false;
      await load();
      onchange();
    } catch (e) {
      error = errorText(e);
      info = null;
    } finally {
      connecting = false;
    }
  }

  async function saveApiKey() {
    error = null;
    try {
      await api.connectApiKey(apiKey, apiKeyLabel || undefined);
      apiKey = "";
      apiKeyLabel = "";
      info = "API key guardada en el almacén seguro del sistema.";
      await load();
      onchange();
    } catch (e) {
      error = errorText(e);
    }
  }

  async function disconnect(account: Account) {
    error = null;
    try {
      await api.disconnectAccount(account.id);
      info = `Cuenta desconectada y secretos eliminados del equipo.`;
      await load();
      onchange();
    } catch (e) {
      error = errorText(e);
    }
  }

  const subscription = $derived(
    accounts.filter((a) => a.credential_kind === "subscription_oauth"),
  );
  const keys = $derived(accounts.filter((a) => a.credential_kind === "api_key"));

  const localAccounts = $derived(
    accounts.filter((a) => a.credential_kind === "local"),
  );

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
        <h2>ChatGPT por suscripción</h2>
        <p class="muted">
          Usa el plan que ya pagas, sin API key y sin coste por token.
        </p>
      </div>
      {#if subscription.length === 0}
        <button class="primary" onclick={() => (showRisk = true)} disabled={connecting}>
          Conectar ChatGPT
        </button>
      {/if}
    </div>

    {#if showRisk && notice}
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
        <div class="row">
          <button
            class="primary"
            disabled={!riskAccepted || connecting}
            onclick={connectSubscription}
          >
            {connecting ? "Esperando autorización…" : "Iniciar sesión en el navegador"}
          </button>
          <button
            class="ghost"
            disabled={connecting}
            onclick={() => {
              showRisk = false;
              riskAccepted = false;
            }}
          >
            Cancelar
          </button>
        </div>
      </div>
    {/if}

    {#each subscription as account (account.id)}
      {@render accountRow(account)}
    {/each}
  </section>

  <section class="card stack">
    <div class="row" style="justify-content: space-between">
      <div>
        <h2>LM Studio</h2>
        <p class="muted">
          Modelos que corren en tu equipo. Nada sale de la máquina y no hay coste
          por token.
        </p>
      </div>
      <div class="row">
        {#if lmstudio?.reachable}
          <span class="badge ok">
            {lmstudio.models} modelo(s) · {lmstudio.loaded} cargado(s)
          </span>
        {:else}
          <span class="badge warn">No detectado</span>
        {/if}
        <button onclick={detectLmstudio} disabled={checkingLmstudio}>
          {checkingLmstudio ? "Comprobando…" : "Comprobar ahora"}
        </button>
      </div>
    </div>

    {#if lmstudio && !lmstudio.reachable}
      <div class="notice warn">
        {lmstudio.detail ?? "No responde."} Abre LM Studio y activa su servidor
        local; Nexo lo detecta al arrancar y cuando pulses «Comprobar ahora».
      </div>
    {/if}

    {#each localAccounts as account (account.id)}
      {@render accountRow(account)}
    {/each}

    <div class="key-form">
      <div>
        <label for="lmsurl">Dirección del servidor</label>
        <input id="lmsurl" bind:value={lmstudioUrl} placeholder="http://127.0.0.1:1234" />
      </div>
      <div></div>
      <button onclick={saveLmstudioUrl} disabled={checkingLmstudio || !lmstudioUrl.trim()}>
        Guardar
      </button>
    </div>
    <p class="muted small">
      La primera petición a un modelo que no esté cargado puede tardar bastante
      —unos 14 segundos en las pruebas con un modelo de 12B— porque LM Studio lo
      carga en ese momento. No es un cuelgue.
    </p>
  </section>

  <section class="card stack">
    <div>
      <h2>OpenAI por API key</h2>
      <p class="muted">
        Vía estable y documentada. Se factura por token y sirve de respaldo si la
        suscripción deja de funcionar.
      </p>
    </div>

    {#each keys as account (account.id)}
      {@render accountRow(account)}
    {/each}

    <div class="key-form">
      <div>
        <label for="apikey">API key</label>
        <input id="apikey" type="password" bind:value={apiKey} placeholder="sk-…" />
      </div>
      <div>
        <label for="apilabel">Etiqueta (opcional)</label>
        <input id="apilabel" bind:value={apiKeyLabel} placeholder="OpenAI personal" />
      </div>
      <button onclick={saveApiKey} disabled={!apiKey.trim()}>Guardar</button>
    </div>
    <p class="muted small">
      Se guarda en el Keychain del sistema, nunca en la base de datos ni en un fichero.
    </p>
  </section>
</div>

{#snippet accountRow(account: Account)}
  <div class="account">
    <div class="stack" style="gap: 0.2rem">
      <div class="row">
        <strong>{account.label}</strong>
        <span
          class="badge"
          class:sub={account.credential_kind === "subscription_oauth"}
          class:key={account.credential_kind === "api_key"}
        >
          {kindLabel(account.credential_kind)}
        </span>
        {#if account.status === "active"}
          <span class="badge ok">Activa</span>
        {:else if account.status === "broken"}
          <span class="badge err">Vía rota</span>
        {:else if account.status === "expired" && account.credential_kind === "local"}
          <span class="badge warn">Servidor apagado</span>
        {:else}
          <span class="badge warn">{account.status}</span>
        {/if}
      </div>
      <span class="muted small">
        Conectada el {formatTime(account.created_at)}
        {#if account.expires_at}
          · token válido hasta {formatTime(account.expires_at)}
        {/if}
      </span>
    </div>
    <button class="danger" onclick={() => disconnect(account)}>Desconectar</button>
  </div>
{/snippet}

<style>
  .account {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 1rem;
    padding: 0.7rem 0.8rem;
    border: 1px solid var(--border);
    border-radius: 10px;
    background: var(--surface-2);
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

  .small {
    font-size: 0.78rem;
  }
</style>
