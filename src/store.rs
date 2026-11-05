use serde::{Deserialize, Serialize};
use std::path::PathBuf;

use crate::theme;

fn default_w() -> i32 {
    theme::DEFAULT_W
}

fn default_h() -> i32 {
    theme::DEFAULT_H
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct NoteData {
    #[serde(default)]
    pub id: u64,
    #[serde(default)]
    pub x: i32,
    #[serde(default)]
    pub y: i32,
    #[serde(default = "default_w")]
    pub w: i32,
    #[serde(default = "default_h")]
    pub h: i32,
    #[serde(default)]
    pub color: u8,
    #[serde(default)]
    pub text: String,
}

impl NoteData {
    pub fn new(id: u64, x: i32, y: i32) -> Self {
        Self {
            id,
            x,
            y,
            w: theme::DEFAULT_W,
            h: theme::DEFAULT_H,
            color: 0,
            text: String::new(),
        }
    }
}

#[derive(Default, Serialize, Deserialize)]
struct FileData {
    #[serde(default)]
    notes: Vec<NoteData>,
    #[serde(default)]
    next_id: u64,
}

pub struct Store {
    path: PathBuf,
    pub notes: Vec<NoteData>,
    pub next_id: u64,
    pub fresh: bool,
}

fn data_dir() -> PathBuf {
    if let Some(appdata) = std::env::var_os("APPDATA") {
        return PathBuf::from(appdata).join("PostIt");
    }
    std::env::current_exe()
        .ok()
        .and_then(|p| p.parent().map(PathBuf::from))
        .unwrap_or_else(|| PathBuf::from("."))
        .join("PostItData")
}

impl Store {
    pub fn load() -> Self {
        let dir = data_dir();
        let path = dir.join("notes.json");

        let mut store = Self {
            path: path.clone(),
            notes: Vec::new(),
            next_id: 1,
            fresh: false,
        };

        let Ok(bytes) = std::fs::read(&path) else {
            store.fresh = true;
            return store;
        };

        match serde_json::from_slice::<FileData>(&bytes) {
            Ok(mut data) => {
                data.notes.retain(|n| n.id > 0);
                let max_id = data.notes.iter().map(|n| n.id).max().unwrap_or(0);
                store.next_id = data.next_id.max(max_id + 1).max(1);
                store.notes = data.notes;
            }
            Err(err) => {
                // Ne jamais ecraser silencieusement une sauvegarde illisible.
                eprintln!("Fichier de notes illisible ({err}), copie de secours.");
                let backup = dir.join("notes.corrupt.json");
                let _ = std::fs::copy(&path, backup);
            }
        }
        store
    }

    pub fn save(&self) -> std::io::Result<()> {
        let dir = self.path.parent().unwrap_or_else(|| std::path::Path::new("."));
        std::fs::create_dir_all(dir)?;

        let data = FileData {
            notes: self.notes.clone(),
            next_id: self.next_id,
        };
        let bytes = serde_json::to_vec(&data).map_err(std::io::Error::other)?;

        let tmp = self.path.with_extension("json.tmp");
        std::fs::write(&tmp, &bytes)?;
        // Sur Windows, fs::rename remplace la destination de facon atomique.
        std::fs::rename(&tmp, &self.path)?;
        Ok(())
    }
}
