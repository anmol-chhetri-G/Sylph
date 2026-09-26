use rusqlite::{params, Connection};
use std::path::{Path, PathBuf};

const SCHEMA: &str = "CREATE TABLE IF NOT EXISTS documents (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    title TEXT NOT NULL DEFAULT 'Untitled',
    created_at DATETIME DEFAULT CURRENT_TIMESTAMP,
    updated_at DATETIME DEFAULT CURRENT_TIMESTAMP
);
CREATE TABLE IF NOT EXISTS crdt_updates (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    document_id INTEGER REFERENCES documents(id),
    update_blob BLOB NOT NULL,
    created_at DATETIME DEFAULT CURRENT_TIMESTAMP
);
CREATE TABLE IF NOT EXISTS app_state (
    key TEXT PRIMARY KEY,
    value TEXT NOT NULL
);
CREATE TABLE IF NOT EXISTS document_models (
    document_id INTEGER PRIMARY KEY REFERENCES documents(id),
    model_json TEXT NOT NULL
);";

/// Platform data directory for Sylph (database, exports, images).
/// Not relative to the launch cwd: starting the app from another folder
/// must not scatter or lose its files.
pub fn data_dir() -> PathBuf {
    dirs::data_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join("sylph")
}

pub struct Storage {
    conn: Connection,
}

impl Storage {
    /// Open the database in the platform data directory.
    pub fn open() -> Result<Self, Box<dyn std::error::Error>> {
        Self::open_in(data_dir())
    }

    /// Open a database inside `dir` (created if missing). Tests use this
    /// with a private temp dir so they never touch the real user data.
    pub fn open_in(dir: impl AsRef<Path>) -> Result<Self, Box<dyn std::error::Error>> {
        let dir = dir.as_ref();
        std::fs::create_dir_all(dir)?;
        let conn = Connection::open(dir.join("sylph.db"))?;
        conn.execute_batch(SCHEMA)?;
        Ok(Self { conn })
    }

    /// Working in-memory database; the fallback when the data dir cannot
    /// be opened (read-only system, missing permissions).
    fn in_memory() -> Self {
        let conn = Connection::open(":memory:").expect("Failed to open in-memory database");
        conn.execute_batch(SCHEMA)
            .expect("Failed to create in-memory tables");
        Self { conn }
    }

    pub fn save_document(
        &self,
        doc_id: i64,
        crdt_update: &[u8],
    ) -> Result<(), Box<dyn std::error::Error>> {
        self.conn.execute(
            "INSERT INTO crdt_updates (document_id, update_blob) VALUES (?1, ?2)",
            params![doc_id, crdt_update],
        )?;
        self.conn.execute(
            "UPDATE documents SET updated_at = CURRENT_TIMESTAMP WHERE id = ?1",
            params![doc_id],
        )?;
        Ok(())
    }

    pub fn load_document(
        &self,
        doc_id: i64,
    ) -> Result<Option<Vec<u8>>, Box<dyn std::error::Error>> {
        let mut stmt = self.conn.prepare(
            "SELECT update_blob FROM crdt_updates WHERE document_id = ?1 ORDER BY id DESC LIMIT 1",
        )?;
        let mut rows = stmt.query(params![doc_id])?;
        if let Some(row) = rows.next()? {
            let blob: Vec<u8> = row.get(0)?;
            Ok(Some(blob))
        } else {
            Ok(None)
        }
    }

    pub fn create_document(&self, title: &str) -> Result<i64, Box<dyn std::error::Error>> {
        self.conn
            .execute("INSERT INTO documents (title) VALUES (?1)", params![title])?;
        Ok(self.conn.last_insert_rowid())
    }

    pub fn list_documents(&self) -> Result<Vec<(i64, String)>, Box<dyn std::error::Error>> {
        let mut stmt = self
            .conn
            .prepare("SELECT id, title FROM documents ORDER BY updated_at DESC")?;
        let rows = stmt.query_map([], |row| Ok((row.get(0)?, row.get(1)?)))?;
        let mut docs = Vec::new();
        for row in rows {
            docs.push(row?);
        }
        Ok(docs)
    }

    pub fn save_text(&self, doc_id: i64, text: &str) -> Result<(), Box<dyn std::error::Error>> {
        self.save_document(doc_id, text.as_bytes())
    }

