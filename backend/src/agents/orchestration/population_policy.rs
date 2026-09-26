/// Population management is orthogonal to agent coordination.
/// Non-fixed policies are explicitly rejected until implemented.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum PopulationPolicy {
    Fixed,
    Adaptive { min_agents: usize, max_agents: usize },
    Evolutionary { population_size: usize, generations: usize },
}
