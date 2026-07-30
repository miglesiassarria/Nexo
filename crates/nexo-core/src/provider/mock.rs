//! Proveedor de pruebas. No sale de la máquina y no necesita credenciales.
//!
//! Existe para validar el gateway, el streaming, las políticas y las
//! estadísticas de extremo a extremo sin gastar cuota ni depender de una ruta
//! frágil. Es también el sujeto de las pruebas de contrato.

use crate::provider::{
    check_capabilities, Accounting, AdapterError, AdapterId, Capabilities, ChatEvent, ChatRequest,
    CredentialKind, EventStream, Health, Limits, ModelDescriptor, ProviderAdapter,
    ResolvedCredential, UsageReport, UsageSource,
};
use async_trait::async_trait;
use std::time::Duration;

pub const PROVIDER: &str = "mock";
pub const MODEL: &str = "mock-echo";

pub struct MockAdapter {
    delay: Duration,
}

impl Default for MockAdapter {
    fn default() -> Self {
        Self { delay: Duration::from_millis(15) }
    }
}

impl MockAdapter {
    pub fn new(delay: Duration) -> Self {
        Self { delay }
    }

    pub fn descriptor() -> ModelDescriptor {
        ModelDescriptor {
            api_id: MODEL.to_string(),
            public_name: format!("{PROVIDER}/{MODEL}"),
            caps: Capabilities {
                text: true,
                tools: false,
                streaming: true,
                json_mode: true,
                ..Default::default()
            },
            limits: Limits {
                context_max: Some(8192),
                input_max: Some(8192),
                output_max: Some(2048),
            },
            accounting: Accounting::Local,
            pricing: None,
        }
    }
}

#[async_trait]
impl ProviderAdapter for MockAdapter {
    fn id(&self) -> AdapterId {
        AdapterId::new(PROVIDER, CredentialKind::Mock)
    }

    async fn catalog(
        &self,
        _cred: &ResolvedCredential,
    ) -> Result<Vec<ModelDescriptor>, AdapterError> {
        Ok(vec![Self::descriptor()])
    }

    async fn stream(
        &self,
        req: &ChatRequest,
        _cred: &ResolvedCredential,
    ) -> Result<EventStream, AdapterError> {
        check_capabilities(req, &Self::descriptor())?;

        let prompt = req
            .messages
            .last()
            .map(|m| m.text())
            .unwrap_or_else(|| "(sin mensaje)".into());
        let reply = format!("eco: {prompt}");
        let words: Vec<String> = reply
            .split_inclusive(' ')
            .map(|s| s.to_string())
            .collect();

        let input_tokens = prompt.split_whitespace().count() as u32;
        let output_tokens = words.len() as u32;
        let delay = self.delay;

        let stream = futures::stream::unfold(
            (0usize, words, false),
            move |(index, words, usage_sent)| async move {
                if index == 0 {
                    return Some((
                        Ok(ChatEvent::Started { provider_request_id: None }),
                        (1, words, usage_sent),
                    ));
                }
                let word_index = index - 1;
                if word_index < words.len() {
                    if !delay.is_zero() {
                        tokio::time::sleep(delay).await;
                    }
                    let text = words[word_index].clone();
                    return Some((
                        Ok(ChatEvent::TextDelta { text }),
                        (index + 1, words, usage_sent),
                    ));
                }
                if !usage_sent {
                    return Some((
                        Ok(ChatEvent::Usage(UsageReport {
                            input_tokens: Some(input_tokens),
                            output_tokens: Some(output_tokens),
                            source: UsageSource::Estimated,
                            ..Default::default()
                        })),
                        (index, words, true),
                    ));
                }
                None
            },
        );

        let stream = futures::StreamExt::chain(
            stream,
            futures::stream::once(async {
                Ok(ChatEvent::Finished { reason: crate::provider::FinishReason::Stop })
            }),
        );

        Ok(Box::pin(stream))
    }

    async fn health(&self, _cred: &ResolvedCredential) -> Health {
        Health::Ok
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::provider::{ContentPart, FinishReason, Message, Role, ToolChoice, ToolDef};
    use futures::StreamExt;

    fn cred() -> ResolvedCredential {
        ResolvedCredential {
            account_id: "mock".into(),
            kind: CredentialKind::Mock,
            secret: String::new(),
            external_id: None,
        }
    }

    fn req(text: &str) -> ChatRequest {
        ChatRequest {
            api_model: MODEL.into(),
            public_model: format!("{PROVIDER}/{MODEL}"),
            messages: vec![Message {
                role: Role::User,
                parts: vec![ContentPart::Text(text.into())],
                tool_call_id: None,
                tool_calls: vec![],
            }],
            tools: vec![],
            tool_choice: ToolChoice::Auto,
            reasoning: None,
            max_output_tokens: None,
            temperature: None,
            top_p: None,
            stop: vec![],
            json_mode: false,
            stream: true,
        }
    }

    async fn collect(req: ChatRequest) -> Vec<ChatEvent> {
        let adapter = MockAdapter::new(Duration::ZERO);
        adapter
            .stream(&req, &cred())
            .await
            .expect("stream")
            .map(|r| r.expect("evento"))
            .collect()
            .await
    }

    #[tokio::test]
    async fn started_comes_first_and_finished_exactly_once() {
        let events = collect(req("hola mundo")).await;
        assert!(matches!(events.first(), Some(ChatEvent::Started { .. })));
        let finished = events
            .iter()
            .filter(|e| matches!(e, ChatEvent::Finished { .. }))
            .count();
        assert_eq!(finished, 1);
        assert!(matches!(
            events.last(),
            Some(ChatEvent::Finished { reason: FinishReason::Stop })
        ));
    }

    #[tokio::test]
    async fn concatenated_text_echoes_the_prompt() {
        let events = collect(req("hola mundo")).await;
        let text: String = events
            .iter()
            .filter_map(|e| match e {
                ChatEvent::TextDelta { text } => Some(text.as_str()),
                _ => None,
            })
            .collect();
        assert_eq!(text, "eco: hola mundo");
    }

    #[tokio::test]
    async fn usage_is_estimated_not_reported() {
        let events = collect(req("uno dos tres")).await;
        let usage = events
            .iter()
            .find_map(|e| match e {
                ChatEvent::Usage(u) => Some(u),
                _ => None,
            })
            .expect("usage");
        assert_eq!(usage.source, UsageSource::Estimated);
        assert_eq!(usage.input_tokens, Some(3));
    }

    #[tokio::test]
    async fn tools_are_rejected_because_mock_does_not_support_them() {
        let mut r = req("hola");
        r.tools = vec![ToolDef {
            name: "t".into(),
            description: None,
            parameters: serde_json::json!({}),
        }];
        let adapter = MockAdapter::new(Duration::ZERO);
        let err = adapter.stream(&r, &cred()).await.unwrap_err();
        assert_eq!(err.http_status(), 422);
    }
}
