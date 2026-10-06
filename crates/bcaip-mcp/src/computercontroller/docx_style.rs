use docx_rs::*;
use image::{self, ImageFormat};
use rmcp::model::{ContentBlock, ErrorCode, ErrorData};
use std::borrow::Cow;
use std::{fs, io::Cursor};

use crate::computercontroller::UpdateMode;
#[derive(Debug, Clone, Default)]
pub struct DocxStyle {
    bold: bool,
    italic: bool,
    underline: bool,
    size: Option<usize>,
    color: Option<String>,
    alignment: Option<AlignmentType>,
}

impl DocxStyle {
    fn from_json(value: &serde_json::Value) -> Option<Self> {
        let obj = value.as_object()?;
        Some(Self {
            bold: obj.get("bold").and_then(|v| v.as_bool()).unwrap_or(false),
            italic: obj.get("italic").and_then(|v| v.as_bool()).unwrap_or(false),
            underline: obj
                .get("underline")
                .and_then(|v| v.as_bool())
                .unwrap_or(false),
            size: obj.get("size").and_then(|v| v.as_u64()).map(|s| s as usize),
            color: obj.get("color").and_then(|v| v.as_str()).map(String::from),
            alignment: obj
                .get("alignment")
                .and_then(|v| v.as_str())
                .and_then(parse_alignment),
        })
    }

    fn apply_to_run(&self, mut run: Run) -> Run {
        if self.bold {
            run = run.bold();
        }
        if self.italic {
            run = run.italic();
        }
        if self.underline {
            run = run.underline("single");
        }
        if let Some(size) = self.size {
            run = run.size(size);
        }
        if let Some(color) = &self.color {
            run = run.color(color);
        }
        run
    }

    fn apply_to_paragraph(&self, mut para: Paragraph) -> Paragraph {
        if let Some(alignment) = self.alignment {
            para = para.align(alignment);
        }
        para
    }
}

fn parse_alignment(a: &str) -> Option<AlignmentType> {
    match a {
        "left" => Some(AlignmentType::Left),
        "center" => Some(AlignmentType::Center),
        "right" => Some(AlignmentType::Right),
        "justified" => Some(AlignmentType::Both),
        _ => None,
    }
}

fn docx_error(message: impl Into<String>) -> ErrorData {
    ErrorData {
        code: ErrorCode::INTERNAL_ERROR,
        message: Cow::from(message.into()),
        data: None,
    }
}

fn invalid_params(message: impl Into<String>) -> ErrorData {
    ErrorData {
        code: ErrorCode::INVALID_PARAMS,
        message: Cow::from(message.into()),
        data: None,
    }
}

fn read_docx_file(path: &str) -> Result<Docx, ErrorData> {
    let file =
        fs::read(path).map_err(|e| docx_error(format!("Failed to read DOCX file: {}", e)))?;
    read_docx(&file).map_err(|e| docx_error(format!("Failed to parse DOCX file: {}", e)))
}

fn read_or_create_docx(path: &str) -> Result<Docx, ErrorData> {
    if std::path::Path::new(path).exists() {
        read_docx_file(path)
    } else {
        Ok(Docx::new())
    }
}

fn write_docx_file(path: &str, doc: Docx) -> Result<(), ErrorData> {
    let mut buf = Vec::new();
    doc.build()
        .pack(&mut Cursor::new(&mut buf))
        .map_err(|e| docx_error(format!("Failed to build DOCX: {}", e)))?;
    fs::write(path, &buf).map_err(|e| docx_error(format!("Failed to write DOCX file: {}", e)))
}

fn extract_paragraph_text(p: &Paragraph) -> String {
    p.children
        .iter()
        .filter_map(|child| {
            if let ParagraphChild::Run(run) = child {
                Some(
                    run.children
                        .iter()
                        .filter_map(|rc| {
                            if let RunChild::Text(t) = rc {
                                Some(t.text.clone())
                            } else {
                                None
                            }
                        })
                        .collect::<Vec<_>>()
                        .join(""),
                )
            } else {
                None
            }
        })
        .collect::<Vec<_>>()
        .join("")
}

fn add_styled_paragraphs(mut doc: Docx, content: &str, style: &Option<DocxStyle>) -> Docx {
    for para in content.split('\n').filter(|p| !p.trim().is_empty()) {
        let mut run = Run::new().add_text(para);
        let mut paragraph = Paragraph::new();
        if let Some(s) = style {
            run = s.apply_to_run(run);
            paragraph = s.apply_to_paragraph(paragraph);
        }
        doc = doc.add_paragraph(paragraph.add_run(run));
    }
    doc
}

