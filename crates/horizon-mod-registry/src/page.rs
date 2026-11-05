//! A project's page in a submission: its categories, tags and description
//! (`horizon_mods::index::page`), and its images, uploaded with the form and
//! written into the registry (`icons/<id>.png`, `screenshots/<id>/<sha256>.<ext>`)
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
    // Only the same bytes may reuse an image URL, including after an earlier
    // edit removed it. Current filenames alone cannot remember deleted images.
    // Uploads::add already fixes the extension from the checked image format.
    for (extension, bytes) in &images.screenshots {
        let digest = horizon_mods::index::sha256(bytes);
        let path = format!("screenshots/{id}/{digest}.{extension}");
        if kept.contains(&path) {
            continue;
        }
        kept.push(path.clone());
        writes.push((path, bytes.clone()));
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    // Original, complete 320x180 grayscale PNGs (black and white), including
    // valid zlib data and CRCs. The upload validator accepts them as screenshots.
    const FIRST: &[u8] = &[
        0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a, 0x00, 0x00, 0x00, 0x0d, 0x49, 0x48, 0x44,
        0x52, 0x00, 0x00, 0x01, 0x40, 0x00, 0x00, 0x00, 0xb4, 0x01, 0x00, 0x00, 0x00, 0x00, 0xcd,
        0x9f, 0xc8, 0xbc, 0x00, 0x00, 0x00, 0x1e, 0x49, 0x44, 0x41, 0x54, 0x78, 0xda, 0xed, 0xc1,
        0x31, 0x01, 0x00, 0x00, 0x00, 0xc2, 0xa0, 0xf5, 0x4f, 0x6d, 0x0d, 0x0f, 0xa0, 0x00, 0x00,
        0x00, 0x00, 0x00, 0x00, 0x00, 0x7e, 0x0d, 0x1c, 0xd4, 0x00, 0x01, 0xbd, 0xf8, 0x70, 0x4a,
        0x00, 0x00, 0x00, 0x00, 0x49, 0x45, 0x4e, 0x44, 0xae, 0x42, 0x60, 0x82,
    ];
    const SECOND: &[u8] = &[
        0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a, 0x00, 0x00, 0x00, 0x0d, 0x49, 0x48, 0x44,
        0x52, 0x00, 0x00, 0x01, 0x40, 0x00, 0x00, 0x00, 0xb4, 0x01, 0x00, 0x00, 0x00, 0x00, 0xcd,
        0x9f, 0xc8, 0xbc, 0x00, 0x00, 0x00, 0x30, 0x49, 0x44, 0x41, 0x54, 0x78, 0xda, 0xed, 0xca,
        0xa1, 0x01, 0x00, 0x00, 0x0c, 0x02, 0x20, 0xff, 0x7f, 0x5a, 0x5f, 0x58, 0x5a, 0x82, 0x4c,
        0x7a, 0x14, 0x51, 0x14, 0x45, 0x51, 0x14, 0x45, 0x51, 0x14, 0x45, 0x51, 0x14, 0x45, 0x51,
        0x14, 0x45, 0x51, 0x14, 0x45, 0x51, 0x14, 0xc5, 0xff, 0x38, 0xa2, 0x1d, 0x05, 0x85, 0x3a,
        0xd7, 0x21, 0xd8, 0x00, 0x00, 0x00, 0x00, 0x49, 0x45, 0x4e, 0x44, 0xae, 0x42, 0x60, 0x82,
    ];

    fn entry() -> Entry {
        serde_json::from_value(serde_json::json!({
            "id": "demo", "kind": "mod", "owners": ["alice"]
        }))
        .unwrap()
    }

    fn form(screenshots: Option<Vec<String>>) -> Form {
        serde_json::from_value(serde_json::json!({
            "id": "demo", "kind": "mod", "name": "Demo", "summary": "Demo",
            "license": "MIT", "screenshots": screenshots
        }))
        .unwrap()
    }

    fn upload(bytes: &[u8]) -> Uploads {
        let mut uploads = Uploads::default();
        uploads
            .add("screenshot", "shot.png", bytes.to_vec())
            .unwrap();
        uploads
    }

    #[test]
    fn removed_images_never_give_different_pixels_a_cached_url() {
        let mut entry = entry();
        let first = place(&mut entry, &form(None), &upload(FIRST)).unwrap();
        assert_eq!(first.len(), 1);
        assert_eq!(first[0].1, FIRST);
        let first_url = first[0].0.clone();
        assert_eq!(
            first_url,
            "screenshots/demo/d1a474de8f36472e63cfbb1c678583a564a8f3c4023f9ea28c230935a7a1c8a3.png"
        );

        // Simulate a completed edit removing it from the persisted entry, then
        // a later edit with different pixels. No history of filenames is kept.
        assert!(
            place(&mut entry, &form(Some(Vec::new())), &Uploads::default())
                .unwrap()
                .is_empty()
        );
        assert!(entry.screenshots.is_empty());
        let second = place(&mut entry, &form(None), &upload(SECOND)).unwrap();
        assert_eq!(second.len(), 1);
        assert_ne!(second[0].0, first_url);
        assert_eq!(
            second[0].0,
            "screenshots/demo/20b74489d572845dba09f753220c35bf2aafdfbfd7442d1d9d367a9b69b4fd46.png"
        );
        assert_eq!(second[0].1, SECOND);

        // Re-adding the original bytes can reuse their original immutable URL.
        place(&mut entry, &form(Some(Vec::new())), &Uploads::default()).unwrap();
        let again = place(&mut entry, &form(None), &upload(FIRST)).unwrap();
        assert_eq!(again[0].0, first_url);
        assert_eq!(again[0].1, FIRST);
    }

    #[test]
    fn identical_uploads_share_one_path_and_one_write() {
        let mut entry = entry();
        let mut uploads = upload(FIRST);
        uploads
            .add("screenshot", "a-second-name.png", FIRST.to_vec())
            .unwrap();
        let writes = place(&mut entry, &form(None), &uploads).unwrap();
        assert_eq!(entry.screenshots.len(), 1);
        assert_eq!(writes.len(), 1);
        assert_eq!(writes[0].1, FIRST);
        let before = entry.screenshots.clone();
        assert!(
            place(&mut entry, &form(None), &upload(FIRST))
                .unwrap()
                .is_empty()
        );
        assert_eq!(entry.screenshots, before);
    }

    #[test]
    fn screenshot_paths_and_validation_keep_their_project_boundary() {
        let mut entry = entry();
        entry.screenshots = vec!["screenshots/demo/legacy.png".into()];
        let original = entry.screenshots.clone();
        let foreign = form(Some(vec!["screenshots/other/1.png".into()]));
        assert!(place(&mut entry, &foreign, &Uploads::default()).is_err());
        assert_eq!(entry.screenshots, original);

        let mut uploads = Uploads::default();
        uploads
            .add("screenshot", "../../outside.png", SECOND.to_vec())
            .unwrap();
        let writes = place(&mut entry, &form(None), &uploads).unwrap();
        assert_eq!(entry.screenshots[0], "screenshots/demo/legacy.png");
        assert_eq!(
            writes[0].0,
            "screenshots/demo/20b74489d572845dba09f753220c35bf2aafdfbfd7442d1d9d367a9b69b4fd46.png"
        );
        assert!(
            Path::new(&writes[0].0)
                .components()
                .all(|p| matches!(p, std::path::Component::Normal(_)))
        );

        // Keep the existing submitted-part limit: it is checked before
        // identical content is coalesced into URLs.
        entry.screenshots = (0..rules::MAX_SCREENSHOTS)
            .map(|n| format!("screenshots/demo/{n}.png"))
            .collect();
        assert!(place(&mut entry, &form(None), &upload(FIRST)).is_err());
        let mut invalid = Uploads::default();
        assert!(
            invalid
                .add("screenshot", "not-an-image.png", vec![0; 32])
                .is_err()
        );
        assert!(invalid.is_empty());
    }
}
