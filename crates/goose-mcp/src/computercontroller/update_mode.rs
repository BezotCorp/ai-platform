use crate::computercontroller::docx_style::DocxStyle;

#[derive(Debug)]
pub(crate) enum UpdateMode {
    Append,
    Replace {
        old_text: String,
    },
    InsertStructured {
        level: Option<String>,
        style: Option<DocxStyle>,
    },
    AddImage {
        image_path: String,
        width: Option<u32>,
        height: Option<u32>,
    },
}
