use crate::fix_action::FixAction;

pub(crate) struct FixPlan {
    actions: Vec<FixAction>,
}

impl FixPlan {
    pub(crate) fn new(actions: Vec<FixAction>) -> Self {
        Self { actions }
    }

    pub(crate) fn actions(&self) -> &[FixAction] {
        &self.actions
    }

    pub(crate) fn len(&self) -> usize {
        self.actions.len()
    }
}
