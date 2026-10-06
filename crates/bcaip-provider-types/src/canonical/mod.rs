mod catalog;
mod model;
mod models_dev;
mod name_builder;
mod registry;

pub use catalog::{
    ModelCapabilities, ModelTemplate, ProviderCatalogEntry, ProviderFormat,
    ProviderSetupCapabilities, ProviderSetupCatalogEntry, ProviderSetupCategory,
    ProviderSetupField, ProviderSetupFieldOverride, ProviderSetupGroup, ProviderSetupMetadata,
    ProviderSetupMethod, ProviderTemplate, get_provider_template, get_providers_by_format,
    get_setup_catalog_entries,
};
pub use model::{CanonicalModel, Limit, Modalities, Modality, Pricing, ThinkingMode};
pub use models_dev::from_models_dev;
pub use name_builder::{
    canonical_name, is_meta_provider, map_provider_name, map_to_canonical_model,
    strip_version_suffix,
};
pub use registry::{CanonicalModelRegistry, load_cached_catalog, refresh_remote_catalog};
