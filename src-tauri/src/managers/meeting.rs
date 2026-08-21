//! Meeting store.
//!
//! Persists recorded meetings — transcript, summary, and metadata — in a
//! dedicated SQLite table, kept separate from `HistoryManager` so meetings are
//! long-lived and never pruned by the dictation-history retention limits.
//! Modeled on `history.rs`.

use anyhow::{anyhow, Result};
use chrono::{DateTime, Local, Utc};
use log::{debug, error, info};
use rusqlite::{params, Connection, OptionalExtension};
use rusqlite_migration::{Migrations, M};
use serde::{Deserialize, Serialize};
use specta::Type;
use std::fs;
use std::path::PathBuf;
use tauri::AppHandle;
use tauri_specta::Event;

static MIGRATIONS: &[M] = &[M::up(
    "CREATE TABLE IF NOT EXISTS meetings (
        id INTEGER PRIMARY KEY AUTOINCREMENT,
        title TEXT NOT NULL,
        timestamp INTEGER NOT NULL,
        audio_file TEXT NOT NULL DEFAULT '',
        transcript TEXT NOT NULL DEFAULT '',
        summary TEXT,
        summary_prompt TEXT,
        model TEXT,
        duration_secs INTEGER NOT NULL DEFAULT 0
    );",
)];

#[derive(Clone, Debug, Serialize, Deserialize, Type)]
pub struct Meeting {
    pub id: i64,
    pub title: String,
    pub timestamp: i64,
    pub audio_file: String,
    pub transcript: String,
    pub summary: Option<String>,
    pub summary_prompt: Option<String>,
    pub model: Option<String>,
    pub duration_secs: i64,
}

#[derive(Clone, Debug, Serialize, Deserialize, Type, tauri_specta::Event)]
#[serde(tag = "action")]
pub enum MeetingUpdatePayload {
    #[serde(rename = "added")]
    Added { meeting: Meeting },
    #[serde(rename = "updated")]
    Updated { meeting: Meeting },
    #[serde(rename = "deleted")]
    Deleted { id: i64 },
}

pub struct MeetingManager {
    app_handle: AppHandle,
    recordings_dir: PathBuf,
    db_path: PathBuf,
}

impl MeetingManager {
    pub fn new(app_handle: &AppHandle) -> Result<Self> {
        let app_data_dir = crate::portable::app_data_dir(app_handle)?;
        let recordings_dir = app_data_dir.join("meetings");
        let db_path = app_data_dir.join("meetings.db");

        if !recordings_dir.exists() {
            fs::create_dir_all(&recordings_dir)?;
            debug!("Created meetings directory: {:?}", recordings_dir);
        }

        let manager = Self {
            app_handle: app_handle.clone(),
            recordings_dir,
            db_path,
        };
        manager.init_database()?;
        Ok(manager)
    }

    fn init_database(&self) -> Result<()> {
        info!("Initializing meetings database at {:?}", self.db_path);
        let mut conn = Connection::open(&self.db_path)?;
        let migrations = Migrations::new(MIGRATIONS.to_vec());
        #[cfg(debug_assertions)]
        migrations.validate().expect("Invalid meeting migrations");
        migrations
            .to_latest(&mut conn)
            .map_err(|e| anyhow!("Meeting DB migration failed: {}", e))?;
        Ok(())
    }

    fn get_connection(&self) -> Result<Connection> {
        Ok(Connection::open(&self.db_path)?)
    }

    pub fn recordings_dir(&self) -> &std::path::Path {
        &self.recordings_dir
    }

    /// Absolute path for a meeting WAV file name inside the meetings directory.
    pub fn audio_file_path(&self, file_name: &str) -> PathBuf {
        self.recordings_dir.join(file_name)
    }

    fn format_title(timestamp: i64) -> String {
        if let Some(utc) = DateTime::from_timestamp(timestamp, 0) {
            utc.with_timezone(&Local)
                .format("%B %e, %Y - %l:%M %p")
                .to_string()
        } else {
            format!("Meeting {}", timestamp)
        }
    }

    fn map_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<Meeting> {
        Ok(Meeting {
            id: row.get("id")?,
            title: row.get("title")?,
            timestamp: row.get("timestamp")?,
            audio_file: row.get("audio_file")?,
            transcript: row.get("transcript")?,
            summary: row.get("summary")?,
            summary_prompt: row.get("summary_prompt")?,
            model: row.get("model")?,
            duration_secs: row.get("duration_secs")?,
        })
    }

