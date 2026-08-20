//! Migraciones versionadas. Solo hacia adelante: no hay `down`.
//!
//! Ver `docs/modelo-datos.md`.

use crate::error::Result;
use rusqlite::Connection;

pub const CURRENT_VERSION: i64 = 4;

pub fn apply(conn: &Connection) -> Result<()> {
    conn.pragma_update(None, "journal_mode", "WAL")?;
    conn.pragma_update(None, "foreign_keys", true)?;
    conn.pragma_update(None, "synchronous", "NORMAL")?;

    let version: i64 = conn.pragma_query_value(None, "user_version", |r| r.get(0))?;

    if version < 1 {
        conn.execute_batch(V1)?;
        conn.pragma_update(None, "user_version", 1)?;
        tracing::info!("esquema migrado a la versión 1");
    }

    if version < 2 {
        conn.execute_batch(V2)?;
        conn.pragma_update(None, "user_version", 2)?;
        tracing::info!("esquema migrado a la versión 2");
    }

    if version < 3 {
        conn.execute_batch(V3)?;
        conn.pragma_update(None, "user_version", 3)?;
        tracing::info!("esquema migrado a la versión 3");
    }

    if version < 4 {
        conn.execute_batch(V4)?;
        conn.pragma_update(None, "user_version", 4)?;
        tracing::info!("esquema migrado a la versión 4");
    }

    Ok(())
}

const V1: &str = r#"
-- Cuentas y credenciales. Ningún secreto vive aquí: solo la referencia al
-- almacén seguro del sistema operativo.
CREATE TABLE accounts (
  id              TEXT PRIMARY KEY,
  provider_id     TEXT NOT NULL,
  credential_kind TEXT NOT NULL,
  label           TEXT NOT NULL,
  keychain_ref    TEXT,
  external_id     TEXT,
  scopes          TEXT,
  expires_at      INTEGER,
  status          TEXT NOT NULL DEFAULT 'active',
  -- Registro de que el usuario recibió y aceptó la advertencia del ADR 0001.
  -- Obligatorio para las cuentas de suscripción.
  risk_ack_at     INTEGER,
  created_at      INTEGER NOT NULL,
  last_used_at    INTEGER,
  UNIQUE (provider_id, credential_kind, external_id)
);

CREATE TABLE apps (
  id           TEXT PRIMARY KEY,
  name         TEXT NOT NULL,
  token_hash   TEXT NOT NULL UNIQUE,
  token_prefix TEXT NOT NULL,
  created_at   INTEGER NOT NULL,
  last_seen_at INTEGER,
  revoked_at   INTEGER,
  notes        TEXT
);

-- Sin fila no hay permiso. El acceso se concede, no se deniega.
CREATE TABLE app_grants (
  app_id           TEXT NOT NULL REFERENCES apps(id) ON DELETE CASCADE,
  provider_id      TEXT NOT NULL,
  credential_kind  TEXT NOT NULL,
  model_pattern    TEXT NOT NULL DEFAULT '*',
  allow_tools      INTEGER NOT NULL DEFAULT 0,
  allow_multimodal INTEGER NOT NULL DEFAULT 0,
  log_content      INTEGER NOT NULL DEFAULT 0,
  PRIMARY KEY (app_id, provider_id, credential_kind, model_pattern)
);

CREATE TABLE app_limits (
  app_id            TEXT NOT NULL REFERENCES apps(id) ON DELETE CASCADE,
  provider_id       TEXT NOT NULL,
  credential_kind   TEXT NOT NULL,
  window_seconds    INTEGER NOT NULL,
  max_requests      INTEGER,
  max_input_tokens  INTEGER,
  max_output_tokens INTEGER,
  PRIMARY KEY (app_id, provider_id, credential_kind, window_seconds)
);

