//! What the browser shows beyond a search hit: a project's page (description, gallery, links,
//! authors) and the search's filters (categories, loader, game version, environment, sort), with
//! the lists of values each source offers for them. Data only: `mods_page` draws it.
use super::{Kind, GAME};
use serde_json::Value;

/// How results are ordered.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum Sort {
    #[default]
    Relevance,
    Downloads,
    Follows,
    Newest,
    Updated,
}

impl Sort {
    pub const ALL: [Sort; 5] = [
        Sort::Relevance,
        Sort::Downloads,
        Sort::Follows,
        Sort::Newest,
        Sort::Updated,
    ];
    pub fn label(self) -> &'static str {
        match self {
            Sort::Relevance => "Relevance",
            Sort::Downloads => "Downloads",
            Sort::Follows => "Follows",
            Sort::Newest => "Newest",
            Sort::Updated => "Updated",
        }
    }
    /// Modrinth's `index`.
    pub fn modrinth(self) -> &'static str {
        match self {
            Sort::Relevance => "relevance",
            Sort::Downloads => "downloads",
            Sort::Follows => "follows",
            Sort::Newest => "newest",
            Sort::Updated => "updated",
        }
    }
    /// CurseForge's `sortField` (`ModsSearchSortField`).
    pub fn curseforge(self) -> u8 {
        match self {
            Sort::Relevance | Sort::Follows => 2,
            Sort::Downloads => 6,
            Sort::Newest => 11,
            Sort::Updated => 3,
        }
    }
}

/// Where a project runs.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Environment {
    Client,
    Server,
}

impl Environment {
    pub const ALL: [Environment; 2] = [Environment::Client, Environment::Server];
    pub fn label(self) -> &'static str {
        match self {
            Environment::Client => "Client",
            Environment::Server => "Server",
        }
    }
}

/// A search's filters. Empty: the browser's defaults (what runs in Horizon).
#[derive(Clone, Debug, Default, PartialEq, Eq, Hash)]
pub struct Filters {
    /// Every one of these categories (`decoration`, `worldgen`...).
    pub categories: Vec<String>,
    /// Every one of these tags: Horizon's registry only, a project's own words (`zoom`, `pvp`...).
    pub tags: Vec<String>,
    /// One loader (`neoforge`, `fabric`...), in place of the kind's defaults.
    pub loader: Option<String>,
    /// One Minecraft version, in place of the one Horizon plays.
    pub game_version: Option<String>,
    pub environment: Option<Environment>,
    /// None: by relevance with search text, else by downloads.
    pub sort: Option<Sort>,
}

impl Filters {
    /// The sort a search uses.
    pub fn sort(&self, text: &str) -> Sort {
        self.sort.unwrap_or(if text.is_empty() {
            Sort::Downloads
        } else {
            Sort::Relevance
        })
    }
    /// A game version other than the one Horizon plays was asked for.
    pub fn other_version(&self) -> bool {
        self.game_version.as_deref().is_some_and(|v| v != GAME)
    }
}

/// One value a filter can take.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Tag {
    pub name: String,
    /// The group a category is in on its source (`categories`, `features`, `resolutions`).
    pub header: String,
}

/// The values each filter can take for a kind.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Tags {
    pub categories: Vec<Tag>,
    /// Horizon's registry only: the tags its projects of the kind carry, the most used first.
    pub tags: Vec<String>,
    pub loaders: Vec<String>,
    /// Release versions, newest first.
    pub game_versions: Vec<String>,
}

/// A category as playhorizon.dev names it (`categoryLabel` in the site's `mods-logic.ts`):
/// `game-mechanics` reads "Game mechanics", `gui` "GUI". English: the launcher translates the
/// names its tables have.
pub fn category_label(slug: &str) -> String {
    let words = match slug {
        "gui" => return "GUI".into(),
        "pbr" => return "PBR".into(),
        "worldgen" => return "World generation".into(),
        "semi-realistic" => return "Semi-realistic".into(),
        "vanilla-like" => return "Vanilla-like".into(),
        "core-shaders" => return "Core shaders".into(),
        _ => slug.replace('-', " "),
    };
    let mut chars = words.chars();
    chars
        .next()
        .map(|first| first.to_uppercase().chain(chars).collect())
        .unwrap_or_default()
}

/// A gallery image.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Image {
    pub url: String,
    pub title: String,
    pub description: String,
    pub featured: bool,
}

