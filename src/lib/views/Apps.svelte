<script lang="ts">
  import {
    api,
    errorText,
    formatTime,
    kindLabel,
    type App,
    type AppDetail,
    type GrantableRoute,
    type IssuedApp,
  } from "../api";

  let { onchange }: { onchange: () => void } = $props();

  /**
   * Vías concedibles. Se piden al núcleo, que las deriva del catálogo: llevarlas
   * escritas aquí hizo que LM Studio quedara sin poder autorizarse al añadirlo.
   */
  let routes = $state<GrantableRoute[]>([]);

  let apps = $state<App[]>([]);
  let details = $state<Record<string, AppDetail>>({});
  let issued = $state<IssuedApp | null>(null);
  let newName = $state("");
  let error = $state<string | null>(null);
  let expanded = $state<string | null>(null);

  async function load() {
    try {
      apps = await api.listApps();
      routes = await api.grantableRoutes();
      error = null;
      for (const app of apps) {
        details[app.id] = await api.appDetail(app.id);
      }
    } catch (e) {
      error = errorText(e);
    }
  }

  async function create() {
    error = null;
    try {
      issued = await api.createApp(newName);
      newName = "";
      await load();
      onchange();
    } catch (e) {
      error = errorText(e);
    }
  }

  async function revoke(app: App) {
    error = null;
    try {
      await api.revokeApp(app.id);
      await load();
      onchange();
    } catch (e) {
      error = errorText(e);
    }
  }

  async function remove(app: App) {
    error = null;
    try {
      await api.deleteApp(app.id);
      await load();
      onchange();
    } catch (e) {
      error = errorText(e);
    }
  }

  function grantFor(appId: string, provider: string, kind: string) {
    return details[appId]?.grants.find(
      (g) => g.provider_id === provider && g.credential_kind === kind,
    );
  }

  function limitFor(appId: string, provider: string, kind: string) {
    return details[appId]?.limits.find(
      (l) => l.provider_id === provider && l.credential_kind === kind,
    );
  }

  async function toggleRoute(
    app: App,
    provider: string,
    kind: string,
    enabled: boolean,
  ) {
    error = null;
    const existing = grantFor(app.id, provider, kind);
    const limit = limitFor(app.id, provider, kind);
    try {
      await api.setAppAccess({
        appId: app.id,
        providerId: provider,
        credentialKind: kind,
        enabled,
        allowTools: existing?.allow_tools ?? true,
        allowMultimodal: existing?.allow_multimodal ?? true,
        maxRequests: limit?.max_requests ?? null,
        windowSeconds: limit?.window_seconds ?? null,
      });
      await load();
      onchange();
    } catch (e) {
      error = errorText(e);
    }
  }

  async function updateLimit(
    app: App,
    provider: string,
    kind: string,
    maxRequests: number,
    windowSeconds: number,
  ) {
    error = null;
    const existing = grantFor(app.id, provider, kind);
    try {
      await api.setAppAccess({
        appId: app.id,
        providerId: provider,
        credentialKind: kind,
        enabled: true,
        allowTools: existing?.allow_tools ?? true,
        allowMultimodal: existing?.allow_multimodal ?? true,
        maxRequests,
        windowSeconds,
      });
      await load();
      onchange();
    } catch (e) {
      error = errorText(e);
    }
  }

  async function copyToken() {
    if (issued) await navigator.clipboard.writeText(issued.token);
  }

  $effect(() => {
    load();
  });
</script>

