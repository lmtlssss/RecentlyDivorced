use rusqlite::{Connection, OptionalExtension};
use serde::Deserialize;
use std::collections::{HashMap, HashSet, VecDeque};
use std::env;
use std::error::Error;
use std::fs::{self, File};
use std::io::{BufRead, BufReader, Read, Seek, SeekFrom, Write};
use std::os::unix::fs::MetadataExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{SystemTime, UNIX_EPOCH};

const INITIAL_MODEL: &str = "gpt-5.6-sol";
const UPDATE_MODEL: &str = "gpt-5.3-codex-spark";
const CAPSULE_CHARS: usize = 480;
const TAIL_BYTES: u64 = 65_536;
const LOCK_TTL_SECONDS: i64 = 21_600;
const HUMAN_THREAD_FILTER: &str =
    "source = 'cli' AND thread_source = 'user' AND agent_role IS NULL AND rollout_path <> ''";

#[derive(Clone)]
struct Job {
    id: String,
    path: PathBuf,
    dev: i64,
    inode: i64,
    length: i64,
    capsule: String,
    activity: String,
    prior: Option<String>,
}

struct Evidence {
    capsule: String,
    activity: String,
}

#[derive(Debug, PartialEq, Eq)]
enum RefreshDecision {
    Reuse,
    Seed,
    Relabel,
}

