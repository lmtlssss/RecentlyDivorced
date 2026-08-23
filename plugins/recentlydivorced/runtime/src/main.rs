use rusqlite::Connection;
use serde::Deserialize;
use std::env;
use std::io::Read;
use std::path::PathBuf;
use std::thread;
use std::time::Duration;

#[derive(Deserialize)]
struct UserPromptSubmit {
    session_id: String,
    prompt: String,
}

fn main() {
    let mut input = String::new();
    if std::io::stdin().read_to_string(&mut input).is_err() {
        return;
    }
    let Ok(event) = serde_json::from_str::<UserPromptSubmit>(&input) else {
        return;
    };
    let prompt = event.prompt.trim();
    if event.session_id.is_empty() || prompt.is_empty() {
        return;
    }

    let codex_home = env::var_os("CODEX_HOME")
        .map(PathBuf::from)
        .or_else(|| env::var_os("HOME").map(|home| PathBuf::from(home).join(".codex")));
    let Some(state_db) = codex_home.map(|home| home.join("state_5.sqlite")) else {
        return;
    };
    update_preview(&state_db, &event.session_id, prompt);
}

fn update_preview(state_db: &PathBuf, session_id: &str, prompt: &str) {
    if !state_db.exists() {
        return;
    }
    for _ in 0..20 {
        if let Ok(connection) = Connection::open(&state_db) {
            let _ = connection.busy_timeout(Duration::from_millis(100));
            if let Ok(updated) = connection.execute(
                "UPDATE threads SET preview = ?1 WHERE id = ?2",
                (prompt, session_id),
            ) {
                if updated > 0 {
                    return;
                }
            }
        }
        thread::sleep(Duration::from_millis(100));
    }
}

#[cfg(test)]
mod tests {
    use super::update_preview;
    use rusqlite::Connection;

    #[test]
    fn updates_only_the_target_thread_preview() {
        let temp = tempfile::tempdir().unwrap();
        let state_db = temp.path().join("state_5.sqlite");
        let connection = Connection::open(&state_db).unwrap();
        connection
            .execute_batch(
                "CREATE TABLE threads (id TEXT PRIMARY KEY, preview TEXT NOT NULL);
                 INSERT INTO threads VALUES ('one', 'first');
                 INSERT INTO threads VALUES ('two', 'untouched');",
            )
            .unwrap();
        drop(connection);

        update_preview(&state_db, "one", "latest ask");

        let connection = Connection::open(&state_db).unwrap();
        assert_eq!(
            connection
                .query_row("SELECT preview FROM threads WHERE id = 'one'", [], |row| row.get::<_, String>(0))
                .unwrap(),
            "latest ask"
        );
        assert_eq!(
            connection
                .query_row("SELECT preview FROM threads WHERE id = 'two'", [], |row| row.get::<_, String>(0))
                .unwrap(),
            "untouched"
        );
    }
}
