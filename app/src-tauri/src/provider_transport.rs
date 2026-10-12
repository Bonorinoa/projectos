//! Fixed-endpoint OpenRouter transport. Secrets and provider bodies never become errors.
use futures_util::StreamExt;
use reqwest::{header, Client};
use serde::Serialize;
use serde_json::{json, Value};
use std::time::Duration;
use tokio::sync::watch;

pub const ENDPOINT: &str = "https://openrouter.ai/api/v1/chat/completions";
const MODELS_ENDPOINT: &str = "https://openrouter.ai/api/v1/models";
const MAX_WIRE_BYTES: usize = 8 * 1024 * 1024;
const MAX_EVENT_BYTES: usize = 256 * 1024;
const MAX_OUTPUT_BYTES: usize = 2 * 1024 * 1024;
const TIMEOUT: Duration = Duration::from_secs(120);
const MAX_ALLOWED_OUTPUT_TOKENS: u64 = 8192;

enum Target {
    Completion,
    Models,
    #[cfg(test)]
    Loopback(String),
}
impl Target {
    fn url(&self) -> &str {
        match self {
            Self::Completion => ENDPOINT,
            Self::Models => MODELS_ENDPOINT,
            #[cfg(test)]
            Self::Loopback(url) => url,
        }
    }
}

#[derive(Clone, Debug, Serialize)]
pub struct ModelInfo {
    pub id: String,
    /// Explicit catalog identity and its canonical alias, frozen at approval.
    pub allowed_resolved_models: Vec<String>,
    pub prompt_usd_per_token: f64,
    pub completion_usd_per_token: f64,
    pub request_usd: f64,
    pub context_length: u64,
    /// Locally allowed output bound, capped at 8192; not a claim of provider capacity.
    pub max_completion_tokens: u64,
}

impl ModelInfo {
    /// One token per UTF-8 byte plus 256 framing tokens is deliberately conservative.
    /// Provider price caps freeze these rates at dispatch; this is not billed cost.
    pub fn estimate_upper(&self, system: &str, prompt: &str, max_tokens: u32, ceiling_usd: f64) -> Result<f64, TransportError> {
        let input = system.len().checked_add(prompt.len()).and_then(|n| n.checked_add(256))
            .ok_or_else(|| TransportError::local("context_limit"))? as u64;
        if max_tokens == 0 || u64::from(max_tokens) > self.max_completion_tokens
            || u64::from(max_tokens) > MAX_ALLOWED_OUTPUT_TOKENS
            || input.checked_add(u64::from(max_tokens)).is_none_or(|n| n > self.context_length)
        { return Err(TransportError::local("context_limit")); }
        if !ceiling_usd.is_finite() || ceiling_usd < 0.0
            || [self.prompt_usd_per_token, self.completion_usd_per_token, self.request_usd].iter().any(|n| !n.is_finite() || *n < 0.0 || !(n * 1_000_000.0).is_finite())
        { return Err(TransportError::local("invalid_budget")); }
        // Tiny safety margin covers floating point arithmetic without treating zero as unknown.
        let raw = input as f64 * self.prompt_usd_per_token
            + f64::from(max_tokens) * self.completion_usd_per_token + self.request_usd;
        let estimate = if raw == 0.0 { 0.0 } else { raw * (1.0 + 1e-9) };
        if !estimate.is_finite() || estimate > ceiling_usd { return Err(TransportError::local("cost_ceiling")); }
        Ok(estimate)
    }
}

pub struct LiveRequest {
    pub model: String,
    pub system: String,
    pub prompt: String,
    pub max_tokens: u32,
    pub pricing: ModelInfo,
}

#[derive(Debug)]
pub struct LiveResponse {
    pub text: String,
    pub resolved_model: String,
    pub usage_tokens: Option<u64>,
    pub provider_cost_usd: Option<f64>,
    pub generation_id: Option<String>,
    pub finish_reason: String,
    pub truncated: bool,
}

