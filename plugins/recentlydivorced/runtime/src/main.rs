use rusqlite::{Connection, OptionalExtension};
use serde::Deserialize;
use std::collections::{HashMap, HashSet, VecDeque};
use std::env;
use std::error::Error;
use std::fs::{self, File};
use std::io::{BufRead, BufReader, Seek, SeekFrom, Write};
use std::os::unix::fs::MetadataExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{SystemTime, UNIX_EPOCH};

const INITIAL_MODEL: &str = "gpt-5.6-sol";
const UPDATE_MODEL: &str = "gpt-5.3-codex-spark";
const CAPSULE_CHARS: usize = 480;
const TAIL_BYTES: u64 = 65_536;
const LOCK_TTL_SECONDS: i64 = 21_600;

#[derive(Clone)]
struct Job {
    id: String,
    path: PathBuf,
    dev: i64,
    inode: i64,
    length: i64,
    capsule: String,
    prior: Option<String>,
}

#[derive(Deserialize)]
struct ModelLabels {
    labels: Vec<ModelLabel>,
}

#[derive(Deserialize)]
struct ModelLabel {
    id: String,
    label: String,
}

fn main() {
    let command = env::args().nth(1).unwrap_or_default();
    let result = match command.as_str() {
        "--trust" => trust_installed_hook(),
        "--catch-up" => run_labels(true),
        "--refresh" if env::var_os("RECENTLYDIVORCED_INTERNAL").is_none() => run_labels(false),
        "--estimate" => print_estimate(),
        "--restore" => restore_stock(),
        _ => Ok(()),
    };
    if let Err(error) = result {
        eprintln!("RecentlyDivorced: {error}");
        std::process::exit(1);
    }
}

fn paths() -> Result<(PathBuf, PathBuf), Box<dyn Error>> {
    let codex_home = env::var_os("CODEX_HOME")
        .map(PathBuf::from)
        .or_else(|| env::var_os("HOME").map(|home| PathBuf::from(home).join(".codex")))
        .ok_or("cannot locate CODEX_HOME")?;
    let plugin_data = env::var_os("PLUGIN_DATA")
        .map(PathBuf::from)
        .unwrap_or_else(|| codex_home.join("plugins/data/recentlydivorced-recentlydivorced"));
    Ok((codex_home.join("state_5.sqlite"), plugin_data))
}

fn open_cache(plugin_data: &Path) -> Result<Connection, Box<dyn Error>> {
    fs::create_dir_all(plugin_data)?;
    let connection = Connection::open(plugin_data.join("state.sqlite"))?;
    connection.execute_batch(
        "CREATE TABLE IF NOT EXISTS summaries (
            thread_id TEXT PRIMARY KEY,
            rollout_path TEXT NOT NULL,
            dev INTEGER NOT NULL,
            inode INTEGER NOT NULL,
            processed_len INTEGER NOT NULL,
            label TEXT NOT NULL
         );
         CREATE TABLE IF NOT EXISTS locks (
            name TEXT PRIMARY KEY,
            owner TEXT NOT NULL,
            acquired_at INTEGER NOT NULL
         );
         CREATE TABLE IF NOT EXISTS meta (
            key TEXT PRIMARY KEY,
            value TEXT NOT NULL
         );",
    )?;
    let generation = connection
        .query_row(
            "SELECT value FROM meta WHERE key = 'capsule_generation'",
            [],
            |row| row.get::<_, String>(0),
        )
        .optional()?;
    if generation.as_deref() != Some("rollout-tail-v1") {
        connection.execute("DELETE FROM summaries", [])?;
        connection.execute("DELETE FROM meta WHERE key = 'bootstrap_complete'", [])?;
        connection.execute(
            "INSERT INTO meta VALUES ('capsule_generation', 'rollout-tail-v1')
             ON CONFLICT(key) DO UPDATE SET value=excluded.value",
            [],
        )?;
    }
    Ok(connection)
}

