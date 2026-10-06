pub(crate) struct ChatTemplateParams<'a> {
    pub messages_json: &'a str,
    pub tools_json: Option<&'a str>,
    pub enable_thinking: bool,
}