-- Catálogo. La clave compuesta es la consecuencia práctica del eje de
-- credencial: el mismo modelo por dos vías son dos filas distintas.
CREATE TABLE models (
  provider_id        TEXT NOT NULL,
  credential_kind    TEXT NOT NULL,
  api_id             TEXT NOT NULL,
  public_name        TEXT NOT NULL,
  caps               TEXT NOT NULL,
  context_max        INTEGER,
  input_max          INTEGER,
  output_max         INTEGER,
  accounting         TEXT NOT NULL,
  price_input        INTEGER,
  price_output       INTEGER,
  price_cached_input INTEGER,
  price_source       TEXT,
  manifest_version   TEXT,
  available          INTEGER NOT NULL DEFAULT 1,
  updated_at         INTEGER NOT NULL,
  PRIMARY KEY (provider_id, credential_kind, api_id)
);

-- Eventos inmutables. El contenido de prompts y respuestas NO está aquí.
CREATE TABLE requests (
  id                  TEXT PRIMARY KEY,
  ts                  INTEGER NOT NULL,
  app_id              TEXT NOT NULL,
  provider_id         TEXT NOT NULL,
  credential_kind     TEXT NOT NULL,
  account_id          TEXT,
  public_model        TEXT NOT NULL,
  api_model           TEXT NOT NULL,
  operation           TEXT NOT NULL DEFAULT 'chat',
  streamed            INTEGER NOT NULL DEFAULT 0,
  status              TEXT NOT NULL,
  error_kind          TEXT,
  http_status         INTEGER,
  latency_ms          INTEGER,
  ttft_ms             INTEGER,
  input_tokens        INTEGER,
  output_tokens       INTEGER,
  cached_input_tokens INTEGER,
  reasoning_tokens    INTEGER,
  total_tokens        INTEGER,
  usage_source        TEXT NOT NULL,
  cost_micros         INTEGER,
  cost_basis          TEXT NOT NULL,
  fallback_from       TEXT,
  provider_usage_raw  TEXT,
  provider_request_id TEXT
);

CREATE INDEX idx_requests_ts ON requests(ts);
CREATE INDEX idx_requests_app_ts ON requests(app_id, ts);
CREATE INDEX idx_requests_model_ts
  ON requests(provider_id, credential_kind, public_model, ts);

-- Contenido, solo si el usuario lo activa por aplicación. Tabla aparte para
-- que borrarlo no implique perder las métricas.
CREATE TABLE request_content (
  request_id TEXT PRIMARY KEY REFERENCES requests(id) ON DELETE CASCADE,
  ts         INTEGER NOT NULL,
  prompt     TEXT,
  completion TEXT
);

CREATE INDEX idx_request_content_ts ON request_content(ts);

-- Rollups horarios. El coste se acumula separado por base para que el panel
-- nunca sume una estimación con un dato y lo presente como dato.
CREATE TABLE usage_hourly (
  hour                  INTEGER NOT NULL,
  app_id                TEXT NOT NULL,
  provider_id           TEXT NOT NULL,
  credential_kind       TEXT NOT NULL,
  public_model          TEXT NOT NULL,
  operation             TEXT NOT NULL,
  requests              INTEGER NOT NULL DEFAULT 0,
  errors                INTEGER NOT NULL DEFAULT 0,
  cancels               INTEGER NOT NULL DEFAULT 0,
  rate_limited          INTEGER NOT NULL DEFAULT 0,
  local_limited         INTEGER NOT NULL DEFAULT 0,
  input_tokens          INTEGER NOT NULL DEFAULT 0,
  output_tokens         INTEGER NOT NULL DEFAULT 0,
  total_tokens          INTEGER NOT NULL DEFAULT 0,
  cost_reported_micros  INTEGER NOT NULL DEFAULT 0,
  cost_estimated_micros INTEGER NOT NULL DEFAULT 0,
  subscription_requests INTEGER NOT NULL DEFAULT 0,
  latency_sum_ms        INTEGER NOT NULL DEFAULT 0,
  latency_max_ms        INTEGER NOT NULL DEFAULT 0,
  ttft_sum_ms           INTEGER NOT NULL DEFAULT 0,
  ttft_count            INTEGER NOT NULL DEFAULT 0,
  PRIMARY KEY (hour, app_id, provider_id, credential_kind, public_model, operation)
);

CREATE TABLE settings (
  key        TEXT PRIMARY KEY,
  value      TEXT NOT NULL,
  updated_at INTEGER NOT NULL
);
"#;

