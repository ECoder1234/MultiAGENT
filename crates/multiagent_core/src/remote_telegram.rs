use reqwest::blocking::Client;
use reqwest::header::{AUTHORIZATION, CONTENT_TYPE, HeaderMap, HeaderValue, USER_AGENT};
use serde_json::{Value, json};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc;
use std::thread;
use std::time::{Duration, Instant};

#[derive(Clone, Debug)]
pub struct TelegramAuthMatch {
    pub chat_id: String,
    pub user_id: String,
    pub username: Option<String>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum RemoteProvider {
    Discord,
    Telegram,
}

impl RemoteProvider {
    fn from_str(provider: &str) -> Self {
        if provider.trim().eq_ignore_ascii_case("telegram") {
            Self::Telegram
        } else {
            Self::Discord
        }
    }

    fn label(self) -> &'static str {
        match self {
            Self::Discord => "Discord",
            Self::Telegram => "Telegram",
        }
    }
}

pub fn start_telegram_auth_poll(
    bot_token: String,
    user_id: String,
    expected_code: String,
    timeout: Duration,
) -> (
    mpsc::Receiver<Result<TelegramAuthMatch, String>>,
    Arc<AtomicBool>,
) {
    start_remote_auth_poll(
        "discord".to_string(),
        bot_token,
        user_id,
        expected_code,
        timeout,
    )
}

pub fn start_remote_auth_poll(
    provider: String,
    bot_token: String,
    user_id: String,
    expected_code: String,
    timeout: Duration,
) -> (
    mpsc::Receiver<Result<TelegramAuthMatch, String>>,
    Arc<AtomicBool>,
) {
    let (tx, rx) = mpsc::channel::<Result<TelegramAuthMatch, String>>();
    let cancel = Arc::new(AtomicBool::new(false));
    let cancel_for_thread = cancel.clone();

    thread::spawn(move || {
        let provider_kind = RemoteProvider::from_str(&provider);
        let client = match TelegramClient::new_for_user_provider(
            provider_kind,
            bot_token.clone(),
            user_id.clone(),
        ) {
            Ok(client) => client,
            Err(err) => {
                let _ = tx.send(Err(err));
                return;
            }
        };

        let mut offset: Option<i64> = client
            .get_updates(None, 20)
            .map(|(next_offset, _)| next_offset)
            .unwrap_or(None);
        let deadline = Instant::now() + timeout;
        let expected_code = expected_code.trim().to_string();
        if provider_kind == RemoteProvider::Discord {
            let auth_prompt = format!(
                "MultiAGENT remote auth\n\nReply in this DM with this 6-digit code:\n{}",
                expected_code
            );
            if let Err(err) = client.send_plain_message(&auth_prompt) {
                let _ = tx.send(Err(err));
                return;
            }
        }
        let mut consecutive_errors = 0usize;

        while Instant::now() < deadline {
            if cancel_for_thread.load(Ordering::Relaxed) {
                let _ = tx.send(Err(format!(
                    "{} authentication was cancelled.",
                    provider_kind.label()
                )));
                return;
            }

            match client.get_updates(offset, 20) {
                Ok((next_offset, updates)) => {
                    consecutive_errors = 0;
                    offset = next_offset.or(offset);
                    for update in updates {
                        if let Some(found) = extract_auth_match(&update, &expected_code) {
                            if !user_id.trim().is_empty() && found.user_id != user_id.trim() {
                                continue;
                            }
                            if provider_kind == RemoteProvider::Telegram {
                                let confirmation = TelegramClient::new_for_channel_provider(
                                    provider_kind,
                                    bot_token.clone(),
                                    found.chat_id.clone(),
                                )
                                .and_then(|client| {
                                    client.send_plain_message("MultiAGENT remote auth complete.")
                                });
                                let _ = confirmation;
                            }
                            let _ = tx.send(Ok(found));
                            return;
                        }
                    }
                }
                Err(err) => {
                    consecutive_errors = consecutive_errors.saturating_add(1);
                    if consecutive_errors >= 3 {
                        let _ = tx.send(Err(err));
                        return;
                    }
                }
            }
            thread::sleep(Duration::from_millis(1200));
        }

        let _ = tx.send(Err(format!(
            "Timed out waiting for the 6-digit code in {} (3 minutes).",
            provider_kind.label()
        )));
    });

    (rx, cancel)
}

pub struct TelegramClient {
    provider: RemoteProvider,
    bot_token: String,
    channel_id: Option<String>,
    http: Client,
}

impl TelegramClient {
    pub fn new(bot_token: String) -> Result<Self, String> {
        Self::new_inner(RemoteProvider::Discord, bot_token, None)
    }

