//! Motor de estadísticas: registro de eventos y agregación incremental.
//!
//! Cada petición se escribe como evento inmutable y actualiza su rollup
//! horario en la misma transacción. El panel nunca recorre el histórico.

use crate::db::Db;
use crate::error::Result;
use crate::provider::{CostBasis, UsageSource};
use crate::util;
use rusqlite::params;
use serde::Serialize;

/// Evento a registrar al cerrar una petición.
#[derive(Debug, Clone)]
pub struct RequestEvent {
    pub id: String,
    pub ts: i64,
    pub app_id: String,
    pub provider_id: String,
    pub credential_kind: String,
    pub account_id: Option<String>,
    pub public_model: String,
    pub api_model: String,
    pub operation: String,
    pub streamed: bool,
    pub status: RequestStatus,
    pub error_kind: Option<String>,
    pub http_status: Option<u16>,
    pub latency_ms: Option<i64>,
    pub ttft_ms: Option<i64>,
    pub input_tokens: Option<u32>,
    pub output_tokens: Option<u32>,
    pub cached_input_tokens: Option<u32>,
    pub reasoning_tokens: Option<u32>,
    pub usage_source: UsageSource,
    pub cost_micros: Option<i64>,
    pub cost_basis: CostBasis,
    /// Vía de la que se cayó, si hubo respaldo.
    pub fallback_from: Option<String>,
    pub provider_usage_raw: Option<serde_json::Value>,
    pub provider_request_id: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RequestStatus {
    Ok,
    Error,
    Cancelled,
}

impl RequestStatus {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Ok => "ok",
            Self::Error => "error",
            Self::Cancelled => "cancelled",
        }
    }
}

impl Db {
    /// Registra el evento y actualiza su rollup horario en una transacción.
    pub fn record_request(&self, e: &RequestEvent) -> Result<()> {
        let mut conn = self.lock();
        let tx = conn.transaction()?;

        let total = match (e.input_tokens, e.output_tokens) {
            (None, None) => None,
            (a, b) => Some(a.unwrap_or(0) + b.unwrap_or(0)),
        };

        tx.execute(
            "INSERT INTO requests
               (id, ts, app_id, provider_id, credential_kind, account_id,
                public_model, api_model, operation, streamed, status, error_kind,
                http_status, latency_ms, ttft_ms, input_tokens, output_tokens,
                cached_input_tokens, reasoning_tokens, total_tokens, usage_source,
                cost_micros, cost_basis, fallback_from, provider_usage_raw,
                provider_request_id)
             VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14,?15,?16,?17,
                     ?18,?19,?20,?21,?22,?23,?24,?25,?26)",
            params![
                e.id,
                e.ts,
                e.app_id,
                e.provider_id,
                e.credential_kind,
                e.account_id,
                e.public_model,
                e.api_model,
                e.operation,
                e.streamed as i64,
                e.status.as_str(),
                e.error_kind,
                e.http_status,
                e.latency_ms,
                e.ttft_ms,
                e.input_tokens,
                e.output_tokens,
                e.cached_input_tokens,
                e.reasoning_tokens,
                total,
                e.usage_source.as_str(),
                e.cost_micros,
                e.cost_basis.as_str(),
                e.fallback_from,
                e.provider_usage_raw
                    .as_ref()
                    .map(|v| v.to_string()),
                e.provider_request_id,
            ],
        )?;

        // El coste se acumula separado por base: nunca se suma una estimación
        // con un dato reportado.
        let (cost_reported, cost_estimated) = match e.cost_basis {
            CostBasis::Reported => (e.cost_micros.unwrap_or(0), 0),
            CostBasis::Estimated => (0, e.cost_micros.unwrap_or(0)),
            CostBasis::Subscription | CostBasis::Unavailable => (0, 0),
        };
        let is_subscription = e.cost_basis == CostBasis::Subscription;

