/// Origine des messages retenus pour une génération.
/// Les propositions MoA et la mémoire sont des contributions non vérifiées.
pub(crate) struct Provenance {
    pub history_message_indices: Vec<usize>,
    pub memory_entry_ids: Vec<i64>,
    pub unverified_agent_ids: Vec<String>,
}
