use serde::Deserialize;

use crate::component_tree::{Card, Link, TextStyle};

#[derive(Debug, Deserialize)]
pub(crate) enum Block {
    Hero {
        title: String,
        subtitle: Option<String>,
        title_style: Option<TextStyle>,

        #[serde(default)]
        actions: Vec<Link>,
    },

    Heading {
        level: u8,
        text: String,
        style: Option<TextStyle>,
    },

    Paragraph {
        text: String,
        style: Option<TextStyle>,
    },

    Container {
        blocks: Vec<Block>,
    },

    Link {
        label: String,
        href: String,
    },

    Image {
        src: String,
        alt: String,
        caption: Option<String>,
    },

    Code {
        language: Option<String>,
        code: String,
    },

    CardGrid {
        cards: Vec<Card>,
    },

    Divider,
}