    pub fn new_for_channel(bot_token: String, channel_id: String) -> Result<Self, String> {
        Self::new_for_channel_provider(RemoteProvider::Discord, bot_token, channel_id)
    }

    fn new_for_channel_provider(
        provider: RemoteProvider,
        bot_token: String,
        channel_id: String,
    ) -> Result<Self, String> {
        let channel_id = channel_id.trim().to_string();
        if channel_id.is_empty() {
            return Err(format!("{} chat/channel ID is required.", provider.label()));
        }
        Self::new_inner(provider, bot_token, Some(channel_id))
    }

    pub fn new_for_channel_named(
        provider: &str,
        bot_token: String,
        channel_id: String,
    ) -> Result<Self, String> {
        Self::new_for_channel_provider(RemoteProvider::from_str(provider), bot_token, channel_id)
    }

    pub fn new_for_user(bot_token: String, user_id: String) -> Result<Self, String> {
        Self::new_for_user_provider(RemoteProvider::Discord, bot_token, user_id)
    }

    fn new_for_user_provider(
        provider: RemoteProvider,
        bot_token: String,
        user_id: String,
    ) -> Result<Self, String> {
        let user_id = user_id.trim().to_string();
        if provider == RemoteProvider::Discord && user_id.is_empty() {
            return Err("Discord user ID is required.".to_string());
        }
        let mut client = Self::new_inner(provider, bot_token, None)?;
        client.verify_token()?;
        if provider == RemoteProvider::Discord {
            let channel_id = client.create_dm_channel(&user_id)?;
            client.channel_id = Some(channel_id);
        }
        Ok(client)
    }

    fn new_inner(
        provider: RemoteProvider,
        bot_token: String,
        channel_id: Option<String>,
    ) -> Result<Self, String> {
        let bot_token = bot_token.trim().to_string();
        if bot_token.is_empty() {
            return Err(format!("{} bot token is required.", provider.label()));
        }
        let http = Client::builder()
            .connect_timeout(Duration::from_secs(8))
            .timeout(Duration::from_secs(35))
            .build()
            .map_err(|err| format!("Failed to create remote HTTP client: {err}"))?;
        Ok(Self {
            provider,
            bot_token,
            channel_id,
            http,
        })
    }

    pub fn verify_token(&self) -> Result<(), String> {
        match self.provider {
            RemoteProvider::Discord => {
                let value = self.get_path("/users/@me")?;
                if value.get("id").and_then(Value::as_str).is_some() {
                    return Ok(());
                }
                Err("Discord token verification failed: response missing bot user id.".to_string())
            }
            RemoteProvider::Telegram => {
                let value = self.telegram_post_method("getMe", json!({}))?;
                if value
                    .get("result")
                    .and_then(|result| result.get("id"))
                    .is_some()
                {
                    return Ok(());
                }
                Err("Telegram token verification failed: response missing bot id.".to_string())
            }
        }
    }

    pub fn send_plain_message(&self, text: &str) -> Result<i64, String> {
        let channel_id = self.channel_id.as_deref().ok_or_else(|| {
            format!(
                "{} chat/channel is required for sending.",
                self.provider.label()
            )
        })?;
        let mut first_message_id: Option<i64> = None;
        for chunk in chunk_message_for_provider(self.provider, text) {
            let value = match self.provider {
                RemoteProvider::Discord => self.post_path(
                    &format!("/channels/{channel_id}/messages"),
                    json!({
                        "content": chunk,
                        "allowed_mentions": { "parse": [] }
                    }),
                )?,
                RemoteProvider::Telegram => self.telegram_post_method(
                    "sendMessage",
                    json!({
                        "chat_id": channel_id,
                        "text": chunk,
                        "disable_web_page_preview": true
                    }),
                )?,
            };
            if first_message_id.is_none() {
                first_message_id = message_id_from_response(self.provider, &value);
            }
        }
        first_message_id.ok_or_else(|| {
            format!(
                "{} create message response missing id.",
                self.provider.label()
            )
        })
    }

    #[allow(dead_code)]
    pub fn send_html_message(
        &self,
        chat_id: &str,
        html_text: &str,
        _reply_to_message_id: Option<i64>,
    ) -> Result<i64, String> {
        self.send_html_message_with_markup(chat_id, html_text, None, None)
    }

