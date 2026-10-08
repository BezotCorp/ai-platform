use dioxus::prelude::*;

use crate::component_tree::{Block, Card, Link};
use crate::render_engine::{RenderContext, render_text_style::render_text_style};

impl RenderContext<'_> {
    pub(crate) fn render_block(&self, block: &Block) -> Element {
        match block {
            Block::Hero {
                title,
                subtitle,
                title_style,
                actions,
            } => {
                let title_style = render_text_style(
                    "margin:0;font-size:clamp(3rem,9vw,7rem);line-height:.95;",
                    title_style.as_ref(),
                );

                rsx! {
                    section {
                        style: "min-height:70vh;display:flex;flex-direction:column;justify-content:center;align-items:center;gap:1rem;padding:2rem;text-align:center;",

                        h1 {
                            style: "{title_style}",
                            "{title}"
                        }

                        if let Some(subtitle) = subtitle {
                            p { "{subtitle}" }
                        }

                        if !actions.is_empty() {
                            div {
                                style: "display:flex;flex-wrap:wrap;justify-content:center;gap:.75rem;",

                                for action in actions {
                                    {self.render_link(action)}
                                }
                            }
                        }
                    }
                }
            }

            Block::Heading { level, text, style } => {
                let style = render_text_style(
                    "max-width:72rem;margin:2rem auto 1rem;padding:0 2rem;",
                    style.as_ref(),
                );

                match level {
                    1 => rsx! { h1 { style: "{style}", "{text}" } },
                    2 => rsx! { h2 { style: "{style}", "{text}" } },
                    3 => rsx! { h3 { style: "{style}", "{text}" } },
                    4 => rsx! { h4 { style: "{style}", "{text}" } },
                    5 => rsx! { h5 { style: "{style}", "{text}" } },
                    _ => rsx! { h6 { style: "{style}", "{text}" } },
                }
            }

            Block::Paragraph { text, style } => {
                let style = render_text_style(
                    "max-width:72rem;margin:0 auto;padding:1rem 2rem;line-height:1.75;",
                    style.as_ref(),
                );

                rsx! {
                    p {
                        style: "{style}",
                        "{text}"
                    }
                }
            }

            Block::Container { blocks } => rsx! {
                section {
                    style: "max-width:76rem;margin:0 auto;padding:1rem 2rem;",

                    for block in blocks {
                        {self.render_block(block)}
                    }
                }
            },

            Block::Link { label, href } => rsx! {
                a {
                    href: "{href}",
                    "{label}"
                }
            },

            Block::Image { src, alt, caption } => rsx! {
                figure {
                    style: "max-width:72rem;margin:2rem auto;padding:0 2rem;",

                    img {
                        src: "{src}",
                        alt: "{alt}",
                        loading: "lazy",
                        style: "display:block;max-width:100%;height:auto;",
                    }

                    if let Some(caption) = caption {
                        figcaption { "{caption}" }
                    }
                }
            },

            Block::Code { language, code } => {
                let class = language
                    .as_ref()
                    .map(|language| format!("language-{language}"));

                rsx! {
                    pre {
                        code {
                            class,
                            "{code}"
                        }
                    }
                }
            }

            Block::CardGrid { cards } => rsx! {
                section {
                    style: "max-width:76rem;margin:1rem auto;padding:1rem 2rem;display:grid;grid-template-columns:repeat(auto-fit,minmax(16rem,1fr));gap:1rem;",

                    for card in cards {
                        {self.render_card(card)}
                    }
                }
            },

            Block::Divider => rsx! { hr {} },
        }
    }

    fn render_link(&self, link: &Link) -> Element {
        let external = link.href.starts_with("https://") || link.href.starts_with("http://");

        if external {
            rsx! {
                a {
                    href: "{link.href}",
                    target: "_blank",
                    rel: "noopener noreferrer",
                    "{link.label}"
                }
            }
        } else {
            rsx! {
                a {
                    href: "{link.href}",
                    "{link.label}"
                }
            }
        }
    }

    fn render_card(&self, card: &Card) -> Element {
        let content = rsx! {
            article {
                style: "height:100%;box-sizing:border-box;padding:1.25rem;border:1px solid currentColor;border-radius:.65rem;",

                h3 { "{card.title}" }

                if let Some(text) = &card.text {
                    p { "{text}" }
                }
            }
        };

        if let Some(href) = &card.href {
            rsx! {
                a {
                    href: "{href}",
                    style: "color:inherit;text-decoration:none;",
                    {content}
                }
            }
        } else {
            content
        }
    }
}
