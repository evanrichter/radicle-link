use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use directories::ProjectDirs;

pub struct Paths(ProjectDirs);

impl Paths {
    pub fn new() -> Result<Self, io::Error> {
        let proj = ProjectDirs::from("xyz", "radicle", "radicle")
            .expect("Unable to determine application directories");
        Paths(proj).init()
    }

    // Don't use system paths, but the supplied directory as a root.
    //
    // For testing, you know.
    pub fn from_root(root: &Path) -> Result<Self, io::Error> {
        Paths(ProjectDirs::from_path(root.to_path_buf()).unwrap()).init()
    }

    pub fn keys_dir(&self) -> PathBuf {
        self.0.config_dir().join("keys")
    }

    fn init(self) -> Result<Self, io::Error> {
        fs::create_dir_all(self.keys_dir())?;
        Ok(self)
    }
}