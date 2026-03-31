use futures_util::{SinkExt, StreamExt};
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use tokio::sync::mpsc::Sender;
use tokio::sync::{Mutex, mpsc};
use tokio::time::{self, Duration};
use tokio_tungstenite::{connect_async, tungstenite::protocol::Message as WsMessage};

use crate::Error;
use crate::api::guild::GuildMember;
use crate::logs::{LogType, print_log};
use crate::{AppAction, api::Message as DiscordMessage};

const GATEWAY_URL: &str = "wss://gateway.discord.gg/?v=10&encoding=json";

#[derive(Serialize)]
struct GatewayCommand {
    op: u8,
    d: serde_json::Value,
}

#[derive(Deserialize, Debug)]
struct GatewayEvent {
    op: u8,
    d: Option<serde_json::Value>,
    s: Option<u64>,
    t: Option<String>,
}

#[derive(Debug, Clone)]
pub struct GatewayClient {
    token: String,
    action_tx: Sender<AppAction>,
    sequence: Arc<Mutex<Option<u64>>>,
    outbound_tx: Sender<serde_json::Value>,
    outbound_rx: Arc<Mutex<mpsc::Receiver<serde_json::Value>>>,
}

impl Default for GatewayClient {
    fn default() -> Self {
        let (tx, rx) = mpsc::channel(32);
        Self {
            token: String::new(),
            action_tx: mpsc::channel::<AppAction>(32).0,
            sequence: Arc::new(Mutex::new(None)),
            outbound_tx: tx,
            outbound_rx: Arc::new(Mutex::new(rx)),
        }
    }
}

impl GatewayClient {
    pub fn new(token: String, action_tx: Sender<AppAction>) -> Self {
        let (tx, rx) = mpsc::channel(32);
        Self {
            token,
            action_tx,
            sequence: Arc::new(Mutex::new(None)),
            outbound_tx: tx,
            outbound_rx: Arc::new(Mutex::new(rx)),
        }
    }

    pub async fn connect(
        &self,
        mut rx_shutdown: tokio::sync::broadcast::Receiver<()>,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        let (ws_stream, _) = connect_async(GATEWAY_URL).await?;
        let (write, mut read) = ws_stream.split();

        let sequence = self.sequence.clone();
        let token = self.token.clone();
        let action_tx = self.action_tx.clone();

        // Wait for Hello to get heartbeat interval
        let heartbeat_interval = if let Some(Ok(msg)) = read.next().await {
            if let WsMessage::Text(text) = msg {
                let event: GatewayEvent = serde_json::from_str(&text)?;
                if event.op == 10 {
                    // Hello
                    let hello_data = event.d.unwrap();
                    hello_data["heartbeat_interval"].as_u64().unwrap_or(41250)
                } else {
                    return Err("Expected Hello".into());
                }
            } else {
                return Err("Expected Text Message".into());
            }
        } else {
            return Err("Connection Closed Before Hello".into());
        };

        let identify = serde_json::json!({
            "op": 2, // Identify
            "d": {
                "token": token,
                "intents": 50364035,
                "capabilities": 30717,
                "properties": {
                    "os": "linux",
                    "browser": "vimcord",
                    "device": "vimcord"
                }
            }
        });

        let write = Arc::new(Mutex::new(write));
        {
            let mut w = write.lock().await;
            w.send(WsMessage::Text(serde_json::to_string(&identify)?.into()))
                .await?;
        }

        // Start heartbeat task
        let write_clone = Arc::clone(&write);
        let seq_clone = Arc::clone(&sequence);
        let heartbeat_task = tokio::spawn(async move {
            let mut interval = time::interval(Duration::from_millis(heartbeat_interval));
            loop {
                interval.tick().await;
                let seq = *seq_clone.lock().await;
                let op = GatewayCommand {
                    op: 1, // Heartbeat
                    d: serde_json::json!(seq),
                };
                let msg = WsMessage::Text(serde_json::to_string(&op).unwrap().into());
                let mut w = write_clone.lock().await;
                if let Err(e) = w.send(msg).await {
                    let _ = print_log(format!("Heartbeat failed: {}", e).into(), LogType::Error);
                    break;
                }
            }
        });

        let write_for_outbound = Arc::clone(&write);
        let mut rx_outbound = self.outbound_rx.lock().await;

        // Listen for events
        loop {
            tokio::select! {
                _ = rx_shutdown.recv() => {
                    break;
                }
                Some(outbound_msg) = rx_outbound.recv() => {
                    let mut w = write_for_outbound.lock().await;
                    if let Err(e) = w.send(WsMessage::Text(serde_json::to_string(&outbound_msg).unwrap().into())).await {
                        let _ = print_log(format!("Failed to send outbound message: {}", e).into(), LogType::Error);
                    }
                }
                msg_result = read.next() => {
                    match msg_result {
                        Some(Ok(WsMessage::Text(text))) => {
                            if let Ok(event) = serde_json::from_str::<GatewayEvent>(&text) {
                                if let Some(s) = event.s {
                                    let mut seq = sequence.lock().await;
                                    *seq = Some(s);
                                }

                                if event.op == 0 {
                                    // Dispatch
                                    if let (Some(t), Some(d)) = (event.t, event.d) {
                                        Self::handle_dispatch(&t, d, &action_tx).await;
                                    }
                                }
                            }
                        }
                        Some(Ok(WsMessage::Close(_))) => {
                            break;
                        }
                        Some(Err(e)) => {
                            let _ = print_log(format!("Gateway error: {}", e).into(), LogType::Error);
                            break;
                        }
                        None => {
                            let _ = print_log("Gateway connection closed unexpectedly".into(), LogType::Error);
                            break;
                        }
                        _ => {}
                    }
                }
            }
        }

        heartbeat_task.abort();
        Ok(())
    }