fn run_labels(verbose: bool) -> Result<(), Box<dyn Error>> {
    let (state_path, plugin_data) = paths()?;
    let cache = open_cache(&plugin_data)?;
    let owner = format!("{}:{}", std::process::id(), unix_time());
    if !acquire_lock(&cache, &owner)? {
        return Ok(());
    }
    let bootstrap = cache
        .query_row(
            "SELECT value FROM meta WHERE key = 'bootstrap_complete'",
            [],
            |row| row.get::<_, String>(0),
        )
        .optional()?
        .is_none();
    let result = run_labels_locked(&state_path, &plugin_data, &cache, verbose, bootstrap);
    if result.is_ok() && bootstrap {
        cache.execute(
            "INSERT INTO meta VALUES ('bootstrap_complete', '1')
             ON CONFLICT(key) DO UPDATE SET value=excluded.value",
            [],
        )?;
    }
    let _ = cache.execute(
        "DELETE FROM locks WHERE name = 'refresh' AND owner = ?1",
        [&owner],
    );
    result
}

fn acquire_lock(cache: &Connection, owner: &str) -> rusqlite::Result<bool> {
    let now = unix_time();
    cache.execute(
        "DELETE FROM locks WHERE name = 'refresh' AND acquired_at < ?1",
        [now - LOCK_TTL_SECONDS],
    )?;
    Ok(cache.execute(
        "INSERT OR IGNORE INTO locks VALUES ('refresh', ?1, ?2)",
        (owner, now),
    )? == 1)
}

fn run_labels_locked(
    state_path: &Path,
    plugin_data: &Path,
    cache: &Connection,
    verbose: bool,
    bootstrap: bool,
) -> Result<(), Box<dyn Error>> {
    let state = Connection::open(state_path)?;
    state.busy_timeout(std::time::Duration::from_secs(10))?;
    let mut statement = state.prepare(
        "SELECT id, rollout_path, first_user_message, preview
         FROM threads WHERE rollout_path <> ''",
    )?;
    let rows = statement.query_map([], |row| {
        Ok((
            row.get::<_, String>(0)?,
            PathBuf::from(row.get::<_, String>(1)?),
            row.get::<_, String>(2)?,
            row.get::<_, String>(3)?,
        ))
    })?;

    let mut initial = Vec::new();
    let mut changed = Vec::new();
    for row in rows.filter_map(Result::ok) {
        let (id, path, first_user_message, preview) = row;
        let Ok(metadata) = fs::metadata(&path) else {
            continue;
        };
        let fingerprint = (
            metadata.dev() as i64,
            metadata.ino() as i64,
            metadata.len() as i64,
        );
        let cached = cache
            .query_row(
                "SELECT dev, inode, processed_len, label FROM summaries WHERE thread_id = ?1",
                [&id],
                |row| {
                    Ok((
                        row.get::<_, i64>(0)?,
                        row.get::<_, i64>(1)?,
                        row.get::<_, i64>(2)?,
                        row.get::<_, String>(3)?,
                    ))
                },
            )
            .optional()?;
        if let Some((dev, inode, length, label)) = &cached {
            let _ = state.execute(
                "UPDATE threads SET preview = ?1 WHERE id = ?2",
                (label, &id),
            );
            if (*dev, *inode, *length) == fingerprint {
                continue;
            }
        }
        let Some(capsule) = conversation_capsule(&path, &first_user_message, &preview) else {
            continue;
        };
        let job = Job {
            id,
            path,
            dev: fingerprint.0,
            inode: fingerprint.1,
            length: fingerprint.2,
            capsule,
            prior: cached.map(|cached| cached.3),
        };
        if job.prior.is_some() || !bootstrap {
            changed.push(job);
        } else {
            initial.push(job);
        }
    }
    drop(statement);

    if verbose {
        eprintln!(
            "RecentlyDivorced: {} archive labels with Sol low; {} changed labels with Spark",
            initial.len(),
            changed.len()
        );
    }
    process_jobs(
        INITIAL_MODEL,
        initial,
        state_path,
        plugin_data,
        cache,
        verbose,
    )?;
    process_jobs(
        UPDATE_MODEL,
        changed,
        state_path,
        plugin_data,
        cache,
        verbose,
    )?;
    Ok(())
}