#[derive(Debug)]
pub struct TransportError {
    pub code: &'static str,
    pub outcome_unknown: bool,
}
impl TransportError {
    fn local(code: &'static str) -> Self { Self { code, outcome_unknown: false } }
    fn remote(code: &'static str) -> Self { Self { code, outcome_unknown: true } }
}

fn client(timeout: Duration) -> Result<Client, TransportError> {
    Client::builder().redirect(reqwest::redirect::Policy::none())
        .retry(reqwest::retry::never()).no_proxy().referer(false)
        .connect_timeout(Duration::from_secs(15)).timeout(timeout)
        .build().map_err(|_| TransportError::local("transport_unavailable"))
}

pub fn validate_model(model: &str) -> Result<(), TransportError> {
    // :free is a catalogued model variant. :online/:nitro/:floor change routing
    // or activate server tools and are not accepted as an explicit model identity.
    let plain = model.strip_suffix(":free").unwrap_or(model);
    if model.len() > 200 || !model.contains('/')
        || !plain.bytes().all(|b| b.is_ascii_alphanumeric() || b"/-_.".contains(&b))
        || model.starts_with("openrouter/") || model.starts_with("@")
        || ["typesafe/jev-router"].contains(&model)
    { return Err(TransportError::local("explicit_model_required")); }
    Ok(())
}

pub async fn fetch_model(model: &str, cancel: watch::Receiver<bool>) -> Result<ModelInfo, TransportError> {
    let http = client(Duration::from_secs(25))?;
    tokio::select! {
        biased;
        _ = cancelled(cancel) => Err(TransportError::local("cancelled")),
        result = fetch_model_at(&http, Target::Models, model) => result,
    }
}

async fn fetch_model_at(http: &Client, endpoint: Target, model: &str) -> Result<ModelInfo, TransportError> {
    validate_model(model)?;
    let response = http.get(endpoint.url()).send().await.map_err(|_| TransportError::local("pricing_unavailable"))?;
    if !response.status().is_success() { return Err(TransportError::local("pricing_unavailable")); }
    let mut stream = response.bytes_stream();
    let mut body = Vec::new();
    while let Some(part) = stream.next().await {
        let part = part.map_err(|_| TransportError::local("pricing_unavailable"))?;
        if body.len() + part.len() > MAX_WIRE_BYTES { return Err(TransportError::local("pricing_unavailable")); }
        body.extend_from_slice(&part);
    }
    let data: Value = serde_json::from_slice(&body).map_err(|_| TransportError::local("pricing_unavailable"))?;
    parse_model(&data, model)
}

fn price(value: Option<&Value>) -> Result<f64, TransportError> {
    let n = match value {
        Some(Value::String(s)) => s.parse::<f64>().ok(),
        Some(Value::Number(n)) => n.as_f64(),
        _ => None,
    }.filter(|n| n.is_finite() && *n >= 0.0);
    n.ok_or_else(|| TransportError::local("pricing_unbounded"))
}

fn parse_model(data: &Value, model: &str) -> Result<ModelInfo, TransportError> {
    let rows = data.get("data").and_then(Value::as_array).ok_or_else(|| TransportError::local("pricing_unavailable"))?;
    let mut matching = rows.iter().filter(|m| m.get("id").and_then(Value::as_str) == Some(model));
    let row = matching.next().ok_or_else(|| TransportError::local("model_unavailable"))?;
    if matching.next().is_some() { return Err(TransportError::local("pricing_unbounded")); }
    let rates = row.get("pricing").and_then(Value::as_object).ok_or_else(|| TransportError::local("pricing_unbounded"))?;
    let prompt = price(rates.get("prompt"))?;
    let completion = price(rates.get("completion"))?;
    if !(prompt * 1_000_000.0).is_finite() || !(completion * 1_000_000.0).is_finite() {
        return Err(TransportError::local("pricing_unbounded"));
    }
    let request = match rates.get("request") { Some(v) => price(Some(v))?, None => 0.0 };
    for (name, value) in rates {
        if ["prompt", "completion", "request"].contains(&name.as_str()) { continue; }
        let extra = price(Some(value))?;
        // Automatic cache reads are discounted input, already covered by the prompt cap.
        // No explicit cache writing or multimedia/tool execution is sent by this adapter.
        if extra != 0.0 && !(name == "input_cache_read" && extra <= prompt) {
            return Err(TransportError::local("pricing_unbounded"));
        }
    }
    let context = row.get("context_length").and_then(Value::as_u64).filter(|n| *n > 0)
        .ok_or_else(|| TransportError::local("context_limit_unknown"))?;
    let provider_context = row.pointer("/top_provider/context_length").and_then(Value::as_u64).unwrap_or(context);
    let output = match row.pointer("/top_provider/max_completion_tokens") {
        None | Some(Value::Null) => context.min(provider_context),
        Some(value) => value.as_u64().filter(|n| *n > 0)
            .ok_or_else(|| TransportError::local("context_limit_unknown"))?,
    }.min(context.min(provider_context)).min(MAX_ALLOWED_OUTPUT_TOKENS);
    let supported = row.get("supported_parameters").and_then(Value::as_array)
        .is_some_and(|params| params.iter().any(|p| p.as_str() == Some("max_tokens")));
    let modalities = row.pointer("/architecture/output_modalities").and_then(Value::as_array);
    if !supported || modalities.is_some_and(|m| m.len() != 1 || m[0].as_str() != Some("text")) {
        return Err(TransportError::local("model_parameters_unsupported"));
    }
    // Catalogs often expose an undated ID while streaming reports its dated canonical
    // slug. Only aliases provided by this exact catalog row are accepted, never a
    // wildcard or an identity newly learned from the completion itself.
    let mut allowed_resolved_models = vec![model.to_string()];
    if let Some(canonical) = row.get("canonical_slug").filter(|v| !v.is_null()) {
        let canonical = canonical.as_str().ok_or_else(|| TransportError::local("model_identity_unknown"))?;
        validate_model(canonical).map_err(|_| TransportError::local("model_identity_unknown"))?;
        if canonical != model { allowed_resolved_models.push(canonical.to_string()); }
    }
    Ok(ModelInfo { id: model.into(), allowed_resolved_models, prompt_usd_per_token: prompt, completion_usd_per_token: completion,
        request_usd: request, context_length: context.min(provider_context), max_completion_tokens: output })
}

pub async fn complete(request: LiveRequest, key: &str, cancel: watch::Receiver<bool>, on_text: impl FnMut(&str)) -> Result<LiveResponse, TransportError> {
    complete_at(&client(TIMEOUT)?, Target::Completion, request, key, cancel, on_text).await
}

/// Composed native integration tests only; absent from the production binary.
#[cfg(test)]
pub(crate) async fn complete_fixture(request: LiveRequest, key: &str,
    cancel: watch::Receiver<bool>, url: String, on_text: impl FnMut(&str)) -> Result<LiveResponse, TransportError> {
    let parsed = reqwest::Url::parse(&url).map_err(|_| TransportError::local("invalid_fixture_endpoint"))?;
    if parsed.scheme() != "http" || parsed.host_str() != Some("127.0.0.1") {
        return Err(TransportError::local("invalid_fixture_endpoint"));
    }
    complete_at(&client(TIMEOUT)?, Target::Loopback(url), request, key, cancel, on_text).await
}

async fn cancelled(mut cancel: watch::Receiver<bool>) {
    loop {
        if *cancel.borrow() { return; }
        // A vanished request owner also cancels instead of leaving an orphan request.
        if cancel.changed().await.is_err() { return; }
    }
}

async fn complete_at(http: &Client, endpoint: Target, request: LiveRequest, key: &str,
    cancel: watch::Receiver<bool>, mut on_text: impl FnMut(&str)) -> Result<LiveResponse, TransportError> {
    validate_model(&request.model)?;
    if request.model != request.pricing.id || !request.pricing.allowed_resolved_models.contains(&request.model) {
        return Err(TransportError::local("pricing_model_mismatch"));
    }
    request.pricing.estimate_upper(&request.system, &request.prompt, request.max_tokens, f64::MAX)?;
    if !(16..=512).contains(&key.len()) || !key.bytes().all(|b| b.is_ascii_graphic()) {
        return Err(TransportError::local("invalid_credential"));
    }
    if *cancel.borrow() { return Err(TransportError::local("cancelled")); }
    let mut authorization = header::HeaderValue::from_str(&format!("Bearer {key}"))
        .map_err(|_| TransportError::local("invalid_credential"))?;
    authorization.set_sensitive(true);
    let payload = json!({"model": request.model, "stream": true, "max_tokens": request.max_tokens, "plugins": [],
        "messages": [{"role":"system","content":request.system},{"role":"user","content":request.prompt}],
        "reasoning": {"exclude": true},
        "provider": {"allow_fallbacks":false,"require_parameters":true,"max_price":{
            "prompt":request.pricing.prompt_usd_per_token * 1_000_000.0,
            "completion":request.pricing.completion_usd_per_token * 1_000_000.0,
            "request":request.pricing.request_usd}}
    });
    let operation = async {
        // From first poll of send onward failure is conservatively ambiguous.
        let response = http.post(endpoint.url()).header(header::AUTHORIZATION, authorization)
            .header(header::ACCEPT, "text/event-stream").json(&payload).send().await
            .map_err(|error| TransportError::remote(if error.is_timeout() {"request_timeout"} else {"transport_interrupted"}))?;
        let status = response.status();
        if !status.is_success() {
            return Err(match status.as_u16() {
                400 | 404 | 422 => TransportError::local("provider_rejected"),
                401 | 403 => TransportError::local("credential_rejected"),
                402 => TransportError::local("credit_required"),
                429 => TransportError::local("rate_limited"),
                300..=399 => TransportError::remote("redirect_rejected"),
                _ => TransportError::remote("provider_failed"),
            });
        }
        let content_type = response.headers().get(header::CONTENT_TYPE).and_then(|v| v.to_str().ok()).unwrap_or("");
        if !content_type.starts_with("text/event-stream") { return Err(TransportError::remote("invalid_stream")); }
        let mut stream = response.bytes_stream();
        let mut parser = SseParser::new(key, &request.pricing.allowed_resolved_models);
        while let Some(part) = stream.next().await {
            let part = part.map_err(|error| TransportError::remote(if error.is_timeout() {"request_timeout"} else {"stream_interrupted"}))?;
            if parser.feed(&part, &mut on_text)? { return parser.finish(&mut on_text); }
        }
        Err(TransportError::remote("stream_truncated"))
    };
    tokio::select! {
        biased;
        _ = cancelled(cancel) => Err(TransportError::remote("cancelled")),
        result = tokio::time::timeout(TIMEOUT, operation) => result.unwrap_or_else(|_| Err(TransportError::remote("request_timeout"))),
    }
}

/// Holds an undecided suffix so a key split across SSE content events never escapes.
struct Redactor<'a> { key: &'a str, pending: String }
impl<'a> Redactor<'a> {
    fn push(&mut self, content: &str, final_chunk: bool) -> String {
        self.pending.push_str(content);
        let safe_end = if final_chunk { self.pending.len() } else { self.pending.len().saturating_sub(self.key.len().saturating_sub(1)) };
        let mut end = safe_end;
        while !self.pending.is_char_boundary(end) { end -= 1; }
        let mut position = 0;
        let mut result = String::new();
        while let Some(relative) = self.pending[position..].find(self.key) {
            let start = position + relative;
            if start >= end { break; }
            result.push_str(&self.pending[position..start]);
            result.push_str("[REDACTED]");
            position = start + self.key.len();
            if position > end { end = position; }
        }
        result.push_str(&self.pending[position..end]);
        self.pending.drain(..end);
        result
    }
}

struct SseParser<'a> {
    wire_bytes: usize, line: Vec<u8>, data: String, text: String,
    model: Option<String>, id: Option<String>, usage: Option<u64>, cost: Option<f64>,
    terminal: bool, finish_reason: Option<String>, allowed_models: Vec<String>, redactor: Redactor<'a>,
}
impl<'a> SseParser<'a> {
    fn new(key: &'a str, allowed_models: &[String]) -> Self {
        Self { wire_bytes: 0, line: Vec::new(), data: String::new(), text: String::new(), model: None,
            id: None, usage: None, cost: None, terminal: false, finish_reason: None,
            allowed_models: allowed_models.to_vec(), redactor: Redactor {key,pending:String::new()} }
    }
    fn emit(&mut self, text: &str, on_text: &mut impl FnMut(&str)) -> Result<(), TransportError> {
        if self.text.len() + text.len() > MAX_OUTPUT_BYTES { return Err(TransportError::remote("output_limit")); }
        if !text.is_empty() { self.text.push_str(text); on_text(text); }
        Ok(())
    }
    fn feed(&mut self, bytes: &[u8], on_text: &mut impl FnMut(&str)) -> Result<bool, TransportError> {
        self.wire_bytes += bytes.len();
        if self.wire_bytes > MAX_WIRE_BYTES { return Err(TransportError::remote("stream_limit")); }
        for byte in bytes {
            if *byte == b'\n' {
                let raw = std::mem::take(&mut self.line);
                let line = std::str::from_utf8(&raw).map_err(|_| TransportError::remote("invalid_stream"))?.trim_end_matches('\r');
                if line.is_empty() {
                    if !self.data.is_empty() {
                        let data = std::mem::take(&mut self.data);
                        if self.event(data.trim_end_matches('\n'), on_text)? { return Ok(true); }
                    }
                } else if let Some(data) = line.strip_prefix("data:") {
                    self.data.push_str(data.strip_prefix(' ').unwrap_or(data)); self.data.push('\n');
                    if self.data.len() > MAX_EVENT_BYTES { return Err(TransportError::remote("stream_limit")); }
                }
            } else {
                self.line.push(*byte);
                if self.line.len() > MAX_EVENT_BYTES { return Err(TransportError::remote("stream_limit")); }
            }
        }
        Ok(false)
    }
    fn event(&mut self, data: &str, on_text: &mut impl FnMut(&str)) -> Result<bool, TransportError> {
        if data == "[DONE]" {
            if !self.terminal || self.model.is_none() { return Err(TransportError::remote("stream_truncated")); }
            return Ok(true);
        }
        let event: Value = serde_json::from_str(data).map_err(|_| TransportError::remote("invalid_stream"))?;
        if !event.is_object() || event.get("error").is_some() { return Err(TransportError::remote("provider_stream_failed")); }
        if let Some(model) = event.get("model").and_then(Value::as_str) {
            if model.len() > 512 { return Err(TransportError::remote("invalid_stream")); }
            if !self.allowed_models.iter().any(|allowed| allowed == model) {
                return Err(TransportError::remote("resolved_model_mismatch"));
            }
            let model = model.replace(self.redactor.key, "[REDACTED]");
            self.model = Some(model);
        }
        if let Some(id) = event.get("id").and_then(Value::as_str) {
            if id.len() > 512 { return Err(TransportError::remote("invalid_stream")); }
            let id = id.replace(self.redactor.key, "[REDACTED]");
            if self.id.as_ref().is_some_and(|prior| prior != &id) { return Err(TransportError::remote("generation_changed")); }
            self.id = Some(id);
        }
        if let Some(usage) = event.get("usage").filter(|v| !v.is_null()) {
            if !usage.is_object() { return Err(TransportError::remote("invalid_usage")); }
            self.usage = usage.get("total_tokens").and_then(Value::as_u64);
            self.cost = match usage.get("cost") {
                Some(value) if !value.is_null() => Some(price(Some(value)).map_err(|_| TransportError::remote("invalid_usage"))?),
                _ => None,
            };
        }
        let choices = event.get("choices").and_then(Value::as_array).ok_or_else(|| TransportError::remote("invalid_stream"))?;
        if choices.len() > 1 { return Err(TransportError::remote("invalid_stream")); }
        if let Some(choice) = choices.first() {
            if choice.get("error").is_some() { return Err(TransportError::remote("provider_stream_failed")); }
            if let Some(reason) = choice.get("finish_reason").and_then(Value::as_str) {
                if !["stop", "length"].contains(&reason) { return Err(TransportError::remote("provider_stream_failed")); }
                if self.finish_reason.as_deref().is_some_and(|prior| prior != reason) {
                    return Err(TransportError::remote("invalid_stream"));
                }
                self.finish_reason = Some(reason.to_string());
                self.terminal = true;
            }
            if let Some(content) = choice.pointer("/delta/content").filter(|v| !v.is_null()) {
                let content = content.as_str().ok_or_else(|| TransportError::remote("invalid_stream"))?;
                let safe = self.redactor.push(content, false);
                self.emit(&safe, on_text)?;
            }
            // Deliberately ignore reasoning, reasoning_details, tool calls and debug payloads.
        }
        Ok(false)
    }
    fn finish(mut self, on_text: &mut impl FnMut(&str)) -> Result<LiveResponse, TransportError> {
        let safe = self.redactor.push("", true);
        self.emit(&safe, on_text)?;
        if self.text.trim().is_empty() { return Err(TransportError::remote("empty_completion")); }
        let finish_reason = self.finish_reason.ok_or_else(|| TransportError::remote("stream_truncated"))?;
        let truncated = finish_reason == "length";
        Ok(LiveResponse { text:self.text, resolved_model:self.model.unwrap(), usage_tokens:self.usage,
            provider_cost_usd:self.cost, generation_id:self.id, finish_reason, truncated })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{Read, Write};
    use std::net::TcpListener;
    use std::sync::mpsc;

    const KEY: &str = "sk-or-test-only-secret";
    fn info() -> ModelInfo {
        ModelInfo { id: "test/fixed".into(), allowed_resolved_models: vec!["test/fixed".into()], prompt_usd_per_token: 0.000001,
            completion_usd_per_token: 0.000002, request_usd: 0.003,
            context_length: 8192, max_completion_tokens: 512 }
    }
    fn request() -> LiveRequest {
        LiveRequest { model: "test/fixed".into(), system: "Only return requested prose.".into(),
            prompt: "Rewrite the selected passage.".into(), max_tokens: 64, pricing: info() }
    }
    fn event(content: &str, reason: Option<&str>, usage: Value) -> String {
        format!("data: {}\r\n\r\n", json!({"id":"gen-test","model":"test/fixed","choices":[{
            "index":0,"delta":{"content":content,"reasoning":"PRIVATE REASONING"},"finish_reason":reason}],"usage":usage}))
    }
    fn success() -> String {
        event("Updated café prose.", None, Value::Null)
            + &event("", Some("stop"), json!({"total_tokens":23,"cost":0.00302}))
            + "data: [DONE]\r\n\r\n"
    }

    /// Real HTTP exercised against an ephemeral loopback server; no production URL override.
    fn server(status: &str, content_type: &str, body: String, delay: Duration) -> (String, mpsc::Receiver<String>, std::thread::JoinHandle<()>) {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let url = format!("http://{}/test", listener.local_addr().unwrap());
        let (tx, rx) = mpsc::channel();
        let status = status.to_string();
        let content_type = content_type.to_string();
        let handle = std::thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            stream.set_read_timeout(Some(Duration::from_secs(3))).unwrap();
            let mut bytes = Vec::new(); let mut buffer = [0; 1024];
            loop {
                let n = stream.read(&mut buffer).unwrap();
                if n == 0 { break; }
                bytes.extend_from_slice(&buffer[..n]);
                if let Some(end) = bytes.windows(4).position(|w| w == b"\r\n\r\n") {
                    let header = String::from_utf8_lossy(&bytes[..end]).to_lowercase();
                    let length = header.lines().find_map(|l| l.strip_prefix("content-length: ")).and_then(|s| s.parse::<usize>().ok()).unwrap_or(0);
                    if bytes.len() >= end + 4 + length { break; }
                }
            }
            tx.send(String::from_utf8(bytes).unwrap()).unwrap();
            std::thread::sleep(delay);
            let headers = format!("HTTP/1.1 {status}\r\nContent-Type: {content_type}\r\nTransfer-Encoding: chunked\r\nConnection: close\r\n\r\n");
            if stream.write_all(headers.as_bytes()).is_err() { return; }
            for chunk in body.as_bytes().chunks(7) {
                if write!(stream, "{:x}\r\n", chunk.len()).is_err()
                    || stream.write_all(chunk).is_err() || stream.write_all(b"\r\n").is_err() { return; }
            }
            let _ = stream.write_all(b"0\r\n\r\n");
        });
        (url, rx, handle)
    }

    #[tokio::test]
    async fn actual_http_stream_records_usage_and_disables_fallback() {
        let (url, captured, handle) = server("200 OK", "text/event-stream", success(), Duration::ZERO);
        let (_tx, cancel) = watch::channel(false);
        let mut emitted = String::new();
        let result = complete_at(&client(TIMEOUT).unwrap(), Target::Loopback(url.clone()), request(), KEY, cancel, |s| emitted.push_str(s)).await.unwrap();
        let sent = captured.recv().unwrap(); handle.join().unwrap();
        assert!(sent.starts_with("POST /test HTTP/1.1"));
        assert!(sent.to_lowercase().contains(&format!("authorization: bearer {KEY}")));
        let body: Value = serde_json::from_str(sent.split_once("\r\n\r\n").unwrap().1).unwrap();
        assert_eq!(body["provider"]["allow_fallbacks"], false);
        assert_eq!(body["provider"]["require_parameters"], true);
        assert_eq!(body["provider"]["max_price"]["prompt"], 1.0);
        assert_eq!(body["provider"]["max_price"]["request"], 0.003);
        assert_eq!(body["model"], "test/fixed"); assert_eq!(body["max_tokens"], 64);
        assert_eq!(body["reasoning"]["exclude"], true);
        assert_eq!(result.text, "Updated café prose."); assert_eq!(result.text, emitted);
        assert!(!emitted.contains("PRIVATE")); assert_eq!(result.usage_tokens, Some(23));
        assert_eq!(result.provider_cost_usd, Some(0.00302)); assert_eq!(result.generation_id.as_deref(), Some("gen-test"));
        assert_eq!(result.finish_reason, "stop"); assert!(!result.truncated);
    }

    #[tokio::test]
    async fn length_finish_is_a_flagged_partial_completion() {
        let body = event("Incomplete sentence", Some("length"), json!({"total_tokens":64,"cost":0.0031}))
            + "data: [DONE]\n\n";
        let (url, captured, handle) = server("200 OK", "text/event-stream", body, Duration::ZERO);
        let (_tx, cancel) = watch::channel(false);
        let response = complete_at(&client(TIMEOUT).unwrap(), Target::Loopback(url), request(), KEY, cancel, |_|{}).await.unwrap();
        captured.recv().unwrap(); handle.join().unwrap();
        assert_eq!(response.text, "Incomplete sentence");
        assert_eq!(response.finish_reason, "length"); assert!(response.truncated);
        assert_eq!(response.usage_tokens, Some(64));
    }

    #[tokio::test]
    async fn frozen_catalog_alias_is_accepted_and_unrelated_model_is_rejected() {
        let mut catalog = model_json(); catalog["data"][0]["canonical_slug"] = json!("test/canonical");
        let pricing = parse_model(&catalog, "test/fixed").unwrap();
        assert_eq!(pricing.allowed_resolved_models, vec!["test/fixed", "test/canonical"]);
        for resolved in ["test/canonical", "other/unapproved-model"] {
            let body = success().replace("test/fixed", resolved);
            let (url, captured, handle) = server("200 OK", "text/event-stream", body, Duration::ZERO);
            let (_tx, cancel) = watch::channel(false);
            let mut outbound = request(); outbound.pricing = pricing.clone();
            let mut text = String::new();
            let result = complete_at(&client(TIMEOUT).unwrap(), Target::Loopback(url), outbound, KEY, cancel, |part| text.push_str(part)).await;
            captured.recv().unwrap(); handle.join().unwrap();
            if resolved == "test/canonical" {
                assert_eq!(result.unwrap().resolved_model, resolved);
                assert_eq!(text, "Updated café prose.");
            } else {
                let error = result.unwrap_err();
                assert_eq!(error.code, "resolved_model_mismatch"); assert!(error.outcome_unknown);
                assert!(text.is_empty());
            }
        }
    }

    #[tokio::test]
    async fn actual_http_redacts_secret_split_across_events_and_utf8_chunks() {
        let body = event("Prefix café sk-or-", None, Value::Null)
            + &event("test-only-", None, Value::Null)
            + &event("secret suffix", Some("stop"), Value::Null) + "data: [DONE]\n\n";
        let (url, captured, handle) = server("200 OK", "text/event-stream", body, Duration::ZERO);
        let (_tx, cancel) = watch::channel(false); let mut emitted = String::new();
        let result = complete_at(&client(TIMEOUT).unwrap(), Target::Loopback(url.clone()), request(), KEY, cancel, |s| emitted.push_str(s)).await.unwrap();
        captured.recv().unwrap(); handle.join().unwrap();
        assert_eq!(result.text, "Prefix café [REDACTED] suffix"); assert_eq!(emitted, result.text);
        assert_eq!(result.provider_cost_usd, None); assert_eq!(result.usage_tokens, None);
    }

    #[tokio::test]
    async fn actual_http_rejections_and_ambiguous_stream_failures_are_static() {
        for (status, body, code, unknown) in [
            ("401 Unauthorized", format!("bad key {KEY}"), "credential_rejected", false),
            ("429 Too Many Requests", "retry me".into(), "rate_limited", false),
            ("200 OK", event("partial", None, Value::Null), "stream_truncated", true),
            ("200 OK", format!("data: {{\"error\":{{\"message\":\"{KEY}\"}}}}\n\n"), "provider_stream_failed", true),
            ("200 OK", "data: invalid\n\n".into(), "invalid_stream", true),
        ] {
            let (url, captured, handle) = server(status, "text/event-stream", body, Duration::ZERO);
            let (_tx, cancel) = watch::channel(false);
            let err = complete_at(&client(TIMEOUT).unwrap(), Target::Loopback(url.clone()), request(), KEY, cancel, |_|{}).await.unwrap_err();
            captured.recv().unwrap(); handle.join().unwrap();
            assert_eq!(err.code, code); assert_eq!(err.outcome_unknown, unknown);
            assert!(!format!("{err:?}").contains(KEY));
        }
    }

    #[tokio::test]
    async fn actual_http_cancellation_and_timeout_do_not_retry() {
        let (url, captured, handle) = server("200 OK", "text/event-stream", success(), Duration::from_millis(120));
        let (tx, cancel) = watch::channel(false);
        let cancel_task = tokio::spawn(async move { tokio::time::sleep(Duration::from_millis(25)).await; tx.send(true).unwrap(); });
        let err = complete_at(&client(TIMEOUT).unwrap(), Target::Loopback(url.clone()), request(), KEY, cancel, |_|{}).await.unwrap_err();
        cancel_task.await.unwrap(); captured.recv().unwrap(); handle.join().unwrap();
        assert_eq!(err.code,"cancelled"); assert!(err.outcome_unknown);
        let (url, captured, handle) = server("200 OK", "text/event-stream", success(), Duration::from_millis(120));
        let (_tx, cancel) = watch::channel(false);
        let err = complete_at(&client(Duration::from_millis(30)).unwrap(), Target::Loopback(url.clone()), request(), KEY, cancel, |_|{}).await.unwrap_err();
        captured.recv().unwrap(); handle.join().unwrap();
        assert_eq!(err.code, "request_timeout"); assert!(err.outcome_unknown);
    }

    #[tokio::test]
    async fn redirect_cannot_receive_credentials() {
        let destination = TcpListener::bind("127.0.0.1:0").unwrap();
        destination.set_nonblocking(true).unwrap();
        let redirect = TcpListener::bind("127.0.0.1:0").unwrap();
        let url = format!("http://{}/request",redirect.local_addr().unwrap());
        let target = destination.local_addr().unwrap();
        let handle = std::thread::spawn(move || {
            let (mut connection,_) = redirect.accept().unwrap(); let mut request=[0;8192];
            let _ = connection.read(&mut request).unwrap();
            write!(connection,"HTTP/1.1 307 Temporary Redirect\r\nLocation: http://{target}/stolen\r\nContent-Length: 0\r\nConnection: close\r\n\r\n").unwrap();
        });
        let (_tx,cancel)=watch::channel(false);
        let error=complete_at(&client(TIMEOUT).unwrap(),Target::Loopback(url.clone()),request(),KEY,cancel,|_|{}).await.unwrap_err();
        handle.join().unwrap(); assert_eq!(error.code,"redirect_rejected");
        assert_eq!(destination.accept().unwrap_err().kind(),std::io::ErrorKind::WouldBlock);
    }

    fn model_json() -> Value {
        json!({"data":[{"id":"test/fixed","pricing":{"prompt":"0.000001","completion":"0.000002","request":"0.003","image":"0"},
            "context_length":8192,"supported_parameters":["max_tokens"],
            "architecture":{"output_modalities":["text"]},"top_provider":{"context_length":8192,"max_completion_tokens":512}}]})
    }

    #[tokio::test]
    async fn public_price_http_never_receives_key() {
        let (url,captured,handle)=server("200 OK","application/json",model_json().to_string(),Duration::ZERO);
        let prices=fetch_model_at(&client(TIMEOUT).unwrap(),Target::Loopback(url.clone()),"test/fixed").await.unwrap();
        let sent=captured.recv().unwrap(); handle.join().unwrap();
        assert!(sent.starts_with("GET /test")); assert!(!sent.to_lowercase().contains("authorization"));
        assert_eq!(prices.request_usd,0.003);
    }

    #[test]
    fn costs_include_system_framing_request_and_maximum_output() {
        let estimate=info().estimate_upper("abcd","123456",64,1.0).unwrap();
        assert!(estimate>=0.003 + 266.0*0.000001 + 64.0*0.000002);
        assert_eq!(info().estimate_upper("abcd","123456",64,0.003).unwrap_err().code,"cost_ceiling");
        assert_eq!(info().estimate_upper("","",513,1.0).unwrap_err().code,"context_limit");
        assert_eq!(info().estimate_upper("","",64,f64::NAN).unwrap_err().code,"invalid_budget");
        let mut free=info();free.prompt_usd_per_token=0.0;free.completion_usd_per_token=0.0;free.request_usd=0.0;
        assert_eq!(free.estimate_upper("","",64,0.0).unwrap(),0.0);
    }

    #[test]
    fn metadata_rejects_unknown_prices_bad_limits_and_router_aliases() {
        let mut data=model_json();data["data"][0]["pricing"]["future_charge"]=json!("0.01");
        assert_eq!(parse_model(&data,"test/fixed").unwrap_err().code,"pricing_unbounded");
        data=model_json();data["data"][0]["pricing"]["prompt"]=json!("NaN");
        assert!(parse_model(&data,"test/fixed").is_err());
        data=model_json();data["data"][0]["top_provider"]["max_completion_tokens"]=Value::Null;
        assert_eq!(parse_model(&data,"test/fixed").unwrap().max_completion_tokens, 8192);
        data["data"][0]["top_provider"]["max_completion_tokens"] = json!(0);
        assert_eq!(parse_model(&data,"test/fixed").unwrap_err().code, "context_limit_unknown");
        data = model_json(); data["data"][0]["canonical_slug"] = json!("openrouter/auto");
        assert_eq!(parse_model(&data,"test/fixed").unwrap_err().code, "model_identity_unknown");
        for model in ["", "openrouter/auto", "openrouter/free", "typesafe/jev-router", "author/model\nheader", "openai/gpt-4o-mini:online", "author/model:nitro"] { assert!(validate_model(model).is_err()); }
        assert!(validate_model("openai/gpt-4o-mini").is_ok());
        assert!(validate_model("author/autocoder-router-v1").is_ok());
        assert!(validate_model("author/model:free").is_ok());
    }

    #[test]
    fn absent_output_capacity_uses_context_and_local_cap_with_parameter_enforcement() {
        let mut catalog = model_json();
        catalog["data"][0]["top_provider"]["max_completion_tokens"] = Value::Null;
        catalog["data"][0]["context_length"] = json!(100_000);
        catalog["data"][0]["top_provider"]["context_length"] = json!(100_000);
        let metadata = parse_model(&catalog, "test/fixed").unwrap();
        assert_eq!(metadata.max_completion_tokens, MAX_ALLOWED_OUTPUT_TOKENS);
        assert!(metadata.estimate_upper("","",8192,1.0).is_ok());
        assert_eq!(metadata.estimate_upper("","",8193,1.0).unwrap_err().code,"context_limit");
        catalog["data"][0]["context_length"] = json!(400);
        catalog["data"][0]["top_provider"]["context_length"] = json!(400);
        let metadata = parse_model(&catalog, "test/fixed").unwrap();
        assert_eq!(metadata.max_completion_tokens, 400);
        assert!(metadata.estimate_upper("","",140,1.0).is_ok());
        assert!(metadata.estimate_upper("","",150,1.0).is_err());
        catalog["data"][0]["supported_parameters"] = json!([]);
        assert_eq!(parse_model(&catalog,"test/fixed").unwrap_err().code,"model_parameters_unsupported");
    }

    #[test]
    fn parser_bounded_frames_and_redaction_hold_for_every_byte_boundary() {
        let body=event(&format!("é before {KEY} after"),Some("stop"),Value::Null)+"data: [DONE]\n\n";
        let mut parser=SseParser::new(KEY, &info().allowed_resolved_models); let mut emitted=String::new(); let mut done=false;
        for byte in body.as_bytes() { done=parser.feed(&[*byte],&mut |s| emitted.push_str(s)).unwrap(); }
        assert!(done);let response=parser.finish(&mut |s| emitted.push_str(s)).unwrap();
        assert_eq!(response.text,"é before [REDACTED] after"); assert_eq!(emitted,response.text);
        let mut parser=SseParser::new(KEY, &info().allowed_resolved_models);
        assert_eq!(parser.feed(&vec![b'x';MAX_EVENT_BYTES+1],&mut |_|{}).unwrap_err().code,"stream_limit");
    }

    #[test]
    fn redaction_protects_metadata_and_every_secret_event_split() {
        for split in 0..=KEY.len() {
            let mut redactor = Redactor { key: KEY, pending: String::new() };
            let first = redactor.push(&format!("prefix {}", &KEY[..split]), false);
            let second = redactor.push(&format!("{} suffix", &KEY[split..]), false);
            let final_chunk = redactor.push("", true);
            assert_eq!(first + &second + &final_chunk, "prefix [REDACTED] suffix");
        }
        let mut parser = SseParser::new(KEY, &[format!("test/{KEY}")]);
        let body = format!("data: {}\n\ndata: [DONE]\n\n", json!({"id":format!("gen-{KEY}"),
            "model":format!("test/{KEY}"),"choices":[{"delta":{"content":"safe result"},"finish_reason":"stop"}]}));
        assert!(parser.feed(body.as_bytes(), &mut |_|{}).unwrap());
        let response = parser.finish(&mut |_|{}).unwrap();
        assert_eq!(response.resolved_model, "test/[REDACTED]");
        assert_eq!(response.generation_id.as_deref(), Some("gen-[REDACTED]"));
        assert!(!format!("{response:?}").contains(KEY));
    }

    /// Explicitly opt-in, unauthenticated GET only: no draft bytes and no billed generation.
    #[tokio::test]
    #[ignore = "requires network; set PROJECTOS_PUBLIC_TEST_MODEL to an exact catalog model ID"]
    async fn live_public_catalog() {
        let model = std::env::var("PROJECTOS_PUBLIC_TEST_MODEL").expect("Set PROJECTOS_PUBLIC_TEST_MODEL; this test never uses an API key.");
        let (_tx, cancel) = watch::channel(false);
        let prices = fetch_model(&model, cancel).await.expect("Public catalog model must have bounded text-only pricing, declared context limits, and enforceable output bounds.");
        assert_eq!(prices.id, model);
        assert!(prices.estimate_upper("Return prose only.", "A short catalog conformance test.", 64, 1.0).is_ok());
    }
}
