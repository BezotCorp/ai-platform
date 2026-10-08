use dioxus::prelude::*;

use crate::component_tree::Page;
use crate::render_engine::RenderContext;

impl RenderContext<'_> {
    pub(crate) fn render_root(&self) -> String {
        let html_lang = self.website.default_language_region.to_string();

        let root_pages = self
            .website
            .pages
            .iter()
            .filter(|page| {
                page.public_path.as_str() == format!("/{}/", page.language_region.route_segment(),)
            })
            .collect::<Vec<_>>();

        let body = dioxus_ssr::render_element(rsx! {
            body {
                style: "margin:0;font-family:system-ui,-apple-system,BlinkMacSystemFont,'Segoe UI',sans-serif;color:#171717;background:#fff;",

                main {
                    style: "min-height:100vh;display:flex;flex-direction:column;justify-content:center;align-items:center;gap:1rem;padding:2rem;text-align:center;",

                    h1 {
                        "{self.website.name}"
                    }

                    if let Some(description) =
                        self.website.description.as_deref()
                    {
                        p {
                            "{description}"
                        }
                    }

                    nav {
                        aria_label: "Language selection",

                        for page in root_pages {
                            a {
                                href: page.public_path.as_str(),
                                style: "margin:.5rem;",
                                "{page.language_region}"
                            }
                        }
                    }
                }
            }
        });

        let description = self
            .website
            .description
            .as_deref()
            .unwrap_or(&self.website.name);

        let canonical = self
            .website
            .base_url
            .as_deref()
            .map(|base_url| {
                format!(
                    r#"<link rel="canonical" href="{}/">"#,
                    escape_html(base_url),
                )
            })
            .unwrap_or_default();

        format!(
            "<!doctype html>\
<html lang=\"{}\">\
<head>\
<meta charset=\"utf-8\">\
<meta name=\"viewport\" content=\"width=device-width, initial-scale=1\">\
<title>{}</title>\
<meta name=\"description\" content=\"{}\">\
{}\
</head>\
{}\
</html>",
            escape_html(&html_lang),
            escape_html(&self.website.name),
            escape_html(description),
            canonical,
            body,
        )
    }

    pub(crate) fn render_not_found(&self) -> String {
        let html_lang = self.website.default_language_region.to_string();

        let body = dioxus_ssr::render_element(rsx! {
            body {
                main {
                    h1 {
                        "Page introuvable"
                    }

                    p {
                        "Cette URL ne correspond à aucune page publiée."
                    }
                }
            }
        });

        format!(
            "<!doctype html>\
<html lang=\"{}\">\
<head>\
<meta charset=\"utf-8\">\
<meta name=\"viewport\" content=\"width=device-width, initial-scale=1\">\
<title>Page introuvable - {}</title>\
<meta name=\"description\" content=\"Cette URL ne correspond à aucune page publiée.\">\
<meta name=\"robots\" content=\"noindex,nofollow\">\
</head>\
{}\
</html>",
            escape_html(&html_lang),
            escape_html(&self.website.name),
            body,
        )
    }

    pub(crate) fn render_page(&self, page: &Page) -> String {
        let body = dioxus_ssr::render_element(rsx! {
            body {
                style: "margin:0;font-family:system-ui,-apple-system,BlinkMacSystemFont,'Segoe UI',sans-serif;color:#171717;background:#fff;",

                header {
                    style: "max-width:76rem;margin:0 auto;padding:1rem 2rem;",

                    nav {
                        style: "display:flex;align-items:center;gap:1.25rem;flex-wrap:wrap;",

                        for candidate in self
                            .website
                            .pages
                            .iter()
                            .filter(|candidate| {
                                candidate.language_region
                                    == page.language_region
                            })
                            .filter(|candidate| {
                                candidate.navigation.is_some()
                            })
                        {
                            a {
                                href: candidate.public_path.as_str(),

                                {
                                    candidate
                                        .navigation
                                        .as_ref()
                                        .expect("filtered")
                                        .label
                                        .as_str()
                                }
                            }
                        }
                    }
                }

                main {
                    for block in &page.blocks {
                        {
                            self.render_block(block)
                        }
                    }
                }

                footer {
                    style: "max-width:76rem;margin:3rem auto 0;padding:2rem;border-top:1px solid currentColor;opacity:.75;",
                    "{self.website.name}"
                }
            }
        });

        let description = page
            .description
            .as_deref()
            .or(self.website.description.as_deref())
            .unwrap_or(&page.title);

        let canonical = self
            .website
            .base_url
            .as_deref()
            .map(|base_url| {
                format!(
                    r#"<link rel="canonical" href="{}{}">"#,
                    escape_html(base_url),
                    escape_html(page.public_path.as_str()),
                )
            })
            .unwrap_or_default();

        let robots = if page.index {
            ""
        } else {
            r#"<meta name="robots" content="noindex,nofollow">"#
        };

        format!(
            "<!doctype html>\
<html lang=\"{}\">\
<head>\
<meta charset=\"utf-8\">\
<meta name=\"viewport\" content=\"width=device-width, initial-scale=1\">\
<title>{}</title>\
<meta name=\"description\" content=\"{}\">\
{}\
{}\
</head>\
{}\
</html>",
            escape_html(&page.language_region.to_string()),
            escape_html(&page.title),
            escape_html(description),
            canonical,
            robots,
            body,
        )
    }
}

fn escape_html(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&#39;")
}