fn process_jobs(
    model: &str,
    jobs: Vec<Job>,
    state_path: &Path,
    plugin_data: &Path,
    cache: &Connection,
    verbose: bool,
) -> Result<(), Box<dyn Error>> {
    let total = jobs.len();
    let mut done = 0;
    for (batch_number, batch) in batches(model, jobs).into_iter().enumerate() {
        match run_model_resilient(model, &batch, plugin_data, batch_number) {
            Ok(labels) => {
                let state = Connection::open(state_path)?;
                state.busy_timeout(std::time::Duration::from_secs(10))?;
                for job in &batch {
                    let Some(label) = labels.get(&job.id) else {
                        continue;
                    };
                    cache.execute(
                        "INSERT INTO summaries VALUES (?1, ?2, ?3, ?4, ?5, ?6)
                         ON CONFLICT(thread_id) DO UPDATE SET
                           rollout_path=excluded.rollout_path,
                           dev=excluded.dev,
                           inode=excluded.inode,
                           processed_len=excluded.processed_len,
                           label=excluded.label",
                        (
                            &job.id,
                            job.path.display().to_string(),
                            job.dev,
                            job.inode,
                            job.length,
                            label,
                        ),
                    )?;
                    let _ = state.execute(
                        "UPDATE threads SET preview = ?1 WHERE id = ?2",
                        (label, &job.id),
                    );
                    done += 1;
                }
            }
            Err(error) => eprintln!("RecentlyDivorced: {model} batch failed: {error}"),
        }
        if verbose {
            eprintln!("RecentlyDivorced: {model} {done}/{total}");
        }
    }
    Ok(())
}

fn run_model_resilient(
    model: &str,
    jobs: &[Job],
    plugin_data: &Path,
    run: usize,
) -> Result<HashMap<String, String>, Box<dyn Error>> {
    match run_model(model, jobs, plugin_data, run) {
        Ok(labels) => Ok(labels),
        Err(_) if jobs.len() > 1 => {
            let middle = jobs.len() / 2;
            let mut labels = run_model_resilient(model, &jobs[..middle], plugin_data, run * 2 + 1)?;
            labels.extend(run_model_resilient(
                model,
                &jobs[middle..],
                plugin_data,
                run * 2 + 2,
            )?);
            Ok(labels)
        }
        Err(error) => Err(error),
    }
}

fn batches(model: &str, jobs: Vec<Job>) -> Vec<Vec<Job>> {
    let (max_threads, max_chars) = if model == INITIAL_MODEL {
        (100, 60_000)
    } else {
        (6, 9_000)
    };
    let mut batches = Vec::new();
    let mut batch = Vec::new();
    let mut chars = 0;
    for job in jobs {
        let size = job.capsule.chars().count() + job.prior.as_deref().unwrap_or("").chars().count();
        if !batch.is_empty() && (batch.len() == max_threads || chars + size > max_chars) {
            batches.push(std::mem::take(&mut batch));
            chars = 0;
        }
        chars += size;
        batch.push(job);
    }
    if !batch.is_empty() {
        batches.push(batch);
    }
    batches
}

