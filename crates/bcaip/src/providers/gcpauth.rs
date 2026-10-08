use async_trait::async_trait;
use jsonwebtoken::{EncodingKey, Header, encode};
use serde::{Deserialize, Serialize};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};
use std::{env, fmt, io};
use std::{path::PathBuf, sync::Arc};
use tokio::sync::RwLock;
/// Represents errors that can occur during GCP authentication.
///
/// This enum encompasses various error conditions that might arise during
/// the authentication process, including credential loading, token creation,
/// and token exchange operations.
#[derive(Debug, thiserror::Error)]
pub enum AuthError {
    /// Error when loading credentials from the filesystem or environment
    #[error("Failed to load credentials: {0}")]
    Credentials(String),

    /// Error during JWT token creation
    #[error("Token creation failed: {0}")]
    TokenCreation(String),

    /// Error during OAuth token exchange
    #[error("Token exchange failed: {0}")]
    TokenExchange(String),
}

/// Represents an authentication token with its type and value.
///
/// This structure holds both the token type (e.g., "Bearer") and its
/// actual value, typically used for authentication with GCP services.
/// The token is obtained either through service account or user credentials.
#[derive(Debug, Clone)]
pub struct AuthToken {
    /// The type of the token (e.g., "Bearer")
    pub token_type: String,
    /// The actual token value
    pub token_value: String,
}

impl fmt::Display for AuthToken {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{} {}", self.token_type, self.token_value)
    }
}

/// Represents the types of Application Default Credentials (ADC) supported.
///
/// GCP supports multiple credential types for authentication. This enum
/// represents the two main types: authorized user and service account.
#[derive(Debug, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
enum AdcCredentials {
    /// Credentials for an authorized user (typically from gcloud auth)
    AuthorizedUser(AuthorizedUserCredentials),
    /// Credentials for a service account
    ServiceAccount(ServiceAccountCredentials),
    /// Credentials for the GCP native default account
    #[serde(skip)]
    DefaultAccount(String),
}

/// Credentials for an authorized user account.
///
/// These credentials are typically obtained through interactive login
/// with the gcloud CLI tool.
#[derive(Debug, Deserialize)]
struct AuthorizedUserCredentials {
    /// OAuth 2.0 client ID
    client_id: String,
    /// OAuth 2.0 client secret
    client_secret: String,
    /// OAuth 2.0 refresh token
    refresh_token: String,
    /// URI for token refresh requests
    #[serde(default = "default_token_uri")]
    token_uri: String,
}

/// Credentials for a service account.
///
/// These credentials are typically obtained from a JSON key file
/// downloaded from the Google Cloud Console.
#[derive(Debug, Deserialize)]
struct ServiceAccountCredentials {
    /// Service account email address
    client_email: String,
    /// The private key from JSON credential for signing JWT tokens
    private_key: String,
    /// URI for token exchange requests
    token_uri: String,
}

/// Returns the default OAuth 2.0 token endpoint.
fn default_token_uri() -> String {
    "https://oauth2.googleapis.com/token".to_string()
}

/// A trait that defines operations for interacting with the filesystem.
///
/// This trait provides an abstraction over filesystem operations, primarily
/// for reading credential files. It enables testing through mock implementations.
#[async_trait]
pub trait FilesystemOps {
    /// Reads the contents of a file into a string.
    ///
    /// # Arguments
    /// * `path` - The path to the file to read
    ///
    /// # Returns
    /// * `Result<String, io::Error>` - The contents of the file or an error
    async fn read_to_string(&self, path: String) -> Result<String, io::Error>;
}

/// A trait that defines operations for accessing environment variables.
///
/// This trait provides an abstraction over environment variable access,
/// enabling testing through mock implementations.
pub trait EnvOps {
    /// Retrieves the value of an environment variable.
    ///
    /// # Arguments
    /// * `key` - The name of the environment variable
    ///
    /// # Returns
    /// * `Result<String, env::VarError>` - The value of the variable or an error if not found
    fn get_var(&self, key: &str) -> Result<String, env::VarError>;
}

/// A concrete implementation of FilesystemOps using the actual filesystem.
///
/// This implementation uses tokio's async filesystem operations for
/// reading files in an asynchronous manner.
pub struct RealFilesystemOps;

/// A concrete implementation of EnvOps using the actual environment.
///
/// This implementation directly accesses system environment variables
/// through the standard library.
pub struct RealEnvOps;

#[async_trait]
impl FilesystemOps for RealFilesystemOps {
    async fn read_to_string(&self, path: String) -> Result<String, io::Error> {
        tokio::fs::read_to_string(path).await
    }
}