/// A link a project gives: its source, issues, wiki, Discord, donations.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Link {
    pub label: String,
    pub url: String,
}

/// A project's GitHub repository and how many stars it has: a registry project whose source is
/// on GitHub.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GitHub {
    /// `https://github.com/<owner>/<repo>`.
    pub url: String,
    pub stars: u64,
}

/// A project's page.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Details {
    /// Markdown (Modrinth's body; HTML blocks in it are left as text the renderer skips).
    pub body: String,
    pub gallery: Vec<Image>,
    pub links: Vec<Link>,
    pub authors: Vec<String>,
    /// Modrinth's followers; a registry project has none (it shows [`Details::github`] instead).
    pub followers: u64,
    /// Where to star a registry project, and its stars there.
    pub github: Option<GitHub>,
    pub published: String,
    pub updated: String,
    pub categories: Vec<String>,
    /// `required`, `optional`, `unsupported` or `unknown`.
    pub client_side: String,
    pub server_side: String,
}

fn strings(v: &Value) -> Vec<String> {
    v.as_array()
        .into_iter()
        .flatten()
        .filter_map(|s| s.as_str().map(str::to_owned))
        .collect()
}

fn text(v: &Value) -> String {
    v.as_str().unwrap_or_default().to_owned()
}

/// Only `https://` links are kept: a page never opens another scheme.
fn https(url: &str) -> Option<String> {
    url.starts_with("https://").then(|| url.to_owned())
}

/// Modrinth's `GET /project/{id}` and `GET /project/{id}/members`.
pub fn modrinth_details(project: &Value, members: &Value) -> Details {
    let mut gallery: Vec<(i64, Image)> = project["gallery"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|g| {
            Some((
                g["ordering"].as_i64().unwrap_or(0),
                Image {
                    url: https(g["url"].as_str()?)?,
                    title: text(&g["title"]),
                    description: text(&g["description"]),
                    featured: g["featured"].as_bool().unwrap_or(false),
                },
            ))
        })
        .collect();
    gallery.sort_by_key(|(order, i)| (!i.featured, *order));
    let mut links = Vec::new();
    for (key, label) in [
        ("source_url", "Source"),
        ("issues_url", "Issues"),
        ("wiki_url", "Wiki"),
        ("discord_url", "Discord"),
    ] {
        if let Some(url) = project[key].as_str().and_then(https) {
            links.push(Link {
                label: label.into(),
                url,
            });
        }
    }
    for d in project["donation_urls"].as_array().into_iter().flatten() {
        if let Some(url) = d["url"].as_str().and_then(https) {
            links.push(Link {
                label: format!("Donate ({})", text(&d["platform"])),
                url,
            });
        }
    }
    let mut owners: Vec<(bool, String)> = members
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|m| {
            let name = m["user"]["username"].as_str()?.to_owned();
            Some((m["role"].as_str() != Some("Owner"), name))
        })
        .collect();
    owners.sort();
    let mut categories = strings(&project["categories"]);
    categories.extend(strings(&project["additional_categories"]));
    Details {
        body: text(&project["body"]),
        gallery: gallery.into_iter().map(|(_, i)| i).collect(),
        links,
        authors: owners.into_iter().map(|(_, n)| n).collect(),
        followers: project["followers"].as_u64().unwrap_or(0),
        published: text(&project["published"]),
        updated: text(&project["updated"]),
        categories,
        client_side: text(&project["client_side"]),
        server_side: text(&project["server_side"]),
        github: None,
    }
}

/// Modrinth's `/tag/category`, `/tag/loader` and `/tag/game_version` for one project type.
pub fn modrinth_tags(
    project_type: &str,
    categories: &Value,
    loaders: &Value,
    versions: &Value,
) -> Tags {
    let loader_names: Vec<String> = loaders
        .as_array()
        .into_iter()
        .flatten()
        .filter(|l| {
            l["supported_project_types"]
                .as_array()
                .is_some_and(|t| t.iter().any(|x| x == project_type))
        })
        .filter_map(|l| l["name"].as_str().map(str::to_owned))
        .collect();
    let mut categories: Vec<Tag> = categories
        .as_array()
        .into_iter()
        .flatten()
        .filter(|c| c["project_type"] == project_type)
        .filter_map(|c| {
            let name = c["name"].as_str()?.to_owned();
            // Loaders are listed as categories too; the loader filter has them.
            (!loader_names.contains(&name)).then(|| Tag {
                name,
                header: text(&c["header"]),
            })
        })
        .collect();
    categories.sort_by(|a, b| (&a.header, &a.name).cmp(&(&b.header, &b.name)));
    Tags {
        categories,
        tags: Vec::new(),
        loaders: loader_names,
        game_versions: versions
            .as_array()
            .into_iter()
            .flatten()
            .filter(|v| v["version_type"] == "release")
            .filter_map(|v| v["version"].as_str().map(str::to_owned))
            .collect(),
    }
}

