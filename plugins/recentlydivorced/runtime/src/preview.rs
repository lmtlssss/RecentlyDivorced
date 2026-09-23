use super::{Job, conversation_evidence, paths, run_model};
use rusqlite::{Connection, OptionalExtension};
use serde::Serialize;
use std::env;
use std::error::Error;
use std::fs;
use std::path::Path;

#[derive(Serialize)]
struct Preview {
    id: String,
    label: String,
    capsule: String,
}

pub(super) fn run() -> Result<(), Box<dyn Error>> {
    let ids: Vec<String> = env::args().skip(2).collect();
    if ids.is_empty() {
        return Err("--preview requires at least one thread ID".into());
    }
    let (state_path, _) = paths()?;
    let state =
        Connection::open_with_flags(&state_path, rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY)?;
    let temp = tempfile::tempdir()?;
    let mut jobs = Vec::new();
    for id in ids {
        let row = state
            .query_row(
                "SELECT rollout_path, first_user_message, preview FROM threads WHERE id=?1",
                [&id],
                |row| {
                    Ok((
                        row.get::<_, String>(0)?,
                        row.get::<_, String>(1)?,
                        row.get::<_, String>(2)?,
                    ))
                },
            )
            .optional()?
            .ok_or_else(|| format!("thread not found: {id}"))?;
        let path = Path::new(&row.0).to_path_buf();
        let evidence = conversation_evidence(&path, &row.1, &row.2, None)
            .ok_or_else(|| format!("conversation evidence unavailable: {id}"))?;
        let capsule = augment(&state_path, &id, evidence.capsule);
        let metadata = fs::metadata(&path)?;
        jobs.push(Job {
            id,
            path,
            dev: std::os::unix::fs::MetadataExt::dev(&metadata) as i64,
            inode: std::os::unix::fs::MetadataExt::ino(&metadata) as i64,
            length: metadata.len() as i64,
            activity: capsule.clone(),
            capsule,
            pending: None,
            prior: None,
        });
    }
    let labels = run_model("gpt-6-sol", &jobs, temp.path(), 0)?;
    let output: Vec<Preview> = jobs
        .into_iter()
        .map(|job| Preview {
            label: labels.get(&job.id).cloned().unwrap_or_default(),
            id: job.id,
            capsule: job.capsule,
        })
        .collect();
    println!("{}", serde_json::to_string(&output)?);
    Ok(())
}

pub(super) fn graph_context(state_path: &Path, id: &str) -> Option<String> {
    let sibling = state_path
        .parent()?
        .join("plugins/data/the-graphfather-the-graphfather/state.sqlite");
    let connection =
        Connection::open_with_flags(sibling, rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY).ok()?;
    let has_aliases = connection
        .query_row(
            "SELECT 1 FROM sqlite_master WHERE type='table' AND name='session_aliases'",
            [],
            |row| row.get::<_, i64>(0),
        )
        .optional()
        .ok()?
        .is_some();
    let session_id = if has_aliases {
        connection
            .query_row(
                "SELECT canonical FROM session_aliases WHERE alias=?1",
                [id],
                |row| row.get::<_, String>(0),
            )
            .optional()
            .ok()?
            .unwrap_or_else(|| id.to_string())
    } else {
        id.to_string()
    };
    let document: String = connection
        .query_row(
            "SELECT document FROM sessions WHERE id=?1",
            [&session_id],
            |row| row.get(0),
        )
        .optional()
        .ok()??;
    let value: serde_json::Value = serde_json::from_str(&document).ok()?;
    let objective = value
        .pointer("/blueprint/objective")
        .and_then(|v| v.as_str())
        .unwrap_or("");
    let next = value
        .pointer("/cursor/next")
        .and_then(|v| v.as_str())
        .unwrap_or("");
    let phase = value
        .pointer("/phase")
        .and_then(|v| v.as_str())
        .unwrap_or("");
    if objective.is_empty() && next.is_empty() && phase.is_empty() {
        return None;
    }
    let objective: String = objective.chars().take(240).collect();
    let next: String = next.chars().take(120).collect();
    let phase: String = phase.chars().take(32).collect();
    Some(format!(
        "graph objective: {objective}; next: {next}; phase: {phase}"
    ))
}

pub(super) fn augment(state_path: &Path, id: &str, capsule: String) -> String {
    let Some(context) = graph_context(state_path, id) else {
        return capsule.chars().take(1500).collect();
    };
    let transcript: String = capsule.chars().take(1100).collect();
    format!("{context}\n{transcript}")
        .chars()
        .take(1500)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::graph_context;
    use rusqlite::Connection;

    #[test]
    fn unavailable_graph_is_empty() {
        let temp = tempfile::tempdir().unwrap();
        let state = temp.path().join("state_5.sqlite");
        Connection::open(&state).unwrap();
        assert!(graph_context(&state, "missing").is_none());
    }

    #[test]
    fn alias_context_is_bounded() {
        let temp = tempfile::tempdir().unwrap();
        let state = temp.path().join("state_5.sqlite");
        let sibling = temp
            .path()
            .join("plugins/data/the-graphfather-the-graphfather");
        std::fs::create_dir_all(&sibling).unwrap();
        let db = Connection::open(sibling.join("state.sqlite")).unwrap();
        db.execute_batch("CREATE TABLE session_aliases(alias TEXT, canonical TEXT); CREATE TABLE sessions(id TEXT, document TEXT);")
            .unwrap();
        db.execute("INSERT INTO session_aliases VALUES ('alias','session')", [])
            .unwrap();
        db.execute("INSERT INTO sessions VALUES ('session','{\"blueprint\":{\"objective\":\"obj\"},\"cursor\":{\"next\":\"next\"},\"phase\":\"behavior\"}')", []).unwrap();
        Connection::open(&state).unwrap();
        let context = graph_context(&state, "alias").unwrap();
        assert!(context.contains("obj"));
        assert!(context.len() <= 400);
    }

    #[test]
    fn canonical_context_works_without_alias_table() {
        let temp = tempfile::tempdir().unwrap();
        let state = temp.path().join("state_5.sqlite");
        let sibling = temp
            .path()
            .join("plugins/data/the-graphfather-the-graphfather");
        std::fs::create_dir_all(&sibling).unwrap();
        let db = Connection::open(sibling.join("state.sqlite")).unwrap();
        db.execute_batch("CREATE TABLE sessions(id TEXT, document TEXT);")
            .unwrap();
        db.execute(
            "INSERT INTO sessions VALUES ('canonical','{\"blueprint\":{\"objective\":\"obj\"}}')",
            [],
        )
        .unwrap();
        Connection::open(&state).unwrap();
        assert!(graph_context(&state, "canonical").unwrap().contains("obj"));
    }
}