    pub fn update_title(&self, doc_id: i64, title: &str) -> Result<(), Box<dyn std::error::Error>> {
        self.conn.execute(
            "UPDATE documents SET title = ?1, updated_at = CURRENT_TIMESTAMP WHERE id = ?2",
            params![title, doc_id],
        )?;
        Ok(())
    }

    pub fn get_title(&self, doc_id: i64) -> Result<String, Box<dyn std::error::Error>> {
        let mut stmt = self
            .conn
            .prepare("SELECT title FROM documents WHERE id = ?1")?;
        let mut rows = stmt.query(params![doc_id])?;
        if let Some(row) = rows.next()? {
            Ok(row.get(0)?)
        } else {
            Ok("Untitled".to_string())
        }
    }

    pub fn load_text(&self, doc_id: i64) -> Result<Option<String>, Box<dyn std::error::Error>> {
        match self.load_document(doc_id)? {
            Some(bytes) => Ok(Some(String::from_utf8(bytes)?)),
            None => Ok(None),
        }
    }

    /// Store a document's structured model (page setup, cover page,
    /// inserted blocks) as JSON, replacing the previous copy.
    pub fn save_model(
        &self,
        doc_id: i64,
        model_json: &str,
    ) -> Result<(), Box<dyn std::error::Error>> {
        self.conn.execute(
            "INSERT OR REPLACE INTO document_models (document_id, model_json) VALUES (?1, ?2)",
            params![doc_id, model_json],
        )?;
        self.conn.execute(
            "UPDATE documents SET updated_at = CURRENT_TIMESTAMP WHERE id = ?1",
            params![doc_id],
        )?;
        Ok(())
    }

    /// The JSON stored by `save_model`, or `None` for a document that
    /// never had one (e.g. created before models were saved).
    pub fn load_model(&self, doc_id: i64) -> Result<Option<String>, Box<dyn std::error::Error>> {
        let mut stmt = self
            .conn
            .prepare("SELECT model_json FROM document_models WHERE document_id = ?1")?;
        let mut rows = stmt.query(params![doc_id])?;
        match rows.next()? {
            Some(row) => Ok(Some(row.get(0)?)),
            None => Ok(None),
        }
    }

    /// Remember which document is open, so the next launch reopens it.
    pub fn set_last_opened(&self, doc_id: i64) -> Result<(), Box<dyn std::error::Error>> {
        self.conn.execute(
            "INSERT OR REPLACE INTO app_state (key, value) VALUES ('last_document_id', ?1)",
            params![doc_id.to_string()],
        )?;
        Ok(())
    }

    /// The document recorded by `set_last_opened`, if it still exists.
    pub fn last_opened(&self) -> Result<Option<i64>, Box<dyn std::error::Error>> {
        let mut stmt = self.conn.prepare(
            "SELECT d.id FROM app_state s
             JOIN documents d ON d.id = CAST(s.value AS INTEGER)
             WHERE s.key = 'last_document_id'",
        )?;
        let mut rows = stmt.query([])?;
        match rows.next()? {
            Some(row) => Ok(Some(row.get(0)?)),
            None => Ok(None),
        }
    }

    /// The document to show at launch: the one open last time; else (a
    /// database from before that was recorded) the latest document; else a
    /// fresh "Untitled". The choice is recorded, so repeated launches reopen
    /// the same document instead of piling up empty "Untitled" rows.
    pub fn open_last_or_create(&self) -> Result<i64, Box<dyn std::error::Error>> {
        let doc_id = match self.last_opened()? {
            Some(doc_id) => doc_id,
            None => match self.latest_document()? {
                Some(doc_id) => doc_id,
                None => self.create_document("Untitled")?,
            },
        };
        self.set_last_opened(doc_id)?;
        Ok(doc_id)
    }

    /// The most recently saved document that still has content, else the
    /// newest one. Throwaway "Untitled" documents are saved empty when the
    /// user switches away, so an empty latest save must not win. Ordered by
    /// `crdt_updates.id` rather than `updated_at`: ids only grow, so the
    /// order is exact even for saves within the same second. Empty and
    /// never-saved documents sort last (NULLs are lowest), newest first.
    fn latest_document(&self) -> Result<Option<i64>, Box<dyn std::error::Error>> {
        let mut stmt = self.conn.prepare(
            "SELECT d.id FROM documents d
             ORDER BY (SELECT CASE WHEN length(u.update_blob) > 0 THEN u.id END
                       FROM crdt_updates u WHERE u.document_id = d.id
                       ORDER BY u.id DESC LIMIT 1) DESC,
                      d.id DESC
             LIMIT 1",
        )?;
        let mut rows = stmt.query([])?;
        match rows.next()? {
            Some(row) => Ok(Some(row.get(0)?)),
            None => Ok(None),
        }
    }
}

