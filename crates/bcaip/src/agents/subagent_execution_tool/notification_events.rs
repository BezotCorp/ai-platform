use serde::{Deserialize, Serialize};
use serde_json::Value;
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum TaskStatus {
    Pending,
    Running,
    Completed,
    Failed,
}

impl std::fmt::Display for TaskStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            TaskStatus::Pending => write!(f, "Pending"),
            TaskStatus::Running => write!(f, "Running"),
            TaskStatus::Completed => write!(f, "Completed"),
            TaskStatus::Failed => write!(f, "Failed"),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "subtype")]
pub enum TaskExecutionNotificationEvent {
    #[serde(rename = "line_output")]
    LineOutput { task_id: String, output: String },
    #[serde(rename = "tasks_update")]
    TasksUpdate {
        stats: TaskExecutionStats,
        tasks: Vec<TaskInfo>,
    },
    #[serde(rename = "tasks_complete")]
    TasksComplete {
        stats: TaskCompletionStats,
        failed_tasks: Vec<FailedTaskInfo>,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TaskExecutionStats {
    pub total: usize,
    pub pending: usize,
    pub running: usize,
    pub completed: usize,
    pub failed: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TaskCompletionStats {
    pub total: usize,
    pub completed: usize,
    pub failed: usize,
    pub success_rate: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TaskInfo {
    pub id: String,
    pub status: TaskStatus,
    pub duration_secs: Option<f64>,
    pub current_output: String,
    pub task_type: String,
    pub task_name: String,
    pub task_metadata: String,
    pub error: Option<String>,
    pub result_data: Option<Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FailedTaskInfo {
    pub id: String,
    pub name: String,
    pub error: Option<String>,
}

impl TaskExecutionNotificationEvent {
    pub fn line_output(task_id: String, output: String) -> Self {
        Self::LineOutput { task_id, output }
    }

    pub fn tasks_update(stats: TaskExecutionStats, tasks: Vec<TaskInfo>) -> Self {
        Self::TasksUpdate { stats, tasks }
    }

    pub fn tasks_complete(stats: TaskCompletionStats, failed_tasks: Vec<FailedTaskInfo>) -> Self {
        Self::TasksComplete {
            stats,
            failed_tasks,
        }
    }

    /// Convert event to JSON format for MCP notification
    pub fn to_notification_data(&self) -> serde_json::Value {
        let mut event_data = serde_json::to_value(self).expect("Failed to serialize event");

        // Add the type field at the root level
        if let serde_json::Value::Object(ref mut map) = event_data {
            map.insert(
                "type".to_string(),
                serde_json::Value::String("task_execution".to_string()),
            );
        }

        event_data
    }
}

impl TaskExecutionStats {
    pub fn new(
        total: usize,
        pending: usize,
        running: usize,
        completed: usize,
        failed: usize,
    ) -> Self {
        Self {
            total,
            pending,
            running,
            completed,
            failed,
        }
    }
}

impl TaskCompletionStats {
    pub fn new(total: usize, completed: usize, failed: usize) -> Self {
        let success_rate = if total > 0 {
            (completed as f64 / total as f64) * 100.0
        } else {
            0.0
        };

        Self {
            total,
            completed,
            failed,
            success_rate,
        }
    }
}