    /// Persist a freshly recorded + transcribed meeting (no summary yet).
    pub fn save_meeting(
        &self,
        audio_file: String,
        transcript: String,
        duration_secs: i64,
    ) -> Result<Meeting> {
        let timestamp = Utc::now().timestamp();
        let title = Self::format_title(timestamp);

        let conn = self.get_connection()?;
        conn.execute(
            "INSERT INTO meetings (title, timestamp, audio_file, transcript, duration_secs)
             VALUES (?1, ?2, ?3, ?4, ?5)",
            params![&title, timestamp, &audio_file, &transcript, duration_secs],
        )?;

        let meeting = Meeting {
            id: conn.last_insert_rowid(),
            title,
            timestamp,
            audio_file,
            transcript,
            summary: None,
            summary_prompt: None,
            model: None,
            duration_secs,
        };

        self.emit(MeetingUpdatePayload::Added {
            meeting: meeting.clone(),
        });
        Ok(meeting)
    }

    /// Store (or replace) a generated summary and the prompt/model used.
    pub fn update_summary(
        &self,
        id: i64,
        summary: String,
        summary_prompt: String,
        model: String,
    ) -> Result<Meeting> {
        let conn = self.get_connection()?;
        let updated = conn.execute(
            "UPDATE meetings SET summary = ?1, summary_prompt = ?2, model = ?3 WHERE id = ?4",
            params![summary, summary_prompt, model, id],
        )?;
        if updated == 0 {
            return Err(anyhow!("Meeting {} not found", id));
        }
        let meeting = self.require(id)?;
        self.emit(MeetingUpdatePayload::Updated {
            meeting: meeting.clone(),
        });
        Ok(meeting)
    }

    /// Rename a meeting.
    pub fn rename(&self, id: i64, title: String) -> Result<Meeting> {
        let conn = self.get_connection()?;
        let updated = conn.execute(
            "UPDATE meetings SET title = ?1 WHERE id = ?2",
            params![title, id],
        )?;
        if updated == 0 {
            return Err(anyhow!("Meeting {} not found", id));
        }
        let meeting = self.require(id)?;
        self.emit(MeetingUpdatePayload::Updated {
            meeting: meeting.clone(),
        });
        Ok(meeting)
    }

    pub fn get_meetings(&self) -> Result<Vec<Meeting>> {
        let conn = self.get_connection()?;
        let mut stmt = conn.prepare(
            "SELECT id, title, timestamp, audio_file, transcript, summary,
                    summary_prompt, model, duration_secs
             FROM meetings ORDER BY timestamp DESC",
        )?;
        let rows = stmt.query_map([], Self::map_row)?;
        let mut out = Vec::new();
        for r in rows {
            out.push(r?);
        }
        Ok(out)
    }

    pub fn get_by_id(&self, id: i64) -> Result<Option<Meeting>> {
        let conn = self.get_connection()?;
        let meeting = conn
            .query_row(
                "SELECT id, title, timestamp, audio_file, transcript, summary,
                        summary_prompt, model, duration_secs
                 FROM meetings WHERE id = ?1",
                params![id],
                Self::map_row,
            )
            .optional()?;
        Ok(meeting)
    }

    fn require(&self, id: i64) -> Result<Meeting> {
        self.get_by_id(id)?
            .ok_or_else(|| anyhow!("Meeting {} not found", id))
    }

    pub fn delete(&self, id: i64) -> Result<()> {
        // Remove the audio file too, if present.
        if let Ok(Some(meeting)) = self.get_by_id(id) {
            if !meeting.audio_file.is_empty() {
                let path = self.audio_file_path(&meeting.audio_file);
                if path.exists() {
                    let _ = fs::remove_file(&path);
                }
            }
        }
        let conn = self.get_connection()?;
        let deleted = conn.execute("DELETE FROM meetings WHERE id = ?1", params![id])?;
        if deleted == 0 {
            return Err(anyhow!("Meeting {} not found", id));
        }
        self.emit(MeetingUpdatePayload::Deleted { id });
        Ok(())
    }

    fn emit(&self, payload: MeetingUpdatePayload) {
        if let Err(e) = payload.emit(&self.app_handle) {
            error!("Failed to emit meeting-updated event: {}", e);
        }
    }
}
