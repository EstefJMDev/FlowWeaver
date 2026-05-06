// synthesis_engine.rs — Fase 3 (T-3-009)
// Propósito: construir payload D25, llamar al proxy FlowWeaver, parsear SSE,
// persistir síntesis cifrada en SQLCipher (tabla syntheses).
// NO decide transiciones (D4 — eso es state_machine.rs).
// NO accede a url ni title cifrados de la BD (D1 — solo category, titles en claro, domains).
// REQUIERE consentimiento previo en consent_log antes de llamar al proxy (D25).
// Degrada gracefully sin red: SynthesisError::NoConnectivity, sin panic (D8).

use crate::{commands, consent_log_store, crypto, syntheses_store};

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub enum SynthesisType {
    Entretenimiento,
    Cocina,
    Noticias,
    Tecnologia,
}

impl SynthesisType {
    pub fn as_str(&self) -> &'static str {
        match self {
            SynthesisType::Entretenimiento => "entretenimiento",
            SynthesisType::Cocina         => "cocina",
            SynthesisType::Noticias       => "noticias",
            SynthesisType::Tecnologia     => "tecnologia",
        }
    }
}

#[derive(Debug)]
pub enum SynthesisError {
    NoToken,
    NoConsent,
    NoConnectivity,
    RateLimitExceeded,
    ProviderUnavailable,
    InvalidToken,
    Persistence(rusqlite::Error),
    Http(String),
}

impl std::fmt::Display for SynthesisError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            SynthesisError::NoToken             => write!(f, "NO_TOKEN"),
            SynthesisError::NoConsent           => write!(f, "NO_CONSENT"),
            SynthesisError::NoConnectivity      => write!(f, "NO_CONNECTIVITY"),
            SynthesisError::RateLimitExceeded   => write!(f, "RATE_LIMIT_EXCEEDED"),
            SynthesisError::ProviderUnavailable => write!(f, "PROVIDER_UNAVAILABLE"),
            SynthesisError::InvalidToken        => write!(f, "INVALID_TOKEN"),
            SynthesisError::Persistence(e)      => write!(f, "PERSISTENCE: {e}"),
            SynthesisError::Http(s)             => write!(f, "HTTP: {s}"),
        }
    }
}

#[derive(Debug, serde::Serialize)]
pub struct SynthesisPayload {
    pub category:       String,
    pub titles:         Vec<String>,
    pub domains:        Vec<String>,
    pub synthesis_type: String,
    pub language:       String,
    pub prompt_version: String,
}

/// TEST ESTRUCTURAL PG-001: exactamente 5 parámetros. Ninguno es url, title_raw ni NewResource.
pub fn build_synthesis_payload(
    category: &str,
    titles: &[&str],
    domains: &[&str],
    synthesis_type: SynthesisType,
    language: &str,
) -> SynthesisPayload {
    SynthesisPayload {
        category:       category.to_string(),
        titles:         titles.iter().map(|s| s.to_string()).collect(),
        domains:        domains.iter().map(|s| s.to_string()).collect(),
        synthesis_type: synthesis_type.as_str().to_string(),
        language:       language.to_string(),
        prompt_version: "v1".to_string(),
    }
}

fn parse_sse_chunk(line: &str) -> Option<String> {
    if line.starts_with("data: [DONE]") {
        return None;
    }
    if let Some(json_str) = line.strip_prefix("data: ") {
        if let Ok(v) = serde_json::from_str::<serde_json::Value>(json_str) {
            if v.get("error").is_some() {
                return None;
            }
            return v["chunk"].as_str().map(String::from);
        }
    }
    None
}

