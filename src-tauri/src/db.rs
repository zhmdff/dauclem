// db.rs — SQLite schema setup (mirrors src/db.js)
use rusqlite::{Connection, Result};
use std::path::Path;

pub fn open(path: &Path) -> Result<Connection> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).ok();
    }
    let conn = Connection::open(path)?;
    conn.execute_batch("
        PRAGMA journal_mode=WAL;
        PRAGMA synchronous=OFF;
        PRAGMA busy_timeout=30000;

        CREATE TABLE IF NOT EXISTS sessions(
            id TEXT PRIMARY KEY,
            project TEXT,
            file TEXT,
            title TEXT,
            cwd TEXT,
            first_prompt TEXT,
            first_ts INTEGER,
            last_ts INTEGER,
            sig TEXT,
            n_me INTEGER,
            n_agent INTEGER,
            n_act INTEGER,
            n_sub INTEGER,
            n_agents INTEGER
        );

        CREATE INDEX IF NOT EXISTS sessions_project ON sessions(project, last_ts);

        CREATE TABLE IF NOT EXISTS events(
            id INTEGER PRIMARY KEY,
            session TEXT,
            project TEXT,
            cat TEXT,
            kind TEXT,
            tool TEXT,
            agent TEXT,
            ts INTEGER,
            seq INTEGER,
            text TEXT
        );

        CREATE INDEX IF NOT EXISTS events_session ON events(session);

        CREATE VIRTUAL TABLE IF NOT EXISTS fts USING fts5(
            text,
            content='events',
            content_rowid='id',
            tokenize='trigram'
        );

        CREATE TRIGGER IF NOT EXISTS events_ai AFTER INSERT ON events BEGIN
            INSERT INTO fts(rowid, text) VALUES (new.id, new.text);
        END;

        CREATE TRIGGER IF NOT EXISTS events_ad AFTER DELETE ON events BEGIN
            INSERT INTO fts(fts, rowid, text) VALUES('delete', old.id, old.text);
        END;
    ")?;
    Ok(conn)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_fts5_trigram() {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch("
            CREATE TABLE events(id INTEGER PRIMARY KEY, text TEXT);
            CREATE VIRTUAL TABLE fts USING fts5(text, content='events', content_rowid='id', tokenize='trigram');
            CREATE TRIGGER events_ai AFTER INSERT ON events BEGIN
                INSERT INTO fts(rowid, text) VALUES (new.id, new.text);
            END;
        ").unwrap();

        conn.execute("INSERT INTO events(text) VALUES(?)", ["Searching in rust backend!"]).unwrap();

        let count: i64 = conn.query_row(
            "SELECT count(*) FROM fts WHERE fts MATCH '\"rust\"'",
            [],
            |r| r.get(0),
        ).unwrap();

        assert_eq!(count, 1);
    }
}