    pub fn send_html_message_with_markup(
        &self,
        chat_id: &str,
        html_text: &str,
        _reply_to_message_id: Option<i64>,
        _reply_markup: Option<Value>,
    ) -> Result<i64, String> {
        let content = match self.provider {
            RemoteProvider::Discord => discord_text_from_html(html_text),
            RemoteProvider::Telegram => html_text.to_string(),
        };
        let mut first_message_id: Option<i64> = None;
        for chunk in chunk_message_for_provider(self.provider, &content) {
            let value = match self.provider {
                RemoteProvider::Discord => self.post_path(
                    &format!("/channels/{}/messages", chat_id.trim()),
                    json!({
                        "content": chunk,
                        "allowed_mentions": { "parse": [] }
                    }),
                )?,
                RemoteProvider::Telegram => {
                    let mut payload = json!({
                        "chat_id": chat_id.trim(),
                        "text": chunk,
                        "parse_mode": "HTML",
                        "disable_web_page_preview": true
                    });
                    if let Some(markup) = _reply_markup.clone() {
                        payload["reply_markup"] = markup;
                    }
                    self.telegram_post_method("sendMessage", payload)?
                }
            };
            if first_message_id.is_none() {
                first_message_id = message_id_from_response(self.provider, &value);
            }
        }
        first_message_id.ok_or_else(|| {
            format!(
                "{} create message response missing id.",
                self.provider.label()
            )
        })
    }

    pub fn edit_html_message_with_markup(
        &self,
        chat_id: &str,
        message_id: i64,
        html_text: &str,
        reply_markup: Option<Value>,
    ) -> Result<(), String> {
        match self.provider {
            RemoteProvider::Discord => {
                let content = discord_text_from_html(html_text);
                let _ = self.patch_path(
                    &format!("/channels/{}/messages/{}", chat_id.trim(), message_id),
                    json!({
                        "content": chunk_discord_message(&content).first().cloned().unwrap_or_default(),
                        "allowed_mentions": { "parse": [] }
                    }),
                )?;
            }
            RemoteProvider::Telegram => {
                let mut payload = json!({
                    "chat_id": chat_id.trim(),
                    "message_id": message_id,
                    "text": html_text,
                    "parse_mode": "HTML",
                    "disable_web_page_preview": true
                });
                if let Some(markup) = reply_markup {
                    payload["reply_markup"] = markup;
                }
                let _ = self.telegram_post_method("editMessageText", payload)?;
            }
        }
        Ok(())
    }

    pub fn answer_callback_query(
        &self,
        callback_query_id: &str,
        text: Option<&str>,
    ) -> Result<(), String> {
        if self.provider == RemoteProvider::Telegram {
            let mut payload = json!({ "callback_query_id": callback_query_id });
            if let Some(text) = text {
                payload["text"] = json!(text);
            }
            let _ = self.telegram_post_method("answerCallbackQuery", payload)?;
        }
        Ok(())
    }

    pub fn get_updates(
        &self,
        offset: Option<i64>,
        timeout_secs: i64,
    ) -> Result<(Option<i64>, Vec<Value>), String> {
        if self.provider == RemoteProvider::Telegram {
            return self.telegram_get_updates(offset, timeout_secs);
        }
        let channel_id = self
            .channel_id
            .as_deref()
            .ok_or_else(|| "Discord DM channel is required for polling.".to_string())?;
        let mut path = format!("/channels/{channel_id}/messages?limit=20");
        if let Some(offset) = offset {
            path.push_str("&after=");
            path.push_str(&offset.to_string());
        }
        let value = self.get_path(&path)?;
        let messages = value
            .as_array()
            .cloned()
            .ok_or_else(|| "Discord messages response was not an array.".to_string())?;
        if offset.is_none() {
            let next_offset = messages
                .iter()
                .filter_map(|message| {
                    message
                        .get("id")
                        .and_then(Value::as_str)
                        .and_then(|id| id.parse::<i64>().ok())
                })
                .max()
                .map(|id| id.saturating_add(1));
            return Ok((next_offset, Vec::new()));
        }

        let mut updates = Vec::new();
        let mut max_id: Option<i64> = offset;
        for message in messages.into_iter().rev() {
            let Some(id) = message
                .get("id")
                .and_then(Value::as_str)
                .and_then(|id| id.parse::<i64>().ok())
            else {
                continue;
            };
            max_id = Some(max_id.map(|current| current.max(id)).unwrap_or(id));
            if message
                .get("author")
                .and_then(|author| author.get("bot"))
                .and_then(Value::as_bool)
                .unwrap_or(false)
            {
                continue;
            }
            updates.push(discord_message_to_update(channel_id, &message, id));
        }

        Ok((max_id.map(|id| id.saturating_add(1)), updates))
    }