    async fn handle_dispatch(t: &str, d: serde_json::Value, action_tx: &Sender<AppAction>) {
        match t {
            "MESSAGE_CREATE" => {
                if let Ok(msg) = serde_json::from_value::<DiscordMessage>(d) {
                    let _ = action_tx.send(AppAction::GatewayMessageCreate(msg)).await;
                }
            }
            "MESSAGE_UPDATE" => {
                if let Ok(msg) = serde_json::from_value::<crate::api::PartialMessage>(d) {
                    let _ = action_tx.send(AppAction::GatewayMessageUpdate(msg)).await;
                }
            }
            "MESSAGE_DELETE" => {
                if let (Some(id), Some(channel_id)) = (d["id"].as_str(), d["channel_id"].as_str()) {
                    let _ = action_tx
                        .send(AppAction::GatewayMessageDelete(
                            id.to_string(),
                            channel_id.to_string(),
                        ))
                        .await;
                }
            }
            "TYPING_START" => {
                if let (Some(channel_id), Some(user_id), Some(_timestamp)) = (
                    d["channel_id"].as_str(),
                    d["user_id"].as_str(),
                    d["timestamp"].as_u64(),
                ) {
                    // Prefer guild nick, fall back to member username, then top-level user (DMs)
                    let display_name = d["member"]["nick"]
                        .as_str()
                        .or_else(|| d["member"]["user"]["username"].as_str())
                        .or_else(|| d["user"]["username"].as_str())
                        .map(|s| s.to_string());
                    let _ = action_tx
                        .send(AppAction::GatewayTypingStart(
                            channel_id.to_string(),
                            user_id.to_string(),
                            display_name,
                        ))
                        .await;
                }
            }
            "READY_SUPPLEMENTAL" => {
                let mut statuses = std::collections::HashMap::new();
                if let Some(guilds) = d["merged_presences"]["guilds"].as_array() {
                    for guild in guilds {
                        if let Some(users) = guild.as_array() {
                            for user in users {
                                if let (Some(user_id), Some(status)) =
                                    (user["user_id"].as_str(), user["status"].as_str())
                                {
                                    statuses.insert(user_id.to_string(), status.to_string());
                                }
                            }
                        }
                    }
                }
                if let Some(friends) = d["merged_presences"]["friends"].as_array() {
                    for friend in friends {
                        if let (Some(user_id), Some(status)) =
                            (friend["user_id"].as_str(), friend["status"].as_str())
                        {
                            statuses.insert(user_id.to_string(), status.to_string());
                        }
                    }
                }
                let _ = action_tx
                    .send(AppAction::GatewayReadySupplemental(statuses))
                    .await;
            }
            "PRESENCE_UPDATE" => {
                if let (Some(user_id), Some(status)) =
                    (d["user"]["id"].as_str(), d["status"].as_str())
                {
                    let _ = action_tx
                        .send(AppAction::GatewayPresenceUpdate(
                            user_id.to_string(),
                            status.to_string(),
                        ))
                        .await;
                }
            }
            "GUILD_MEMBERS_CHUNK" => {
                print_log("GUILD_MEMBERS_CHUNK event received".into(), LogType::Debug).ok();
                if let Ok(not_found) = serde_json::from_value::<Vec<String>>(d["not_found"].clone())
                {
                    print_log(
                        format!("Error in GUILD_MEMBERS_CHUNK event: not found: {not_found:?}")
                            .into(),
                        LogType::Error,
                    )
                    .ok();
                }
                if let (Some(guild_id), Ok(members), Some(chunk_index), Some(chunk_count)) = (
                    d["guild_id"].as_str(),
                    serde_json::from_value::<Vec<GuildMember>>(d["members"].clone()),
                    d["chunk_index"].as_str(),
                    d["chunk_count"].as_str(),
                ) {
                    print_log(
                        format!("GUILD_MEMBERS_CHUNK event parsed: {members:?}").into(),
                        LogType::Debug,
                    )
                    .ok();
                    action_tx
                        .send(AppAction::GatewayGuildMembersChunk(
                            guild_id.to_string(),
                            members.clone(),
                            chunk_index.to_string(),
                            chunk_count.to_string(),
                        ))
                        .await
                        .ok();
                }
            }
            e => {
                print_log(
                    format!("Unhandled event received: {e}").into(),
                    LogType::Info,
                )
                .ok();
            }
        }
    }

    pub async fn request_guild_members(&self, guild_id: &str) -> Result<(), Error> {
        let request = serde_json::json!({
            "op": 8,
            "d": {
                "guild_id": guild_id,
                "query": "",
                "limit": 0,
            }
        });

        if let Err(e) = self.outbound_tx.send(request).await {
            print_log(
                format!("Error sending request guild members: {e}").into(),
                LogType::Error,
            )
            .ok();
        }

        Ok(())
    }
}
