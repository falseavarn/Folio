//! Folio's data on disk, kept out of sight in `.folio` folders. `.folio/projects.json` in the
//! Folio folder lists the project folders. Each project folder stands on its own: its
//! `.folio/project.json` holds its name, links, notes and descriptions, next to a copy of each
//! of its images. An asset tagged with several projects is written to every one of their folders.

use serde::{Deserialize, Serialize, de::DeserializeOwned};
use std::{
    fs,
    hash::{BuildHasher, Hasher},
    io,
    path::{Path, PathBuf},
    time::{SystemTime, UNIX_EPOCH},
};

pub const DATA_DIR: &str = ".folio";
const REGISTRY: &str = ".folio/projects.json";
pub const PROJECT_FILE: &str = ".folio/project.json";
pub const IMAGE_EXTENSIONS: [&str; 5] = ["png", "jpg", "jpeg", "gif", "webp"];

#[derive(Serialize, Deserialize, Default)]
struct Registry {
    #[serde(default)]
    folders: Vec<PathBuf>,
}

#[derive(Serialize, Deserialize)]
struct ProjectFile {
    id: u64,
    name: String,
    #[serde(default)]
    assets: Vec<StoredAsset>,
}

#[derive(Serialize, Deserialize)]
struct StoredAsset {
    id: u64,
    #[serde(flatten)]
    kind: Kind,
    #[serde(default)]
    description: String,
    #[serde(default)]
    added: u64,
}

pub struct Project {
    pub id: u64,
    pub name: String,
    pub dir: PathBuf,
}

#[derive(Clone)]
pub struct Asset {
    pub id: u64,
    pub kind: Kind,
    pub description: String,
    pub projects: Vec<u64>,
    /// Unix seconds; orders the list newest first.
    pub added: u64,
}

#[derive(Serialize, Deserialize, Clone, PartialEq, Debug)]
#[serde(tag = "type", rename_all = "lowercase")]
pub enum Kind {
    Link {
        url: String,
    },
    /// `file` is the image's name inside each of its projects' folders.
    Image {
        file: String,
    },
    Note {
        body: String,
    },
}

#[derive(Default)]
pub struct Library {
    pub projects: Vec<Project>,
    pub assets: Vec<Asset>,
}

pub struct Store {
    root: PathBuf,
    // Every registered folder, including ones that could not be read, so they are kept on save.
    folders: Vec<PathBuf>,
    pub lib: Library,
    /// Project folders that could not be read at startup. Folio never writes to them.
    pub warnings: Vec<String>,
}

fn invalid(msg: &str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidInput, msg)
}

pub fn is_image(path: &Path) -> bool {
    path.extension()
        .and_then(|e| e.to_str())
        .is_some_and(|e| IMAGE_EXTENSIONS.contains(&e.to_lowercase().as_str()))
}

fn new_id() -> u64 {
    // RandomState is seeded per process and differs per call; mixing in the clock keeps ids unique across runs.
    let mut h = std::collections::hash_map::RandomState::new().build_hasher();
    h.write_u128(
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos(),
    );
    h.finish()
}

fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_secs())
}

fn read_json<T: DeserializeOwned>(path: &Path) -> io::Result<Option<T>> {
    match fs::read(path) {
        Ok(bytes) => serde_json::from_slice(&bytes).map(Some).map_err(|e| {
            io::Error::new(
                io::ErrorKind::InvalidData,
                format!("{} is unreadable: {e}", path.display()),
            )
        }),
        Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(e),
    }
}

/// Write-then-rename so a crash mid-write never leaves a half-written file.
fn write_json<T: Serialize>(path: &Path, value: &T) -> io::Result<()> {
    let tmp = path.with_extension("json.tmp");
    fs::write(&tmp, serde_json::to_vec_pretty(value)?)?;
    fs::rename(tmp, path)
}

impl Store {
    /// `$FOLIO_DIR`, else `~/Folio`, so the files are easy to find and back up.
    pub fn default_root() -> PathBuf {
        if let Some(dir) = std::env::var_os("FOLIO_DIR") {
            return dir.into();
        }
        let home = std::env::var_os("HOME")
            .or_else(|| std::env::var_os("USERPROFILE"))
            .unwrap_or_else(|| ".".into());
        PathBuf::from(home).join("Folio")
    }