impl EnvOps for RealEnvOps {
    fn get_var(&self, key: &str) -> Result<String, env::VarError> {
        env::var(key)
    }
}

impl AdcCredentials {
    /// Loads credentials from the default locations.
    /// https://cloud.google.com/docs/authentication/application-default-credentials#personal
    ///
    /// Attempts to load credentials in the following order:
    /// 1. GOOGLE_APPLICATION_CREDENTIALS environment variable
    /// 2. Default gcloud credentials path (~/.config/gcloud/application_default_credentials.json)
    /// 3. Metadata server if running in GCP
    async fn load() -> Result<Self, AuthError> {
        Self::load_impl(
            &RealFilesystemOps,
            &RealEnvOps,
            "http://metadata.google.internal",
        )
        .await
    }

    async fn load_impl(
        fs_ops: &impl FilesystemOps,
        env_ops: &impl EnvOps,
        metadata_base_url: &str,
    ) -> Result<Self, AuthError> {
        match env_ops.get_var("GOOGLE_APPLICATION_CREDENTIALS") {
            Ok(cred_path) => return Self::load_from_file(fs_ops, &cred_path).await,
            Err(env::VarError::NotPresent) => {}
            Err(error) => {
                return Err(AuthError::Credentials(format!(
                    "Failed to read GOOGLE_APPLICATION_CREDENTIALS: {}",
                    error
                )));
            }
        }

        // Try default gcloud credentials path
        if let Ok(cred_path) = Self::get_default_credentials_path(env_ops) {
            match fs_ops.read_to_string(cred_path.clone()).await {
                Ok(content) => return Self::parse_file_contents(&content),
                Err(error) if error.kind() == io::ErrorKind::NotFound => {}
                Err(error) => {
                    return Err(AuthError::Credentials(format!(
                        "Failed to read credentials from {}: {}",
                        cred_path, error
                    )));
                }
            }
        }

        // Try metadata server if running on GCP
        if let Ok(creds) = Self::load_from_metadata_server(metadata_base_url).await {
            return Ok(creds);
        }

        Err(AuthError::Credentials(
            "No valid credentials found in any location".to_string(),
        ))
    }

    async fn load_from_file(fs_ops: &impl FilesystemOps, path: &str) -> Result<Self, AuthError> {
        let content = fs_ops.read_to_string(path.to_string()).await.map_err(|e| {
            AuthError::Credentials(format!("Failed to read credentials from {}: {}", path, e))
        })?;

        Self::parse_file_contents(&content)
    }

    fn parse_file_contents(content: &str) -> Result<Self, AuthError> {
        serde_json::from_str(content)
            .map_err(|e| AuthError::Credentials(format!("Invalid credentials format: {}", e)))
    }

    fn get_default_credentials_path(env_ops: &impl EnvOps) -> Result<String, AuthError> {
        let (env_var, subpath) = if cfg!(windows) {
            ("APPDATA", "gcloud\\application_default_credentials.json")
        } else {
            (
                "HOME",
                ".config/gcloud/application_default_credentials.json",
            )
        };

        env_ops
            .get_var(env_var)
            .map(|dir| {
                PathBuf::from(dir)
                    .join(subpath)
                    .to_string_lossy()
                    .into_owned()
            })
            .map_err(|_| {
                AuthError::Credentials("Could not determine user home directory".to_string())
            })
    }

    async fn load_from_metadata_server(base_url: &str) -> Result<Self, AuthError> {
        let client = reqwest::Client::new();
        let metadata_path = "/computeMetadata/v1/instance/service-accounts/default/token";

        let response = client
            .get(format!("{}{}", base_url, metadata_path))
            .header("Metadata-Flavor", "Google")
            .send()
            .await
            .map_err(|e| {
                AuthError::Credentials(format!("Metadata server request failed: {}", e))
            })?;

        if !response.status().is_success() {
            return Err(AuthError::Credentials(
                "Not running on GCP or metadata server unavailable".to_string(),
            ));
        }

        // Note: When using metadata server, we have access to the OAuth2 access token
        // that can be used to authenticate applications.
        // However, this token expires. Better to keep the base_url, and fetch a new token
        // when needed
        Ok(AdcCredentials::DefaultAccount(base_url.to_string()))
    }
}

/// Claims structure for JWT tokens.
///
/// These claims are included in the JWT token used for service account
/// authentication.
#[derive(Debug, Serialize)]
struct JwtClaims {
    /// Token issuer (service account email)
    iss: String,
    /// Token subject (service account email)
    sub: String,
    /// Service account scope within role
    scope: String,
    /// Token audience (OAuth endpoint)
    aud: String,
    /// Token issued at timestamp
    iat: u64,
    /// Token expiration timestamp
    exp: u64,
}

