pub(crate) struct FileSnapshot {
    pub bytes: Vec<u8>,
    pub device: u64,
    pub inode: u64,
    pub mode: u32,
}

impl FileSnapshot {
    pub(crate) fn matches(&self, other: &Self) -> bool {
        self.device == other.device
            && self.inode == other.inode
            && self.mode == other.mode
            && self.bytes == other.bytes
    }
}