        tx.execute(
            "INSERT INTO usage_hourly
               (hour, app_id, provider_id, credential_kind, public_model, operation,
                requests, errors, cancels, rate_limited, local_limited,
                input_tokens, output_tokens, total_tokens,
                cost_reported_micros, cost_estimated_micros, subscription_requests,
                latency_sum_ms, latency_max_ms, ttft_sum_ms, ttft_count)
             VALUES (?1,?2,?3,?4,?5,?6, 1,?7,?8,?9,?10, ?11,?12,?13, ?14,?15,?16,
                     ?17,?18,?19,?20)
             ON CONFLICT(hour, app_id, provider_id, credential_kind, public_model, operation)
             DO UPDATE SET
               requests = requests + 1,
               errors = errors + ?7,
               cancels = cancels + ?8,
               rate_limited = rate_limited + ?9,
               local_limited = local_limited + ?10,
               input_tokens = input_tokens + ?11,
               output_tokens = output_tokens + ?12,
               total_tokens = total_tokens + ?13,
               cost_reported_micros = cost_reported_micros + ?14,
               cost_estimated_micros = cost_estimated_micros + ?15,
               subscription_requests = subscription_requests + ?16,
               latency_sum_ms = latency_sum_ms + ?17,
               latency_max_ms = MAX(latency_max_ms, ?18),
               ttft_sum_ms = ttft_sum_ms + ?19,
               ttft_count = ttft_count + ?20",
            params![
                util::hour_floor_ms(e.ts),
                e.app_id,
                e.provider_id,
                e.credential_kind,
                e.public_model,
                e.operation,
                (e.status == RequestStatus::Error) as i64,
                (e.status == RequestStatus::Cancelled) as i64,
                (e.error_kind.as_deref() == Some("rate_limited")) as i64,
                (e.error_kind.as_deref() == Some("local_limit")) as i64,
                e.input_tokens.unwrap_or(0) as i64,
                e.output_tokens.unwrap_or(0) as i64,
                total.unwrap_or(0) as i64,
                cost_reported,
                cost_estimated,
                is_subscription as i64,
                e.latency_ms.unwrap_or(0),
                e.latency_ms.unwrap_or(0),
                e.ttft_ms.unwrap_or(0),
                e.ttft_ms.is_some() as i64,
            ],
        )?;

