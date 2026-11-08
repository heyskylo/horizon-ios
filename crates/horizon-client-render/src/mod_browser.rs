//! What Options > Mods > Browse mods shows (docs/design/mod-hotload.md): the
//! registry's content mods this game installs, a search over them, and the one
//! whose page is open. The game fills it from a worker
//! (`horizon_client::mod_registry`, `game/mod_browser.rs`); the options screen
//! only reads it and asks for installs through its events.

/// One project of the registry, as the list and its page show it.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Listing {
    pub id: String,
    pub name: String,
    pub summary: String,
    /// Markdown, shown as plain text.
    pub description: String,
    pub authors: Vec<String>,
    pub license: String,
    pub categories: Vec<String>,
    pub tags: Vec<String>,
    /// The version this game would install, its date and changelog.
    pub version: String,
    pub published: String,
    pub changelog: String,
    pub downloads: u64,
    /// The project's page on the website.
    pub page: String,
    /// The version installed in this game folder, if any.
    pub installed: Option<String>,
}

impl Listing {
    /// Whether a search for `query` (already lower case) finds it: its name,
    /// id, summary, authors, categories or tags.
    pub fn matches(&self, query: &str) -> bool {
        let query = query.trim();
        query.is_empty()
            || [&self.name, &self.id, &self.summary]
                .into_iter()
                .chain(&self.authors)
                .chain(&self.categories)
                .chain(&self.tags)
                .any(|field| field.to_lowercase().contains(query))
    }

    /// Installed, but not the version the registry offers now.
    pub fn has_update(&self) -> bool {
        self.installed
            .as_ref()
            .is_some_and(|installed| *installed != self.version)
    }
}

/// Where the browser is.
#[derive(Clone, Debug, Default, PartialEq)]
pub enum Status {
    /// Not asked yet.
    #[default]
    Idle,
    /// The index is being read.
    Loading,
    /// The projects are listed.
    Ready,
    /// The index couldn't be read: why, in English (shown inside a translated line).
    Failed(String),
}

#[derive(Clone, Debug, Default)]
pub struct ModBrowser {
    pub status: Status,
    pub listings: Vec<Listing>,
    /// The project an install runs for.
    pub installing: Option<String>,
    /// The last install's outcome, already translated, for the page it ran on.
    pub message: Option<(String, bool)>,
}

impl ModBrowser {
    /// The listings a search for `query` finds, by index into `listings`.
    pub fn found(&self, query: &str) -> Vec<usize> {
        let query = query.to_lowercase();
        (0..self.listings.len())
            .filter(|&i| self.listings[i].matches(&query))
            .collect()
    }

    pub fn listing(&self, id: &str) -> Option<&Listing> {
        self.listings.iter().find(|l| l.id == id)
    }
}

/// A project's markdown description as lines of plain text for the page:
/// heading marks, emphasis and list bullets dropped, links shown as their text,
/// blank lines and fences left out.
pub fn plain_lines(markdown: &str) -> Vec<String> {
    let mut out = Vec::new();
    for line in markdown.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with("```") || line.starts_with("<!--") {
            continue;
        }
        let line = line.trim_start_matches('#').trim_start();
        let line = match line.strip_prefix("- ").or_else(|| line.strip_prefix("* ")) {
            Some(rest) => format!("• {rest}"),
            None => line.to_owned(),
        };
        let line = links_as_text(&line).replace("**", "").replace('`', "");
        if !line.is_empty() {
            out.push(line);
        }
    }
    out
}

/// `[text](url)` → `text`, `![alt](url)` → nothing.
fn links_as_text(line: &str) -> String {
    let mut out = String::with_capacity(line.len());
    let mut rest = line;
    while let Some(open) = rest.find('[') {
        let Some(close) = rest[open..].find("](").map(|c| open + c) else {
            break;
        };
        let Some(end) = rest[close..].find(')').map(|e| close + e) else {
            break;
        };
        let image = open > 0 && rest[..open].ends_with('!');
        out.push_str(&rest[..if image { open - 1 } else { open }]);
        if !image {
            out.push_str(&rest[open + 1..close]);
        }
        rest = &rest[end + 1..];
    }
    out.push_str(rest);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn listing(id: &str, tags: &[&str]) -> Listing {
        Listing {
            id: id.into(),
            name: id.to_uppercase(),
            summary: format!("All about {id}"),
            tags: tags.iter().map(|t| (*t).to_owned()).collect(),
            version: "1.0".into(),
            ..Listing::default()
        }
    }

    #[test]
    fn a_search_finds_names_tags_and_categories_in_any_case() {
        let mut pipes = listing("pipes", &["tech"]);
        pipes.categories = vec!["technology".into()];
        let browser = ModBrowser {
            listings: vec![listing("ruby", &["gems"]), pipes],
            ..ModBrowser::default()
        };
        assert_eq!(browser.found(""), [0, 1]);
        assert_eq!(browser.found("RUBY"), [0]);
        assert_eq!(browser.found("gems"), [0]);
        assert_eq!(browser.found("techno"), [1]);
        assert!(browser.found("nothing like it").is_empty());
    }

    #[test]
    fn an_update_is_another_version_than_the_installed_one() {
        let mut l = listing("ruby", &[]);
        assert!(!l.has_update());
        l.installed = Some("1.0".into());
        assert!(!l.has_update());
        l.installed = Some("0.9".into());
        assert!(l.has_update());
    }

    #[test]
    fn markdown_reads_as_plain_lines() {
        let text = "# Ruby\n\nAdds **rubies**. See [the wiki](https://x.y).\n\n![shot](https://x.y/a.png)\n- one\n* two\n```\ncode\n```\n";
        assert_eq!(
            plain_lines(text),
            [
                "Ruby",
                "Adds rubies. See the wiki.",
                "• one",
                "• two",
                "code"
            ]
        );
    }
}