/// Llama al proxy y parsea el stream SSE. Toma datos en propiedad — es Send.
/// Usada por commands::generate_synthesis (que no puede tener &Connection en el await).
pub(crate) async fn fetch_from_proxy(
    token: &str,
    body: String,
    proxy_url: &str,
    on_chunk: impl Fn(&str),
) -> Result<String, SynthesisError> {
    use futures_util::StreamExt;

    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(30))
        .build()
        .map_err(|e| SynthesisError::Http(e.to_string()))?;

    let response = client
        .post(proxy_url)
        .header("Content-Type", "application/json")
        .header("Authorization", format!("Bearer {token}"))
        .body(body)
        .send()
        .await
        .map_err(|_| SynthesisError::NoConnectivity)?;

    match response.status().as_u16() {
        401 => return Err(SynthesisError::InvalidToken),
        429 => return Err(SynthesisError::RateLimitExceeded),
        503 => return Err(SynthesisError::ProviderUnavailable),
        200 => {}
        _   => return Err(SynthesisError::Http(format!("HTTP {}", response.status()))),
    }

    let mut stream = response.bytes_stream();
    let mut full_content = String::new();
    let mut buffer = String::new();

    while let Some(chunk) = stream.next().await {
        let chunk = chunk.map_err(|_| SynthesisError::NoConnectivity)?;
        buffer.push_str(&String::from_utf8_lossy(&chunk));

        while let Some(pos) = buffer.find("\n\n") {
            let line = buffer[..pos].trim().to_string();
            buffer = buffer[pos + 2..].to_string();

            if line.starts_with("data: [DONE]") {
                break;
            }
            if let Some(text) = parse_sse_chunk(&line) {
                full_content.push_str(&text);
                on_chunk(&text);
            }
        }
    }

    if full_content.is_empty() {
        return Err(SynthesisError::ProviderUnavailable);
    }
    Ok(full_content)
}

/// Genera una síntesis y la persiste en SQLCipher.
/// PRECONDICIÓN: consent_log tiene synthesis_v1 (D25). install_token configurado.
/// Retorna el anchor_key de la síntesis persistida.
///
/// NOTA: esta función toma &Connection, por lo que es !Send.
/// commands::generate_synthesis usa fetch_from_proxy directamente para mantener
/// el contrato Send del async command de Tauri (drop del lock antes del .await).
pub async fn generate_and_persist(
    conn: &rusqlite::Connection,
    app: &tauri::AppHandle,
    category: &str,
    titles: &[&str],
    domains: &[&str],
    synthesis_type: SynthesisType,
    anchor_key: &str,
    anchor_type: &str,
    proxy_url: &str,
    on_chunk: impl Fn(&str) + Send + 'static,
) -> Result<String, SynthesisError> {
    // 1. Verificar consentimiento (D25) — antes de construir el payload
    consent_log_store::ensure_schema(conn).map_err(SynthesisError::Persistence)?;
    if !consent_log_store::has_consent(conn, "synthesis", "synthesis_v1")
        .map_err(SynthesisError::Persistence)?
    {
        return Err(SynthesisError::NoConsent);
    }

    // 2. Obtener token
    let token = commands::get_synthesis_token_plain(conn, app)
        .map_err(SynthesisError::Http)?
        .ok_or(SynthesisError::NoToken)?;

    // 3. Construir payload
    let payload = build_synthesis_payload(category, titles, domains, synthesis_type, "es");
    let syn_type_str = payload.synthesis_type.clone();
    let body = serde_json::to_string(&payload)
        .map_err(|e| SynthesisError::Http(e.to_string()))?;

    // 4. HTTP call — fetch_from_proxy maneja la conexión y el parsing SSE
    let full_content = fetch_from_proxy(&token, body, proxy_url, on_chunk).await?;

    // 5. Persistir cifrado
    let key = commands::db_key(app);
    let encrypted = crypto::encrypt_aes(&full_content, &key);
    let now_unix = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0);

    syntheses_store::ensure_schema(conn).map_err(SynthesisError::Persistence)?;
    syntheses_store::save(conn, &syntheses_store::SynthesisEntry {
        anchor_key:        anchor_key.to_string(),
        anchor_type:       anchor_type.to_string(),
        category:          category.to_string(),
        synthesis_type:    syn_type_str,
        content_encrypted: encrypted,
        generated_at:      now_unix,
    })
    .map_err(SynthesisError::Persistence)?;

    Ok(anchor_key.to_string())
}
