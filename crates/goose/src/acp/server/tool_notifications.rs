use crate::agents::platform_extensions::developer::shell::{
    ShellOutputNotificationParams, parse_shell_output_notification,
};
use agent_client_protocol::schema::v1::{
    Meta, ToolCallId, ToolCallStatus, ToolCallUpdate, ToolCallUpdateFields,
};
#[expect(deprecated)]
use rmcp::model::LoggingMessageNotificationParam;
use rmcp::model::{ProgressNotificationParam, ServerNotification};
use serde::Serialize;

#[expect(deprecated)]
#[derive(Debug, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
enum ToolNotification {
    Message {
        params: LoggingMessageNotificationParam,
    },
    Progress {
        params: ProgressNotificationParam,
    },
    PlatformEvent {
        params: serde_json::Value,
    },
    LiveOutput {
        params: ShellOutputNotificationParams,
    },
}

pub(super) fn tool_notification_update(
    tool_call_id: impl Into<ToolCallId>,
    notification: ServerNotification,
) -> Option<ToolCallUpdate> {
    let tool_notification = match notification {
        ServerNotification::LoggingMessageNotification(notification) => ToolNotification::Message {
            params: notification.params,
        },
        ServerNotification::ProgressNotification(notification) => ToolNotification::Progress {
            params: notification.params,
        },
        ServerNotification::CustomNotification(notification) => {
            if let Some(params) = parse_shell_output_notification(&notification) {
                ToolNotification::LiveOutput { params }
            } else if notification.method == "platform_event" {
                ToolNotification::PlatformEvent {
                    params: notification.params.unwrap_or(serde_json::Value::Null),
                }
            } else {
                return None;
            }
        }
        _ => return None,
    };

    let mut meta = Meta::new();
    meta.insert(
        "toolNotification".to_string(),
        serde_json::to_value(tool_notification).ok()?,
    );

    Some(
        ToolCallUpdate::new(
            tool_call_id,
            ToolCallUpdateFields::new().status(ToolCallStatus::InProgress),
        )
        .meta(meta),
    )
}
