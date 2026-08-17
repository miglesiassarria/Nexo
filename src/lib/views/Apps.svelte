<script lang="ts">
  import {
    api,
    errorText,
    formatTime,
    kindLabel,
    type App,
    type AppDetail,
    type Grant,
    type GrantableRoute,
    type IssuedApp,
    type RouteModels,
    type ModelGrant,
    NEXO_REASONING_LEVELS,
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

  /** Modelos de cada vía de la aplicación desplegada, por `proveedor|vía`. */
  let routeModels = $state<Record<string, RouteModels>>({});
  /** Vía cuya lista de modelos está abierta, y el texto del buscador. */
  let openRoute = $state<string | null>(null);
  let filter = $state("");
  let saving = $state(false);
  /** Id de la aplicación cuya clave se acaba de copiar, para el aviso breve. */
  let copiedPrefixFor = $state<string | null>(null);
  /** Si lo copiado fue la clave completa o solo el prefijo (spec 0011). */
  let copiedKind = $state<"full" | "prefix-only">("full");

  const routeKey = (provider: string, kind: string) => `${provider}|${kind}`;

  async function loadRouteModels(app: App) {
    const next: Record<string, RouteModels> = {};
    for (const route of routes) {
      next[routeKey(route.provider_id, route.credential_kind)] = await api.appRouteModels({
        appId: app.id,
        providerId: route.provider_id,
        credentialKind: route.credential_kind,
      });
    }
    routeModels = next;
  }

  async function togglePermissions(app: App) {
    if (expanded === app.id) {
      expanded = null;
      openRoute = null;
      return;
    }
    expanded = app.id;
    openRoute = null;
    filter = "";
    error = null;
    try {
      await loadRouteModels(app);
    } catch (e) {
      error = errorText(e);
    }
  }

  function visible(models: RouteModels["models"]) {
    const needle = filter.trim().toLowerCase();
    if (!needle) return models;
    return models.filter((m) => m.public_name.toLowerCase().includes(needle));
  }

  /**
   * Guarda el conjunto entero de modelos de una vía. Un conjunto vacío retira la
   * vía: no existe «concedida sin modelos».
   */
  async function saveModels(app: App, route: GrantableRoute, models: ModelGrant[]) {
    saving = true;
    error = null;
    const grant = grantFor(app.id, route.provider_id, route.credential_kind);
    const limit = limitFor(app.id, route.provider_id, route.credential_kind);
    try {
      await api.setAppModels({
        appId: app.id,
        providerId: route.provider_id,
        credentialKind: route.credential_kind,
        models,
        allowTools: grant?.allow_tools ?? true,
        allowMultimodal: grant?.allow_multimodal ?? true,
        maxRequests: limit?.max_requests ?? null,
        windowSeconds: limit?.window_seconds ?? null,
      });
      await load();
      await loadRouteModels(app);
      onchange();
    } catch (e) {
      error = errorText(e);
    } finally {
      saving = false;
    }
  }

  /**
   * La selección actual de una vía **con el nivel de cada modelo**.
   *
   * Se reenvía completa en cada cambio porque el núcleo borra y reinserta las
   * filas del permiso: mandar solo los nombres dejaría a nulo el nivel de todos
   * los modelos sin que nadie lo hubiera pedido.
   */
  function selectedGrants(key: string): ModelGrant[] {
    return (routeModels[key]?.models ?? [])
      .filter((m) => m.selected)
      .map((m) => ({ public_name: m.public_name, reasoning_effort: m.configured_effort }));
  }

  function toggleModel(app: App, route: GrantableRoute, name: string, on: boolean) {
    const key = routeKey(route.provider_id, route.credential_kind);
    const current = new Map(selectedGrants(key).map((g) => [g.public_name, g]));
    if (on) current.set(name, { public_name: name, reasoning_effort: null });
    else current.delete(name);
    saveModels(app, route, [...current.values()]);
  }

  /** Marca o desmarca lo que el filtro deja a la vista, no todo el catálogo. */
  function toggleVisible(app: App, route: GrantableRoute, on: boolean) {
    const key = routeKey(route.provider_id, route.credential_kind);
    const shown = visible(routeModels[key]?.models ?? []).map((m) => m.public_name);
    const current = new Map(selectedGrants(key).map((g) => [g.public_name, g]));
    for (const name of shown) {
      if (on) current.set(name, current.get(name) ?? { public_name: name, reasoning_effort: null });
      else current.delete(name);
    }
    saveModels(app, route, [...current.values()]);
  }

  /**
   * Cambia el nivel de esfuerzo de un modelo. Cadena vacía = sin especificar.
   *
   * Es un valor por defecto: solo se aplica cuando la aplicación cliente no
   * manda su propio `reasoning_effort`. Nunca sobrescribe lo que pida.
   */
  function setEffort(app: App, route: GrantableRoute, name: string, effort: string) {
    const key = routeKey(route.provider_id, route.credential_kind);
    const current = new Map(selectedGrants(key).map((g) => [g.public_name, g]));
    current.set(name, { public_name: name, reasoning_effort: effort === "" ? null : effort });
    saveModels(app, route, [...current.values()]);
  }

  /** Los niveles del modelo que esta versión de Nexo sabe enviar. */
  function usableLevels(model: RouteModels["models"][number]): string[] {
    return model.caps.reasoning_levels.filter((l) => NEXO_REASONING_LEVELS.includes(l));
  }

  /** Un nivel configurado que el modelo ya no declara: se conserva, no se manda. */
  function effortIsStale(model: RouteModels["models"][number]): boolean {
    return (
      !!model.configured_effort && !model.caps.reasoning_levels.includes(model.configured_effort)
    );
  }

  async function load() {
    try {
      apps = await api.listApps();
      routes = await api.grantableRoutes();
      error = null;
      const next: Record<string, AppDetail> = {};
      for (const app of apps) {
        next[app.id] = await api.appDetail(app.id);
      }
      details = next;
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

  /**
   * Una insignia por vía (proveedor + tipo de credencial), no una por
   * modelo concedido. `grants` trae una fila por modelo, así que una app con
   * varios modelos marcados en la misma vía repite `provider_id` +
   * `credential_kind`: iterar la lista cruda con esa combinación como clave
   * de `{#each}` colisiona (`each_key_duplicate`), lo que interrumpe esa
   * pasada de reactividad de Svelte y deja huérfanos otros cambios de estado
   * pendientes en el mismo ciclo — incluida la actualización de `details`
   * que decide el aviso de «sin modelos marcados». De ahí que ese aviso
   * pudiera quedarse mal calculado aunque los datos fueran correctos.
   */
  function distinctRoutes(grants: Grant[]): Grant[] {
    const byRoute = new Map<string, Grant>();
    for (const g of grants) {
      byRoute.set(g.provider_id + g.credential_kind, g);
    }
    return [...byRoute.values()];
  }

  async function updateLimit(
    app: App,
    route: GrantableRoute,
    maxRequests: number,
    windowSeconds: number,
  ) {
    error = null;
    const key = routeKey(route.provider_id, route.credential_kind);
    const grant = grantFor(app.id, route.provider_id, route.credential_kind);
    try {
      await api.setAppModels({
        appId: app.id,
        providerId: route.provider_id,
        credentialKind: route.credential_kind,
        // El límite se cambia sin tocar la selección: se reenvía la que hay.
        models: routeModels[key]?.inherited_all
          ? [{ public_name: "*", reasoning_effort: null }]
          : selectedGrants(key),
        allowTools: grant?.allow_tools ?? true,
        allowMultimodal: grant?.allow_multimodal ?? true,
        maxRequests,
        windowSeconds,
      });
      await load();
      await loadRouteModels(app);
      onchange();
    } catch (e) {
      error = errorText(e);
    }
  }

  async function copyToken() {
    if (issued) await navigator.clipboard.writeText(issued.token);
  }

  /**
   * Copia la clave. La pregunta se hace al hacer clic, no antes (D3 de la
   * spec 0011): la lista no sabe de antemano si esta aplicación tiene un
   * token recuperable en el almacén seguro, así que no hay ningún indicio
   * visual previo — solo el resultado tras pulsar, distinto en cada caso
   * para que no se descubra la diferencia solo al pegarla y que falle.
   *
   * Solo se llama para aplicaciones ACTIVAS: una revocada no ofrece ni
   * siquiera el botón (D4) — no tiene sentido preguntar por un secreto que ya
   * se borró a propósito al revocar.
   */
  async function copyKey(app: App) {
    const full = await api.appTokenSecret(app.id);
    if (full !== null) {
      await navigator.clipboard.writeText(full);
      copiedPrefixFor = app.id;
      copiedKind = "full";
    } else {
      await navigator.clipboard.writeText(app.token_prefix);
      copiedPrefixFor = app.id;
      copiedKind = "prefix-only";
    }
    setTimeout(() => {
      if (copiedPrefixFor === app.id) copiedPrefixFor = null;
    }, 2500);
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
      Nace <strong>sin acceso a ningún modelo</strong>: después, en
      <strong>Permisos</strong>, marcas los que puede usar.
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
              <!--
                Sin botón a propósito (D4 de la spec 0011): un token revocado
                no autentica, así que no se ofrece como copiable ni el
                prefijo con apariencia de acción — sería una clave que
                parece servir y no sirve.
              -->
              <code>{app.token_prefix}…</code>
            {:else}
              <span class="badge ok">Activa</span>
              <button
                class="ghost copy-prefix"
                onclick={() => copyKey(app)}
                title="Copia la clave completa al portapapeles"
              >
                <code>{app.token_prefix}…</code>
                {#if copiedPrefixFor === app.id}
                  {copiedKind === "full" ? "✓ clave copiada" : "✓ solo prefijo (clave no recuperable)"}
                {/if}
              </button>
            {/if}
            {#each distinctRoutes(details[app.id]?.grants ?? []) as g (g.provider_id + g.credential_kind)}
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
          <button class="ghost" onclick={() => togglePermissions(app)}>
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
          Esta aplicación no tiene ningún modelo marcado, así que
          <code>GET /v1/models</code> le devuelve una lista vacía. La mayoría de
          clientes lo muestran como «no se encontraron modelos». Marca los modelos
          que puede usar en <strong>Permisos</strong>.
        </div>
      {/if}

      {#if expanded === app.id}
        <div class="routes">
          {#each routes as route (route.provider_id + route.credential_kind)}
            {@const key = routeKey(route.provider_id, route.credential_kind)}
            {@const grant = grantFor(app.id, route.provider_id, route.credential_kind)}
            {@const limit = limitFor(app.id, route.provider_id, route.credential_kind)}
            {@const rm = routeModels[key]}
            <div class="route">
              <div class="route-head">
                <button
                  class="ghost route-toggle"
                  onclick={() => {
                    openRoute = openRoute === key ? null : key;
                    filter = "";
                  }}
                >
                  <span class="caret">{openRoute === key ? "▾" : "▸"}</span>
                  <span>{route.provider_id} · {kindLabel(route.credential_kind)}</span>
                </button>
                {#if rm?.inherited_all}
                  <span class="badge sub" title="Incluye los que el proveedor añada después">
                    todos ({route.models})
                  </span>
                {:else}
                  <span class="badge" class:ok={(rm?.selected ?? 0) > 0}>
                    {rm?.selected ?? 0} de {route.models}
                  </span>
                {/if}
                {#if !route.connected}
                  <span class="badge warn" title="Conéctala en Proveedores">sin cuenta</span>
                {/if}
              </div>

              {#if openRoute === key && rm}
                <div class="models stack">
                  {#if rm.inherited_all}
                    <p class="muted small">
                      Esta vía viene de un permiso anterior que da acceso a
                      <strong>todos</strong> sus modelos, incluidos los que el proveedor
                      añada en el futuro. Si marcas modelos concretos, pasará a servir
                      solo esos.
                    </p>
                  {/if}

                  <div class="filter-row">
                    <input
                      bind:value={filter}
                      placeholder="Buscar entre {route.models} modelos…"
                    />
                    <button
                      onclick={() => toggleVisible(app, route, true)}
                      disabled={saving}
                    >
                      Marcar los visibles
                    </button>
                    <button
                      onclick={() => toggleVisible(app, route, false)}
                      disabled={saving}
                    >
                      Desmarcar
                    </button>
                  </div>

                  {#each visible(rm.models) as model (model.public_name)}
                    <label class="check model">
                      <input
                        type="checkbox"
                        checked={model.selected}
                        disabled={saving}
                        onchange={(e) =>
                          toggleModel(
                            app,
                            route,
                            model.public_name,
                            e.currentTarget.checked,
                          )}
                      />
                      <code>{model.public_name}</code>
                      {#if model.missing}
                        <span
                          class="badge warn"
                          title="Marcado, pero el proveedor ya no lo ofrece"
                        >
                          ya no está en el catálogo
                        </span>
                      {:else}
                        {#if model.caps.tools}<span class="badge">herramientas</span>{/if}
                        {#if model.caps.vision}<span class="badge">visión</span>{/if}
                        {#if model.accounting === "subscription"}
                          <span class="badge sub">suscripción</span>
                        {:else if model.accounting === "local"}
                          <span class="badge">local</span>
                        {:else if model.priced}
                          <span class="badge key">por token</span>
                        {/if}

                        {#if model.selected && model.caps.reasoning && model.caps.reasoning_levels.length > 0}
                          <span class="effort">
                            <span class="effort-label">esfuerzo</span>
                            <select
                              value={model.configured_effort ?? ""}
                              disabled={saving}
                              title="Solo se aplica si la aplicación no manda su propio nivel"
                              onchange={(e) =>
                                setEffort(app, route, model.public_name, e.currentTarget.value)}
                            >
                              <option value="">sin especificar</option>
                              {#each usableLevels(model) as level (level)}
                                <option value={level}>{level}</option>
                              {/each}
                            </select>
                            {#if effortIsStale(model)}
                              <span
                                class="badge warn"
                                title="Configurado, pero el modelo ya no lo admite. Se conserva y no se envía."
                              >
                                «{model.configured_effort}» ya no está admitido
                              </span>
                            {/if}
                            {#each model.caps.reasoning_levels.filter((l) => !NEXO_REASONING_LEVELS.includes(l)) as unknown (unknown)}
                              <span
                                class="badge"
                                title="El proveedor lo admite, pero esta versión de Nexo todavía no sabe enviarlo"
                              >
                                {unknown}: aún no soportado
                              </span>
                            {/each}
                          </span>
                        {/if}
                      {/if}
                    </label>
                  {/each}

                  {#if visible(rm.models).length === 0}
                    <p class="muted small">Ningún modelo coincide con «{filter}».</p>
                  {/if}
                </div>
              {/if}

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
                        route,
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
                        route,
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

  .copy-prefix {
    display: inline-flex;
    align-items: center;
    gap: 0.35rem;
    padding: 0;
    font-size: 0.85rem;
    color: var(--text-muted);
    cursor: pointer;
  }

  .copy-prefix:hover {
    color: var(--text);
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

  .route-head {
    display: flex;
    align-items: center;
    gap: 0.5rem;
  }

  .route-toggle {
    display: flex;
    align-items: center;
    gap: 0.4rem;
    padding: 0.1rem 0;
    font-size: 0.875rem;
    color: var(--text);
  }

  .caret {
    color: var(--muted);
    font-size: 0.75rem;
    width: 0.8rem;
  }

  .models {
    gap: 0.35rem;
    margin-left: 1.2rem;
    padding: 0.6rem 0.7rem;
    border: 1px solid var(--border);
    border-radius: 10px;
    background: var(--surface-2);
    /* Sesenta modelos no pueden empujar el resto de la página fuera de vista. */
    max-height: 22rem;
    overflow-y: auto;
  }

  .filter-row {
    display: grid;
    grid-template-columns: 1fr auto auto;
    gap: 0.4rem;
    position: sticky;
    top: 0;
    background: var(--surface-2);
    padding-bottom: 0.4rem;
  }

  .model code {
    font-size: 0.78rem;
  }

  .effort {
    display: inline-flex;
    align-items: center;
    gap: 0.3rem;
    margin-left: 0.2rem;
  }

  .effort-label {
    font-size: 0.7rem;
    color: var(--text-muted);
  }

  .effort select {
    font-size: 0.72rem;
    padding: 0.1rem 0.25rem;
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
