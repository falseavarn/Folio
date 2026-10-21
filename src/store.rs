//! The library on disk: `library.json` holds projects and asset records,
//! `files/` holds copies of imported images.

use serde::{Deserialize, Serialize};
use std::{
    fs, io,
    path::{Path, PathBuf},
};

const LIBRARY: &str = "library.json";
const FILES: &str = "files";

#[derive(Serialize, Deserialize, Default)]
pub struct Library {
    // Never reused, so a deleted image's id can't collide with a cached thumbnail.
    #[serde(default)]
    next_id: u64,
    #[serde(default)]
    pub projects: Vec<Project>,
    #[serde(default)]
    pub assets: Vec<Asset>,
}

#[derive(Serialize, Deserialize, Clone)]
pub struct Project {
    pub id: u64,
    pub name: String,
}

#[derive(Serialize, Deserialize, Clone)]
pub struct Asset {
    pub id: u64,
    #[serde(flatten)]
    pub kind: Kind,
    #[serde(default)]
    pub description: String,
    pub projects: Vec<u64>,
}

#[derive(Serialize, Deserialize, Clone, PartialEq, Debug)]
#[serde(tag = "type", rename_all = "lowercase")]
pub enum Kind {
    Link { url: String },
    Image { file: String },
    Note { body: String },
}

pub struct Store {
    root: PathBuf,
    pub lib: Library,
}

