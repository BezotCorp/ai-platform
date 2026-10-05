#[derive(Debug, Clone)]
pub(crate) struct DeclarationIssue {
    pub(crate) kind: String,
    pub(crate) line: usize,
    pub(crate) found_name: String,
    pub(crate) expected_name: String,
    pub(crate) expected_file_name: String,
    pub(crate) type_ok: bool,
    pub(crate) file_ok: bool,
}
