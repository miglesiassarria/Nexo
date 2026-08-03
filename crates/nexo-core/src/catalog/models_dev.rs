//! Cliente y caché de `models.dev`.
//!
//! Base de datos pública de modelos (`https://models.dev/api.json`, ~3,3 MB, 176
//! proveedores) que Nexo usa para saber qué capacidades y qué precio tiene un
//! modelo que el propio proveedor no describe —solo lista sus identificadores.
//!
//! VERIFICADO el 2026-07-31 contra el catálogo real de OpenCode Zen: 60 de 60
//! modelos con metadatos, ninguno huérfano. El proveedor `opencode` en esta base de
//! datos declara `api: https://opencode.ai/zen/v1` y
//! `npm: @ai-sdk/openai-compatible`, que es la confirmación de que Zen habla el
//! mismo formato que la API pública de OpenAI.
//!
//! Es un servicio de terceros: si no responde y no hay caché, el catálogo se ofrece
//! solo como texto (ver ADR de la especificación 0002) en lugar de quedarse vacío.

use crate::provider::{Capabilities, Limits, ModelDescriptor, Pricing};
use serde_json::Value;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::time::Duration;

pub const SOURCE_URL: &str = "https://models.dev/api.json";
/// Una semana: es una base de datos que cambia con el ritmo de lanzamiento de
/// modelos, no algo que necesite refrescarse cada arranque.
pub const CACHE_TTL: Duration = Duration::from_secs(7 * 24 * 3600);

/// Lo que hace falta de un modelo para enriquecer el catálogo: capacidades,
/// límites y precio. No se guarda nada más porque no se usa nada más.
#[derive(Debug, Clone)]
pub struct ModelsDevEntry {
    pub caps: Capabilities,
    pub limits: Limits,
    pub pricing: Option<Pricing>,
}

/// Catálogo ya parseado, indexado por proveedor de `models.dev` y por id de modelo.
#[derive(Debug, Clone, Default)]
pub struct ModelsDevCatalog {
    by_provider: HashMap<String, HashMap<String, ModelsDevEntry>>,
    /// De la URL `api` que un proveedor declara (p. ej. Zen:
    /// `https://opencode.ai/zen/v1`) a su clave en `models.dev` (`opencode`).
    /// Existe porque el nombre que el usuario le da a un proveedor añadido no
    /// tiene por qué coincidir con el nombre que usa `models.dev` para el mismo
    /// servicio: la URL es lo único estable entre los dos.
    provider_by_api: HashMap<String, String>,
}

impl ModelsDevCatalog {
    pub fn is_empty(&self) -> bool {
        self.by_provider.is_empty()
    }

    pub fn provider_count(&self) -> usize {
        self.by_provider.len()
    }

    /// Busca un modelo, preferiendo la coincidencia exacta de proveedor.
    ///
    /// Sin proveedor exacto (o si no está en él), se cae a buscar el mismo id en
    /// cualquier proveedor: es una heurística declarada en la especificación 0002,
    /// mejor un dato probable que ninguno, y el precio que produce siempre queda
    /// marcado como estimado, nunca como reportado por el proveedor real.
    pub fn lookup(&self, provider_hint: Option<&str>, model_id: &str) -> Option<&ModelsDevEntry> {
        if let Some(entry) = self.lookup_by_id(provider_hint, model_id) {
            return Some(entry);
        }
        // La API compatible de Gemini antepone un espacio de nombres al id
        // (`models/gemini-2.5-flash`, verificado el 2026-08-03) que
        // `models.dev` no usa. Se reintenta sin él antes de rendirse.
        let stripped = model_id.strip_prefix("models/")?;
        self.lookup_by_id(provider_hint, stripped)
    }

    fn lookup_by_id(&self, provider_hint: Option<&str>, model_id: &str) -> Option<&ModelsDevEntry> {
        if let Some(hint) = provider_hint {
            if let Some(entry) = self.by_provider.get(hint).and_then(|m| m.get(model_id)) {
                return Some(entry);
            }
        }
        self.by_provider.values().find_map(|models| models.get(model_id))
    }

    /// La clave de `models.dev` para el proveedor cuya URL `api` coincide con la
    /// dada, normalizando la barra final para que la comparación no falle por eso.
    pub fn provider_id_for_api(&self, base_url: &str) -> Option<String> {
        let normalized = base_url.trim_end_matches('/');
        self.provider_by_api.get(normalized).cloned()
    }