/// Proveedores que añade el usuario indicando nombre, dirección y clave.
///
/// Tienen tabla propia y no se deducen de `accounts` por dos razones: hay que poder
/// distinguir un proveedor añadido a propósito de un `provider_id` desconocido por
/// corrupción, y el nombre legible tiene que sobrevivir a desconectar la cuenta sin
/// borrar el proveedor.
///
/// `id` es el slug derivado del nombre y es la clave primaria: eso hace que dos
/// proveedores con el mismo nombre no puedan existir, sin comprobación aparte.
const V2: &str = r#"
CREATE TABLE custom_providers (
  id         TEXT PRIMARY KEY,
  name       TEXT NOT NULL,
  base_url   TEXT NOT NULL,
  -- Formato de cable que habla. Hoy solo 'openai_compat'; el de Anthropic queda
  -- aplazado (ver spec 0002) y entrará como otro valor, sin migración.
  compat     TEXT NOT NULL DEFAULT 'openai_compat',
  created_at INTEGER NOT NULL
);
"#;

const V3: &str = r#"
-- Nivel de esfuerzo de razonamiento por permiso (spec 0009).
--
-- Es el PRIMER valor de esta tabla que es del modelo y no de la vía: el resto
-- de columnas se escriben iguales en todas las filas de un proveedor+vía, y
-- esta no. `grant_for` elige la fila más específica, así que el nivel que se
-- aplica es el de la fila que autoriza la petición.
--
-- Nulo significa «sin especificar»: Nexo no manda nada y decide el proveedor,
-- que es exactamente el comportamiento anterior a esta columna. Por eso no
-- lleva DEFAULT: una base de datos migrada queda igual que estaba.
ALTER TABLE app_grants ADD COLUMN reasoning_effort TEXT;
"#;

/// Almacén de secretos cifrados en reposo con AES-256-GCM (ADR 0006, spec 0015).
/// La clave maestra vive en el Llavero del sistema; en SQLite solo los blobs cifrados.
const V4: &str = r#"
CREATE TABLE IF NOT EXISTS encrypted_secrets (
  key         TEXT PRIMARY KEY,
  nonce       BLOB NOT NULL,
  ciphertext  BLOB NOT NULL,
  updated_at  INTEGER NOT NULL
);
"#;

#[cfg(test)]
mod tests {
    use super::*;

    fn mem() -> Connection {
        let conn = Connection::open_in_memory().unwrap();
        apply(&conn).unwrap();
        conn
    }

    #[test]
    fn applies_and_records_version() {
        let conn = mem();
        let v: i64 = conn
            .pragma_query_value(None, "user_version", |r| r.get(0))
            .unwrap();
        assert_eq!(v, CURRENT_VERSION);
    }

    #[test]
    fn is_idempotent() {
        let conn = mem();
        apply(&conn).unwrap();
        apply(&conn).unwrap();
        let v: i64 = conn
            .pragma_query_value(None, "user_version", |r| r.get(0))
            .unwrap();
        assert_eq!(v, CURRENT_VERSION);
    }

    /// Criterio 3 de la spec 0009: una base de datos creada ANTES de esta
    /// especificación se abre sin error y sus permisos siguen ahí, sin nivel.
    /// Se construye a propósito el esquema en la versión 2 (V1 + V2, sin V3) y
    /// se inserta un permiso como lo haría la versión anterior de Nexo.
    #[test]
    fn migration_v3_adds_the_effort_column_and_keeps_grants() {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch(V1).unwrap();
        conn.execute_batch(V2).unwrap();
        conn.pragma_update(None, "user_version", 2).unwrap();

        conn.execute(
            "INSERT INTO apps (id, name, token_hash, token_prefix, created_at)
             VALUES ('a1', 'vieja', 'h', 'nx_', 0)",
            [],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO app_grants
               (app_id, provider_id, credential_kind, model_pattern, allow_tools)
             VALUES ('a1', 'openai', 'subscription_oauth', 'openai/gpt-5.6-sol', 1)",
            [],
        )
        .unwrap();

        // La migración de verdad, sobre datos que ya existían.
        apply(&conn).unwrap();

        let v: i64 = conn
            .pragma_query_value(None, "user_version", |r| r.get(0))
            .unwrap();
        assert_eq!(v, CURRENT_VERSION);

        let (pattern, allow_tools, effort): (String, i64, Option<String>) = conn
            .query_row(
                "SELECT model_pattern, allow_tools, reasoning_effort
                 FROM app_grants WHERE app_id = 'a1'",
                [],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
            )
            .expect("el permiso que ya existía debe seguir ahí");
        assert_eq!(pattern, "openai/gpt-5.6-sol");
        assert_eq!(allow_tools, 1, "migrar no puede alterar lo que ya estaba");
        assert_eq!(
            effort, None,
            "sin especificar: idéntico al comportamiento anterior a la columna"
        );
    }

