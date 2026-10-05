use std::path::PathBuf;

use crate::declaration_issue::DeclarationIssue;

#[derive(Debug, Clone)]
pub(crate) struct FileIssue {
    pub(crate) path: PathBuf,
    pub(crate) declarations: Vec<DeclarationIssue>,
}