fn parse_update_mode(
    params: Option<&serde_json::Value>,
) -> Result<(UpdateMode, Option<DocxStyle>), ErrorData> {
    let Some(params) = params else {
        return Ok((UpdateMode::Append, None));
    };

    let mode_str = params
        .get("mode")
        .and_then(|v| v.as_str())
        .unwrap_or("append");
    let style = params.get("style").and_then(DocxStyle::from_json);

    let mode = match mode_str {
        "append" => UpdateMode::Append,
        "replace" => {
            let old_text = params
                .get("old_text")
                .and_then(|v| v.as_str())
                .ok_or_else(|| invalid_params("old_text parameter required for replace mode"))?;
            UpdateMode::Replace {
                old_text: old_text.to_string(),
            }
        }
        "structured" => UpdateMode::InsertStructured {
            level: params
                .get("level")
                .and_then(|v| v.as_str())
                .map(String::from),
            style: style.clone(),
        },
        "add_image" => {
            let image_path = params
                .get("image_path")
                .and_then(|v| v.as_str())
                .ok_or_else(|| {
                    invalid_params("image_path parameter required for add_image mode")
                })?;
            UpdateMode::AddImage {
                image_path: image_path.to_string(),
                width: params
                    .get("width")
                    .and_then(|v| v.as_u64())
                    .map(|w| w as u32),
                height: params
                    .get("height")
                    .and_then(|v| v.as_u64())
                    .map(|h| h as u32),
            }
        }
        _ => {
            return Err(invalid_params(
                "Invalid mode. Must be 'append', 'replace', 'structured', or 'add_image'",
            ));
        }
    };
    Ok((mode, style))
}

fn extract_text_from_docx(docx: &Docx) -> String {
    let mut text = String::new();
    for element in docx.document.children.iter() {
        if let DocumentChild::Paragraph(p) = element {
            let para_text = extract_paragraph_text(p);
            if !para_text.trim().is_empty() {
                text.push_str(&para_text);
                text.push('\n');
            }
        }
    }
    text
}

fn extract_structure_from_docx(docx: &Docx) -> Vec<String> {
    let mut structure = Vec::new();
    let mut current_level = None;

    for element in docx.document.children.iter() {
        if let DocumentChild::Paragraph(p) = element {
            if let Some(style) = p.property.style.as_ref()
                && style.val.starts_with("Heading")
            {
                current_level = Some(style.val.clone());
                structure.push(format!("{}: ", style.val));
            }
            let para_text = extract_paragraph_text(p);
            if !para_text.trim().is_empty() && current_level.is_some() {
                if let Some(s) = structure.last_mut() {
                    s.push_str(&para_text);
                }
                current_level = None;
            }
        }
    }
    structure
}

fn do_extract_text(path: &str) -> Result<Vec<ContentBlock>, ErrorData> {
    let docx = read_docx_file(path)?;
    let text = extract_text_from_docx(&docx);
    let structure = extract_structure_from_docx(&docx);

    let result = if !structure.is_empty() {
        format!(
            "Document Structure:\n{}\n\nFull Text:\n{}",
            structure.join("\n"),
            text
        )
    } else {
        format!("Extracted Text:\n{}", text)
    };
    Ok(vec![ContentBlock::text(result)])
}

fn do_append(
    path: &str,
    content: &str,
    style: &Option<DocxStyle>,
) -> Result<Vec<ContentBlock>, ErrorData> {
    let doc = read_or_create_docx(path)?;
    let doc = add_styled_paragraphs(doc, content, style);
    write_docx_file(path, doc)?;
    Ok(vec![ContentBlock::text(format!(
        "Successfully wrote content to {}",
        path
    ))])
}

fn do_replace(
    path: &str,
    content: &str,
    old_text: &str,
    style: &Option<DocxStyle>,
) -> Result<Vec<ContentBlock>, ErrorData> {
    let docx = read_docx_file(path)?;
    let mut new_doc = Docx::new();
    let mut found_text = false;

    for element in docx.document.children.iter() {
        if let DocumentChild::Paragraph(p) = element {
            let para_text = extract_paragraph_text(p);
            if para_text.contains(old_text) {
                found_text = true;
                new_doc = add_styled_paragraphs(new_doc, content, style);
            } else {
                let mut para = Paragraph::new();
                if let Some(s) = &p.property.style {
                    para = para.style(&s.val);
                }
                for child in p.children.iter() {
                    if let ParagraphChild::Run(run) = child {
                        for rc in run.children.iter() {
                            if let RunChild::Text(t) = rc {
                                para = para.add_run(Run::new().add_text(&t.text));
                            }
                        }
                    }
                }
                new_doc = new_doc.add_paragraph(para);
            }
        }
    }

    if !found_text {
        return Err(docx_error(format!(
            "Could not find text to replace: {}",
            old_text
        )));
    }
    write_docx_file(path, new_doc)?;
    Ok(vec![ContentBlock::text(format!(
        "Successfully replaced content in {}",
        path
    ))])
}

