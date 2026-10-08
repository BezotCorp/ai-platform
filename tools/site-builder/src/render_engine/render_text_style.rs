use crate::component_tree::{TextAlign, TextStyle};

pub(crate) fn render_text_style(base: &str, style: Option<&TextStyle>) -> String {
    let mut rendered = base.to_owned();

    let Some(style) = style else {
        return rendered;
    };

    if let Some(color) = style.color.as_deref() {
        rendered.push_str("color:");
        rendered.push_str(color);
        rendered.push(';');
    }

    if let Some(font_size) = style.font_size.as_deref() {
        rendered.push_str("font-size:");
        rendered.push_str(font_size);
        rendered.push(';');
    }

    if let Some(font_weight) = style.font_weight {
        rendered.push_str("font-weight:");
        rendered.push_str(&font_weight.to_string());
        rendered.push(';');
    }

    if let Some(align) = style.align.as_ref() {
        rendered.push_str("text-align:");
        rendered.push_str(match align {
            TextAlign::Start => "start",
            TextAlign::Center => "center",
            TextAlign::End => "end",
        });
        rendered.push(';');
    }

    rendered
}
