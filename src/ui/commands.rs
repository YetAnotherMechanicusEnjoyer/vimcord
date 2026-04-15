use tokio::sync::{mpsc::Sender, MutexGuard};

use crate::{
    logs::{print_log, LogType},
    App, AppAction, KeywordAction,
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
            if let (Some(status), Some(status_text)) = (args.first(), args.get(1)) {
                if let Err(e) = state
                    .api_client
                    .modify_user_settings(serde_json::json!({
                        "custom_status": {
                            "text": status_text,
                            //"emoji_name": emoji.name,
                            //"emoji_id": emoji.id,
                            //"expires_at": never (for now)
                        },
                        "status": status,
                    }))
                    .await
                {
                    print_log(
                        format!("Failed to change status: {e}").into(),
                        LogType::Error,
                    )
                    .await
                    .ok();
                }
            } else {
                print_log(
                    format!(
                        "Failed to change status: Bad usage: \"status <online|dnd|idle|invisible|offline> <text>\": {:?}",
                        args
                    )
                    .into(),
                    LogType::Error,
                )
                .await
                .ok();
            }
        }
        _ => {}
    }
    None
}