    #[test]
    fn migration_v4_creates_encrypted_secrets_table() {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch(V1).unwrap();
        conn.execute_batch(V2).unwrap();
        conn.execute_batch(V3).unwrap();
        conn.pragma_update(None, "user_version", 3).unwrap();

        apply(&conn).unwrap();

        let v: i64 = conn
            .pragma_query_value(None, "user_version", |r| r.get(0))
            .unwrap();
        assert_eq!(v, 4);

        conn.execute(
            "INSERT INTO encrypted_secrets (key, nonce, ciphertext, updated_at)
             VALUES ('test/key', X'0102030405060708090a0b0c', X'aabbcc', 1234)",
            [],
        )
        .expect("la tabla encrypted_secrets debe aceptar escrituras");
    }

    #[test]
    fn creates_every_expected_table() {
        let conn = mem();
        let mut stmt = conn
            .prepare("SELECT name FROM sqlite_master WHERE type='table' ORDER BY name")
            .unwrap();
        let tables: Vec<String> = stmt
            .query_map([], |r| r.get::<_, String>(0))
            .unwrap()
            .map(|r| r.unwrap())
            .collect();
        for expected in [
            "accounts",
            "app_grants",
            "app_limits",
            "apps",
            "encrypted_secrets",
            "models",
            "request_content",
            "requests",
            "settings",
            "usage_hourly",
        ] {
            assert!(tables.contains(&expected.to_string()), "falta {expected}");
        }
    }

    #[test]
    fn deleting_an_app_cascades_grants_and_limits() {
        let conn = mem();
        conn.execute(
            "INSERT INTO apps (id, name, token_hash, token_prefix, created_at)
             VALUES ('a1', 'test', 'h', 'nx_', 0)",
            [],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO app_grants (app_id, provider_id, credential_kind)
             VALUES ('a1', 'openai', 'subscription_oauth')",
            [],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO app_limits
               (app_id, provider_id, credential_kind, window_seconds, max_requests)
             VALUES ('a1', 'openai', 'subscription_oauth', 3600, 60)",
            [],
        )
        .unwrap();

        conn.execute("DELETE FROM apps WHERE id = 'a1'", []).unwrap();

        let grants: i64 = conn
            .query_row("SELECT COUNT(*) FROM app_grants", [], |r| r.get(0))
            .unwrap();
        let limits: i64 = conn
            .query_row("SELECT COUNT(*) FROM app_limits", [], |r| r.get(0))
            .unwrap();
        assert_eq!((grants, limits), (0, 0));
    }

    #[test]
    fn models_are_keyed_by_credential_kind_too() {
        let conn = mem();
        let insert = "INSERT INTO models
            (provider_id, credential_kind, api_id, public_name, caps, accounting, updated_at)
            VALUES ('openai', ?1, 'gpt-5.5', 'openai/gpt-5.5', '{}', ?2, 0)";
        conn.execute(insert, ("api_key", "metered")).unwrap();
        // La misma pareja proveedor+modelo por otra vía debe convivir.
        conn.execute(insert, ("subscription_oauth", "subscription"))
            .unwrap();
        let n: i64 = conn
            .query_row("SELECT COUNT(*) FROM models", [], |r| r.get(0))
            .unwrap();
        assert_eq!(n, 2);
    }
}