fn do_insert_structured(
    path: &str,
    content: &str,
    level: &Option<String>,
    style: &Option<DocxStyle>,
) -> Result<Vec<ContentBlock>, ErrorData> {
    let mut doc = read_or_create_docx(path)?;

    for para in content.split('\n').filter(|p| !p.trim().is_empty()) {
        let mut run = Run::new().add_text(para);
        let mut paragraph = Paragraph::new();
        if let Some(lvl) = level {
            paragraph = paragraph.style(lvl);
        }
        if let Some(s) = style {
            run = s.apply_to_run(run);
            paragraph = s.apply_to_paragraph(paragraph);
        }
        doc = doc.add_paragraph(paragraph.add_run(run));
    }

    write_docx_file(path, doc)?;
    Ok(vec![ContentBlock::text(format!(
        "Successfully added structured content to {}",
        path
    ))])
}

fn load_image_as_png(image_path: &str) -> Result<Vec<u8>, ErrorData> {
    let image_data = fs::read(image_path)
        .map_err(|e| docx_error(format!("Failed to read image file: {}", e)))?;

    let extension = std::path::Path::new(image_path)
        .extension()
        .and_then(|e| e.to_str())
        .ok_or_else(|| docx_error("Invalid image file extension"))?
        .to_lowercase();

    if extension == "png" {
        return Ok(image_data);
    }

    let img = image::load_from_memory(&image_data)
        .map_err(|e| docx_error(format!("Failed to load image: {}", e)))?;
    let mut png_data = Vec::new();
    img.write_to(&mut Cursor::new(&mut png_data), ImageFormat::Png)
        .map_err(|e| docx_error(format!("Failed to convert image to PNG: {}", e)))?;
    Ok(png_data)
}

fn do_add_image(
    path: &str,
    content: &str,
    image_path: &str,
    width: Option<u32>,
    height: Option<u32>,
    style: &Option<DocxStyle>,
) -> Result<Vec<ContentBlock>, ErrorData> {
    let mut doc = read_or_create_docx(path)?;
    let image_data = load_image_as_png(image_path)?;

    if !content.trim().is_empty() {
        let mut caption = Paragraph::new();
        if let Some(s) = style {
            caption = s.apply_to_paragraph(caption);
            caption = caption.add_run(s.apply_to_run(Run::new().add_text(content)));
        } else {
            caption = caption.add_run(Run::new().add_text(content));
        }
        doc = doc.add_paragraph(caption);
    }

    let mut paragraph = Paragraph::new();
    if let Some(s) = style {
        paragraph = s.apply_to_paragraph(paragraph);
    }

    let mut pic = Pic::new(&image_data);
    if let (Some(w), Some(h)) = (width, height) {
        pic = pic.size(w, h);
    }

    paragraph = paragraph.add_run(Run::new().add_image(pic));
    doc = doc.add_paragraph(paragraph);

    write_docx_file(path, doc)?;
    Ok(vec![ContentBlock::text(format!(
        "Successfully added image to {}",
        path
    ))])
}

pub async fn docx_tool(
    path: &str,
    operation: &str,
    content: Option<&str>,
    params: Option<&serde_json::Value>,
) -> Result<Vec<ContentBlock>, ErrorData> {
    match operation {
        "extract_text" => do_extract_text(path),
        "update_doc" => {
            let content = content
                .ok_or_else(|| invalid_params("Content parameter required for update_doc"))?;
            let (mode, style) = parse_update_mode(params)?;

            match mode {
                UpdateMode::Append => do_append(path, content, &style),
                UpdateMode::Replace { old_text } => do_replace(path, content, &old_text, &style),
                UpdateMode::InsertStructured {
                    level,
                    style: mode_style,
                } => do_insert_structured(path, content, &level, &mode_style.or(style)),
                UpdateMode::AddImage {
                    image_path,
                    width,
                    height,
                } => do_add_image(path, content, &image_path, width, height, &style),
            }
        }
        _ => Err(invalid_params(format!(
            "Invalid operation: {}. Valid operations are: 'extract_text', 'update_doc'",
            operation
        ))),
    }
}