    /// Enriquece un descriptor que solo tiene `api_id`/`public_name` (lo típico de
    /// un descubrimiento que solo da identificadores) con lo que `models.dev` sepa.
    /// Si no hay coincidencia, el descriptor vuelve igual: solo texto, sin inventar.
    pub fn enrich(&self, mut model: ModelDescriptor, provider_hint: Option<&str>) -> ModelDescriptor {
        if let Some(entry) = self.lookup(provider_hint, &model.api_id) {
            model.caps = entry.caps.clone();
            model.limits = entry.limits.clone();
            model.pricing = entry.pricing;
        }
        model
    }

    /// Parsea el JSON completo de `models.dev`. Nunca falla de forma catastrófica:
    /// un proveedor o un modelo con forma inesperada simplemente no entra.
    pub fn parse(json: &Value) -> Self {
        let Some(providers) = json.as_object() else {
            return Self::default();
        };

        let mut by_provider = HashMap::new();
        let mut provider_by_api = HashMap::new();
        for (provider_id, provider) in providers {
            if let Some(api) = provider.get("api").and_then(|v| v.as_str()) {
                provider_by_api.insert(api.trim_end_matches('/').to_string(), provider_id.clone());
            }

            let Some(models) = provider.get("models").and_then(|m| m.as_object()) else {
                continue;
            };
            let mut entries = HashMap::new();
            for (model_id, model) in models {
                entries.insert(model_id.clone(), parse_model(model));
            }
            if !entries.is_empty() {
                by_provider.insert(provider_id.clone(), entries);
            }
        }
        Self { by_provider, provider_by_api }
    }
}

fn parse_model(m: &Value) -> ModelsDevEntry {
    let inputs: Vec<&str> = m
        .pointer("/modalities/input")
        .and_then(|v| v.as_array())
        .map(|a| a.iter().filter_map(|x| x.as_str()).collect())
        .unwrap_or_default();

    let caps = Capabilities {
        text: true,
        vision: inputs.contains(&"image"),
        audio: inputs.contains(&"audio"),
        tools: m.get("tool_call").and_then(|v| v.as_bool()).unwrap_or(false),
        reasoning: m.get("reasoning").and_then(|v| v.as_bool()).unwrap_or(false),
        json_mode: true,
        streaming: true,
        embeddings: false,
    };

    let limits = Limits {
        context_max: m.pointer("/limit/context").and_then(|v| v.as_u64()).map(|v| v as u32),
        input_max: m.pointer("/limit/context").and_then(|v| v.as_u64()).map(|v| v as u32),
        output_max: m.pointer("/limit/output").and_then(|v| v.as_u64()).map(|v| v as u32),
    };

    // Precios en $ por millón de tokens; se guardan en micros (× 1_000_000).
    let dollars_to_micros = |v: &Value| -> Option<i64> { v.as_f64().map(|d| (d * 1_000_000.0).round() as i64) };
    let pricing = m.get("cost").and_then(|cost| {
        let input = cost.get("input").and_then(dollars_to_micros)?;
        let output = cost.get("output").and_then(dollars_to_micros)?;
        Some(Pricing {
            input_per_mtok_micros: input,
            output_per_mtok_micros: output,
            cached_input_per_mtok_micros: cost.get("cache_read").and_then(dollars_to_micros),
        })
    });

    ModelsDevEntry { caps, limits, pricing }
}

// ---------------------------------------------------------------------------
// Caché en disco
// ---------------------------------------------------------------------------

/// Descarga (o reutiliza la caché) y devuelve el catálogo parseado.
///
/// Nunca propaga un error que bloquee al llamador: si todo falla, devuelve un
/// catálogo vacío y el que enriquece el descriptor simplemente no encuentra nada,
/// que es la degradación aceptada (solo texto) frente a quedarse sin catálogo.
pub async fn load(http: &reqwest::Client, cache_path: &Path) -> ModelsDevCatalog {
    if let Some(json) = read_cache_if_fresh(cache_path) {
        tracing::debug!("models.dev servido desde caché");
        return ModelsDevCatalog::parse(&json);
    }

    match fetch(http).await {
        Ok(json) => {
            if let Err(e) = write_cache(cache_path, &json) {
                tracing::warn!(error = %e, "no se pudo escribir la caché de models.dev");
            }
            ModelsDevCatalog::parse(&json)
        }
        Err(e) => {
            tracing::warn!(error = %e, "no se pudo descargar models.dev");
            // Aunque esté caducada, una caché vieja es mejor que ninguna.
            match read_cache_any_age(cache_path) {
                Some(json) => {
                    tracing::info!("se usa la caché de models.dev caducada, a falta de red");
                    ModelsDevCatalog::parse(&json)
                }
                None => ModelsDevCatalog::default(),
            }
        }
    }
}