impl Default for Storage {
    fn default() -> Self {
        Self::open().unwrap_or_else(|_| Self::in_memory())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};

    /// A unique on-disk database per call: parallel tests never share
    /// state and the real user data dir stays untouched.
    fn temp_storage() -> Storage {
        static N: AtomicUsize = AtomicUsize::new(0);
        let dir = std::env::temp_dir().join(format!(
            "sylph-storage-test-{}-{}",
            std::process::id(),
            N.fetch_add(1, Ordering::SeqCst)
        ));
        Storage::open_in(&dir).unwrap()
    }

    // ── Launch document ───────────────────────────────────────────

    #[test]
    fn test_launch_reuses_the_document_instead_of_piling_up() {
        let storage = temp_storage();
        let first = storage.open_last_or_create().unwrap();
        let second = storage.open_last_or_create().unwrap();
        assert_eq!(first, second);
        assert_eq!(storage.list_documents().unwrap().len(), 1);
    }

    #[test]
    fn test_launch_prefers_the_last_opened_document() {
        let storage = temp_storage();
        let a = storage.create_document("A").unwrap();
        let b = storage.create_document("B").unwrap();
        storage.save_text(a, "saved after B was opened").unwrap();
        storage.set_last_opened(b).unwrap();
        assert_eq!(storage.open_last_or_create().unwrap(), b);
    }

    #[test]
    fn test_launch_without_a_record_opens_the_latest_save() {
        // A database from before last-opened was recorded: every earlier
        // launch left an empty "Untitled", and the newest row is one.
        let storage = temp_storage();
        let a = storage.create_document("A").unwrap();
        let b = storage.create_document("B").unwrap();
        storage.save_text(b, "b").unwrap();
        storage.save_text(a, "a").unwrap();
        storage.create_document("Untitled").unwrap();
        // Switching away saves a throwaway document empty; that later but
        // empty save must not beat the document with real content.
        let throwaway = storage.create_document("Untitled").unwrap();
        storage.save_text(throwaway, "").unwrap();
        assert_eq!(storage.open_last_or_create().unwrap(), a);
    }

    #[test]
    fn test_last_opened_ignores_a_deleted_document() {
        let storage = temp_storage();
        let a = storage.create_document("A").unwrap();
        let b = storage.create_document("B").unwrap();
        storage.set_last_opened(b).unwrap();
        storage
            .conn
            .execute("DELETE FROM documents WHERE id = ?1", params![b])
            .unwrap();
        assert_eq!(storage.last_opened().unwrap(), None);
        assert_eq!(storage.open_last_or_create().unwrap(), a);
    }

    // ── Structured model ──────────────────────────────────────────