    /// Fails only when the project list itself is unreadable; unreadable project folders become warnings.
    pub fn open(root: PathBuf) -> io::Result<Self> {
        fs::create_dir_all(root.join(DATA_DIR))?;
        let registry: Registry = read_json(&root.join(REGISTRY))?.unwrap_or_default();
        let mut store = Self {
            root,
            folders: registry.folders,
            lib: Library::default(),
            warnings: Vec::new(),
        };
        for dir in store.folders.clone() {
            if let Err(e) = store.load_project(&dir) {
                store
                    .warnings
                    .push(format!("Skipped {}: {e}", dir.display()));
            }
        }
        Ok(store)
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    /// Where a new project named `name` goes unless the user picks another folder.
    pub fn suggested_dir(&self, name: &str) -> PathBuf {
        self.root.join(name.trim().replace(['/', '\\'], "-"))
    }

    pub fn project(&self, id: u64) -> Option<&Project> {
        self.lib.projects.iter().find(|p| p.id == id)
    }

    pub fn project_name(&self, id: u64) -> &str {
        self.project(id).map_or("", |p| p.name.as_str())
    }

    /// The image file shown for an asset: the copy in its first project's folder.
    pub fn image_of(&self, asset: &Asset) -> Option<PathBuf> {
        let Kind::Image { file } = &asset.kind else {
            return None;
        };
        Some(
            self.project(*asset.projects.first()?)?
                .dir
                .join(DATA_DIR)
                .join(file),
        )
    }

    /// Assets that would be deleted with this project because it is their only tag.
    pub fn only_in(&self, project: u64) -> usize {
        self.lib
            .assets
            .iter()
            .filter(|a| a.projects == [project])
            .count()
    }

    /// A Markdown summary of a project for people and AI agents: every asset with where it lives
    /// and its description, newest first. Image paths point at this project's own copies.
    pub fn export_markdown(&self, project: u64) -> Option<String> {
        let p = self.project(project)?;
        let mut assets: Vec<&Asset> = self
            .lib
            .assets
            .iter()
            .filter(|a| a.projects.contains(&project))
            .collect();
        assets.sort_by_key(|a| std::cmp::Reverse(a.added));
        let count = |f: fn(&Kind) -> bool| assets.iter().filter(|a| f(&a.kind)).count();
        let (links, images, notes) = (
            count(|k| matches!(k, Kind::Link { .. })),
            count(|k| matches!(k, Kind::Image { .. })),
            count(|k| matches!(k, Kind::Note { .. })),
        );
        let mut out = format!(
            "# {}\n\nResearch assets collected in Folio: {links} links, {images} images, {notes} notes.\n\nProject folder: `{}`\n",
            p.name,
            p.dir.display()
        );
        for (heading, group) in [("Links", 0), ("Images", 1), ("Notes", 2)] {
            let items: Vec<&&Asset> = assets
                .iter()
                .filter(|a| {
                    matches!(
                        (group, &a.kind),
                        (0, Kind::Link { .. }) | (1, Kind::Image { .. }) | (2, Kind::Note { .. })
                    )
                })
                .collect();
            if items.is_empty() {
                continue;
            }
            out += &format!("\n## {heading}\n\n");
            for a in items {
                match &a.kind {
                    Kind::Link { url } => out += &format!("- <{url}>\n"),
                    Kind::Image { file } => {
                        out += &format!("- `{}`\n", p.dir.join(DATA_DIR).join(file).display())
                    }
                    Kind::Note { body } => {
                        out += "- Note\n";
                        for line in body.trim().lines() {
                            out += &format!("  > {line}\n");
                        }
                    }
                }
                if !a.description.is_empty() {
                    out += &format!(
                        "  Description: {}\n",
                        a.description
                            .split_whitespace()
                            .collect::<Vec<_>>()
                            .join(" ")
                    );
                }
                let others: Vec<&str> = a
                    .projects
                    .iter()
                    .filter(|&&o| o != project)
                    .map(|&o| self.project_name(o))
                    .collect();
                if !others.is_empty() {
                    out += &format!("  Also in: {}\n", others.join(", "));
                }
            }
        }
        Some(out)
    }

    fn load_project(&mut self, dir: &Path) -> io::Result<u64> {
        let file: ProjectFile = read_json(&dir.join(PROJECT_FILE))?.ok_or_else(|| {
            io::Error::new(io::ErrorKind::NotFound, "no Folio project in this folder")
        })?;
        if self.project(file.id).is_some() {
            return Err(invalid("this project is already open from another folder"));
        }
        for stored in file.assets {
            match self.lib.assets.iter_mut().find(|a| a.id == stored.id) {
                // shortcut: copies of a shared asset edited by hand can disagree; the first folder read wins.
                Some(a) => a.projects.push(file.id),
                None => self.lib.assets.push(Asset {
                    id: stored.id,
                    kind: stored.kind,
                    description: stored.description,
                    projects: vec![file.id],
                    added: stored.added,
                }),
            }
        }
        self.lib.projects.push(Project {
            id: file.id,
            name: file.name,
            dir: dir.to_path_buf(),
        });
        Ok(file.id)
    }

    /// Creates a project in `dir`, or opens the project already saved there.
    pub fn add_project(&mut self, name: &str, dir: PathBuf) -> io::Result<u64> {
        if self.folders.contains(&dir) {
            return Err(invalid("This folder is already a project in Folio."));
        }
        let id = if dir.join(PROJECT_FILE).exists() {
            self.load_project(&dir)?
        } else {
            let name = name.trim();
            if name.is_empty() {
                return Err(invalid("Give the project a name."));
            }
            fs::create_dir_all(dir.join(DATA_DIR))?;
            let id = new_id();
            self.lib.projects.push(Project {
                id,
                name: name.into(),
                dir: dir.clone(),
            });
            id
        };
        self.folders.push(dir);
        self.save()?;
        Ok(id)
    }

    pub fn rename_project(&mut self, id: u64, name: &str) -> io::Result<()> {
        let name = name.trim();
        if name.is_empty() {
            return Err(invalid("Give the project a name."));
        }
        if let Some(p) = self.lib.projects.iter_mut().find(|p| p.id == id) {
            p.name = name.into();
        }
        self.save()
    }

    /// Removes the project, its file and its image copies, and deletes the assets left with no project.
    /// The folder itself is removed only if nothing else is in it.
    pub fn delete_project(&mut self, id: u64) -> io::Result<()> {
        let Some(i) = self.lib.projects.iter().position(|p| p.id == id) else {
            return Ok(());
        };
        let project = self.lib.projects.remove(i);
        self.folders.retain(|f| f != &project.dir);
        let mut files = Vec::new();
        self.lib.assets.retain_mut(|a| {
            if a.projects.contains(&id) {
                a.projects.retain(|&p| p != id);
                if let Kind::Image { file } = &a.kind {
                    files.push(project.dir.join(DATA_DIR).join(file));
                }
            }
            !a.projects.is_empty()
        });
        self.save()?;
        files.push(project.dir.join(PROJECT_FILE));
        for f in files {
            let _ = fs::remove_file(f);
        }
        // Only removed when empty, so anything the user keeps there survives.
        let _ = fs::remove_dir(project.dir.join(DATA_DIR));
        let _ = fs::remove_dir(&project.dir);
        Ok(())
    }

    /// Creates (`id == None`) or replaces an asset. `image` is a new image to copy into its projects' folders.
    pub fn put_asset(
        &mut self,
        id: Option<u64>,
        mut kind: Kind,
        description: &str,
        mut projects: Vec<u64>,
        image: Option<&Path>,
    ) -> io::Result<()> {
        projects.retain(|p| self.project(*p).is_some());
        if projects.is_empty() {
            return Err(invalid("Tag the asset with at least one project."));
        }
        let old = match id {
            Some(id) => Some(
                self.lib
                    .assets
                    .iter()
                    .find(|a| a.id == id)
                    .cloned()
                    .ok_or_else(|| invalid("This asset no longer exists."))?,
            ),
            None => None,
        };
        // Where a project keeps its image copies.
        let dir = |store: &Self, p: u64| {
            store
                .project(p)
                .map(|p| p.dir.join(DATA_DIR))
                .unwrap_or_default()
        };

        // Image copies to make now, and old copies to remove once the save has succeeded.
        let mut copies = Vec::new();
        match &mut kind {
            Kind::Link { url } => {
                *url = url.trim().into();
                let rest = url
                    .strip_prefix("https://")
                    .or_else(|| url.strip_prefix("http://"));
                if rest.is_none_or(str::is_empty) {
                    return Err(invalid(
                        "Enter a link that starts with http:// or https://.",
                    ));
                }
            }
            Kind::Note { body } if body.trim().is_empty() => {
                return Err(invalid("Write the note before saving."));
            }
            Kind::Note { .. } => {}
            Kind::Image { file } => match image {
                Some(src) => {
                    if !is_image(src) {
                        return Err(invalid("Choose a PNG, JPEG, GIF or WebP image."));
                    }
                    let ext = src
                        .extension()
                        .and_then(|e| e.to_str())
                        .unwrap_or_default()
                        .to_lowercase();
                    *file = format!("{:016x}.{ext}", new_id());
                    copies.extend(
                        projects
                            .iter()
                            .map(|&p| (src.to_path_buf(), dir(self, p).join(&*file))),
                    );
                }
                None if file.is_empty() => return Err(invalid("Choose an image file.")),
                None => {
                    let source = old
                        .as_ref()
                        .and_then(|o| self.image_of(o))
                        .ok_or_else(|| invalid("Choose an image file."))?;
                    let had = old.as_ref().map(|o| o.projects.clone()).unwrap_or_default();
                    copies.extend(
                        projects
                            .iter()
                            .filter(|p| !had.contains(p))
                            .map(|&p| (source.clone(), dir(self, p).join(&*file))),
                    );
                }
            },
        }
        let mut stale = Vec::new();
        if let Some(Asset {
            kind: Kind::Image { file: old_file },
            projects: old_projects,
            ..
        }) = &old
        {
            let kept = |p: &u64| {
                kind == (Kind::Image {
                    file: old_file.clone(),
                }) && projects.contains(p)
            };
            stale.extend(
                old_projects
                    .iter()
                    .filter(|p| !kept(p))
                    .map(|&p| dir(self, p).join(old_file)),
            );
        }

        for (src, dst) in &copies {
            fs::copy(src, dst)?;
        }
        let asset = Asset {
            id: old.as_ref().map_or_else(new_id, |o| o.id),
            kind,
            description: description.trim().into(),
            projects,
            added: old.as_ref().map_or_else(now, |o| o.added),
        };
        match self.lib.assets.iter_mut().find(|a| a.id == asset.id) {
            Some(slot) => *slot = asset,
            None => self.lib.assets.push(asset),
        }
        self.save()?;
        for f in stale {
            let _ = fs::remove_file(f);
        }
        Ok(())
    }

    pub fn delete_asset(&mut self, id: u64) -> io::Result<()> {
        let Some(i) = self.lib.assets.iter().position(|a| a.id == id) else {
            return Ok(());
        };
        let asset = self.lib.assets.remove(i);
        let files: Vec<PathBuf> = match &asset.kind {
            Kind::Image { file } => asset
                .projects
                .iter()
                .filter_map(|&p| Some(self.project(p)?.dir.join(DATA_DIR).join(file)))
                .collect(),
            _ => Vec::new(),
        };
        self.save()?;
        for f in files {
            let _ = fs::remove_file(f);
        }
        Ok(())
    }

    fn save(&self) -> io::Result<()> {
        for p in &self.lib.projects {
            let assets = self
                .lib
                .assets
                .iter()
                .filter(|a| a.projects.contains(&p.id))
                .map(|a| StoredAsset {
                    id: a.id,
                    kind: a.kind.clone(),
                    description: a.description.clone(),
                    added: a.added,
                })
                .collect();
            write_json(
                &p.dir.join(PROJECT_FILE),
                &ProjectFile {
                    id: p.id,
                    name: p.name.clone(),
                    assets,
                },
            )?;
        }
        write_json(
            &self.root.join(REGISTRY),
            &Registry {
                folders: self.folders.clone(),
            },
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_dir(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("folio-test-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        dir
    }

    fn link(url: &str) -> Kind {
        Kind::Link { url: url.into() }
    }

    fn project_file(dir: &Path) -> ProjectFile {
        read_json(&dir.join(PROJECT_FILE)).unwrap().unwrap()
    }

    #[test]
    fn projects_live_in_their_own_folders() {
        let t = temp_dir("folders");
        let mut s = Store::open(t.join("root")).unwrap();
        let a = s.add_project("Thesis", t.join("thesis")).unwrap();
        let b = s.add_project("Heat", t.join("elsewhere/heat")).unwrap();
        s.put_asset(
            None,
            link(" https://example.org "),
            "  intro ",
            vec![a, b],
            None,
        )
        .unwrap();
        s.put_asset(
            None,
            Kind::Note {
                body: "only b".into(),
            },
            "",
            vec![b],
            None,
        )
        .unwrap();
        assert_eq!(project_file(&t.join("thesis")).assets.len(), 1);
        assert_eq!(project_file(&t.join("elsewhere/heat")).assets.len(), 2);

        let s = Store::open(t.join("root")).unwrap();
        let shared = s
            .lib
            .assets
            .iter()
            .find(|x| x.kind == link("https://example.org"))
            .unwrap();
        assert_eq!(
            (shared.projects.len(), shared.description.as_str()),
            (2, "intro")
        );
        assert_eq!(s.project_name(a), "Thesis");
    }

    #[test]
    fn rejects_invalid_input() {
        let t = temp_dir("invalid");
        let mut s = Store::open(t.join("root")).unwrap();
        let p = s.add_project("A", t.join("a")).unwrap();
        assert!(
            s.put_asset(None, link("https://x"), "", vec![], None)
                .is_err()
        );
        assert!(
            s.put_asset(None, link("https://x"), "", vec![999], None)
                .is_err()
        );
        assert!(
            s.put_asset(None, link("example.org"), "", vec![p], None)
                .is_err()
        );
        assert!(
            s.put_asset(None, link("https://"), "", vec![p], None)
                .is_err()
        );
        assert!(
            s.put_asset(None, Kind::Note { body: "  ".into() }, "", vec![p], None)
                .is_err()
        );
        assert!(
            s.put_asset(
                None,
                Kind::Image {
                    file: String::new()
                },
                "",
                vec![p],
                None
            )
            .is_err()
        );
        assert!(s.add_project("  ", t.join("b")).is_err());
        assert!(s.add_project("Again", t.join("a")).is_err());
        assert!(s.lib.assets.is_empty());
    }

    #[test]
    fn deleting_project_keeps_shared_assets_and_cleans_its_folder() {
        let t = temp_dir("delete-project");
        let mut s = Store::open(t.join("root")).unwrap();
        let a = s.add_project("A", t.join("a")).unwrap();
        let b = s.add_project("B", t.join("b")).unwrap();
        s.put_asset(None, link("https://only-a"), "", vec![a], None)
            .unwrap();
        s.put_asset(None, link("https://both"), "", vec![a, b], None)
            .unwrap();
        assert_eq!(s.only_in(a), 1);
        s.delete_project(a).unwrap();
        assert!(!t.join("a").exists());
        assert_eq!(s.lib.assets.len(), 1);
        assert_eq!(s.lib.assets[0].projects, vec![b]);
        assert_eq!(Store::open(t.join("root")).unwrap().lib.projects.len(), 1);
    }

    #[test]
    fn image_copies_follow_their_projects() {
        let t = temp_dir("image");
        let mut s = Store::open(t.join("root")).unwrap();
        let a = s.add_project("A", t.join("a")).unwrap();
        let b = s.add_project("B", t.join("b")).unwrap();
        let src = t.join("source.PNG");
        fs::write(&src, b"not really a png").unwrap();
        let file = |s: &Store| match &s.lib.assets[0].kind {
            Kind::Image { file } => file.clone(),
            _ => panic!(),
        };

        s.put_asset(
            None,
            Kind::Image {
                file: String::new(),
            },
            "",
            vec![a],
            Some(&src),
        )
        .unwrap();
        let first = file(&s);
        assert!(first.ends_with(".png") && t.join("a").join(DATA_DIR).join(&first).exists());

        // Tagging another project copies the image there; untagging removes that copy.
        let id = s.lib.assets[0].id;
        let kind = s.lib.assets[0].kind.clone();
        s.put_asset(Some(id), kind.clone(), "", vec![a, b], None)
            .unwrap();
        assert!(t.join("b").join(DATA_DIR).join(&first).exists());
        s.put_asset(Some(id), kind, "", vec![b], None).unwrap();
        assert!(
            !t.join("a").join(DATA_DIR).join(&first).exists()
                && t.join("b").join(DATA_DIR).join(&first).exists()
        );

        // Replacing the image removes the old copy.
        s.put_asset(
            Some(id),
            s.lib.assets[0].kind.clone(),
            "",
            vec![b],
            Some(&src),
        )
        .unwrap();
        let second = file(&s);
        assert!(
            !t.join("b").join(DATA_DIR).join(&first).exists()
                && t.join("b").join(DATA_DIR).join(&second).exists()
        );

        s.delete_asset(id).unwrap();
        assert!(!t.join("b").join(DATA_DIR).join(&second).exists());
    }

    #[test]
    fn exports_a_project_as_markdown() {
        let t = temp_dir("export");
        let mut s = Store::open(t.join("root")).unwrap();
        let a = s.add_project("Heat", t.join("heat")).unwrap();
        let b = s.add_project("Thesis", t.join("thesis")).unwrap();
        let src = t.join("map.png");
        fs::write(&src, b"png").unwrap();
        s.put_asset(
            None,
            link("https://example.org"),
            "EPA\noverview",
            vec![a, b],
            None,
        )
        .unwrap();
        s.put_asset(
            None,
            Kind::Image {
                file: String::new(),
            },
            "",
            vec![a],
            Some(&src),
        )
        .unwrap();
        s.put_asset(
            None,
            Kind::Note {
                body: "line one\nline two".into(),
            },
            "",
            vec![a],
            None,
        )
        .unwrap();
        s.put_asset(None, link("https://elsewhere.org"), "", vec![b], None)
            .unwrap();

        let md = s.export_markdown(a).unwrap();
        let image = s.lib.assets.iter().find_map(|x| s.image_of(x)).unwrap();
        assert!(md.starts_with("# Heat\n"));
        assert!(md.contains("1 links, 1 images, 1 notes"));
        assert!(
            md.contains(
                "- <https://example.org>\n  Description: EPA overview\n  Also in: Thesis\n"
            )
        );
        assert!(md.contains(&format!("- `{}`", image.display())) && image.exists());
        assert!(md.contains("- Note\n  > line one\n  > line two\n"));
        assert!(!md.contains("elsewhere"));
        // Folio's own files stay hidden; the project folder shows only `.folio`.
        let visible: Vec<_> = fs::read_dir(t.join("heat"))
            .unwrap()
            .map(|e| e.unwrap().file_name())
            .collect();
        assert_eq!(visible, [DATA_DIR]);
    }

    #[test]
    fn opening_an_existing_folder_adopts_its_project() {
        let t = temp_dir("adopt");
        let mut s = Store::open(t.join("root-1")).unwrap();
        let p = s.add_project("Shared", t.join("shared")).unwrap();
        s.put_asset(None, link("https://example.org"), "", vec![p], None)
            .unwrap();

        let mut other = Store::open(t.join("root-2")).unwrap();
        assert_eq!(other.add_project("", t.join("shared")).unwrap(), p);
        assert_eq!(
            (other.project_name(p), other.lib.assets.len()),
            ("Shared", 1)
        );
    }

    #[test]
    fn unreadable_project_folders_are_kept_but_never_written() {
        let t = temp_dir("unreadable");
        let mut s = Store::open(t.join("root")).unwrap();
        let a = s.add_project("A", t.join("a")).unwrap();
        s.add_project("B", t.join("b")).unwrap();
        fs::write(t.join("b").join(PROJECT_FILE), b"{ broken").unwrap();

        let mut s = Store::open(t.join("root")).unwrap();
        assert_eq!((s.lib.projects.len(), s.warnings.len()), (1, 1));
        s.put_asset(None, link("https://example.org"), "", vec![a], None)
            .unwrap();
        assert_eq!(
            fs::read(t.join("b").join(PROJECT_FILE)).unwrap(),
            b"{ broken"
        );
        assert_eq!(Store::open(t.join("root")).unwrap().warnings.len(), 1);
    }
}
