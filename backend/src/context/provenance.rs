/// Origine des messages retenus pour une génération.
/// Les propositions MoA restent des contributions non vérifiées.
pub(crate) struct Provenance {
    pub history_message_indices: Vec<usize>,
    pub unverified_agent_ids: Vec<String>,
}
