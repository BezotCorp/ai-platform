use crate::agents::extension::{Envs, ExtensionConfig};
use serde::{Deserialize, de::Deserializer};
use std::collections::HashMap;
#[derive(Deserialize)]
#[serde(tag = "type")]
enum RecipeExtensionConfigInternal {
    #[serde(rename = "stdio")]
    Stdio {
        name: String,
        #[serde(default)]
        description: Option<String>,
        cmd: String,
        args: Vec<String>,
        #[serde(default)]
        envs: Envs,
        #[serde(default)]
        env_keys: Vec<String>,
        timeout: Option<u64>,
        #[serde(default)]
        cwd: Option<String>,
        #[serde(default)]
        bundled: Option<bool>,
        #[serde(default)]
        available_tools: Vec<String>,
    },
    #[serde(rename = "builtin")]
    Builtin {
        name: String,
        #[serde(default)]
        description: Option<String>,
        display_name: Option<String>,
        timeout: Option<u64>,
        #[serde(default)]
        bundled: Option<bool>,
        #[serde(default)]
        available_tools: Vec<String>,
    },
    #[serde(rename = "platform")]
    Platform {
        name: String,
        #[serde(default)]
        description: Option<String>,
        #[serde(default)]
        display_name: Option<String>,
        #[serde(default)]
        bundled: Option<bool>,
        #[serde(default)]
        available_tools: Vec<String>,
    },
    #[serde(rename = "streamable_http")]
    StreamableHttp {
        name: String,
        #[serde(default)]
        description: Option<String>,
        uri: String,
        #[serde(default)]
        envs: Envs,
        #[serde(default)]
        env_keys: Vec<String>,
        #[serde(default)]
        headers: HashMap<String, String>,
        timeout: Option<u64>,
        #[serde(default)]
        socket: Option<String>,
        #[serde(default)]
        client_id: Option<String>,
        #[serde(default)]
        client_secret_key: Option<String>,
        #[serde(default)]
        scopes: Vec<String>,
        #[serde(default)]
        bundled: Option<bool>,
        #[serde(default)]
        available_tools: Vec<String>,
    },
}

macro_rules! map_recipe_extensions {
    ($value:expr; $( $variant:ident { $( $field:ident ),* $(,)? } ),+ $(,)?) => {{
        match $value {
            $(
                RecipeExtensionConfigInternal::$variant {
                    name,
                    description,
                    $( $field ),*
                } => ExtensionConfig::$variant {
                    name,
                    description: description.unwrap_or_default(),
                    $( $field ),*
                },
            )+
        }
    }};
}

impl From<RecipeExtensionConfigInternal> for ExtensionConfig {
    fn from(internal_variant: RecipeExtensionConfigInternal) -> Self {
        map_recipe_extensions!(
        internal_variant;
            Stdio {
                cmd,
                args,
                envs,
                env_keys,
                timeout,
                cwd,
                bundled,
                available_tools
            },
            Builtin {
                display_name,
                timeout,
                bundled,
                available_tools
            },
            Platform {
                display_name,
                bundled,
                available_tools
            },
            StreamableHttp {
                uri,
                envs,
                env_keys,
                headers,
                timeout,
                socket,
                client_id,
                client_secret_key,
                scopes,
                bundled,
                available_tools
            }
        )
    }
}

pub fn deserialize_recipe_extensions<'de, D>(
    deserializer: D,
) -> Result<Option<Vec<ExtensionConfig>>, D::Error>
where
    D: Deserializer<'de>,
{
    let remotes = Option::<Vec<RecipeExtensionConfigInternal>>::deserialize(deserializer)?;
    Ok(remotes.map(|items| items.into_iter().map(ExtensionConfig::from).collect()))
}
