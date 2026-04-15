use tokio::sync::{MutexGuard, mpsc::Sender};

use crate::{
    App, AppAction, KeywordAction,
    logs::{LogType, print_log},
};

pub async fn handle_command(
    state: &mut MutexGuard<'_, App>,
    tx_action: &Sender<AppAction>,
    input: String,
) -> Option<KeywordAction> {
    let (cmd, args) = {
        let mut s = input.split(' ');
        (
            s.next().unwrap_or_default().to_lowercase(),
            s.map(|s| s.to_string()).collect::<Vec<String>>(),
        )
    };

    match cmd.as_str() {
        "quit" | "q" => {
            return Some(KeywordAction::Break);
        }
        "debug" => {
            print_log(args.join(" ").as_str().into(), LogType::Debug)
                .await
                .ok();
        }
        "logs" => {
            tx_action.send(AppAction::TransitionToLogs).await.ok();
        }
        "status" => {
            if let Some(status) = args.first() {
                let status_text = if args.len() > 1 {
                    Some(args[1..].join(" "))
                } else {
                    None
                };

                let mut settings_payload = serde_json::json!({
                    "status": status,
                });

                if let Some(text) = &status_text {
                    settings_payload["custom_status"] = serde_json::json!({
                        "text": text,
                    });
                } else {
                    settings_payload["custom_status"] = serde_json::json!(null);
                }

                if let Err(e) = state
                    .api_client
                    .modify_user_settings(settings_payload)
                    .await
                {
                    print_log(
                        format!("Failed to change status settings: {e}").into(),
                        LogType::Error,
                    )
                    .await
                    .ok();
                }

                if let Err(e) = state
                    .gateway_client
                    .update_presence(status, status_text.as_deref())
                    .await
                {
                    print_log(
                        format!("Failed to update live presence: {e}").into(),
                        LogType::Error,
                    )
                    .await
                    .ok();
                }
            } else {
                print_log(
                    "Failed to change status: Bad usage: \"status <online|dnd|idle|invisible|invisible_dnd> [text]\"".into(),
                    LogType::Error,
                ).await.ok();
            }
        }
        _ => {}
    }
    None
}
