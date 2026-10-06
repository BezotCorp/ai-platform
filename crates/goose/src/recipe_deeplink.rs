use crate::recipe::Recipe;
use anyhow::Result;
use base64::{Engine as _, engine::general_purpose::URL_SAFE_NO_PAD};
use thiserror::Error;
#[derive(Error, Debug)]
pub enum DecodeError {
    #[error("Failed to decode recipe deeplink")]
    AllMethodsFailed,
}

pub fn encode(recipe: &Recipe) -> Result<String, serde_json::Error> {
    let recipe_json = serde_json::to_string(recipe)?;
    let encoded = URL_SAFE_NO_PAD.encode(recipe_json.as_bytes());
    Ok(encoded)
}

pub fn decode(link: &str) -> Result<Recipe, DecodeError> {
    // Handle the current format: URL-safe Base64 without padding.
    if let Ok(decoded_bytes) = URL_SAFE_NO_PAD.decode(link) {
        if let Ok(recipe_json) = String::from_utf8(decoded_bytes) {
            if let Ok(recipe) = serde_json::from_str::<Recipe>(&recipe_json) {
                return Ok(recipe);
            }
        }
    }

    // Handle legacy formats of 'standard base64 encoded' and standard base64 encoded that was then url encoded.
    if let Ok(url_decoded) = urlencoding::decode(link) {
        if let Ok(decoded_bytes) =
            base64::engine::general_purpose::STANDARD.decode(url_decoded.as_bytes())
        {
            if let Ok(recipe_json) = String::from_utf8(decoded_bytes) {
                if let Ok(recipe) = serde_json::from_str::<Recipe>(&recipe_json) {
                    return Ok(recipe);
                }
            }
        }
    }

    Err(DecodeError::AllMethodsFailed)
}