        tx.commit()?;
        Ok(())
    }

    pub fn record_content(
        &self,
        request_id: &str,
        ts: i64,
        prompt: &str,
        completion: &str,
    ) -> Result<()> {
        let conn = self.lock();
        conn.execute(
            "INSERT INTO request_content (request_id, ts, prompt, completion)
             VALUES (?1,?2,?3,?4)
             ON CONFLICT(request_id) DO UPDATE SET prompt = ?3, completion = ?4",
            params![request_id, ts, prompt, completion],
        )?;
        Ok(())
    }

    /// Peticiones de una aplicación dentro de una ventana. Reconstruye el
    /// contador de límites al arrancar.
    ///
    /// Solo cuenta inferencia: una consulta de catálogo no consume cuota del
    /// proveedor, así que no puede consumir el límite local.
    pub fn requests_in_window(
        &self,
        app_id: &str,
        provider_id: &str,
        credential_kind: &str,
        since_ms: i64,
    ) -> Result<i64> {
        let conn = self.lock();
        Ok(conn.query_row(
            "SELECT COUNT(*) FROM requests
             WHERE app_id = ?1 AND provider_id = ?2 AND credential_kind = ?3
               AND ts >= ?4 AND status != 'error' AND operation = 'chat'",
            params![app_id, provider_id, credential_kind, since_ms],
            |r| r.get(0),
        )?)
    }

    /// Resumen agregado para el panel.
    ///
    /// `operation` filtra por tipo: `Some("chat")` deja fuera las consultas de
    /// catálogo, que no consumen nada y falsearían los totales de uso.
    ///
    /// El rollup `usage_hourly` está redondeado a la hora, así que no puede
    /// responder con exactitud a una ventana más fina (un filtro de «1 hora»
    /// podría seguir enseñando algo de hace casi 2 horas si cae en el mismo
    /// cubo). Para ventanas de un día o menos se consulta `requests`
    /// directamente, que tiene el timestamp exacto de cada petición. Para
    /// ventanas más largas se sigue usando el rollup: es lo que permite que el
    /// histórico agregado sobreviva al borrado por retención del detalle (ver
    /// `apply_retention` y `retention_deletes_detail_but_keeps_aggregates`).
    pub fn usage_summary(
        &self,
        since_ms: i64,
        group: GroupBy,
        operation: Option<&str>,
    ) -> Result<Vec<UsageBucket>> {
        const SHORT_WINDOW_MS: i64 = 86_400_000;
        let conn = self.lock();
        let rows = if util::now_ms() - since_ms <= SHORT_WINDOW_MS {
            let column = group.raw_column();
            let sql = format!(
                "SELECT {column} AS bucket,
                        COUNT(*), SUM(status = 'error'), SUM(status = 'cancelled'),
                        SUM(CASE WHEN error_kind = 'rate_limited' THEN 1 ELSE 0 END),
                        SUM(CASE WHEN error_kind = 'local_limit' THEN 1 ELSE 0 END),
                        COALESCE(SUM(input_tokens), 0), COALESCE(SUM(output_tokens), 0),
                        COALESCE(SUM(total_tokens), 0),
                        SUM(CASE WHEN cost_basis = 'reported' THEN cost_micros ELSE 0 END),
                        SUM(CASE WHEN cost_basis = 'estimated' THEN cost_micros ELSE 0 END),
                        SUM(cost_basis = 'subscription'),
                        COALESCE(SUM(latency_ms), 0), COALESCE(MAX(latency_ms), 0),
                        COALESCE(SUM(ttft_ms), 0), SUM(ttft_ms IS NOT NULL)
                 FROM requests
                 WHERE ts >= ?1 AND (?2 IS NULL OR operation = ?2)
                 GROUP BY bucket
                 ORDER BY COUNT(*) DESC"
            );
            let mut stmt = conn.prepare(&sql)?;
            let out = stmt
                .query_map(params![since_ms, operation], usage_bucket_from_row)?
                .collect::<rusqlite::Result<Vec<_>>>()?;
            out
        } else {
            let column = group.column();
            let sql = format!(
                "SELECT {column} AS bucket,
                        SUM(requests), SUM(errors), SUM(cancels),
                        SUM(rate_limited), SUM(local_limited),
                        SUM(input_tokens), SUM(output_tokens), SUM(total_tokens),
                        SUM(cost_reported_micros), SUM(cost_estimated_micros),
                        SUM(subscription_requests),
                        SUM(latency_sum_ms), MAX(latency_max_ms),
                        SUM(ttft_sum_ms), SUM(ttft_count)
                 FROM usage_hourly
                 WHERE hour >= ?1 AND (?2 IS NULL OR operation = ?2)
                 GROUP BY bucket
                 ORDER BY SUM(requests) DESC"
            );
            let mut stmt = conn.prepare(&sql)?;
            let out = stmt
                .query_map(params![util::hour_floor_ms(since_ms), operation], usage_bucket_from_row)?
                .collect::<rusqlite::Result<Vec<_>>>()?;
            out
        };
        Ok(rows)
    }

    /// Últimas peticiones, para el diagnóstico. `since_ms = 0` no filtra por
    /// tiempo (todo `ts` real es positivo).
    pub fn recent_requests(&self, since_ms: i64, limit: i64) -> Result<Vec<RequestRow>> {
        let conn = self.lock();
        let mut stmt = conn.prepare(
            "SELECT r.id, r.ts, COALESCE(a.name, r.app_id), r.provider_id,
                    r.credential_kind, r.public_model, r.status, r.error_kind,
                    r.latency_ms, r.ttft_ms, r.input_tokens, r.output_tokens,
                    r.total_tokens, r.usage_source,
                    r.cost_micros, r.cost_basis, r.fallback_from, r.operation
             FROM requests r LEFT JOIN apps a ON a.id = r.app_id
             WHERE r.ts >= ?1
             ORDER BY r.ts DESC LIMIT ?2",
        )?;
        let rows = stmt.query_map(params![since_ms, limit], |r| {
            Ok(RequestRow {
                id: r.get(0)?,
                ts: r.get(1)?,
                app: r.get(2)?,
                provider_id: r.get(3)?,
                credential_kind: r.get(4)?,
                public_model: r.get(5)?,
                status: r.get(6)?,
                error_kind: r.get(7)?,
                latency_ms: r.get(8)?,
                ttft_ms: r.get(9)?,
                input_tokens: r.get(10)?,
                output_tokens: r.get(11)?,
                total_tokens: r.get(12)?,
                usage_source: r.get(13)?,
                cost_micros: r.get(14)?,
                cost_basis: r.get(15)?,
                fallback_from: r.get(16)?,
                operation: r.get(17)?,
            })
        })?;
        Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
    }

    /// Aplica la retención. Nunca borra `usage_hourly`: el histórico agregado
    /// sobrevive al borrado del detalle.
    pub fn apply_retention(&self, retention_days: i64, content_days: i64) -> Result<(usize, usize)> {
        let conn = self.lock();
        let now = util::now_ms();
        let content_cut = now - content_days * 86_400_000;
        let requests_cut = now - retention_days * 86_400_000;
        let content = conn.execute(
            "DELETE FROM request_content WHERE ts < ?1",
            params![content_cut],
        )?;
        let requests = conn.execute("DELETE FROM requests WHERE ts < ?1", params![requests_cut])?;
        Ok((requests, content))
    }

    pub fn purge_all_stats(&self) -> Result<()> {
        let conn = self.lock();
        conn.execute_batch(
            "DELETE FROM request_content; DELETE FROM requests; DELETE FROM usage_hourly;",
        )?;
        Ok(())
    }
}

