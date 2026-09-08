use rusqlite::{params, Connection};
use std::path::Path;

const DB_PATH: &str = "output/sylph.db";

pub struct Storage {
    conn: Connection,
}

impl Storage {
    pub fn open() -> Result<Self, Box<dyn std::error::Error>> {
        if let Some(parent) = Path::new(DB_PATH).parent() {
            std::fs::create_dir_all(parent)?;
        }
        let conn = Connection::open(DB_PATH)?;
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
        )?;
        Ok(Self { conn })
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
}

impl Default for Storage {
    fn default() -> Self {
        Self::open().unwrap_or_else(|_| {
            let conn = Connection::open(":memory:").expect("Failed to open in-memory database");
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
            .expect("Failed to create in-memory tables");
            Self { conn }
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // ── Construction ──────────────────────────────────────────────

    #[test]
    fn test_open_creates_database() {
        let storage = Storage::open().unwrap();
        let id = storage.create_document("Test Doc").unwrap();
        assert!(id > 0);
    }

    #[test]
    fn test_default_uses_in_memory_fallback() {
        let storage = Storage::default();
        let id = storage.create_document("Fallback Doc").unwrap();
        assert!(id > 0);
    }

    #[test]
    fn test_default_has_correct_schema() {
        let storage = Storage::default();
        let id = storage.create_document("Schema Test").unwrap();
        storage.save_text(id, "test content").unwrap();
        let loaded = storage.load_text(id).unwrap();
        assert_eq!(loaded.as_deref(), Some("test content"));
    }

    // ── Create Document ───────────────────────────────────────────

    #[test]
    fn test_create_document_returns_unique_ids() {
        let storage = Storage::open().unwrap();
        let id1 = storage.create_document("Doc 1").unwrap();
        let id2 = storage.create_document("Doc 2").unwrap();
        let id3 = storage.create_document("Doc 3").unwrap();
        assert_ne!(id1, id2);
        assert_ne!(id2, id3);
        assert_ne!(id1, id3);
    }

    #[test]
    fn test_create_document_with_empty_title() {
        let storage = Storage::open().unwrap();
        let id = storage.create_document("").unwrap();
        let title = storage.get_title(id).unwrap();
        assert_eq!(title, "");
    }

    #[test]
    fn test_create_document_with_long_title() {
        let storage = Storage::open().unwrap();
        let long_title = "A".repeat(1000);
        let id = storage.create_document(&long_title).unwrap();
        let title = storage.get_title(id).unwrap();
        assert_eq!(title, long_title);
    }

    // ── Save & Load Text ──────────────────────────────────────────

    #[test]
    fn test_save_and_load_text_round_trip() {
        let storage = Storage::open().unwrap();
        let id = storage.create_document("Round-trip").unwrap();
        storage.save_text(id, "Hello, Sylph!").unwrap();
        let loaded = storage.load_text(id).unwrap();
        assert_eq!(loaded.as_deref(), Some("Hello, Sylph!"));
    }

    #[test]
    fn test_save_empty_text() {
        let storage = Storage::open().unwrap();
        let id = storage.create_document("Empty").unwrap();
        storage.save_text(id, "").unwrap();
        let loaded = storage.load_text(id).unwrap();
        assert_eq!(loaded.as_deref(), Some(""));
    }

    #[test]
    fn test_save_unicode_text() {
        let storage = Storage::open().unwrap();
        let id = storage.create_document("Unicode").unwrap();
        let text = "Hello 🌍 こんにちは مرحبا";
        storage.save_text(id, text).unwrap();
        let loaded = storage.load_text(id).unwrap();
        assert_eq!(loaded.as_deref(), Some(text));
    }

    #[test]
    fn test_save_multiline_text() {
        let storage = Storage::open().unwrap();
        let id = storage.create_document("Multiline").unwrap();
        let text = "line1\nline2\nline3\n";
        storage.save_text(id, text).unwrap();
        let loaded = storage.load_text(id).unwrap();
        assert_eq!(loaded.as_deref(), Some(text));
    }

    #[test]
    fn test_overwrite_text() {
        let storage = Storage::open().unwrap();
        let id = storage.create_document("Overwrite").unwrap();
        storage.save_text(id, "first version").unwrap();
        storage.save_text(id, "second version").unwrap();
        let loaded = storage.load_text(id).unwrap();
        assert_eq!(loaded.as_deref(), Some("second version"));
    }

    #[test]
    fn test_load_nonexistent_document() {
        let storage = Storage::open().unwrap();
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
        let storage = Storage::open().unwrap();
        let _ = storage.create_document("Doc A");
        let _ = storage.create_document("Doc B");
        let _ = storage.create_document("Doc C");
        let docs = storage.list_documents().unwrap();
        assert!(docs.len() >= 3);
    }

    #[test]
    fn test_list_documents_contains_titles() {
        let storage = Storage::open().unwrap();
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
        let storage = Storage::open().unwrap();
        let id = storage.create_document("Old Title").unwrap();
        storage.update_title(id, "New Title").unwrap();
        let title = storage.get_title(id).unwrap();
        assert_eq!(title, "New Title");
    }

    #[test]
    fn test_update_title_to_empty() {
        let storage = Storage::open().unwrap();
        let id = storage.create_document("Title").unwrap();
        storage.update_title(id, "").unwrap();
        let title = storage.get_title(id).unwrap();
        assert_eq!(title, "");
    }

    #[test]
    fn test_get_title_nonexistent_document() {
        let storage = Storage::open().unwrap();
        let title = storage.get_title(99999).unwrap();
        assert_eq!(title, "Untitled");
    }

    // ── Save Document (CRDT blob) ─────────────────────────────────

    #[test]
    fn test_save_and_load_document_blob() {
        let storage = Storage::open().unwrap();
        let id = storage.create_document("Blob Test").unwrap();
        let blob = vec![0x01, 0x02, 0x03, 0xFF];
        storage.save_document(id, &blob).unwrap();
        let loaded = storage.load_document(id).unwrap();
        assert_eq!(loaded, Some(blob));
    }

    #[test]
    fn test_save_document_loads_latest() {
        let storage = Storage::open().unwrap();
        let id = storage.create_document("Latest").unwrap();
        storage.save_document(id, b"first").unwrap();
        storage.save_document(id, b"second").unwrap();
        let loaded = storage.load_document(id).unwrap();
        assert_eq!(loaded, Some(b"second".to_vec()));
    }

    #[test]
    fn test_load_document_nonexistent() {
        let storage = Storage::open().unwrap();
        let loaded = storage.load_document(99999).unwrap();
        assert_eq!(loaded, None);
    }

    // ── Compile-time Type Checks ──────────────────────────────────

    #[test]
    fn test_api_signatures_compile() {
        fn assert_send<T: Send>() {}

        assert_send::<Storage>();

        let storage = Storage::default();
        let _: Result<i64, Box<dyn std::error::Error>> = storage.create_document("test");
        let _: Result<(), Box<dyn std::error::Error>> = storage.save_text(1, "test");
        let _: Result<Option<String>, Box<dyn std::error::Error>> = storage.load_text(1);
        let _: Result<Vec<(i64, String)>, Box<dyn std::error::Error>> = storage.list_documents();
        let _: Result<(), Box<dyn std::error::Error>> = storage.update_title(1, "test");
        let _: Result<String, Box<dyn std::error::Error>> = storage.get_title(1);
    }
}
