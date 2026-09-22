use parking_lot::Mutex;
use serde::Serialize;
use std::{
    fs::{create_dir_all, File, OpenOptions},
    io::Write,
    path::Path,
};

pub struct Journal {
    file: Mutex<File>,
}

impl Journal {
    pub fn open(path: &Path) -> anyhow::Result<Self> {
        if let Some(parent) = path.parent() {
            if !parent.as_os_str().is_empty() {
                create_dir_all(parent)?;
            }
        }
        let file = OpenOptions::new().create(true).append(true).open(path)?;
        Ok(Self { file: Mutex::new(file) })
    }

    pub fn append<T: Serialize>(&self, value: &T) {
        if let Ok(line) = serde_json::to_string(value) {
            let mut file = self.file.lock();
            let _ = writeln!(file, "{line}");
            let _ = file.flush();
        }
    }
}
