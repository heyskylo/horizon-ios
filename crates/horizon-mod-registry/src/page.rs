//! A project's page in a submission: its categories, tags and description
//! (`horizon_mods::index::page`), and its images, uploaded with the form and
//! written into the registry (`icons/<id>.png`, `screenshots/<id>/<n>.<ext>`)
//! so the site and the launcher load them only from the registry's own host.
//! Also "Edit project": an owner changes the page with no new version.
use crate::{Config, Form, Ownership, Refusal, bad, github::GitHub, github_down, now, ownership};
use axum::http::StatusCode;
use horizon_mods::index::{Kind, page as rules, registry::Entry};

/// The biggest image part: a screenshot (an icon is smaller, checked when added).
pub const MAX_IMAGE: usize = rules::MAX_SCREENSHOT as usize;
/// All of a submission's images together.
pub const MAX_IMAGES: usize = MAX_IMAGE * rules::MAX_SCREENSHOTS + MAX_ICON;
const MAX_ICON: usize = 256 << 10;

/// The images a submission uploads, checked.
#[derive(Default)]
pub struct Uploads {
    icon: Option<Vec<u8>>,
    /// Extension and bytes, in the order sent.
    screenshots: Vec<(String, Vec<u8>)>,
}

impl Uploads {
    /// Takes one image part (`icon` or `screenshot`) named `name` by the browser.
    pub fn add(&mut self, part: &str, name: &str, bytes: Vec<u8>) -> Result<(), String> {
        if part == "icon" {
            if bytes.len() > MAX_ICON {
                return Err(format!("the icon is over {} KiB", MAX_ICON >> 10));
            }
            rules::check_icon(&bytes)?;
            self.icon = Some(bytes);
            return Ok(());
        }
        if self.screenshots.len() == rules::MAX_SCREENSHOTS {
            return Err(format!("at most {} screenshots", rules::MAX_SCREENSHOTS));
        }
        rules::check_screenshot(name, &bytes)?;
        let (format, ..) = rules::image_size(&bytes).ok_or("not an image")?;
        self.screenshots
            .push((format.extensions()[0].to_owned(), bytes));
        Ok(())
    }

    pub fn is_empty(&self) -> bool {
        self.icon.is_none() && self.screenshots.is_empty()
    }
}

/// The page's own rules, for any submission: categories of the kind, plain tags, no icon address,
/// and a `shaderpack`'s statement that it is the submitter's to hand out.
pub fn check_form(form: &Form, kind: &Kind) -> Result<(), String> {
    let mut errors = rules::check(kind, &form.description, &form.categories, &form.tags);
    if form.icon.is_some() {
        errors.push("upload the icon (a square PNG) instead of giving its address".into());
    }
    if *kind == Kind::ShaderPack && !form.own_work {
        errors.push(
            "a shaders/-folder pack is listed only by its author, under a licence that lets the registry hand it out: \
             confirm both"
                .into(),
        );
    }
    if errors.is_empty() {
        Ok(())
    } else {
        Err(errors.join("; "))
    }
}

/// What a new project must have: a description, a category and an icon.
pub fn check_new(form: &Form, images: &Uploads) -> Result<(), String> {
    let mut errors = Vec::new();
    let length = form.description.trim().chars().count();
    if length < rules::MIN_DESCRIPTION {
        errors.push(format!(
            "the description is {length} characters: write at least {} (what it does, how to use it)",
            rules::MIN_DESCRIPTION
        ));
    }
    if form.categories.is_empty() {
        errors.push(format!("pick 1 to {} categories", rules::MAX_CATEGORIES));
    }
    if images.icon.is_none() {
        errors.push("upload an icon: a square PNG, 64 to 1024 pixels".into());
    }
    if errors.is_empty() {
        Ok(())
    } else {
        Err(errors.join("; "))
    }
}

/// Puts the uploaded images in `entry` and returns the files to write. The form's `screenshots`
/// keeps those of the entry it lists, in its order (absent: all of them); uploads come after.
pub fn place(
    entry: &mut Entry,
    form: &Form,
    images: &Uploads,
) -> Result<Vec<(String, Vec<u8>)>, String> {
    let id = entry.id.clone();
    let mut writes = Vec::new();
    if let Some(icon) = &images.icon {
        let path = format!("icons/{id}.png");
        entry.icon = Some(path.clone());
        writes.push((path, icon.clone()));
    }
    let mut kept = match &form.screenshots {
        None => entry.screenshots.clone(),
        Some(list) => {
            if let Some(stranger) = list.iter().find(|s| !entry.screenshots.contains(s)) {
                return Err(format!("{stranger:?} is not one of {id}'s screenshots"));
            }
            list.clone()
        }
    };
    if kept.len() + images.screenshots.len() > rules::MAX_SCREENSHOTS {
        return Err(format!("at most {} screenshots", rules::MAX_SCREENSHOTS));
    }
    // New names never reuse one the entry has had, so a page cached elsewhere never shows the wrong image.
    let taken: Vec<String> = entry
        .screenshots
        .iter()
        .filter_map(|s| s.rsplit('/').next()?.split('.').next().map(str::to_owned))
        .collect();
    let mut n = 1;
    for (extension, bytes) in &images.screenshots {
        while taken.contains(&n.to_string()) {
            n += 1;
        }
        let path = format!("screenshots/{id}/{n}.{extension}");
        kept.push(path.clone());
        writes.push((path, bytes.clone()));
        n += 1;
    }
    entry.screenshots = kept;
    Ok(writes)
}