/// Holds a cached token and its expiration time.
///
/// Used internally to implement token caching and automatic refresh.
#[derive(Debug, Clone)]
struct CachedToken {
    /// The cached authentication token
    token: AuthToken,
    /// When the token will expire
    expires_at: Instant,
}

/// Response structure for token exchange requests.
#[derive(Debug, Deserialize, Clone)]
struct TokenResponse {
    /// The access token string
    access_token: String,
    /// Token lifetime in seconds
    expires_in: u64,
    /// Token type (e.g., "Bearer")
    #[serde(default)]
    token_type: String,
}

/// Handles authentication with Google Cloud Platform services.
///
/// This struct manages the complete authentication lifecycle including:
/// - Loading and validating credentials
/// - Creating and refreshing tokens
/// - Caching tokens for efficient reuse
/// - Managing concurrent access through atomic operations
///
/// It supports both service account and authorized user authentication methods,
/// automatically selecting the appropriate method based on available credentials.
/// ```
#[derive(Debug)]
pub struct GcpAuth {
    /// The loaded credentials (service account or authorized user)
    credentials: RwLock<AdcCredentials>,
    /// HTTP client for making token exchange requests
    client: reqwest::Client,
    /// Thread-safe cache for the current token
    cached_token: Arc<RwLock<Option<CachedToken>>>,
}

impl GcpAuth {
    /// Creates a new GCP authentication handler.
    ///
    /// Initializes the authentication handler by:
    /// 1. Loading credentials from default locations
    /// 2. Setting up an HTTP client for token requests
    /// 3. Initializing the token cache
    ///
    /// The credentials are loaded in the following order:
    /// 1. GOOGLE_APPLICATION_CREDENTIALS environment variable
    /// 2. Default gcloud credentials path
    /// 3. GCP metadata server (when running on GCP)
    ///
    /// # Returns
    /// * `Result<Self, AuthError>` - A new GcpAuth instance or an error if initialization fails
    pub async fn new() -> Result<Self, AuthError> {
        Ok(Self {
            credentials: RwLock::new(AdcCredentials::load().await?),
            client: reqwest::Client::new(),
            cached_token: Arc::new(RwLock::new(None)),
        })
    }

    pub async fn refresh_credentials(&self) -> Result<(), AuthError> {
        let reloaded = AdcCredentials::load().await?;
        *self.credentials.write().await = reloaded;
        *self.cached_token.write().await = None;
        Ok(())
    }

    /// Retrieves a valid authentication token.
    ///
    /// This method implements an efficient token management strategy:
    /// 1. Checks the cache for a valid token
    /// 2. Returns the cached token if not expired
    /// 3. Obtains a new token if needed or expired
    /// 4. Uses double-checked locking for thread safety
    ///
    /// The returned token includes a type (usually "Bearer") and the actual
    /// token value used for authentication with GCP services.
    ///
    /// # Returns
    /// * `Result<AuthToken, AuthError>` - A valid authentication token or an error
    pub async fn get_token(&self) -> Result<AuthToken, AuthError> {
        // Try read lock first for better concurrency
        if let Some(cached) = self.cached_token.read().await.as_ref()
            && cached.expires_at > Instant::now()
        {
            return Ok(cached.token.clone());
        }

        // Take write lock only if needed
        let mut token_guard = self.cached_token.write().await;

        // Double-check expiration after acquiring write lock
        if let Some(cached) = token_guard.as_ref()
            && cached.expires_at > Instant::now()
        {
            return Ok(cached.token.clone());
        }

        // Get new token
        let token_response = match &*self.credentials.read().await {
            AdcCredentials::ServiceAccount(creds) => self.get_service_account_token(creds).await?,
            AdcCredentials::AuthorizedUser(creds) => self.get_authorized_user_token(creds).await?,
            AdcCredentials::DefaultAccount(creds) => self.get_default_access_token(creds).await?,
        };

        let auth_token = AuthToken {
            token_type: if token_response.token_type.is_empty() {
                "Bearer".to_string()
            } else {
                token_response.token_type
            },
            token_value: token_response.access_token,
        };

        let expires_at = Instant::now()
            + Duration::from_secs(
                token_response.expires_in.saturating_sub(30), // 30 second buffer
            );

        *token_guard = Some(CachedToken {
            token: auth_token.clone(),
            expires_at,
        });

        Ok(auth_token)
    }

