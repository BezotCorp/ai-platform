use serde::Deserialize;

#[derive(Debug, Deserialize)]
#[serde(tag = "type")]
pub(crate) enum Command {
    #[serde(rename = "authenticate")]
    Authenticate {
        token: String,
    },
    #[serde(rename = "models.list")]
    ModelsList {
        request_id: String,
    },
    #[serde(rename = "run.cancel")]
    RunCancel {
        request_id: String,
    },
}