fn refresh_decision(
    cached: &str,
    same_fingerprint: bool,
    pending: bool,
    extracted: &str,
) -> RefreshDecision {
    if !cached.is_empty() && (extracted.is_empty() || extracted == cached) {
        RefreshDecision::Reuse
    } else if !pending && same_fingerprint && cached.is_empty() {
        RefreshDecision::Seed
    } else {
        RefreshDecision::Relabel
    }
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

#[derive(Deserialize)]
struct ActivityInput {
    session_id: String,
    prompt: String,
}

fn main() {
    let command = env::args().nth(1).unwrap_or_default();
    let result = match command.as_str() {
        "--trust" => trust_installed_hook(),
        "--catch-up" => run_labels(true),
        "--refresh" if env::var_os("RECENTLYDIVORCED_INTERNAL").is_none() => run_labels(false),
        "--estimate" => print_estimate(),
        "--activity" => {
            let _ = activity_hook();
            Ok(())
        }
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
            label TEXT NOT NULL,
            activity TEXT NOT NULL DEFAULT ''
         );
         CREATE TABLE IF NOT EXISTS original_names (thread_id TEXT PRIMARY KEY, name TEXT);
         CREATE TABLE IF NOT EXISTS pending_activity (thread_id TEXT PRIMARY KEY, activity TEXT NOT NULL);
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
    let _ = connection.execute(
        "ALTER TABLE summaries ADD COLUMN activity TEXT NOT NULL DEFAULT ''",
        [],
    );
    let generation = connection
        .query_row(
            "SELECT value FROM meta WHERE key = 'capsule_generation'",
            [],
            |row| row.get::<_, String>(0),
        )
        .optional()?;
    if generation.as_deref() != Some("source-ladder-v2") {
        connection.execute("DELETE FROM summaries", [])?;
        connection.execute("DELETE FROM meta WHERE key = 'bootstrap_complete'", [])?;
        connection.execute(
            "INSERT INTO meta VALUES ('capsule_generation', 'source-ladder-v2')
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
        &format!("SELECT id, rollout_path, first_user_message, preview, name FROM threads WHERE {HUMAN_THREAD_FILTER}"),
    )?;
    let rows = statement.query_map([], |row| {
        Ok((
            row.get::<_, String>(0)?,
            PathBuf::from(row.get::<_, String>(1)?),
            row.get::<_, String>(2)?,
            row.get::<_, String>(3)?,
            row.get::<_, Option<String>>(4)?,
        ))
    })?;

    let mut initial = Vec::new();
    let mut changed = Vec::new();
    let mut cached_projection = Vec::new();
    for row in rows.filter_map(Result::ok) {
        let (id, path, first_user_message, preview, original_name) = row;
        cache.execute(
            "INSERT OR IGNORE INTO original_names(thread_id,name) VALUES (?1,?2)",
            (&id, original_name.as_deref()),
        )?;
        let Ok(metadata) = fs::metadata(&path) else {
            continue;
        };
        let fingerprint = (
            metadata.dev() as i64,
            metadata.ino() as i64,
            metadata.len() as i64,
        );
        let pending = cache
            .query_row(
                "SELECT activity FROM pending_activity WHERE thread_id=?1",
                [&id],
                |r| r.get::<_, String>(0),
            )
            .optional()?;
        let cached = cache
            .query_row(
            "SELECT dev, inode, processed_len, label, activity FROM summaries WHERE thread_id = ?1",
                [&id],
                |row| {
                    Ok((
                        row.get::<_, i64>(0)?,
                        row.get::<_, i64>(1)?,
                        row.get::<_, i64>(2)?,
                    row.get::<_, String>(3)?,
                    row.get::<_, String>(4)?,
                    ))
                },
            )
            .optional()?;
        if let Some((dev, inode, length, label, cached_activity)) = &cached {
            if pending.as_deref().unwrap_or("").is_empty()
                && (*dev, *inode, *length) == fingerprint
                && !cached_activity.is_empty()
            {
                cached_projection.push((id.clone(), label.clone()));
                continue;
            }
        }
        let Some(evidence) =
            conversation_evidence(&path, &first_user_message, &preview, pending.as_deref())
        else {
            continue;
        };
        if let Some((dev, inode, length, label, cached_activity)) = &cached {
            let extracted = evidence.activity.as_str();
            let effective = if extracted.is_empty() {
                cached_activity.as_str()
            } else {
                extracted
            };
            let decision = refresh_decision(
                cached_activity,
                (*dev, *inode, *length) == fingerprint,
                pending.as_ref().is_some_and(|p| !p.is_empty()),
                effective,
            );
            if decision != RefreshDecision::Relabel {
                let tx = cache.unchecked_transaction()?;
                tx.execute("UPDATE summaries SET dev=?1,inode=?2,processed_len=?3,activity=?4 WHERE thread_id=?5", (fingerprint.0, fingerprint.1, fingerprint.2, effective, &id))?;
                tx.execute("DELETE FROM pending_activity WHERE thread_id=?1", [&id])?;
                tx.commit()?;
                cached_projection.push((id.clone(), label.clone()));
                continue;
            }
        }
        let job = Job {
            id,
            path,
            dev: fingerprint.0,
            inode: fingerprint.1,
            length: fingerprint.2,
            capsule: evidence.capsule,
            activity: evidence.activity,
            prior: cached.map(|cached| cached.3),
        };
        if job.prior.is_some() || !bootstrap {
            changed.push(job);
        } else {
            initial.push(job);
        }
    }
    drop(statement);
    if let Ok(transaction) = state.unchecked_transaction() {
        for (id, label) in cached_projection {
            let _ = transaction.execute(
                "UPDATE threads SET name = ?1, preview = ?1 WHERE id = ?2",
                (label, id),
            );
        }
        let _ = transaction.commit();
    }

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
                let cache_transaction = cache.unchecked_transaction()?;
                for job in &batch {
                    let Some(label) = labels.get(&job.id) else {
                        continue;
                    };
                    cache_transaction.execute(
                        "INSERT INTO summaries(thread_id,rollout_path,dev,inode,processed_len,label,activity) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)
                         ON CONFLICT(thread_id) DO UPDATE SET
                           rollout_path=excluded.rollout_path,
                           dev=excluded.dev,
                           inode=excluded.inode,
                           processed_len=excluded.processed_len,
                           label=excluded.label,
                           activity=excluded.activity",
                        (
                            &job.id,
                            job.path.display().to_string(),
                            job.dev,
                            job.inode,
                            job.length,
                            label,
                            &job.activity,
                        ),
                    )?;
                    cache_transaction
                        .execute("DELETE FROM pending_activity WHERE thread_id=?1", [&job.id])?;
                }
                cache_transaction.commit()?;
                done += labels.len();

                let state = Connection::open(state_path)?;
                state.busy_timeout(std::time::Duration::from_secs(10))?;
                if let Ok(transaction) = state.unchecked_transaction() {
                    for (id, label) in &labels {
                        let _ = transaction.execute(
                            "UPDATE threads SET name = ?1, preview = ?1 WHERE id = ?2",
                            (label, id),
                        );
                    }
                    let _ = transaction.commit();
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
         CURRENT ACTIVITY overrides PRIOR and BACKGROUND when they disagree. \
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

    let mut child = Command::new(stock_codex())
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

fn conversation_evidence(
    path: &Path,
    first_user: &str,
    preview: &str,
    pending: Option<&str>,
) -> Option<Evidence> {
    let mut embedded = embedded_capsule(first_user);
    let mut file = match File::open(path) {
        Ok(file) => file,
        Err(_) => {
            return embedded.map(|capsule| Evidence {
                capsule,
                activity: pending.unwrap_or("").to_string(),
            });
        }
    };
    let length = file.metadata().ok()?.len();
    let start = length.saturating_sub(TAIL_BYTES);
    file.seek(SeekFrom::Start(start)).ok()?;
    let mut partial_activity = String::new();
    if start > 0 {
        let mut prefix = Vec::new();
        let mut probe = File::open(path).ok()?;
        probe.seek(SeekFrom::Start(start)).ok()?;
        probe.take(TAIL_BYTES).read_to_end(&mut prefix).ok()?;
        if let Some(nl) = prefix.iter().position(|b| *b == b'\n') {
            partial_activity = recover_partial_text(&prefix[..nl]);
        }
    }
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
    let mut context_summary = None;
    for line in reader.lines().map_while(Result::ok) {
        if !line.contains("\"type\":\"compacted\"")
            && (!line.contains("\"type\":\"response_item\"")
                || !line.contains("\"type\":\"message\""))
        {
            continue;
        }
        let Ok(item) = serde_json::from_str::<serde_json::Value>(&line) else {
            continue;
        };
        let payload = &item["payload"];
        if item["type"] == "compacted" {
            context_summary = payload["message"]
                .as_str()
                .map(|message| compact_text(message, 360));
            continue;
        }
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
        if role == "user" {
            if let Some(base) = embedded_capsule(&text) {
                embedded = Some(base);
                continue;
            }
            if synthetic_prompt(&text) {
                continue;
            }
        }
        let text = compact_text(&text, 180);
        if text.is_empty() {
            continue;
        }
        tail.push_back(format!("{role}: {text}"));
        if tail.len() > 3 {
            tail.pop_front();
        }
    }
    let latest = tail
        .iter()
        .rev()
        .find(|message| message.starts_with("user: "))
        .map(|message| compact_text(message.strip_prefix("user: ").unwrap_or(message), 180))
        .unwrap_or_default();
    let recovered = if latest.is_empty() {
        partial_activity
    } else {
        latest.clone()
    };
    let current_activity = pending.filter(|s| !s.is_empty()).unwrap_or(&recovered);
    if let Some(summary) = context_summary.filter(|summary| !summary.is_empty()) {
        let latest = tail
            .iter()
            .rev()
            .find(|message| message.starts_with("user: "))
            .map(|message| compact_text(message, 110))
            .unwrap_or_default();
        return Some(Evidence {
            capsule: compact_text(
                &format!(
                    "{}background: {summary}\nrecent: {latest}",
                    if current_activity.is_empty() {
                        String::new()
                    } else {
                        format!("current activity: {current_activity}\n")
                    }
                ),
                CAPSULE_CHARS,
            ),
            activity: compact_text(current_activity, CAPSULE_CHARS),
        });
    }
    if let Some(base) = embedded {
        let latest = tail
            .iter()
            .rev()
            .find(|message| message.starts_with("user: "))
            .map(|message| compact_text(message, 110))
            .unwrap_or_default();
        return Some(Evidence {
            capsule: compact_text(
                &format!(
                    "{}background: {base}\nrecent: {latest}",
                    if current_activity.is_empty() {
                        String::new()
                    } else {
                        format!("current activity: {current_activity}\n")
                    }
                ),
                CAPSULE_CHARS,
            ),
            activity: compact_text(current_activity, CAPSULE_CHARS),
        });
    }
    let mut capsule = String::new();
    let activity = current_activity;
    if !activity.is_empty() {
        capsule.push_str("current activity: ");
        capsule.push_str(activity);
        capsule.push('\n');
    }
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
    (!capsule.is_empty()).then_some(Evidence {
        capsule: compact_text(&capsule, CAPSULE_CHARS),
        activity: compact_text(activity, CAPSULE_CHARS),
    })
}

fn recover_partial_text(line: &[u8]) -> String {
    let text = String::from_utf8_lossy(line);
    let Some(start) = text.rfind("\"type\":\"input_text\"") else {
        return String::new();
    };
    let Some(text_start) = text[start..].find("\"text\":\"") else {
        return String::new();
    };
    let fragment = &text[start + text_start + 8..];
    let mut encoded = String::new();
    let mut escaped = false;
    for ch in fragment.chars() {
        if !escaped && ch == '"' {
            break;
        }
        encoded.push(ch);
        escaped = !escaped && ch == '\\';
        if ch != '\\' {
            escaped = false;
        }
    }
    serde_json::from_str::<String>(&format!("\"{encoded}\"")).unwrap_or_default()
}

fn conversation_capsule(path: &Path, first_user: &str, preview: &str) -> Option<String> {
    conversation_evidence(path, first_user, preview, None).map(|e| e.capsule)
}

fn synthetic_prompt(text: &str) -> bool {
    text.starts_with("# AGENTS.md instructions")
        || text.starts_with("<environment_context>")
        || text.contains("<skills_instructions>")
        || text.contains("<permissions instructions>")
        || text.contains("<collaboration_mode>")
        || text.contains("--- capsule ---")
        || (text.starts_with("Read ")
            && text.contains("/project-maps/")
            && text.contains("Use its Objective, Cursor, and Next action."))
}

fn embedded_capsule(text: &str) -> Option<String> {
    if !text.contains("# CompactVeteran handoff") {
        return None;
    }
    let start = text.find("--- capsule ---")?;
    let body = &text[start..];
    let objective = body
        .split_once("## Objective\n\n")?
        .1
        .split_once("\n\n## Cursor")?
        .0;
    let cursor = body
        .split_once("## Cursor\n\n")?
        .1
        .split_once("\n\n## Next action")?
        .0;
    let next = body
        .split_once("## Next action\n\n")?
        .1
        .split_once("\n\n## Recent commits")?
        .0;
    Some(format!(
        "objective: {}\ncursor: {}\nnext: {}",
        compact_text(objective, 160),
        compact_text(cursor, 120),
        compact_text(next, 100)
    ))
}

fn stock_codex() -> String {
    for path in [
        env::var_os("CODEX_HOME")
            .map(|p| PathBuf::from(p).join("packages/standalone/current/bin/codex")),
        env::var_os("HOME")
            .map(|p| PathBuf::from(p).join(".codex/packages/standalone/current/bin/codex")),
    ]
    .into_iter()
    .flatten()
    {
        if path.is_file() {
            return path.display().to_string();
        }
    }
    "codex".into()
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
    let mut statement = connection.prepare(&format!(
        "SELECT rollout_path, first_user_message, preview FROM threads WHERE {HUMAN_THREAD_FILTER}"
    ))?;
    let mut count = 0;
    let mut bytes = 0u64;
    for (path, first_user, preview) in statement
        .query_map([], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
            ))
        })?
        .filter_map(Result::ok)
    {
        if conversation_capsule(Path::new(&path), &first_user, &preview).is_some() {
            count += 1;
            if let Ok(metadata) = fs::metadata(path) {
                bytes += metadata.len().min(TAIL_BYTES);
            }
        }
    }
    println!(
        "{count} labelable conversations; maximum capsule characters: {}; bounded local-tail: {:.2} MiB",
        count * CAPSULE_CHARS,
        bytes as f64 / 1_048_576.0
    );
    Ok(())
}

fn activity_hook() -> Result<(), Box<dyn Error>> {
    let mut input = String::new();
    std::io::stdin().read_to_string(&mut input)?;
    let Ok(event) = serde_json::from_str::<ActivityInput>(&input) else {
        return Ok(());
    };
    let stripped = strip_images(&event.prompt);
    let original = compact_text(&stripped, CAPSULE_CHARS);
    let activity = provisional_label(&original);
    if activity.is_empty() {
        return Ok(());
    }
    let (state_path, plugin_data) = paths()?;
    let state = Connection::open(state_path)?;
    let Some(old): Option<Option<String>> = state
        .query_row(
            "SELECT name FROM threads WHERE id = ?1",
            [&event.session_id],
            |r| r.get(0),
        )
        .optional()?
    else {
        return Ok(());
    };
    let cache = open_cache(&plugin_data)?;
    cache.execute("INSERT INTO original_names(thread_id,name) VALUES (?1,?2) ON CONFLICT(thread_id) DO NOTHING", (&event.session_id, old.as_deref()))?;
    cache.execute("INSERT INTO pending_activity(thread_id,activity) VALUES (?1,?2) ON CONFLICT(thread_id) DO UPDATE SET activity=excluded.activity", (&event.session_id, &original))?;
    let tx = state.unchecked_transaction()?;
    tx.execute(
        "UPDATE threads SET name=?1, preview=?1 WHERE id=?2",
        (&activity, &event.session_id),
    )?;
    tx.commit()?;
    Ok(())
}

fn provisional_label(prompt: &str) -> String {
    let prompt = strip_images(prompt);
    let lower = prompt.to_lowercase();
    if synthetic_prompt(&prompt)
        || [
            "ok",
            "okay",
            "thanks",
            "thank you",
            "continue",
            "yes",
            "no",
            "sure",
        ]
        .contains(&lower.trim())
    {
        return String::new();
    }
    let filler = [
        "please",
        "can",
        "you",
        "could",
        "would",
        "just",
        "help",
        "me",
        "i",
        "need",
        "want",
        "to",
        "the",
        "a",
        "an",
        "and",
        "also",
        "now",
        "make",
        "it",
        "this",
        "that",
        "is",
        "are",
        "have",
        "has",
        "we",
        "our",
        "my",
        "so",
        "on",
        "for",
        "example",
        "conversation",
        "still",
        "gotta",
        "able",
        "be",
        "do",
        "its",
        "it's",
        "most",
    ];
    let mut out = Vec::new();
    let mut seen = HashSet::new();
    for raw in prompt.split_whitespace() {
        let word = raw.trim_matches(|c: char| {
            !c.is_alphanumeric() && c != '#' && c != '/' && c != '-' && c != '_'
        });
        let original = word.to_lowercase();
        let stem = original.trim_end_matches('s').to_string();
        if word.is_empty() || filler.contains(&original.as_str()) || !seen.insert(stem.clone()) {
            continue;
        }
        out.push(word.to_string());
        if out.len() == 12 {
            break;
        }
    }
    let mut label = out.join(" ");
    if let Some(first) = label.get_mut(0..1) {
        first.make_ascii_uppercase();
    }
    label
}

fn strip_images(prompt: &str) -> String {
    let mut out = prompt.to_string();
    while let Some(start) = out.find("<image") {
        if let Some(end) = out[start..].find("</image>") {
            out.replace_range(start..start + end + 8, "");
        } else {
            break;
        }
    }
    let mut clean = String::new();
    let mut rest = out.as_str();
    while let Some(start) = rest.find("[Image #") {
        clean.push_str(&rest[..start]);
        let Some(end) = rest[start..].find(']') else {
            break;
        };
        rest = &rest[start + end + 1..];
    }
    clean.push_str(rest);
    clean.trim().to_string()
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
    let originals = cache
        .prepare("SELECT thread_id, name FROM original_names")?
        .query_map([], |r| {
            Ok((r.get::<_, String>(0)?, r.get::<_, Option<String>>(1)?))
        })?
        .filter_map(Result::ok)
        .collect::<Vec<_>>();
    for (id, name) in originals {
        state.execute(
            "UPDATE threads SET preview = first_user_message, name = ?2 WHERE id = ?1",
            (&id, name),
        )?;
    }
    drop(statement);
    drop(cache);
    fs::remove_file(cache_path)?;
    Ok(())
}

fn trust_installed_hook() -> Result<(), Box<dyn Error>> {
    let cwd = env::current_dir()?.display().to_string();
    let mut child = Command::new(stock_codex())
        .args(["app-server", "--stdio"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()?;
    let mut stdin = child.stdin.take().ok_or("missing app-server stdin")?;
    let mut stdout = BufReader::new(child.stdout.take().ok_or("missing app-server stdout")?);
    write_jsonrpc(
        &mut stdin,
        serde_json::json!({"method":"initialize","id":1,"params":{"clientInfo":{"name":"recentlydivorced-installer","version":"0.3.10"}}}),
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
    let found = hooks["result"]["data"][0]["hooks"]
        .as_array()
        .ok_or("hooks list missing")?;
    let pairs = found
        .iter()
        .filter(|h| h["pluginId"] == "recentlydivorced@recentlydivorced")
        .filter_map(|h| {
            Some((
                h["key"].as_str()?.to_string(),
                h["currentHash"].as_str()?.to_string(),
            ))
        })
        .collect::<Vec<_>>();
    if pairs.is_empty() {
        return Err("RecentlyDivorced hooks were not discovered by stock Codex".into());
    }
    let mut state = serde_json::Map::new();
    for (key, hash) in pairs {
        state.insert(key, serde_json::json!({"enabled":true,"trusted_hash":hash}));
    }
    write_jsonrpc(
        &mut stdin,
        serde_json::json!({
            "method":"config/batchWrite","id":3,"params":{
                "edits": [{"keyPath":"hooks.state","value":state,"mergeStrategy":"upsert"}],
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
        CAPSULE_CHARS, HUMAN_THREAD_FILTER, INITIAL_MODEL, Job, RefreshDecision, UPDATE_MODEL,
        conversation_capsule, normalize_label, provisional_label, refresh_decision, run_model,
    };
    use rusqlite::Connection;

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

        std::fs::write(
            &rollout,
            r#"{"type":"compacted","payload":{"message":"camera works; sole lighting remains","replacement_history":null}}"#,
        )
        .unwrap();
        assert_eq!(
            conversation_capsule(&rollout, "ignored fallback", "").unwrap(),
            "background: camera works; sole lighting remains recent:"
        );
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
    fn activity_label_is_concise_and_dedupes_plural_stems() {
        let label = provisional_label(
            "Please update updates RecentlyDivorced description for recent activity on PartOfThis usage [Image #1]",
        );
        assert!(label.contains("RecentlyDivorced"));
        assert!(!label.to_lowercase().contains("conversation"));
        assert!(!label.to_lowercase().contains(" update updates"));
    }

    #[test]
    fn exact_current_request_label_and_trivial_prompt() {
        let prompt = "<image name=[Image #1]>ignored</image> also can you update the RecentlyDivorced so that it updates the description based on the most recent activity, so for example this conversation is still labeled partofthis it's gotta be able to do this without draining usage";
        assert_eq!(
            provisional_label(prompt),
            "Update RecentlyDivorced description based recent activity labeled partofthis without draining usage"
        );
        assert_eq!(provisional_label("continue"), "");
    }

    #[test]
    fn partial_tail_prefers_last_input_text_before_summary() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("long.jsonl");
        let prefix = "x".repeat(70_000);
        let line = format!(
            r#"{{"type":"response_item","payload":{{"type":"message","role":"user","content":[{{"type":"input_text","text":"image junk"}},{{"type":"input_text","text":"repair recent labels"}}]}}}}}}"#
        );
        let summary =
            r#"{"type":"compacted","payload":{"message":"background: retained project context"}}"#;
        std::fs::write(&path, format!("{}{}\n{}\n", prefix, line, summary)).unwrap();
        let evidence = super::conversation_evidence(&path, "old", "", None).unwrap();
        assert!(
            evidence
                .capsule
                .starts_with("current activity: repair recent labels")
        );
        assert!(
            evidence.capsule.find("background:").unwrap()
                > evidence.capsule.find("current activity:").unwrap()
        );
        assert!(evidence.capsule.chars().count() <= CAPSULE_CHARS);
    }

    #[test]
    fn refresh_decisions_cover_reuse_seed_and_relabel() {
        assert_eq!(
            refresh_decision("activity", true, false, ""),
            RefreshDecision::Reuse
        );
        assert_eq!(
            refresh_decision("", true, false, "new"),
            RefreshDecision::Seed
        );
        assert_eq!(
            refresh_decision("old", false, true, "new"),
            RefreshDecision::Relabel
        );
        assert_eq!(
            refresh_decision("same", false, true, "same"),
            RefreshDecision::Reuse
        );
        assert_eq!(
            refresh_decision("same", false, false, "same"),
            RefreshDecision::Reuse
        );
    }

    #[test]
    fn compactveteran_capsule_survives_missing_rollout_and_tail_embedding() {
        let embedded = "--- capsule ---\n# CompactVeteran handoff\n\n## Objective\n\nship the objective\n\n## Cursor\n\ncurrent cursor\n\n## Next action\n\nperform the next action\n\n## Recent commits\n\nnone";
        let capsule = conversation_capsule(std::path::Path::new("/missing"), embedded, "").unwrap();
        assert!(capsule.contains("objective: ship the objective"));
        assert!(capsule.contains("cursor: current cursor"));
        assert!(capsule.contains("next: perform the next action"));
        let temp = tempfile::tempdir().unwrap();
        let rollout = temp.path().join("tail.jsonl");
        std::fs::write(&rollout, [
            serde_json::json!({"type":"response_item","payload":{"type":"message","role":"user","content":[{"text":embedded}]}}).to_string(),
            serde_json::json!({"type":"response_item","payload":{"type":"message","role":"user","content":[{"text":"ship the compatibility fix"}]}}).to_string(),
        ].join("\n")).unwrap();
        let capsule = conversation_capsule(&rollout, "ordinary first ask", "").unwrap();
        assert!(capsule.contains("objective: ship the objective"));
        assert!(capsule.contains("cursor: current cursor"));
        assert!(capsule.contains("next: perform the next action"));
        assert!(capsule.contains("current activity: ship the compatibility fix"));
        assert!(!capsule.contains("Continue a prior Sol"));
        assert!(!capsule.contains("--- capsule ---"));
        assert!(capsule.chars().count() <= CAPSULE_CHARS);
    }

    #[test]
    fn human_thread_filter_covers_legacy_and_paginated_only() {
        let db = Connection::open_in_memory().unwrap();
        db.execute("CREATE TABLE threads (id TEXT, source TEXT, thread_source TEXT, agent_role TEXT, rollout_path TEXT, history_mode TEXT)", []).unwrap();
        for row in [("legacy", "legacy"), ("page", "paginated")] {
            db.execute(
                "INSERT INTO threads VALUES (?1,'cli','user',NULL,'/tmp/x',?2)",
                (&row.0, &row.1),
            )
            .unwrap();
        }
        db.execute("INSERT INTO threads VALUES ('sub','cli','user','worker','/tmp/x','paginated'),('fork','cli','fork',NULL,'/tmp/x','paginated')", []).unwrap();
        let count: i64 = db
            .query_row(
                &format!("SELECT count(*) FROM threads WHERE {HUMAN_THREAD_FILTER}"),
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(count, 2);
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
            activity: "make resume labels recognizable".into(),
            prior: None,
        };
        for (batch, model) in [INITIAL_MODEL, UPDATE_MODEL].into_iter().enumerate() {
            let labels = run_model(model, std::slice::from_ref(&job), temp.path(), batch).unwrap();
            assert!(!labels["probe"].is_empty());
        }
    }
}