    #[test]
    fn test_model_round_trips_and_replaces() {
        let storage = temp_storage();
        let id = storage.create_document("Report").unwrap();
        assert_eq!(storage.load_model(id).unwrap(), None);
        storage.save_model(id, r#"{"page_size":"A4"}"#).unwrap();
        storage.save_model(id, r#"{"page_size":"Letter"}"#).unwrap();
        // One row per document: the newer model replaces the older one.
        assert_eq!(
            storage.load_model(id).unwrap().as_deref(),
            Some(r#"{"page_size":"Letter"}"#)
        );
    }

    #[test]
    fn test_models_are_kept_per_document() {
        let storage = temp_storage();
        let a = storage.create_document("A").unwrap();
        let b = storage.create_document("B").unwrap();
        storage.save_model(a, "a-model").unwrap();
        assert_eq!(storage.load_model(a).unwrap().as_deref(), Some("a-model"));
        assert_eq!(storage.load_model(b).unwrap(), None);
    }

    // ── Construction ──────────────────────────────────────────────

    #[test]
    fn test_data_dir_is_not_relative_to_the_launch_cwd() {
        let dir = data_dir();
        assert!(
            dir.is_absolute(),
            "data dir must not depend on the launch cwd: {}",
            dir.display()
        );
        assert!(dir.ends_with("sylph"));
    }

    #[test]
    fn test_open_creates_database() {
        let storage = temp_storage();
        let id = storage.create_document("Test Doc").unwrap();
        assert!(id > 0);
    }

    #[test]
    fn test_in_memory_fallback_creates_working_database() {
        let storage = Storage::in_memory();
        let id = storage.create_document("Fallback Doc").unwrap();
        assert!(id > 0);
    }

    #[test]
    fn test_schema_is_identical_for_disk_and_memory() {
        for storage in [temp_storage(), Storage::in_memory()] {
            let id = storage.create_document("Schema Test").unwrap();
            storage.save_text(id, "test content").unwrap();
            let loaded = storage.load_text(id).unwrap();
            assert_eq!(loaded.as_deref(), Some("test content"));
        }
    }

    // ── Create Document ───────────────────────────────────────────

    #[test]
    fn test_create_document_returns_unique_ids() {
        let storage = temp_storage();
        let id1 = storage.create_document("Doc 1").unwrap();
        let id2 = storage.create_document("Doc 2").unwrap();
        let id3 = storage.create_document("Doc 3").unwrap();
        assert_ne!(id1, id2);
        assert_ne!(id2, id3);
        assert_ne!(id1, id3);
    }

    #[test]
    fn test_create_document_with_empty_title() {
        let storage = temp_storage();
        let id = storage.create_document("").unwrap();
        let title = storage.get_title(id).unwrap();
        assert_eq!(title, "");
    }

    #[test]
    fn test_create_document_with_long_title() {
        let storage = temp_storage();
        let long_title = "A".repeat(1000);
        let id = storage.create_document(&long_title).unwrap();
        let title = storage.get_title(id).unwrap();
        assert_eq!(title, long_title);
    }

    // ── Save & Load Text ──────────────────────────────────────────

    #[test]
    fn test_save_and_load_text_round_trip() {
        let storage = temp_storage();
        let id = storage.create_document("Round-trip").unwrap();
        storage.save_text(id, "Hello, Sylph!").unwrap();
        let loaded = storage.load_text(id).unwrap();
        assert_eq!(loaded.as_deref(), Some("Hello, Sylph!"));
    }

    #[test]
    fn test_save_empty_text() {
        let storage = temp_storage();
        let id = storage.create_document("Empty").unwrap();
        storage.save_text(id, "").unwrap();
        let loaded = storage.load_text(id).unwrap();
        assert_eq!(loaded.as_deref(), Some(""));
    }

    #[test]
    fn test_save_unicode_text() {
        let storage = temp_storage();
        let id = storage.create_document("Unicode").unwrap();
        let text = "Hello 🌍 こんにちは مرحبا";
        storage.save_text(id, text).unwrap();
        let loaded = storage.load_text(id).unwrap();
        assert_eq!(loaded.as_deref(), Some(text));
    }

    #[test]
    fn test_save_multiline_text() {
        let storage = temp_storage();
        let id = storage.create_document("Multiline").unwrap();
        let text = "line1\nline2\nline3\n";
        storage.save_text(id, text).unwrap();
        let loaded = storage.load_text(id).unwrap();
        assert_eq!(loaded.as_deref(), Some(text));
    }

    #[test]
    fn test_overwrite_text() {
        let storage = temp_storage();
        let id = storage.create_document("Overwrite").unwrap();
        storage.save_text(id, "first version").unwrap();
        storage.save_text(id, "second version").unwrap();
        let loaded = storage.load_text(id).unwrap();
        assert_eq!(loaded.as_deref(), Some("second version"));
    }

    #[test]
    fn test_load_nonexistent_document() {
        let storage = temp_storage();
        let loaded = storage.load_text(99999).unwrap();
        assert_eq!(loaded, None);
    }

    // ── List Documents ────────────────────────────────────────────

    #[test]
    fn test_list_documents_empty() {
        let conn = Connection::open(":memory:").unwrap();
        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS documents (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                title TEXT NOT NULL DEFAULT 'Untitled',
                created_at DATETIME DEFAULT CURRENT_TIMESTAMP,
                updated_at DATETIME DEFAULT CURRENT_TIMESTAMP
            );
            CREATE TABLE IF NOT EXISTS crdt_updates (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                document_id INTEGER REFERENCES documents(id),
                update_blob BLOB NOT NULL,
                created_at DATETIME DEFAULT CURRENT_TIMESTAMP
            );",
        )
        .unwrap();
        let storage = Storage { conn };
        let docs = storage.list_documents().unwrap();
        assert!(docs.is_empty());
    }

    #[test]
    fn test_list_documents_multiple() {
        let storage = temp_storage();
        let _ = storage.create_document("Doc A");
        let _ = storage.create_document("Doc B");
        let _ = storage.create_document("Doc C");
        let docs = storage.list_documents().unwrap();
        assert!(docs.len() >= 3);
    }

    #[test]
    fn test_list_documents_contains_titles() {
        let storage = temp_storage();
        let _id = storage.create_document("My Document").unwrap();
        let docs = storage.list_documents().unwrap();
        let titles: Vec<&str> = docs.iter().map(|(_, t)| t.as_str()).collect();
        assert!(titles.contains(&"My Document"));
    }

    #[test]
    fn test_list_documents_ordered_by_updated() {
        let conn = Connection::open(":memory:").unwrap();
        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS documents (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                title TEXT NOT NULL DEFAULT 'Untitled',
                created_at DATETIME DEFAULT CURRENT_TIMESTAMP,
                updated_at DATETIME DEFAULT CURRENT_TIMESTAMP
            );
            CREATE TABLE IF NOT EXISTS crdt_updates (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                document_id INTEGER REFERENCES documents(id),
                update_blob BLOB NOT NULL,
                created_at DATETIME DEFAULT CURRENT_TIMESTAMP
            );",
        )
        .unwrap();
        let storage = Storage { conn };
        let id1 = storage.create_document("First").unwrap();
        let id2 = storage.create_document("Second").unwrap();
        let docs = storage.list_documents().unwrap();
        let ids: Vec<i64> = docs.iter().map(|(id, _)| *id).collect();
        assert!(ids.contains(&id1));
        assert!(ids.contains(&id2));
    }

    // ── Update Title ──────────────────────────────────────────────

    #[test]
    fn test_update_title() {
        let storage = temp_storage();
        let id = storage.create_document("Old Title").unwrap();
        storage.update_title(id, "New Title").unwrap();
        let title = storage.get_title(id).unwrap();
        assert_eq!(title, "New Title");
    }

    #[test]
    fn test_update_title_to_empty() {
        let storage = temp_storage();
        let id = storage.create_document("Title").unwrap();
        storage.update_title(id, "").unwrap();
        let title = storage.get_title(id).unwrap();
        assert_eq!(title, "");
    }

    #[test]
    fn test_get_title_nonexistent_document() {
        let storage = temp_storage();
        let title = storage.get_title(99999).unwrap();
        assert_eq!(title, "Untitled");
    }

    // ── Save Document (CRDT blob) ─────────────────────────────────

    #[test]
    fn test_save_and_load_document_blob() {
        let storage = temp_storage();
        let id = storage.create_document("Blob Test").unwrap();
        let blob = vec![0x01, 0x02, 0x03, 0xFF];
        storage.save_document(id, &blob).unwrap();
        let loaded = storage.load_document(id).unwrap();
        assert_eq!(loaded, Some(blob));
    }

    #[test]
    fn test_save_document_loads_latest() {
        let storage = temp_storage();
        let id = storage.create_document("Latest").unwrap();
        storage.save_document(id, b"first").unwrap();
        storage.save_document(id, b"second").unwrap();
        let loaded = storage.load_document(id).unwrap();
        assert_eq!(loaded, Some(b"second".to_vec()));
    }

    #[test]
    fn test_load_document_nonexistent() {
        let storage = temp_storage();
        let loaded = storage.load_document(99999).unwrap();
        assert_eq!(loaded, None);
    }

    // ── Compile-time Type Checks ──────────────────────────────────

    #[test]
    fn test_api_signatures_compile() {
        fn assert_send<T: Send>() {}

        assert_send::<Storage>();

        let storage = Storage::in_memory();
        let _: Result<i64, Box<dyn std::error::Error>> = storage.create_document("test");
        let _: Result<(), Box<dyn std::error::Error>> = storage.save_text(1, "test");
        let _: Result<Option<String>, Box<dyn std::error::Error>> = storage.load_text(1);
        let _: Result<Vec<(i64, String)>, Box<dyn std::error::Error>> = storage.list_documents();
        let _: Result<(), Box<dyn std::error::Error>> = storage.update_title(1, "test");
        let _: Result<String, Box<dyn std::error::Error>> = storage.get_title(1);
    }
}
