use std::collections::BTreeSet;

fn terms(text: &str) -> BTreeSet<String> {
    text.split(|character: char| !character.is_alphanumeric())
        .filter(|term| term.chars().count() >= 3)
        .map(str::to_lowercase)
        .filter(|term| {
            !matches!(
                term.as_str(),
                "avec"
                    | "dans"
                    | "pour"
                    | "cette"
                    | "comme"
                    | "mais"
                    | "plus"
                    | "the"
                    | "and"
                    | "for"
                    | "with"
                    | "from"
                    | "that"
                    | "this"
            )
        })
        .collect()
}

pub(crate) fn relevance(query: &str, exchange: &str) -> usize {
    let query_terms = terms(query);
    if query_terms.is_empty() {
        return 0;
    }
    let exchange_terms = terms(exchange);
    query_terms.intersection(&exchange_terms).count()
}