    fn telegram_get_updates(
        &self,
        offset: Option<i64>,
        timeout_secs: i64,
    ) -> Result<(Option<i64>, Vec<Value>), String> {
        let mut payload = json!({
            "limit": 20,
            "timeout": timeout_secs.clamp(0, 30),
            "allowed_updates": ["message", "callback_query"]
        });
        if let Some(offset) = offset {
            payload["offset"] = json!(offset);
        }
        let value = self.telegram_post_method("getUpdates", payload)?;
        let updates = value
            .get("result")
            .and_then(Value::as_array)
            .cloned()
            .ok_or_else(|| "Telegram getUpdates response missing result array.".to_string())?;
        let next_offset = updates
            .iter()
            .filter_map(|update| update.get("update_id").and_then(Value::as_i64))
            .max()
            .map(|id| id.saturating_add(1));
        if offset.is_none() {
            return Ok((next_offset, Vec::new()));
        }
        Ok((next_offset.or(offset), updates))
    }

    fn get_path(&self, path: &str) -> Result<Value, String> {
        let url = discord_url(path);
        let response = self
            .http
            .get(&url)
            .headers(self.headers()?)
            .send()
            .map_err(|err| format!("Discord request `{path}` failed: {err}"))?;
        self.parse_response(path, response)
    }

    fn telegram_post_method(&self, method: &str, payload: Value) -> Result<Value, String> {
        let url = telegram_url(&self.bot_token, method);
        let response = self
            .http
            .post(&url)
            .header(CONTENT_TYPE, "application/json")
            .json(&payload)
            .send()
            .map_err(|err| format!("Telegram request `{method}` failed: {err}"))?;
        self.parse_telegram_response(method, response)
    }

    fn post_path(&self, path: &str, payload: Value) -> Result<Value, String> {
        let url = discord_url(path);
        let response = self
            .http
            .post(&url)
            .headers(self.headers()?)
            .json(&payload)
            .send()
            .map_err(|err| format!("Discord request `{path}` failed: {err}"))?;
        self.parse_response(path, response)
    }

    fn create_dm_channel(&self, user_id: &str) -> Result<String, String> {
        let value = self.post_path(
            "/users/@me/channels",
            json!({
                "recipient_id": user_id.trim()
            }),
        )?;
        value
            .get("id")
            .and_then(Value::as_str)
            .map(str::to_string)
            .ok_or_else(|| "Discord DM channel response missing id.".to_string())
    }

    fn patch_path(&self, path: &str, payload: Value) -> Result<Value, String> {
        let url = discord_url(path);
        let response = self
            .http
            .patch(&url)
            .headers(self.headers()?)
            .json(&payload)
            .send()
            .map_err(|err| format!("Discord request `{path}` failed: {err}"))?;
        self.parse_response(path, response)
    }

    fn headers(&self) -> Result<HeaderMap, String> {
        let mut headers = HeaderMap::new();
        let auth = format!("Bot {}", self.bot_token);
        headers.insert(
            AUTHORIZATION,
            HeaderValue::from_str(&auth).map_err(|err| format!("Invalid Discord token: {err}"))?,
        );
        headers.insert(CONTENT_TYPE, HeaderValue::from_static("application/json"));
        headers.insert(USER_AGENT, HeaderValue::from_static("CodexEditor/0.1"));
        Ok(headers)
    }

    fn parse_response(
        &self,
        path: &str,
        response: reqwest::blocking::Response,
    ) -> Result<Value, String> {
        let status = response.status();
        let parsed: Value = response.json().map_err(|err| {
            format!("Discord request `{path}` returned invalid JSON: {err} (status: {status})")
        })?;
        if !status.is_success() {
            return Err(discord_error_from_response(
                &parsed,
                &format!("Discord request `{path}` failed with HTTP {status}"),
            ));
        }
        Ok(parsed)
    }

    fn parse_telegram_response(
        &self,
        method: &str,
        response: reqwest::blocking::Response,
    ) -> Result<Value, String> {
        let status = response.status();
        let parsed: Value = response.json().map_err(|err| {
            format!("Telegram request `{method}` returned invalid JSON: {err} (status: {status})")
        })?;
        let ok = parsed.get("ok").and_then(Value::as_bool).unwrap_or(false);
        if !status.is_success() || !ok {
            let description = parsed
                .get("description")
                .and_then(Value::as_str)
                .unwrap_or("Telegram request failed");
            return Err(format!(
                "Telegram request `{method}` failed with HTTP {status}: {description}"
            ));
        }
        Ok(parsed)
    }
}

fn discord_url(path: &str) -> String {
    let path = path.trim_start_matches('/');
    format!("https://discord.com/api/v10/{path}")
}

fn telegram_url(bot_token: &str, method: &str) -> String {
    format!(
        "https://api.telegram.org/bot{}/{}",
        bot_token.trim(),
        method.trim_start_matches('/')
    )
}