/// The loaders a kind lists by default, when no loader is picked: Java mod loaders for mods.
pub fn default_loaders(kind: Kind) -> Vec<&'static str> {
    match kind {
        Kind::Mod => super::java::LOADERS.to_vec(),
        _ => Vec::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn a_modrinth_page() {
        let project = json!({
            "body": "# Fences\nMore fences.",
            "gallery": [
                {"url": "https://cdn.modrinth.com/data/x/images/b.png", "featured": false, "ordering": 0, "title": "B"},
                {"url": "https://cdn.modrinth.com/data/x/images/a.png", "featured": true, "ordering": 5, "title": "A"},
                {"url": "http://example.com/c.png", "featured": false, "ordering": 1}
            ],
            "source_url": "https://github.com/x/y", "issues_url": "javascript:alert(1)",
            "wiki_url": null, "discord_url": "https://discord.gg/x",
            "donation_urls": [{"platform": "Ko-fi", "url": "https://ko-fi.com/x"}],
            "followers": 12, "categories": ["decoration"], "additional_categories": ["utility"],
            "client_side": "required", "server_side": "required"
        });
        let members = json!([
            {"user": {"username": "helper"}, "role": "Member"},
            {"user": {"username": "Macaw"}, "role": "Owner"}
        ]);
        let d = modrinth_details(&project, &members);
        assert_eq!(
            d.gallery
                .iter()
                .map(|i| i.title.as_str())
                .collect::<Vec<_>>(),
            ["A", "B"]
        );
        assert_eq!(
            d.links.iter().map(|l| l.label.as_str()).collect::<Vec<_>>(),
            ["Source", "Discord", "Donate (Ko-fi)"]
        );
        assert_eq!(d.authors, ["Macaw", "helper"]);
        assert_eq!(d.categories, ["decoration", "utility"]);
        assert_eq!(d.followers, 12);
    }

    #[test]
    fn modrinth_tags_by_project_type() {
        let categories = json!([
            {"name": "decoration", "project_type": "mod", "header": "categories"},
            {"name": "fabric", "project_type": "mod", "header": "loaders"},
            {"name": "16x", "project_type": "resourcepack", "header": "resolutions"}
        ]);
        let loaders = json!([
            {"name": "fabric", "supported_project_types": ["mod", "modpack"]},
            {"name": "minecraft", "supported_project_types": ["resourcepack"]}
        ]);
        let versions = json!([
            {"version": "26.3", "version_type": "release"},
            {"version": "26.4-snapshot-1", "version_type": "snapshot"}
        ]);
        let t = modrinth_tags("mod", &categories, &loaders, &versions);
        assert_eq!(t.categories.len(), 1);
        assert_eq!(t.categories[0].name, "decoration");
        assert_eq!(t.loaders, ["fabric"]);
        assert_eq!(t.game_versions, ["26.3"]);
    }

    #[test]
    fn categories_read_as_the_site_names_them() {
        assert_eq!(category_label("game-mechanics"), "Game mechanics");
        assert_eq!(category_label("kitchen-sink"), "Kitchen sink");
        assert_eq!(category_label("worldgen"), "World generation");
        assert_eq!(category_label("vanilla-like"), "Vanilla-like");
        assert_eq!(category_label("gui"), "GUI");
        assert_eq!(category_label("16x"), "16x");
        assert_eq!(category_label(""), "");
    }

    #[test]
    fn sorts_default_to_relevance_with_text() {
        let f = Filters::default();
        assert_eq!(f.sort(""), Sort::Downloads);
        assert_eq!(f.sort("fences"), Sort::Relevance);
        let f = Filters {
            sort: Some(Sort::Updated),
            game_version: Some("1.21.1".into()),
            ..Filters::default()
        };
        assert_eq!(f.sort("x"), Sort::Updated);
        assert!(f.other_version());
    }
}