    /// Creates a JWT token for service account authentication.
    ///
    /// # Arguments
    /// * `creds` - Service account credentials for signing the token
    ///
    /// # Returns
    /// * `Result<String>` - A signed JWT token
    fn create_jwt_token(&self, creds: &ServiceAccountCredentials) -> Result<String, AuthError> {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_err(|e| AuthError::TokenCreation(e.to_string()))?
            .as_secs();

        let claims = JwtClaims {
            iss: creds.client_email.clone(),
            sub: creds.client_email.clone(),
            scope: "https://www.googleapis.com/auth/cloud-platform".to_string(),
            aud: creds.token_uri.clone(),
            iat: now,
            exp: now + 3600, // 1 hours validity
        };

        let encoding_key = EncodingKey::from_rsa_pem(creds.private_key.as_bytes())
            .map_err(|e| AuthError::TokenCreation(format!("Invalid private key: {}", e)))?;

        encode(
            &Header::new(jsonwebtoken::Algorithm::RS256),
            &claims,
            &encoding_key,
        )
        .map_err(|e| AuthError::TokenCreation(format!("Failed to create JWT: {}", e)))
    }

    /// Exchanges a token or assertion for an access token.
    ///
    /// # Arguments
    /// * `token_uri` - The token exchange endpoint
    /// * `params` - Parameters for the token exchange request
    ///
    /// # Returns
    /// * `Result<TokenResponse>` - The token exchange response
    async fn exchange_token(
        &self,
        token_uri: &str,
        params: &[(&str, &str)],
    ) -> Result<TokenResponse, AuthError> {
        let response = self
            .client
            .post(token_uri)
            .form(params)
            .send()
            .await
            .map_err(|e| AuthError::TokenExchange(e.to_string()))?;

        let status = response.status();
        if !status.is_success() {
            let error_text = response
                .text()
                .await
                .unwrap_or_else(|_| "Unknown error".to_string());
            return Err(AuthError::TokenExchange(format!(
                "Status {}: {}",
                status, error_text
            )));
        }

        response
            .json::<TokenResponse>()
            .await
            .map_err(|e| AuthError::TokenExchange(format!("Invalid response: {}", e)))
    }

    /// Gets a token using service account credentials.
    ///
    /// # Arguments
    /// * `creds` - Service account credentials
    ///
    /// # Returns
    /// * `Result<TokenResponse>` - The token response
    async fn get_service_account_token(
        &self,
        creds: &ServiceAccountCredentials,
    ) -> Result<TokenResponse, AuthError> {
        let jwt = self.create_jwt_token(creds)?;
        let params = [
            ("grant_type", "urn:ietf:params:oauth:grant-type:jwt-bearer"),
            ("assertion", &jwt),
            ("scope", "https://www.googleapis.com/auth/cloud-platform"),
        ];

        self.exchange_token(&creds.token_uri, &params).await
    }

    /// Gets a token using authorized user credentials.
    ///
    /// # Arguments
    /// * `creds` - Authorized user credentials
    ///
    /// # Returns
    /// * `Result<TokenResponse>` - The token response
    async fn get_authorized_user_token(
        &self,
        creds: &AuthorizedUserCredentials,
    ) -> Result<TokenResponse, AuthError> {
        let params = [
            ("client_id", creds.client_id.as_str()),
            ("client_secret", creds.client_secret.as_str()),
            ("refresh_token", creds.refresh_token.as_str()),
            ("grant_type", "refresh_token"),
            ("scope", "https://www.googleapis.com/auth/cloud-platform"),
        ];

        self.exchange_token(&creds.token_uri, &params).await
    }

    /// Gets a token directly from the GCP metadata endpoint.
    ///
    /// # Arguments
    /// * `base_url` - Default Access Token base url
    ///
    /// # Returns
    /// * `Result<TokenResponse>` - The token response
    async fn get_default_access_token(&self, base_url: &str) -> Result<TokenResponse, AuthError> {
        let metadata_path = "/computeMetadata/v1/instance/service-accounts/default/token";
        let response = self
            .client
            .get(format!("{}{}", base_url, metadata_path))
            .header("Metadata-Flavor", "Google")
            .send()
            .await
            .map_err(|e| AuthError::TokenExchange(e.to_string()))?;

        let status = response.status();
        if !status.is_success() {
            let error_text = response
                .text()
                .await
                .unwrap_or_else(|_| "Unknown error".to_string());
            return Err(AuthError::TokenExchange(format!(
                "Status {}: {}",
                status, error_text
            )));
        }

        response
            .json::<TokenResponse>()
            .await
            .map_err(|e| AuthError::TokenExchange(format!("Invalid response: {}", e)))
    }
}
