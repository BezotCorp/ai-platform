use std::path::PathBuf;

use crate::build_output::{GeneratedFile, PublicFile};

#[derive(Debug)]
pub(crate) enum BuildChange {
    WriteGenerated {
        target: PathBuf,
        file: GeneratedFile,
    },
    CopyPublic {
        target: PathBuf,
        file: PublicFile,
    },
    Remove {
        target: PathBuf,
    },
}
