#!/usr/bin/env python3
"""Proof implementation for the RecentlyDivorced UserPromptSubmit hook."""

import json
import os
import sqlite3
import sys
import time
from pathlib import Path


def main() -> int:
    event = json.load(sys.stdin)
    session_id = event.get("session_id", "")
    prompt = event.get("prompt", "").strip()
    if not session_id or not prompt:
        return 0

    codex_home = Path(os.environ.get("CODEX_HOME", Path.home() / ".codex"))
    state_db = codex_home / "state_5.sqlite"
    if not state_db.exists():
        return 0

    for _ in range(20):
        try:
            connection = sqlite3.connect(state_db, timeout=0.1)
            try:
                connection.execute("PRAGMA busy_timeout = 100")
                updated = connection.execute(
                    "UPDATE threads SET preview = ? WHERE id = ?",
                    (prompt, session_id),
                ).rowcount
                connection.commit()
                if updated:
                    return 0
            finally:
                connection.close()
        except sqlite3.Error:
            pass
        time.sleep(0.1)
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