/// Writes the images on `branch`, each replacing what main has at its path.
pub fn write(
    gh: &GitHub,
    writes: &[(String, Vec<u8>)],
    branch: &str,
    message: &str,
) -> Result<(), Refusal> {
    for (path, bytes) in writes {
        let blob = gh.blob(path).map_err(github_down)?;
        gh.put(path, bytes, branch, blob.as_deref(), message)
            .map_err(github_down)?;
    }
    Ok(())
}

/// "Edit project": an owner (by GitHub ID) changes the page of `form.id`, no version added. Opens
/// a pull request labelled `owner-edit`; returns its address.
pub fn edit(
    config: &Config,
    login: &str,
    user_id: u64,
    form: &Form,
    images: &Uploads,
) -> Result<String, Refusal> {
    let gh = &config.github;
    let Some((before, blob)) = gh.entry(&form.id).map_err(github_down)? else {
        return Err(Refusal::new(
            StatusCode::NOT_FOUND,
            format!("{} is not in the registry", form.id),
        ));
    };
    if ownership(&before, login, user_id) != Ownership::ById {
        return Err(Refusal::new(
            StatusCode::FORBIDDEN,
            format!(
                "{} is {}'s: only its owners edit its page",
                form.id,
                before.owners.join(", ")
            ),
        ));
    }
    check_form(form, &before.kind).map_err(bad)?;
    let mut entry = before.clone();
    entry.name = form.name.trim().to_owned();
    entry.summary = form.summary.trim().to_owned();
    entry.description = form.description.clone();
    entry.authors = form.authors.clone();
    entry.license = form.license.trim().to_owned();
    entry.source = form.source.trim().to_owned();
    entry.categories = form.categories.clone();
    entry.tags = form.tags.clone();
    let writes = place(&mut entry, form, images).map_err(bad)?;
    let problems = entry.check_fields();
    if !problems.is_empty() {
        return Err(bad(problems.join("; ")));
    }
    if entry == before && writes.is_empty() {
        return Err(bad("nothing changed"));
    }
    let changed: Vec<&str> = [
        ("name", entry.name != before.name),
        ("summary", entry.summary != before.summary),
        ("description", entry.description != before.description),
        ("authors", entry.authors != before.authors),
        ("licence", entry.license != before.license),
        ("source", entry.source != before.source),
        ("categories", entry.categories != before.categories),
        ("tags", entry.tags != before.tags),
        ("icon", images.icon.is_some()),
        ("screenshots", entry.screenshots != before.screenshots),
    ]
    .into_iter()
    .filter_map(|(field, changed)| changed.then_some(field))
    .collect();
    let branch = format!("edit/{}-{}", form.id, now());
    if !gh.branch(&branch).map_err(github_down)? {
        return Err(Refusal::new(
            StatusCode::CONFLICT,
            "an edit of this project was just sent: wait a second and send it again",
        ));
    }
    let message = format!(
        "Edit {}'s page\n\nSubmitted by @{login} through the form.\n\nCo-authored-by: {login} <{user_id}+{login}@users.noreply.github.com>",
        form.id
    );
    let text = entry.to_toml().map_err(bad)?;
    gh.put(
        &format!("mods/{}.toml", form.id),
        &text,
        &branch,
        Some(&blob),
        &message,
    )
    .map_err(github_down)?;
    write(gh, &writes, &branch, &message)?;
    let body = format!(
        "<!-- submitted-by: {login} -->\n🤖 Page edit sent through the form by @{login}, an owner of `{id}` (GitHub ID \
         {user_id}). The form opens the pull request with the registry's token: neither Ludovic nor a reviewer wrote it.\n\n\
         No new version: it changes the {fields}. Reviewers: read the new page text and look at the images.",
        id = form.id,
        fields = changed.join(", "),
    );
    let (url, number) = gh
        .pull(&format!("Edit {}'s page", form.id), &branch, &body)
        .map_err(github_down)?;
    if let Err(e) = gh.label(number, "owner-edit") {
        eprintln!("labelling {url}: {} {}", e.status, e.message);
    }
    Ok(url)
}