fn discord_message_to_update(channel_id: &str, message: &Value, message_id: i64) -> Value {
    let author = message.get("author").cloned().unwrap_or(Value::Null);
    let author_id = author.get("id").and_then(Value::as_str).unwrap_or("");
    let username = author
        .get("global_name")
        .or_else(|| author.get("username"))
        .and_then(Value::as_str)
        .unwrap_or("");
    let content = message
        .get("content")
        .and_then(Value::as_str)
        .unwrap_or("")
        .to_string();

    json!({
        "update_id": message_id,
        "message": {
            "message_id": message_id,
            "text": content,
            "chat": { "id": channel_id },
            "from": {
                "id": author_id,
                "username": username,
                "is_bot": false
            },
            "reply_to_message": message
                .get("referenced_message")
                .and_then(|referenced| referenced.get("id"))
                .and_then(Value::as_str)
                .and_then(|id| id.parse::<i64>().ok())
                .map(|id| json!({ "message_id": id }))
                .unwrap_or(Value::Null)
        }
    })
}

fn discord_text_from_html(input: &str) -> String {
    input
        .replace("<b>", "**")
        .replace("</b>", "**")
        .replace("<code>", "`")
        .replace("</code>", "`")
        .replace("<pre>", "```")
        .replace("</pre>", "```")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&amp;", "&")
        .replace("<br>", "\n")
}

fn message_id_from_response(provider: RemoteProvider, value: &Value) -> Option<i64> {
    match provider {
        RemoteProvider::Discord => value
            .get("id")
            .and_then(Value::as_str)
            .and_then(|id| id.parse::<i64>().ok()),
        RemoteProvider::Telegram => value
            .get("result")
            .and_then(|result| result.get("message_id"))
            .and_then(Value::as_i64),
    }
}

fn chunk_message_for_provider(provider: RemoteProvider, text: &str) -> Vec<String> {
    match provider {
        RemoteProvider::Discord => chunk_discord_message(text),
        RemoteProvider::Telegram => chunk_telegram_plain_message(text),
    }
}

fn chunk_discord_message(text: &str) -> Vec<String> {
    const LIMIT: usize = 1900;
    if text.len() <= LIMIT {
        return vec![text.to_string()];
    }
    let mut chunks = Vec::new();
    let mut remaining = text.trim();
    while remaining.len() > LIMIT {
        let split_at = remaining[..LIMIT]
            .rfind('\n')
            .or_else(|| remaining[..LIMIT].rfind(' '))
            .unwrap_or(LIMIT);
        let (head, tail) = remaining.split_at(split_at.max(1));
        chunks.push(head.trim().to_string());
        remaining = tail.trim_start();
    }
    if !remaining.is_empty() {
        chunks.push(remaining.to_string());
    }
    chunks
}

fn chunk_telegram_plain_message(text: &str) -> Vec<String> {
    const LIMIT: usize = 3900;
    if text.len() <= LIMIT {
        return vec![text.to_string()];
    }
    let mut chunks = Vec::new();
    let mut remaining = text.trim();
    while remaining.len() > LIMIT {
        let split_at = remaining[..LIMIT]
            .rfind('\n')
            .or_else(|| remaining[..LIMIT].rfind(' '))
            .unwrap_or(LIMIT);
        let (head, tail) = remaining.split_at(split_at.max(1));
        chunks.push(head.trim().to_string());
        remaining = tail.trim_start();
    }
    if !remaining.is_empty() {
        chunks.push(remaining.to_string());
    }
    chunks
}

fn discord_error_from_response(value: &Value, default_message: &str) -> String {
    let message = value
        .get("message")
        .and_then(Value::as_str)
        .map(str::trim)
        .unwrap_or("");
    if message.is_empty() {
        default_message.to_string()
    } else {
        format!("{default_message}: {message}")
    }
}

fn extract_auth_match(update: &Value, expected_code: &str) -> Option<TelegramAuthMatch> {
    let message = update.get("message")?;
    let text = message
        .get("text")
        .and_then(Value::as_str)?
        .trim()
        .to_string();
    if text != expected_code {
        return None;
    }
    let chat = message.get("chat")?;
    let from = message.get("from")?;
    let chat_id = chat.get("id")?.to_string().trim_matches('"').to_string();
    let user_id = from.get("id")?.to_string().trim_matches('"').to_string();
    let username = from
        .get("username")
        .and_then(Value::as_str)
        .map(|value| value.to_string());
    Some(TelegramAuthMatch {
        chat_id,
        user_id,
        username,
    })
}
