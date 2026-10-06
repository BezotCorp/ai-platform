#[derive(Clone, Copy)]
pub(crate) enum ToolMode {
    None,
    Native,
    Emulated { code_mode_enabled: bool },
}