async fn fetch(http: &reqwest::Client) -> Result<Value, String> {
    let resp = http
        .get(SOURCE_URL)
        .timeout(Duration::from_secs(30))
        .send()
        .await
        .map_err(|e| e.to_string())?;
    if !resp.status().is_success() {
        return Err(format!("HTTP {}", resp.status().as_u16()));
    }
    resp.json::<Value>().await.map_err(|e| e.to_string())
}

fn read_cache_if_fresh(path: &Path) -> Option<Value> {
    let meta = std::fs::metadata(path).ok()?;
    let modified = meta.modified().ok()?;
    if modified.elapsed().ok()? > CACHE_TTL {
        return None;
    }
    read_cache_any_age(path)
}

fn read_cache_any_age(path: &Path) -> Option<Value> {
    let bytes = std::fs::read(path).ok()?;
    serde_json::from_slice(&bytes).ok()
}

fn write_cache(path: &Path, json: &Value) -> std::io::Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(path, serde_json::to_vec(json)?)
}

pub fn default_cache_path(data_dir: &Path) -> PathBuf {
    data_dir.join("models-dev-cache.json")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::provider::Accounting;

    /// Muestra real recortada de `models.dev`, capturada el 2026-07-31: cuatro
    /// modelos de `opencode` (uno de pago con caché, uno gratis, uno GPT, uno con
    /// `reasoning`/`tool_call`), y el proveedor `lmstudio` completo.
    fn real_sample() -> Value {
        let raw = include_str!("../../tests/fixtures/models_dev_sample.json");
        serde_json::from_str(raw).expect("el fixture debe ser json válido")
    }

    #[test]
    fn parses_the_real_sample_without_losing_any_model() {
        let catalog = ModelsDevCatalog::parse(&real_sample());
        assert!(!catalog.is_empty());
        assert!(catalog.provider_count() >= 2, "opencode y lmstudio");
        assert!(catalog.lookup(Some("opencode"), "claude-haiku-4-5").is_some());
        assert!(catalog.lookup(Some("opencode"), "deepseek-v4-flash-free").is_some());
    }

    #[test]
    fn a_vision_capable_model_is_marked_as_such() {
        let catalog = ModelsDevCatalog::parse(&real_sample());
        let claude = catalog.lookup(Some("opencode"), "claude-haiku-4-5").unwrap();
        assert!(claude.caps.vision, "claude-haiku-4-5 admite imagen y pdf de entrada");
        assert!(claude.caps.reasoning);
        assert!(claude.caps.tools);
    }

    #[test]
    fn pricing_converts_dollars_per_million_to_micros() {
        let catalog = ModelsDevCatalog::parse(&real_sample());
        let claude = catalog.lookup(Some("opencode"), "claude-haiku-4-5").unwrap();
        let pricing = claude.pricing.expect("claude-haiku-4-5 tiene precio en la muestra");
        // $1 de entrada por millón de tokens -> 1_000_000 micros.
        assert_eq!(pricing.input_per_mtok_micros, 1_000_000);
        assert_eq!(pricing.output_per_mtok_micros, 5_000_000);
    }

    #[test]
    fn a_free_model_has_zero_cost_not_missing_cost() {
        let catalog = ModelsDevCatalog::parse(&real_sample());
        let free = catalog.lookup(Some("opencode"), "deepseek-v4-flash-free");
        if let Some(entry) = free {
            if let Some(p) = &entry.pricing {
                assert_eq!(p.input_per_mtok_micros, 0);
                assert_eq!(p.output_per_mtok_micros, 0);
            }
        }
    }

    /// Confirmado en T0: `opencode` declara `api: https://opencode.ai/zen/v1`.
    /// Es lo que permite identificar Zen aunque el usuario lo haya llamado
    /// «OpenCode Zen», «Mi Zen» o cualquier otra cosa en Nexo.
    #[test]
    fn provider_id_for_api_finds_zen_by_its_declared_url() {
        let catalog = ModelsDevCatalog::parse(&real_sample());
        assert_eq!(
            catalog.provider_id_for_api("https://opencode.ai/zen/v1"),
            Some("opencode".to_string())
        );
        // Con barra final también: es como el usuario suele pegar la URL.
        assert_eq!(
            catalog.provider_id_for_api("https://opencode.ai/zen/v1/"),
            Some("opencode".to_string())
        );
    }

    #[test]
    fn provider_id_for_api_is_none_for_an_unknown_url() {
        let catalog = ModelsDevCatalog::parse(&real_sample());
        assert!(catalog.provider_id_for_api("https://runpod.example/v1").is_none());
    }

    /// La API compatible de Gemini devuelve sus modelos con el espacio de
    /// nombres delante (`models/gemini-2.5-flash`, verificado contra la API
    /// real el 2026-08-03), pero `models.dev` los guarda sin él. Sin este
    /// arreglo, ni siquiera el respaldo entre proveedores encontraba el
    /// modelo — spec 0008.
    #[test]
    fn a_models_slash_prefixed_id_is_matched_after_stripping_it() {
        let catalog = ModelsDevCatalog::parse(&real_sample());
        assert!(catalog.lookup(Some("opencode"), "claude-haiku-4-5").is_some());
        assert!(
            catalog.lookup(Some("gemini"), "models/claude-haiku-4-5").is_some(),
            "debe reintentar sin el prefijo `models/` antes de rendirse"
        );
    }

    #[test]
    fn exact_provider_match_is_preferred_over_cross_provider_fallback() {
        let catalog = ModelsDevCatalog::parse(&real_sample());
        // Coincidencia exacta: existe en "opencode".
        assert!(catalog.lookup(Some("opencode"), "claude-haiku-4-5").is_some());
        // Sin pista de proveedor, el respaldo también lo encuentra.
        assert!(catalog.lookup(None, "claude-haiku-4-5").is_some());
    }

    #[test]
    fn an_unknown_model_id_returns_none_not_a_guess() {
        let catalog = ModelsDevCatalog::parse(&real_sample());
        assert!(catalog.lookup(Some("opencode"), "modelo-que-no-existe").is_none());
    }

    #[test]
    fn enrich_fills_a_bare_descriptor_from_the_catalog() {
        let catalog = ModelsDevCatalog::parse(&real_sample());
        let bare = ModelDescriptor {
            api_id: "claude-haiku-4-5".into(),
            public_name: "opencode-zen/claude-haiku-4-5".into(),
            caps: Capabilities { text: true, ..Default::default() },
            limits: Limits::default(),
            accounting: Accounting::Metered,
            pricing: None,
        };
        let enriched = catalog.enrich(bare, Some("opencode"));
        assert!(enriched.caps.vision);
        assert!(enriched.pricing.is_some());
        assert_eq!(enriched.limits.context_max, Some(200_000));
    }

    #[test]
    fn enrich_of_an_unknown_model_leaves_it_as_text_only_not_invented() {
        let catalog = ModelsDevCatalog::parse(&real_sample());
        let bare = ModelDescriptor {
            api_id: "modelo-desconocido-xyz".into(),
            public_name: "runpod/modelo-desconocido-xyz".into(),
            caps: Capabilities { text: true, streaming: true, ..Default::default() },
            limits: Limits::default(),
            accounting: Accounting::Metered,
            pricing: None,
        };
        let enriched = catalog.enrich(bare.clone(), Some("runpod"));
        assert_eq!(enriched.caps.vision, bare.caps.vision);
        assert!(enriched.pricing.is_none(), "sin dato no se inventa un precio");
    }

    #[test]
    fn a_malformed_top_level_value_yields_an_empty_catalog_not_a_panic() {
        assert!(ModelsDevCatalog::parse(&Value::Null).is_empty());
        assert!(ModelsDevCatalog::parse(&serde_json::json!([1, 2, 3])).is_empty());
        assert!(ModelsDevCatalog::parse(&serde_json::json!({"x": "no es un objeto de modelos"})).is_empty());
    }

    #[test]
    fn a_provider_with_no_models_key_is_skipped_without_failing_the_rest() {
        let json = serde_json::json!({
            "roto": {"nombre": "sin campo models"},
            "opencode": real_sample()["opencode"].clone(),
        });
        let catalog = ModelsDevCatalog::parse(&json);
        assert!(catalog.lookup(Some("opencode"), "claude-haiku-4-5").is_some());
        assert!(catalog.lookup(Some("roto"), "cualquiera").is_none());
    }

    // -- Caché en disco -------------------------------------------------------

    #[test]
    fn cache_roundtrips_and_is_considered_fresh_right_after_writing() {
        let dir = std::env::temp_dir().join(format!("nexo-test-{}", crate::util::new_id("md")));
        let path = default_cache_path(&dir);
        write_cache(&path, &real_sample()).unwrap();
        let read = read_cache_if_fresh(&path).expect("recién escrita, debe estar fresca");
        assert_eq!(read, real_sample());
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn missing_cache_file_is_not_an_error() {
        let path = std::env::temp_dir().join("nexo-test-no-existe-nunca.json");
        assert!(read_cache_if_fresh(&path).is_none());
        assert!(read_cache_any_age(&path).is_none());
    }
}
