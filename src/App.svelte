<script lang="ts">
  import { api, errorText, type GatewayStatus } from "./lib/api";
  import Dashboard from "./lib/views/Dashboard.svelte";
  import Providers from "./lib/views/Providers.svelte";
  import Apps from "./lib/views/Apps.svelte";
  import Models from "./lib/views/Models.svelte";
  import SettingsView from "./lib/views/Settings.svelte";

  type Tab = "dashboard" | "providers" | "apps" | "models" | "settings";

  const tabs: { id: Tab; label: string }[] = [
    { id: "dashboard", label: "Panel" },
    { id: "providers", label: "Proveedores" },
    { id: "apps", label: "Aplicaciones" },
    { id: "models", label: "Modelos" },
    { id: "settings", label: "Configuración" },
  ];

  let tab = $state<Tab>("dashboard");
  let status = $state<GatewayStatus | null>(null);
  let error = $state<string | null>(null);

  async function refresh() {
    try {
      status = await api.gatewayStatus();
      error = null;
    } catch (e) {
      error = errorText(e);
    }
  }

  async function togglePause() {
    if (!status) return;
    try {
      await api.setPaused(!status.paused);
      await refresh();
    } catch (e) {
      error = errorText(e);
    }
  }

  $effect(() => {
    refresh();
    const timer = setInterval(refresh, 5000);
    return () => clearInterval(timer);
  });
</script>

<div class="shell">
  <header>
    <div class="row brand">
      <svg viewBox="0 0 32 32" width="22" height="22" aria-hidden="true">
        <rect width="32" height="32" rx="8" fill="var(--navy)" />
        <path d="M11 22V10l10 12V10" stroke="#fff" stroke-width="2.4" fill="none"
              stroke-linecap="round" stroke-linejoin="round" />
        <circle cx="16" cy="16" r="2.6" fill="var(--cobalt)" />
      </svg>
      <h1>Nexo</h1>
    </div>

    <nav>
      {#each tabs as t (t.id)}
        <button class="tab" class:active={tab === t.id} onclick={() => (tab = t.id)}>
          {t.label}
        </button>
      {/each}
    </nav>

    <div class="row status">
      {#if status}
        {#if status.bind_error}
          <span class="badge err">Sin escuchar</span>
        {:else}
          <span class="badge" class:ok={!status.paused} class:warn={status.paused}>
            {status.paused ? "En pausa" : "Activo"}
          </span>
        {/if}
        <code>{status.base_url}</code>
        <button onclick={togglePause} disabled={!!status.bind_error}>
          {status.paused ? "Reanudar" : "Pausar"}
        </button>
      {/if}
    </div>
  </header>

  {#if error}
    <div class="banner"><div class="notice err">{error}</div></div>
  {/if}

  {#if status?.bind_error}
    <div class="banner">
      <div class="notice err">
        <strong>El gateway no está escuchando.</strong> {status.bind_error}
      </div>
    </div>
  {/if}

  {#if status && status.apps_missing_limits.length > 0}
    <div class="banner">
      <div class="notice warn">
        <strong>{status.apps_missing_limits.length}</strong> aplicación(es) tienen acceso
        por suscripción sin límite configurado. Nexo rechazará sus peticiones: la vía de
        suscripción reparte una única cuota personal y no puede quedar sin protección.
      </div>
    </div>
  {/if}

  {#if status && status.broken_accounts > 0}
    <div class="banner">
      <div class="notice err">
        La vía de suscripción ha dejado de funcionar en {status.broken_accounts} cuenta(s).
        No es un mecanismo soportado por el proveedor y puede romperse sin aviso. Vuelve a
        conectar la cuenta o configura una API key como respaldo.
      </div>
    </div>
  {/if}

  <main>
    {#if tab === "dashboard"}
      <Dashboard {status} />
    {:else if tab === "providers"}
      <Providers onchange={refresh} />
    {:else if tab === "apps"}
      <Apps onchange={refresh} baseUrl={status?.base_url ?? null} />
    {:else if tab === "models"}
      <Models />
    {:else}
      <SettingsView {status} />
    {/if}
  </main>
</div>

<style>
  .shell {
    display: flex;
    flex-direction: column;
    height: 100%;
  }

  header {
    display: flex;
    align-items: center;
    gap: 1.25rem;
    padding: 0.6rem 1.1rem;
    border-bottom: 1px solid var(--border);
    background: var(--surface);
    flex-shrink: 0;
  }

  .brand {
    gap: 0.5rem;
  }

  nav {
    display: flex;
    gap: 0.15rem;
    flex: 1;
  }

  .tab {
    border-color: transparent;
    background: transparent;
    color: var(--text-muted);
    font-size: 0.85rem;
    padding: 0.35rem 0.7rem;
  }

  .tab.active {
    background: var(--surface-2);
    color: var(--text);
    font-weight: 600;
  }

  .status {
    flex-shrink: 0;
  }

  .status code {
    font-size: 0.75rem;
  }

  .banner {
    padding: 0.6rem 1.1rem 0;
  }

  main {
    flex: 1;
    overflow-y: auto;
    padding: 1.1rem;
  }
</style>