<div class="stack">
  {#if error}<div class="notice err">{error}</div>{/if}

  {#if issued}
    <div class="card token-card stack">
      <h2>Token de «{issued.app.name}»</h2>
      <p class="muted">
        Cópialo ahora. Nexo guarda solo su hash: no puede volver a mostrártelo.
        En tu herramienta va en el campo de clave API, con la URL base
        <code>http://127.0.0.1:8787/v1</code>.
      </p>
      <div class="row">
        <code class="token">{issued.token}</code>
        <button onclick={copyToken}>Copiar</button>
        <button class="ghost" onclick={() => (issued = null)}>Hecho</button>
      </div>
    </div>
  {/if}

  <section class="card stack">
    <h2>Nueva aplicación</h2>
    <p class="muted">
      Cada herramienta recibe su propio token, revocable de forma independiente.
      Nace con acceso a las vías que ya tengan una cuenta conectada.
    </p>
    <div class="new-form">
      <input bind:value={newName} placeholder="Nombre, p. ej. «Cursor» o «script de notas»" />
      <button class="primary" onclick={create} disabled={!newName.trim()}>
        Crear y emitir token
      </button>
    </div>
  </section>

  {#each apps as app (app.id)}
    <section class="card stack">
      <div class="row" style="justify-content: space-between">
        <div class="stack" style="gap: 0.2rem">
          <div class="row">
            <strong>{app.name}</strong>
            {#if app.revoked_at}
              <span class="badge err">Revocada</span>
            {:else}
              <span class="badge ok">Activa</span>
            {/if}
            <code>{app.token_prefix}…</code>
            {#each details[app.id]?.grants ?? [] as g (g.provider_id + g.credential_kind)}
              <span
                class="badge"
                class:sub={g.credential_kind === "subscription_oauth"}
                class:key={g.credential_kind === "api_key"}
              >
                {g.provider_id} · {kindLabel(g.credential_kind)}
              </span>
            {/each}
          </div>
          <span class="muted small">
            Creada el {formatTime(app.created_at)} · último uso {formatTime(app.last_seen_at)}
          </span>
        </div>
        <div class="row">
          <button
            class="ghost"
            onclick={() => (expanded = expanded === app.id ? null : app.id)}
          >
            {expanded === app.id ? "Ocultar permisos" : "Permisos"}
          </button>
          {#if !app.revoked_at}
            <button class="danger" onclick={() => revoke(app)}>Revocar</button>
          {:else}
            <button class="danger" onclick={() => remove(app)}>Eliminar</button>
          {/if}
        </div>
      </div>

      {#if !app.revoked_at && (details[app.id]?.grants.length ?? 0) === 0}
        <div class="notice warn">
          Esta aplicación no tiene ninguna vía concedida, así que
          <code>GET /v1/models</code> le devuelve una lista vacía. La mayoría de
          clientes lo muestran como «no se encontraron modelos». Concédele acceso
          en <strong>Permisos</strong>.
        </div>
      {/if}

      {#if expanded === app.id}
        <div class="routes">
          {#each routes as route (route.provider_id + route.credential_kind)}
            {@const grant = grantFor(app.id, route.provider_id, route.credential_kind)}
            {@const limit = limitFor(app.id, route.provider_id, route.credential_kind)}
            <div class="route">
              <label class="check">
                <input
                  type="checkbox"
                  checked={!!grant}
                  onchange={(e) =>
                    toggleRoute(
                      app,
                      route.provider_id,
                      route.credential_kind,
                      e.currentTarget.checked,
                    )}
                />
                <span>
                  {route.provider_id} · {kindLabel(route.credential_kind)}
                </span>
                <span class="badge">{route.models} modelo(s)</span>
                {#if !route.connected}
                  <span class="badge warn" title="Conéctala en Proveedores">
                    sin cuenta
                  </span>
                {/if}
              </label>

              {#if grant && route.requires_limit}
                <div class="limit">
                  <span class="muted small">Límite obligatorio:</span>
                  <input
                    type="number"
                    min="1"
                    value={limit?.max_requests ?? 60}
                    onchange={(e) =>
                      updateLimit(
                        app,
                        route.provider_id,
                        route.credential_kind,
                        Number(e.currentTarget.value),
                        limit?.window_seconds ?? 3600,
                      )}
                  />
                  <span class="muted small">peticiones cada</span>
                  <select
                    value={String(limit?.window_seconds ?? 3600)}
                    onchange={(e) =>
                      updateLimit(
                        app,
                        route.provider_id,
                        route.credential_kind,
                        limit?.max_requests ?? 60,
                        Number(e.currentTarget.value),
                      )}
                  >
                    <option value="60">minuto</option>
                    <option value="3600">hora</option>
                    <option value="86400">día</option>
                  </select>
                </div>
                <p class="muted small">
                  Esta vía reparte la cuota de tu suscripción personal entre todas las
                  aplicaciones. Sin límite, Nexo rechaza las peticiones.
                </p>
              {/if}
            </div>
          {/each}
        </div>
      {/if}
    </section>
  {/each}

  {#if apps.length === 0}
    <p class="muted">No hay aplicaciones todavía.</p>
  {/if}
</div>

<style>
  .new-form {
    display: grid;
    grid-template-columns: 1fr auto;
    gap: 0.6rem;
  }

  .token-card {
    border-color: color-mix(in srgb, var(--cyan) 45%, var(--border));
    background: color-mix(in srgb, var(--cyan) 8%, transparent);
  }

  .token {
    flex: 1;
    overflow-x: auto;
    white-space: nowrap;
    padding: 0.45rem 0.6rem;
  }

  .routes {
    display: flex;
    flex-direction: column;
    gap: 0.6rem;
    border-top: 1px solid var(--border);
    padding-top: 0.8rem;
  }

  .route {
    display: flex;
    flex-direction: column;
    gap: 0.35rem;
  }

  .check {
    display: flex;
    align-items: center;
    gap: 0.5rem;
    margin: 0;
    color: var(--text);
    font-size: 0.875rem;
  }

  .check input {
    width: auto;
  }

  .limit {
    display: flex;
    align-items: center;
    gap: 0.4rem;
    padding-left: 1.5rem;
  }

  .limit input {
    width: 5rem;
  }

  .limit select {
    width: auto;
  }

  .small {
    font-size: 0.78rem;
  }
</style>
