//! Manifiesto versionado de modelos.
//!
//! Las capacidades de un modelo NO son descubribles mediante las APIs de los
//! proveedores: solo se puede consultar qué modelos existen, no qué hacen. Por
//! eso viven aquí, versionadas, y se cruzan con lo que el proveedor anuncie.
//!
//! Precedencia: anulaciones del usuario > este manifiesto > endpoint de modelos.

use crate::provider::{Accounting, Capabilities, Limits, ModelDescriptor, Pricing};

pub const MANIFEST_VERSION: &str = "2026-07-30";

fn caps_full() -> Capabilities {
    Capabilities {
        text: true,
        vision: true,
        audio: false,
        tools: true,
        reasoning: true,
        json_mode: true,
        streaming: true,
        embeddings: false,
    }
}

/// Modelos accesibles por la vía de suscripción de ChatGPT.
///
/// El catálogo de esta vía es un SUBCONJUNTO del de la API pública y con
/// capacidades recortadas: no hay precios porque no hay coste marginal, y los
/// modos de razonamiento más costosos quedan fuera.
///
/// ÚLTIMA VERIFICACIÓN: 2026-07-30. Debe confirmarse con el spike de OAuth,
/// porque esta lista solo se puede comprobar ejerciendo el flujo real.
pub fn chatgpt_subscription_models() -> Vec<ModelDescriptor> {
    ["gpt-5.5", "gpt-5.4", "gpt-5.4-mini"]
        .into_iter()
        .map(|api_id| ModelDescriptor {
            api_id: api_id.to_string(),
            public_name: format!("openai/{api_id}"),
            caps: caps_full(),
            limits: limits_for(api_id),
            accounting: Accounting::Subscription,
            // Sin precio: el coste marginal es cero y la cuota es desconocida.
            pricing: None,
        })
        .collect()
}

/// Modelos accesibles por API key contra `api.openai.com`.
pub fn openai_apikey_models() -> Vec<ModelDescriptor> {
    [
        ("gpt-5.5", 1_250_000i64, 10_000_000i64, Some(125_000i64)),
        ("gpt-5.4", 1_000_000, 8_000_000, Some(100_000)),
        ("gpt-5.4-mini", 250_000, 2_000_000, Some(25_000)),
    ]
    .into_iter()
    .map(|(api_id, input, output, cached)| ModelDescriptor {
        api_id: api_id.to_string(),
        public_name: format!("openai/{api_id}"),
        caps: caps_full(),
        limits: limits_for(api_id),
        accounting: Accounting::Metered,
        pricing: Some(Pricing {
            input_per_mtok_micros: input,
            output_per_mtok_micros: output,
            cached_input_per_mtok_micros: cached,
        }),
    })
    .collect()
}

fn limits_for(api_id: &str) -> Limits {
    match api_id {
        "gpt-5.5" => Limits {
            context_max: Some(400_000),
            input_max: Some(272_000),
            output_max: Some(128_000),
        },
        _ => Limits {
            context_max: Some(272_000),
            input_max: Some(200_000),
            output_max: Some(64_000),
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn subscription_models_never_carry_prices() {
        for m in chatgpt_subscription_models() {
            assert!(
                m.pricing.is_none(),
                "{} no debe tener precio: el coste marginal es cero",
                m.public_name
            );
            assert_eq!(m.accounting, Accounting::Subscription);
        }
    }

    #[test]
    fn api_key_models_are_metered_and_priced() {
        for m in openai_apikey_models() {
            assert_eq!(m.accounting, Accounting::Metered);
            assert!(m.pricing.is_some(), "{} debe tener precio", m.public_name);
        }
    }

    #[test]
    fn subscription_catalog_is_a_subset_of_api_key_catalog() {
        let api: Vec<String> = openai_apikey_models()
            .into_iter()
            .map(|m| m.api_id)
            .collect();
        for m in chatgpt_subscription_models() {
            assert!(
                api.contains(&m.api_id),
                "{} aparece por suscripción pero no por API key",
                m.api_id
            );
        }
    }

    #[test]
    fn public_names_always_carry_the_provider() {
        for m in openai_apikey_models()
            .into_iter()
            .chain(chatgpt_subscription_models())
        {
            assert!(
                m.public_name.contains('/'),
                "{} debe llevar el proveedor delante",
                m.public_name
            );
        }
    }
}