fn run_model(
    model: &str,
    jobs: &[Job],
    plugin_data: &Path,
    batch_number: usize,
) -> Result<HashMap<String, String>, Box<dyn Error>> {
    let run = format!("{}-{batch_number}", std::process::id());
    let schema_path = plugin_data.join(format!(".{run}.schema.json"));
    let output_path = plugin_data.join(format!(".{run}.output.json"));
    let schema = serde_json::json!({
        "type": "object",
        "additionalProperties": false,
        "properties": {
            "labels": {
                "type": "array",
                "items": {
                    "type": "object",
                    "additionalProperties": false,
                    "properties": {
                        "id": { "type": "string" },
                        "label": { "type": "string", "maxLength": 120 }
                    },
                    "required": ["id", "label"]
                }
            }
        },
        "required": ["labels"]
    });
    fs::write(&schema_path, serde_json::to_vec(&schema)?)?;

    let mut prompt = String::from(
        "Label each Codex conversation for a /resume index. Return exactly one item per ID. \
         Each label is one concrete sentence fragment, at most 12 words, naming the work and current point. \
         No generic phrases such as 'discussion about', no IDs inside labels, no markdown.\n",
    );
    for job in jobs {
        prompt.push_str("\nID ");
        prompt.push_str(&job.id);
        if let Some(prior) = &job.prior {
            prompt.push_str("\nPRIOR ");
            prompt.push_str(prior);
        }
        prompt.push_str("\nCONVERSATION\n");
        prompt.push_str(&job.capsule);
        prompt.push('\n');
    }

    let mut child = Command::new("codex")
        .args([
            "exec",
            "--ephemeral",
            "--ignore-user-config",
            "--ignore-rules",
            "--skip-git-repo-check",
            "--sandbox",
            "read-only",
            "--color",
            "never",
            "--model",
            model,
            "-c",
            "model_reasoning_effort=\"low\"",
            "--output-schema",
        ])
        .arg(&schema_path)
        .arg("--output-last-message")
        .arg(&output_path)
        .arg("-")
        .current_dir("/tmp")
        .env("RECENTLYDIVORCED_INTERNAL", "1")
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()?;
    child
        .stdin
        .take()
        .ok_or("missing Codex stdin")?
        .write_all(prompt.as_bytes())?;
    let status = child.wait()?;
    let output = fs::read_to_string(&output_path);
    let _ = fs::remove_file(&schema_path);
    let _ = fs::remove_file(&output_path);
    if !status.success() {
        return Err(format!("Codex exited with {status}").into());
    }
    let response: ModelLabels = serde_json::from_str(&output?)?;
    let expected = jobs
        .iter()
        .map(|job| job.id.as_str())
        .collect::<HashSet<_>>();
    let mut labels = HashMap::new();
    for item in response.labels {
        if expected.contains(item.id.as_str()) {
            let label = normalize_label(&item.label);
            if !label.is_empty() {
                labels.insert(item.id, label);
            }
        }
    }
    if labels.len() != jobs.len() {
        return Err("model returned an incomplete label set".into());
    }
    Ok(labels)
}

fn conversation_capsule(path: &Path, first_user: &str, preview: &str) -> Option<String> {
    let mut file = File::open(path).ok()?;
    let length = file.metadata().ok()?.len();
    let start = length.saturating_sub(TAIL_BYTES);
    file.seek(SeekFrom::Start(start)).ok()?;
    let mut reader = BufReader::new(file);
    if start > 0 {
        loop {
            let buffer = reader.fill_buf().ok()?;
            if buffer.is_empty() {
                break;
            }
            let newline = buffer.iter().position(|byte| *byte == b'\n');
            let consumed = newline.map_or(buffer.len(), |position| position + 1);
            let found_newline = newline.is_some();
            reader.consume(consumed);
            if found_newline {
                break;
            }
        }
    }
    let mut tail = VecDeque::new();
    for line in reader.lines().map_while(Result::ok) {
        if !line.contains("\"type\":\"response_item\"") || !line.contains("\"type\":\"message\"") {
            continue;
        }
        let Ok(item) = serde_json::from_str::<serde_json::Value>(&line) else {
            continue;
        };
        let payload = &item["payload"];
        if item["type"] != "response_item" || payload["type"] != "message" {
            continue;
        }
        let Some(role) = payload["role"].as_str() else {
            continue;
        };
        if role != "user" && role != "assistant" {
            continue;
        }
        let text = payload["content"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|part| part["text"].as_str())
            .collect::<Vec<_>>()
            .join(" ");
        let text = compact_text(&text, 180);
        if text.is_empty() || (role == "user" && synthetic_prompt(&text)) {
            continue;
        }
        tail.push_back(format!("{role}: {text}"));
        if tail.len() > 3 {
            tail.pop_front();
        }
    }
    let mut capsule = String::new();
    let first = compact_text(first_user, 180);
    if !first.is_empty() && !synthetic_prompt(&first) {
        capsule.push_str("goal: ");
        capsule.push_str(&first);
        capsule.push('\n');
    }
    let preview = compact_text(preview, 120);
    if !preview.is_empty() && preview != first {
        capsule.push_str("previous index: ");
        capsule.push_str(&preview);
        capsule.push('\n');
    }
    capsule.push_str(&tail.into_iter().collect::<Vec<_>>().join("\n"));
    let capsule = compact_text(&capsule, CAPSULE_CHARS);
    (!capsule.is_empty()).then_some(capsule)
}

