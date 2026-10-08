use bcaip_provider_types::{
    ProviderCatalogEntry, ProviderFormat, ProviderSetupCatalogEntry, ProviderTemplate,
    get_provider_template as bcaip_get_provider_template,
    get_providers_by_format as bcaip_get_providers_by_format,
    get_setup_catalog_entries as bcaip_get_setup_catalog_entries,
};

use std::collections::HashSet;
pub async fn get_providers_by_format(format: ProviderFormat) -> Vec<ProviderCatalogEntry> {
    let native_provider_ids = super::init::providers()
        .await
        .into_iter()
        .map(|(metadata, _)| metadata.name)
        .collect::<HashSet<_>>();

    bcaip_get_providers_by_format(format, &native_provider_ids)
}
pub async fn get_setup_catalog_entries() -> Vec<ProviderSetupCatalogEntry> {
    bcaip_get_setup_catalog_entries(
        super::providers()
            .await
            .into_iter()
            .map(|(metadata, _)| metadata),
    )
}

pub fn get_provider_template(provider_id: &str) -> Option<ProviderTemplate> {
    bcaip_get_provider_template(provider_id)
}