#[derive(Debug, Clone, Copy)]
pub enum GroupBy {
    App,
    Provider,
    CredentialKind,
    Model,
    Hour,
}

impl GroupBy {
    fn column(&self) -> &'static str {
        match self {
            Self::App => "app_id",
            Self::Provider => "provider_id",
            Self::CredentialKind => "credential_kind",
            Self::Model => "public_model",
            Self::Hour => "CAST(hour AS TEXT)",
        }
    }

    /// Igual que `column()`, pero para agrupar directamente sobre `requests`,
    /// que no tiene una columna `hour` ya redondeada como `usage_hourly`.
    fn raw_column(&self) -> &'static str {
        match self {
            Self::Hour => "CAST(ts - (ts % 3600000) AS TEXT)",
            other => other.column(),
        }
    }

    pub fn parse(s: &str) -> Self {
        match s {
            "provider" => Self::Provider,
            "credential" => Self::CredentialKind,
            "model" => Self::Model,
            "hour" => Self::Hour,
            _ => Self::App,
        }
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct UsageBucket {
    pub bucket: String,
    pub requests: i64,
    pub errors: i64,
    pub cancels: i64,
    pub rate_limited: i64,
    pub local_limited: i64,
    pub input_tokens: i64,
    pub output_tokens: i64,
    pub total_tokens: i64,
    pub cost_reported_micros: i64,
    pub cost_estimated_micros: i64,
    pub subscription_requests: i64,
    pub avg_latency_ms: i64,
    pub max_latency_ms: i64,
    pub avg_ttft_ms: Option<i64>,
}

/// Comparte el mapeo de fila entre las dos consultas de `usage_summary`
/// (`requests` y `usage_hourly`): ambas producen las columnas en el mismo
/// orden, solo cambia de dónde salen.
fn usage_bucket_from_row(r: &rusqlite::Row<'_>) -> rusqlite::Result<UsageBucket> {
    let requests: i64 = r.get(1)?;
    let latency_sum: i64 = r.get(12)?;
    let ttft_sum: i64 = r.get(14)?;
    let ttft_count: i64 = r.get(15)?;
    Ok(UsageBucket {
        bucket: r.get(0)?,
        requests,
        errors: r.get(2)?,
        cancels: r.get(3)?,
        rate_limited: r.get(4)?,
        local_limited: r.get(5)?,
        input_tokens: r.get(6)?,
        output_tokens: r.get(7)?,
        total_tokens: r.get(8)?,
        cost_reported_micros: r.get(9)?,
        cost_estimated_micros: r.get(10)?,
        subscription_requests: r.get(11)?,
        avg_latency_ms: if requests > 0 { latency_sum / requests } else { 0 },
        max_latency_ms: r.get(13)?,
        avg_ttft_ms: if ttft_count > 0 { Some(ttft_sum / ttft_count) } else { None },
    })
}

#[derive(Debug, Clone, Serialize)]
pub struct RequestRow {
    pub id: String,
    pub ts: i64,
    pub app: String,
    pub provider_id: String,
    pub credential_kind: String,
    pub public_model: String,
    pub status: String,
    pub error_kind: Option<String>,
    pub latency_ms: Option<i64>,
    pub ttft_ms: Option<i64>,
    pub input_tokens: Option<i64>,
    pub output_tokens: Option<i64>,
    pub total_tokens: Option<i64>,
    pub usage_source: String,
    pub cost_micros: Option<i64>,
    pub cost_basis: String,
    pub fallback_from: Option<String>,
    pub operation: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn db() -> Db {
        Db::open_in_memory().unwrap()
    }

    fn event(kind: &str, basis: CostBasis, cost: Option<i64>) -> RequestEvent {
        RequestEvent {
            id: util::new_id("req"),
            ts: util::now_ms(),
            app_id: "app1".into(),
            provider_id: "openai".into(),
            credential_kind: kind.into(),
            account_id: Some("acc1".into()),
            public_model: "openai/gpt-5.5".into(),
            api_model: "gpt-5.5".into(),
            operation: "chat".into(),
            streamed: true,
            status: RequestStatus::Ok,
            error_kind: None,
            http_status: Some(200),
            latency_ms: Some(500),
            ttft_ms: Some(120),
            input_tokens: Some(10),
            output_tokens: Some(20),
            cached_input_tokens: None,
            reasoning_tokens: None,
            usage_source: UsageSource::Reported,
            cost_micros: cost,
            cost_basis: basis,
            fallback_from: None,
            provider_usage_raw: Some(serde_json::json!({"input_tokens": 10})),
            provider_request_id: Some("resp_1".into()),
        }
    }

    #[test]
    fn records_event_and_rollup_together() {
        let db = db();
        db.record_request(&event("api_key", CostBasis::Estimated, Some(1000)))
            .unwrap();
        let summary = db.usage_summary(0, GroupBy::App, None).unwrap();
        assert_eq!(summary.len(), 1);
        assert_eq!(summary[0].requests, 1);
        assert_eq!(summary[0].total_tokens, 30);
        assert_eq!(summary[0].avg_latency_ms, 500);
        assert_eq!(summary[0].avg_ttft_ms, Some(120));
    }

    #[test]
    fn estimated_and_reported_costs_never_mix() {
        let db = db();
        db.record_request(&event("api_key", CostBasis::Estimated, Some(1000)))
            .unwrap();
        db.record_request(&event("api_key", CostBasis::Reported, Some(500)))
            .unwrap();
        let s = &db.usage_summary(0, GroupBy::App, None).unwrap()[0];
        assert_eq!(s.cost_estimated_micros, 1000);
        assert_eq!(s.cost_reported_micros, 500);
    }

    #[test]
    fn subscription_requests_are_counted_apart_and_add_no_cost() {
        let db = db();
        db.record_request(&event("subscription_oauth", CostBasis::Subscription, None))
            .unwrap();
        let s = &db.usage_summary(0, GroupBy::App, None).unwrap()[0];
        assert_eq!(s.subscription_requests, 1);
        assert_eq!(s.cost_reported_micros, 0);
        assert_eq!(s.cost_estimated_micros, 0);
    }

    #[test]
    fn raw_provider_usage_is_preserved() {
        let db = db();
        let e = event("api_key", CostBasis::Estimated, Some(1));
        db.record_request(&e).unwrap();
        let conn = db.lock();
        let raw: String = conn
            .query_row(
                "SELECT provider_usage_raw FROM requests WHERE id = ?1",
                params![e.id],
                |r| r.get(0),
            )
            .unwrap();
        assert!(raw.contains("input_tokens"));
    }

    #[test]
    fn local_limit_and_rate_limit_are_counted_separately() {
        let db = db();
        let mut a = event("subscription_oauth", CostBasis::Subscription, None);
        a.status = RequestStatus::Error;
        a.error_kind = Some("local_limit".into());
        let mut b = event("subscription_oauth", CostBasis::Subscription, None);
        b.status = RequestStatus::Error;
        b.error_kind = Some("rate_limited".into());
        db.record_request(&a).unwrap();
        db.record_request(&b).unwrap();

        let s = &db.usage_summary(0, GroupBy::App, None).unwrap()[0];
        assert_eq!(s.local_limited, 1);
        assert_eq!(s.rate_limited, 1);
        assert_eq!(s.errors, 2);
    }

    #[test]
    fn window_count_excludes_errors() {
        let db = db();
        db.record_request(&event("subscription_oauth", CostBasis::Subscription, None))
            .unwrap();
        let mut failed = event("subscription_oauth", CostBasis::Subscription, None);
        failed.status = RequestStatus::Error;
        failed.error_kind = Some("upstream".into());
        db.record_request(&failed).unwrap();

        let n = db
            .requests_in_window("app1", "openai", "subscription_oauth", 0)
            .unwrap();
        assert_eq!(n, 1, "un error del proveedor no debe consumir cuota local");
    }

    #[test]
    fn grouping_by_credential_kind_separates_routes() {
        let db = db();
        db.record_request(&event("api_key", CostBasis::Estimated, Some(10)))
            .unwrap();
        db.record_request(&event("subscription_oauth", CostBasis::Subscription, None))
            .unwrap();
        let s = db.usage_summary(0, GroupBy::CredentialKind, None).unwrap();
        assert_eq!(s.len(), 2);
    }

    #[test]
    fn retention_deletes_detail_but_keeps_aggregates() {
        let db = db();
        let mut old = event("api_key", CostBasis::Estimated, Some(10));
        old.ts = util::now_ms() - 200 * 86_400_000;
        db.record_request(&old).unwrap();
        db.record_content(&old.id, old.ts, "p", "c").unwrap();

        let before = db.usage_summary(old.ts - 1000, GroupBy::App, None).unwrap();
        assert_eq!(before[0].requests, 1);

        let (requests, content) = db.apply_retention(90, 7).unwrap();
        assert_eq!((requests, content), (1, 1));

        let after = db.usage_summary(old.ts - 1000, GroupBy::App, None).unwrap();
        assert_eq!(
            after[0].requests, 1,
            "el rollup debe sobrevivir al borrado del detalle"
        );
        assert_eq!(db.recent_requests(0, 10).unwrap().len(), 0);
    }

    #[test]
    fn purge_removes_everything_including_aggregates() {
        let db = db();
        db.record_request(&event("api_key", CostBasis::Estimated, Some(10)))
            .unwrap();
        db.purge_all_stats().unwrap();
        assert!(db.usage_summary(0, GroupBy::App, None).unwrap().is_empty());
    }

    #[test]
    fn usage_summary_excludes_a_row_from_two_hours_ago_in_a_one_hour_window() {
        let db = db();
        let mut old = event("api_key", CostBasis::Estimated, Some(10));
        old.ts = util::now_ms() - 2 * 3_600_000;
        db.record_request(&old).unwrap();
        db.record_request(&event("api_key", CostBasis::Estimated, Some(10)))
            .unwrap();

        let since = util::now_ms() - 3_600_000;
        let s = db.usage_summary(since, GroupBy::App, None).unwrap();
        assert_eq!(
            s[0].requests, 1,
            "la petición de hace 2 horas no debe contar en una ventana de 1 hora"
        );
    }

    #[test]
    fn recent_requests_excludes_rows_older_than_the_window() {
        let db = db();
        let mut old = event("api_key", CostBasis::Estimated, Some(10));
        old.ts = util::now_ms() - 2 * 3_600_000; // hace 2 horas
        db.record_request(&old).unwrap();

        let recent = event("api_key", CostBasis::Estimated, Some(10));
        db.record_request(&recent).unwrap();

        let since = util::now_ms() - 3_600_000; // ventana de 1 hora
        let rows = db.recent_requests(since, 10).unwrap();
        assert!(rows.iter().any(|r| r.id == recent.id));
        assert!(
            !rows.iter().any(|r| r.id == old.id),
            "una fila de hace 2 horas no debe aparecer en una ventana de 1 hora"
        );
    }

    #[test]
    fn recent_requests_exposes_input_and_output_tokens_separately() {
        let db = db();
        let mut e = event("api_key", CostBasis::Estimated, Some(10));
        e.input_tokens = Some(10);
        e.output_tokens = Some(20);
        db.record_request(&e).unwrap();

        let row = &db.recent_requests(0, 10).unwrap()[0];
        assert_eq!(row.input_tokens, Some(10));
        assert_eq!(row.output_tokens, Some(20));
        assert_eq!(row.total_tokens, Some(30));
    }
}