fn synthetic_prompt(text: &str) -> bool {
    text.starts_with("# AGENTS.md instructions")
        || text.starts_with("<environment_context>")
        || text.contains("<skills_instructions>")
        || text.contains("<permissions instructions>")
        || text.contains("<collaboration_mode>")
}

fn compact_text(text: &str, max_chars: usize) -> String {
    text.split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .chars()
        .take(max_chars)
        .collect()
}

fn normalize_label(label: &str) -> String {
    label
        .split_whitespace()
        .take(12)
        .collect::<Vec<_>>()
        .join(" ")
        .trim_matches(['"', '\'', '`'])
        .to_string()
}

fn print_estimate() -> Result<(), Box<dyn Error>> {
    let (state_path, _) = paths()?;
    let connection = Connection::open(state_path)?;
    let mut statement =
        connection.prepare("SELECT rollout_path FROM threads WHERE rollout_path <> ''")?;
    let mut count = 0;
    let mut bytes = 0;
    for path in statement
        .query_map([], |row| row.get::<_, String>(0))?
        .filter_map(Result::ok)
    {
        if let Ok(metadata) = fs::metadata(path) {
            count += 1;
            bytes += metadata.len();
        }
    }
    println!(
        "{count} conversations ({:.0} MiB of local rollouts)",
        bytes as f64 / 1_048_576.0
    );
    Ok(())
}

fn restore_stock() -> Result<(), Box<dyn Error>> {
    let (state_path, plugin_data) = paths()?;
    let cache_path = plugin_data.join("state.sqlite");
    if !cache_path.exists() {
        return Ok(());
    }
    let cache = Connection::open(&cache_path)?;
    let state = Connection::open(state_path)?;
    let mut statement = cache.prepare("SELECT thread_id FROM summaries")?;
    let ids = statement
        .query_map([], |row| row.get::<_, String>(0))?
        .filter_map(Result::ok)
        .collect::<Vec<_>>();
    for id in ids {
        state.execute(
            "UPDATE threads SET preview = first_user_message WHERE id = ?1",
            [&id],
        )?;
    }
    drop(statement);
    drop(cache);
    fs::remove_file(cache_path)?;
    Ok(())
}

