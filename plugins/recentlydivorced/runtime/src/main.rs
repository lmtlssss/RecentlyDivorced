use rusqlite::Connection;
use serde::Deserialize;
use std::env;
use std::fs::File;
use std::io::{BufRead, BufReader, Read, Write};
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::thread;
use std::time::Duration;

#[derive(Deserialize)]
struct UserPromptSubmit {
    session_id: String,
    prompt: String,
}

fn main() {
    if env::args().nth(1).as_deref() == Some("--trust") {
        let _ = trust_installed_hook();
        return;
    }
    if env::args().nth(1).as_deref() == Some("--backfill") {
        backfill_existing_previews();
        return;
    }
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

fn trust_installed_hook() -> Result<(), Box<dyn std::error::Error>> {
    let cwd = env::current_dir()?.display().to_string();
    let mut child = Command::new("codex")
        .args(["app-server", "--stdio"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()?;
    let mut stdin = child.stdin.take().ok_or("missing app-server stdin")?;
    let stdout = child.stdout.take().ok_or("missing app-server stdout")?;
    let mut stdout = BufReader::new(stdout);

    write_jsonrpc(
        &mut stdin,
        serde_json::json!({
            "method": "initialize",
            "id": 1,
            "params": { "clientInfo": { "name": "recentlydivorced-installer", "version": "0.1.0" } }
        }),
    )?;
    write_jsonrpc(&mut stdin, serde_json::json!({ "method": "initialized", "params": {} }))?;
    read_response(&mut stdout, 1)?;

    write_jsonrpc(
        &mut stdin,
        serde_json::json!({
            "method": "hooks/list",
            "id": 2,
            "params": { "cwds": [cwd] }
        }),
    )?;
    let hooks = read_response(&mut stdout, 2)?;
    let hook = hooks["result"]["data"][0]["hooks"]
        .as_array()
        .and_then(|hooks| hooks.iter().find(|hook| hook["pluginId"] == "recentlydivorced@recentlydivorced"))
        .ok_or("RecentlyDivorced hook was not discovered by stock Codex")?;
    let key = hook["key"].as_str().ok_or("missing hook key")?;
    let trusted_hash = hook["currentHash"].as_str().ok_or("missing hook hash")?;

    write_jsonrpc(
        &mut stdin,
        serde_json::json!({
            "method": "config/batchWrite",
            "id": 3,
            "params": {
                "edits": [{
                    "keyPath": "hooks.state",
                    "value": { key: { "enabled": true, "trusted_hash": trusted_hash } },
                    "mergeStrategy": "upsert"
                }],
                "reloadUserConfig": true
            }
        }),
    )?;
    read_response(&mut stdout, 3)?;
    drop(stdin);
    let _ = child.wait();
    Ok(())
}

fn write_jsonrpc(
    stdin: &mut impl Write,
    value: serde_json::Value,
) -> Result<(), Box<dyn std::error::Error>> {
    serde_json::to_writer(&mut *stdin, &value)?;
    stdin.write_all(b"\n")?;
    stdin.flush()?;
    Ok(())
}

fn read_response(
    stdout: &mut impl BufRead,
    id: u64,
) -> Result<serde_json::Value, Box<dyn std::error::Error>> {
    let mut line = String::new();
    loop {
        line.clear();
        if stdout.read_line(&mut line)? == 0 {
            return Err("app-server closed before response".into());
        }
        let value: serde_json::Value = serde_json::from_str(&line)?;
        if value["id"].as_u64() == Some(id) {
            if let Some(error) = value.get("error") {
                return Err(format!("app-server error: {error}").into());
            }
            return Ok(value);
        }
    }
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

fn backfill_existing_previews() {
    let codex_home = env::var_os("CODEX_HOME")
        .map(PathBuf::from)
        .or_else(|| env::var_os("HOME").map(|home| PathBuf::from(home).join(".codex")));
    let Some(state_db) = codex_home.map(|home| home.join("state_5.sqlite")) else {
        return;
    };
    backfill_previews(&state_db);
}

fn backfill_previews(state_db: &PathBuf) {
    let Ok(connection) = Connection::open(&state_db) else {
        return;
    };
    let Ok(mut statement) = connection.prepare("SELECT id, rollout_path FROM threads") else {
        return;
    };
    let Ok(rows) = statement.query_map([], |row| {
        Ok((row.get::<_, String>(0)?, PathBuf::from(row.get::<_, String>(1)?)))
    }) else {
        return;
    };
    let threads = rows.filter_map(Result::ok).collect::<Vec<_>>();
    drop(statement);
    drop(connection);
    for (session_id, rollout_path) in threads {
        if let Some(prompt) = latest_user_prompt(&rollout_path) {
            update_preview(&state_db, &session_id, &prompt);
        }
    }
}

fn latest_user_prompt(path: &PathBuf) -> Option<String> {
    let file = File::open(path).ok()?;
    let reader = BufReader::new(file);
    let mut latest = None;
    for line in reader.lines().map_while(Result::ok) {
        let Ok(item) = serde_json::from_str::<serde_json::Value>(&line) else {
            continue;
        };
        let payload = &item["payload"];
        if item["type"] != "response_item"
            || payload["type"] != "message"
            || payload["role"] != "user"
        {
            continue;
        }
        let prompt = payload["content"]
            .as_array()
            .and_then(|content| {
                content.iter().find_map(|part| {
                    (part["type"] == "input_text")
                        .then(|| part["text"].as_str())
                        .flatten()
                })
            })
            .map(str::trim)
            .filter(|prompt| !prompt.is_empty())
            .map(str::to_owned);
        if prompt.is_some() {
            latest = prompt;
        }
    }
    latest
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

    #[test]
    fn reads_the_last_user_prompt_from_a_rollout() {
        let temp = tempfile::tempdir().unwrap();
        let rollout = temp.path().join("rollout.jsonl");
        std::fs::write(
            &rollout,
            [
                r#"{"type":"response_item","payload":{"type":"message","role":"user","content":[{"type":"input_text","text":"first"}]}}"#,
                r#"{"type":"response_item","payload":{"type":"message","role":"assistant","content":[]}}"#,
                r#"{"type":"response_item","payload":{"type":"message","role":"user","content":[{"type":"input_text","text":" latest "}]}}"#,
            ]
            .join("\n"),
        )
        .unwrap();
        assert_eq!(super::latest_user_prompt(&rollout).as_deref(), Some("latest"));
    }

    #[test]
    fn backfill_updates_existing_thread_preview() {
        let temp = tempfile::tempdir().unwrap();
        let state_db = temp.path().join("state_5.sqlite");
        let rollout = temp.path().join("rollout.jsonl");
        std::fs::write(
            &rollout,
            r#"{"type":"response_item","payload":{"type":"message","role":"user","content":[{"type":"input_text","text":"latest"}]}}"#,
        )
        .unwrap();
        let connection = Connection::open(&state_db).unwrap();
        connection
            .execute_batch("CREATE TABLE threads (id TEXT PRIMARY KEY, rollout_path TEXT, preview TEXT NOT NULL);")
            .unwrap();
        connection
            .execute(
                "INSERT INTO threads VALUES (?1, ?2, 'original')",
                ("thread-1", rollout.display().to_string()),
            )
            .unwrap();
        drop(connection);

        super::backfill_previews(&state_db);

        let connection = Connection::open(&state_db).unwrap();
        assert_eq!(
            connection
                .query_row("SELECT preview FROM threads WHERE id = 'thread-1'", [], |row| row.get::<_, String>(0))
                .unwrap(),
            "latest"
        );
    }
}