fn invalid(msg: &str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidInput, msg)
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

    pub fn open(root: PathBuf) -> io::Result<Self> {
        fs::create_dir_all(root.join(FILES))?;
        let lib = match fs::read(root.join(LIBRARY)) {
            // Refuse to start on a corrupt file rather than overwrite it with an empty library.
            Ok(bytes) => serde_json::from_slice(&bytes).map_err(|e| {
                io::Error::new(
                    io::ErrorKind::InvalidData,
                    format!("{LIBRARY} is unreadable: {e}"),
                )
            })?,
            Err(e) if e.kind() == io::ErrorKind::NotFound => Library::default(),
            Err(e) => return Err(e),
        };
        Ok(Self { root, lib })
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    pub fn image_path(&self, file: &str) -> PathBuf {
        self.root.join(FILES).join(file)
    }

    pub fn project_name(&self, id: u64) -> &str {
        self.lib
            .projects
            .iter()
            .find(|p| p.id == id)
            .map_or("", |p| p.name.as_str())
    }

    /// Assets that would be deleted with this project because it is their only tag.
    pub fn only_in(&self, project: u64) -> usize {
        self.lib
            .assets
            .iter()
            .filter(|a| a.projects == [project])
            .count()
    }

    pub fn add_project(&mut self, name: &str) -> io::Result<u64> {
        let name = name.trim();
        if name.is_empty() {
            return Err(invalid("Give the project a name."));
        }
        let id = self.next_id();
        self.lib.projects.push(Project {
            id,
            name: name.into(),
        });
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

    /// Deletes the project, untags its assets, and deletes the assets left with no project.
    pub fn delete_project(&mut self, id: u64) -> io::Result<()> {
        self.lib.projects.retain(|p| p.id != id);
        let mut gone = Vec::new();
        self.lib.assets.retain_mut(|a| {
            a.projects.retain(|&p| p != id);
            if a.projects.is_empty() {
                gone.push(a.kind.clone());
            }
            !a.projects.is_empty()
        });
        self.save()?;
        self.drop_unused(gone);
        Ok(())
    }

    /// Creates (`id == None`) or replaces an asset. `image` is a new image to copy into the library.
    pub fn put_asset(
        &mut self,
        id: Option<u64>,
        mut kind: Kind,
        description: &str,
        mut projects: Vec<u64>,
        image: Option<&Path>,
    ) -> io::Result<()> {
        projects.retain(|p| self.lib.projects.iter().any(|q| q.id == *p));
        if projects.is_empty() {
            return Err(invalid("Tag the asset with at least one project."));
        }
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
                Some(src) => *file = self.import_image(src)?,
                None if file.is_empty() => return Err(invalid("Choose an image file.")),
                None => {}
            },
        }
        let description = description.trim().to_string();
        let replaced = match id {
            Some(id) => {
                let slot = self
                    .lib
                    .assets
                    .iter_mut()
                    .find(|a| a.id == id)
                    .ok_or_else(|| invalid("This asset no longer exists."))?;
                let old = std::mem::replace(
                    slot,
                    Asset {
                        id,
                        kind,
                        description,
                        projects,
                    },
                );
                vec![old.kind]
            }
            None => {
                let id = self.next_id();
                self.lib.assets.push(Asset {
                    id,
                    kind,
                    description,
                    projects,
                });
                vec![]
            }
        };
        self.save()?;
        self.drop_unused(replaced);
        Ok(())
    }

    pub fn delete_asset(&mut self, id: u64) -> io::Result<()> {
        let gone: Vec<Kind> = self
            .lib
            .assets
            .iter()
            .filter(|a| a.id == id)
            .map(|a| a.kind.clone())
            .collect();
        self.lib.assets.retain(|a| a.id != id);
        self.save()?;
        self.drop_unused(gone);
        Ok(())
    }

    fn import_image(&mut self, src: &Path) -> io::Result<String> {
        let ext = src
            .extension()
            .and_then(|e| e.to_str())
            .unwrap_or("img")
            .to_lowercase();
        let file = format!("{}.{ext}", self.next_id());
        fs::copy(src, self.image_path(&file))?;
        Ok(file)
    }

    /// Removes image files no asset points at any more. Runs only after a successful save.
    fn drop_unused(&self, kinds: Vec<Kind>) {
        for kind in kinds {
            if let Kind::Image { file } = &kind
                && !self.lib.assets.iter().any(|a| a.kind == kind)
            {
                let _ = fs::remove_file(self.image_path(file));
            }
        }
    }

    fn next_id(&mut self) -> u64 {
        self.lib.next_id += 1;
        self.lib.next_id
    }

    /// Write-then-rename so a crash mid-write never leaves a half-written library.
    fn save(&self) -> io::Result<()> {
        let tmp = self.root.join("library.json.tmp");
        fs::write(&tmp, serde_json::to_vec_pretty(&self.lib)?)?;
        fs::rename(tmp, self.root.join(LIBRARY))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_store(name: &str) -> Store {
        let dir = std::env::temp_dir().join(format!("folio-test-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        Store::open(dir).unwrap()
    }

    fn link(url: &str) -> Kind {
        Kind::Link { url: url.into() }
    }

    #[test]
    fn persists_across_reopen() {
        let mut s = temp_store("reopen");
        let p = s.add_project("Thesis").unwrap();
        s.put_asset(
            None,
            link(" https://example.org "),
            "  intro ",
            vec![p],
            None,
        )
        .unwrap();
        let s = Store::open(s.root().to_path_buf()).unwrap();
        assert_eq!(s.lib.projects[0].name, "Thesis");
        assert_eq!(s.lib.assets[0].kind, link("https://example.org"));
        assert_eq!(s.lib.assets[0].description, "intro");
    }

    #[test]
    fn rejects_invalid_assets() {
        let mut s = temp_store("invalid");
        let p = s.add_project("A").unwrap();
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
        assert!(s.add_project("  ").is_err());
        assert!(s.lib.assets.is_empty());
    }

    #[test]
    fn deleting_project_keeps_shared_assets() {
        let mut s = temp_store("delete-project");
        let a = s.add_project("A").unwrap();
        let b = s.add_project("B").unwrap();
        s.put_asset(None, link("https://only-a"), "", vec![a], None)
            .unwrap();
        s.put_asset(None, link("https://both"), "", vec![a, b], None)
            .unwrap();
        assert_eq!(s.only_in(a), 1);
        s.delete_project(a).unwrap();
        assert_eq!(s.lib.assets.len(), 1);
        assert_eq!(s.lib.assets[0].kind, link("https://both"));
        assert_eq!(s.lib.assets[0].projects, vec![b]);
    }

    #[test]
    fn image_files_follow_their_asset() {
        let mut s = temp_store("image");
        let p = s.add_project("A").unwrap();
        let src = s.root().join("source.PNG");
        fs::write(&src, b"not really a png").unwrap();
        let image = || Kind::Image {
            file: String::new(),
        };

        s.put_asset(None, image(), "", vec![p], Some(&src)).unwrap();
        let Kind::Image { file: first } = s.lib.assets[0].kind.clone() else {
            panic!()
        };
        assert!(first.ends_with(".png") && s.image_path(&first).exists());

        // Replacing the image removes the old copy.
        let id = s.lib.assets[0].id;
        s.put_asset(
            Some(id),
            s.lib.assets[0].kind.clone(),
            "",
            vec![p],
            Some(&src),
        )
        .unwrap();
        let Kind::Image { file: second } = s.lib.assets[0].kind.clone() else {
            panic!()
        };
        assert!(!s.image_path(&first).exists() && s.image_path(&second).exists());

        s.delete_asset(id).unwrap();
        assert!(!s.image_path(&second).exists());
    }
}