fn trust_installed_hook() -> Result<(), Box<dyn Error>> {
    let cwd = env::current_dir()?.display().to_string();
    let mut child = Command::new("codex")
        .args(["app-server", "--stdio"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()?;
    let mut stdin = child.stdin.take().ok_or("missing app-server stdin")?;
    let mut stdout = BufReader::new(child.stdout.take().ok_or("missing app-server stdout")?);
    write_jsonrpc(
        &mut stdin,
        serde_json::json!({"method":"initialize","id":1,"params":{"clientInfo":{"name":"recentlydivorced-installer","version":"0.2.0"}}}),
    )?;
    write_jsonrpc(
        &mut stdin,
        serde_json::json!({"method":"initialized","params":{}}),
    )?;
    read_response(&mut stdout, 1)?;
    write_jsonrpc(
        &mut stdin,
        serde_json::json!({"method":"hooks/list","id":2,"params":{"cwds":[cwd]}}),
    )?;
    let hooks = read_response(&mut stdout, 2)?;
    let hook = hooks["result"]["data"][0]["hooks"]
        .as_array()
        .and_then(|hooks| {
            hooks
                .iter()
                .find(|hook| hook["pluginId"] == "recentlydivorced@recentlydivorced")
        })
        .ok_or("RecentlyDivorced hook was not discovered by stock Codex")?;
    let key = hook["key"].as_str().ok_or("missing hook key")?;
    let hash = hook["currentHash"].as_str().ok_or("missing hook hash")?;
    write_jsonrpc(
        &mut stdin,
        serde_json::json!({
            "method":"config/batchWrite","id":3,"params":{
                "edits":[
                    {"keyPath":"hooks.state","value":{key:{"enabled":true,"trusted_hash":hash}},"mergeStrategy":"upsert"},
                    {"keyPath":"hooks.state.\"recentlydivorced@recentlydivorced:hooks/hooks.json:user_prompt_submit:0:0\"","value":null,"mergeStrategy":"replace"}
                ],
                "reloadUserConfig":true
            }
        }),
    )?;
    read_response(&mut stdout, 3)?;
    drop(stdin);
    let _ = child.wait();
    Ok(())
}

fn write_jsonrpc(stdin: &mut impl Write, value: serde_json::Value) -> Result<(), Box<dyn Error>> {
    serde_json::to_writer(&mut *stdin, &value)?;
    stdin.write_all(b"\n")?;
    stdin.flush()?;
    Ok(())
}

fn read_response(stdout: &mut impl BufRead, id: u64) -> Result<serde_json::Value, Box<dyn Error>> {
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

fn unix_time() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs() as i64
}

#[cfg(test)]
mod tests {
    use super::{
        CAPSULE_CHARS, INITIAL_MODEL, Job, UPDATE_MODEL, conversation_capsule, normalize_label,
        run_model,
    };

    #[test]
    fn capsule_is_small_and_keeps_the_human_goal_and_tail() {
        let temp = tempfile::tempdir().unwrap();
        let rollout = temp.path().join("rollout.jsonl");
        std::fs::write(
            &rollout,
            [
                r##"{"type":"response_item","payload":{"type":"message","role":"user","content":[{"type":"input_text","text":"# AGENTS.md instructions fake"}]}}"##,
                r#"{"type":"response_item","payload":{"type":"message","role":"user","content":[{"type":"input_text","text":"build a boot visualizer"}]}}"#,
                r#"{"type":"response_item","payload":{"type":"message","role":"assistant","content":[{"type":"output_text","text":"implemented the camera"}]}}"#,
                r#"{"type":"response_item","payload":{"type":"message","role":"user","content":[{"type":"input_text","text":"fix the sole lighting"}]}}"#,
            ]
            .join("\n"),
        )
        .unwrap();
        let capsule = conversation_capsule(&rollout, "build a boot visualizer", "").unwrap();
        assert!(capsule.contains("goal: build a boot visualizer"));
        assert!(capsule.contains("user: fix the sole lighting"));
        assert!(!capsule.contains("AGENTS"));
        assert!(capsule.chars().count() <= CAPSULE_CHARS);
    }

    #[test]
    fn labels_are_one_short_line() {
        assert_eq!(
            normalize_label(
                "  building   the resume index with compact conversation labels that stay useful after updates forever  "
            ),
            "building the resume index with compact conversation labels that stay useful after"
        );
    }

    #[test]
    #[ignore = "uses live Codex model usage"]
    fn configured_models_return_structured_fingertip_labels() {
        let temp = tempfile::tempdir().unwrap();
        let job = Job {
            id: "probe".into(),
            path: temp.path().join("unused.jsonl"),
            dev: 0,
            inode: 0,
            length: 0,
            capsule: "goal: make /resume conversations recognizable at a glance".into(),
            prior: None,
        };
        for (batch, model) in [INITIAL_MODEL, UPDATE_MODEL].into_iter().enumerate() {
            let labels = run_model(model, std::slice::from_ref(&job), temp.path(), batch).unwrap();
            assert!(!labels["probe"].is_empty());
        }
    }
}
