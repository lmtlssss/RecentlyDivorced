//! Local ownership only. Codex does not expose a shared title-origin/CAS contract.
//! An observed external name is pinned; unknown pre-existing names require --auto.
use rusqlite::{Connection, OptionalExtension, Transaction, TransactionBehavior};

#[derive(Clone, Debug)]
pub(super) struct Owner {
    pub automatic: bool,
    pub expected_name: Option<String>,
    pub revision: i64,
}

pub(super) fn transaction(cache: &Connection) -> rusqlite::Result<Transaction<'_>> {
    Transaction::new_unchecked(cache, TransactionBehavior::Immediate)
}

pub(super) fn read(cache: &Connection, id: &str) -> rusqlite::Result<Option<Owner>> {
    cache
        .query_row(
            "SELECT mode, expected_name, revision FROM title_ownership WHERE thread_id=?1",
            [id],
            |row| {
                Ok(Owner {
                    automatic: row.get::<_, String>(0)? == "automatic",
                    expected_name: row.get(1)?,
                    revision: row.get(2)?,
                })
            },
        )
        .optional()
}

pub(super) fn pin(cache: &Connection, id: &str, name: Option<&str>) -> rusqlite::Result<()> {
    cache.execute(
        "UPDATE title_ownership SET mode='manual',expected_name=?2,revision=revision+1 WHERE thread_id=?1",
        (id, name),
    )?;
    cache.execute("DELETE FROM pending_activity WHERE thread_id=?1", [id])?;
    Ok(())
}

pub(super) fn observe(
    cache: &Connection,
    id: &str,
    current: Option<&str>,
) -> rusqlite::Result<Owner> {
    let tx = transaction(cache)?;
    let prior: Option<String> = tx
        .query_row(
            "SELECT label FROM summaries WHERE thread_id=?1",
            [id],
            |row| row.get(0),
        )
        .optional()?;
    let known = current.is_none_or(str::is_empty)
        || prior
            .as_deref()
            .is_some_and(|label| !label.is_empty() && Some(label) == current);
    tx.execute(
        "INSERT OR IGNORE INTO title_ownership(thread_id,mode,expected_name,revision) VALUES (?1,?2,?3,0)",
        (id, if known { "automatic" } else { "manual" }, current),
    )?;
    tx.execute(
        "INSERT OR IGNORE INTO original_names(thread_id,name) VALUES (?1,?2)",
        (id, current),
    )?;
    let mut owner = read(&tx, id)?.ok_or(rusqlite::Error::QueryReturnedNoRows)?;
    if owner.automatic && owner.expected_name.as_deref() != current {
        pin(&tx, id, current)?;
        owner.automatic = false;
        owner.expected_name = current.map(str::to_owned);
        owner.revision += 1;
    }
    tx.commit()?;
    Ok(owner)
}

pub(super) fn set(
    cache: &Connection,
    id: &str,
    current: Option<&str>,
    automatic: bool,
) -> rusqlite::Result<()> {
    let tx = transaction(cache)?;
    tx.execute(
        "INSERT INTO title_ownership(thread_id,mode,expected_name,revision) VALUES (?1,?2,?3,1)
         ON CONFLICT(thread_id) DO UPDATE SET mode=excluded.mode,expected_name=excluded.expected_name,revision=revision+1",
        (id, if automatic { "automatic" } else { "manual" }, current),
    )?;
    tx.execute("DELETE FROM pending_activity WHERE thread_id=?1", [id])?;
    if automatic {
        // Explicit opt-in adopts exactly the name the person chose to replace.
        tx.execute(
            "INSERT INTO original_names(thread_id,name) VALUES (?1,?2)
            ON CONFLICT(thread_id) DO UPDATE SET name=excluded.name",
            (id, current),
        )?;
        tx.execute(
            "UPDATE summaries SET activity='',processed_len=-1 WHERE thread_id=?1",
            [id],
        )?;
    }
    tx.commit()
}
