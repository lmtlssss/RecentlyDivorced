#!/usr/bin/env python3
"""Production-shaped lifecycle proof for RecentlyDivorced without live state."""
import json, os, pathlib, sqlite3, subprocess, tempfile

ROOT = pathlib.Path(__file__).resolve().parents[1]
RD = pathlib.Path(os.environ.get("RD_BIN", ROOT / "plugins/recentlydivorced/runtime/target/debug/recentlydivorced"))

with tempfile.TemporaryDirectory() as d:
    home = pathlib.Path(d); codex = home / "packages/standalone/current/bin"; codex.mkdir(parents=True)
    data = home / "plugins/data/recentlydivorced-recentlydivorced"; data.mkdir(parents=True)
    calls = home / "calls"; calls.write_text("0")
    marker = home / "race"
    fake = codex / "codex"
    fake.write_text("""#!/usr/bin/env python3
import json,sys,pathlib
a=sys.argv; out=pathlib.Path(a[a.index('--output-last-message')+1]); text=sys.stdin.read()
ids=[x.split(maxsplit=1)[1] for x in text.splitlines() if x.startswith('ID ')]
p=pathlib.Path(r'''CALLS'''); p.write_text(str(int(p.read_text())+1))
if pathlib.Path(r'''MARKER''').exists():
 import sqlite3,os
 c=sqlite3.connect(os.environ['CODEX_HOME']+'/plugins/data/recentlydivorced-recentlydivorced/state.sqlite')
 c.execute("INSERT OR REPLACE INTO pending_activity VALUES (?,?)",('thread-1','newer prompt during model call')); c.commit(); c.close()
out.write_text(json.dumps({'labels':[{'id':i,'label':'Build boot viewer with camera controls'} for i in ids]}))
""".replace('CALLS', str(calls)).replace('MARKER', str(marker)))
    fake.chmod(0o755)
    db = sqlite3.connect(home / "state_5.sqlite")
    db.execute("CREATE TABLE threads (id TEXT PRIMARY KEY, rollout_path TEXT, first_user_message TEXT, preview TEXT, name TEXT, source TEXT, thread_source TEXT, agent_role TEXT)")
    rollout = home / "thread.jsonl"
    rollout.write_text(json.dumps({'type':'response_item','payload':{'type':'message','role':'user','content':[{'text':'Build boot viewer with camera controls'}]}}, separators=(',',':'))+'\n')
    db.execute("INSERT INTO threads VALUES (?,?,?,?,?,?,?,?)", ('thread-1',str(rollout),'Build boot viewer','old preview','Old title','cli','user',None)); db.commit(); db.close()
    env = dict(os.environ, CODEX_HOME=str(home), PLUGIN_DATA=str(data))
    def run(*args, inp=None): return subprocess.run([str(RD),*args], env=env, input=inp, text=True, capture_output=True, check=True)
    run('--activity', inp=json.dumps({'session_id':'thread-1','prompt':'Build boot viewer with camera controls'}))
    c=sqlite3.connect(home/'state_5.sqlite'); assert c.execute("SELECT name,preview FROM threads").fetchone()==('Old title','old preview'); c.close()
    run('--catch-up'); c=sqlite3.connect(home/'state_5.sqlite'); assert c.execute("SELECT name FROM threads").fetchone()[0] == 'Build boot viewer with camera controls'; c.close(); c=sqlite3.connect(data/'state.sqlite'); assert c.execute("SELECT name FROM original_names WHERE thread_id='thread-1'").fetchone()[0] == 'Old title'; c.close()
    n=int(calls.read_text()); run('--catch-up'); assert int(calls.read_text())==n
    run('--activity', inp=json.dumps({'session_id':'thread-1','prompt':'Add boot rotation controls'}))
    with rollout.open('a') as f: f.write(json.dumps({'type':'response_item','payload':{'type':'message','role':'assistant','content':[{'text':'Boot camera shipped'}]}}, separators=(',',':'))+'\n')
    run('--catch-up'); assert int(calls.read_text())>n
    marker.touch(); run('--catch-up'); c=sqlite3.connect(data/'state.sqlite'); assert c.execute("SELECT activity FROM pending_activity WHERE thread_id='thread-1'").fetchone()[0]=='newer prompt during model call'; c.close()
    run('--activity', inp=json.dumps({'session_id':'thread-1','prompt':'<codex_internal_context source="goal"> Continue working toward the active thread goal</codex_internal_context>'}))
    c=sqlite3.connect(home/'state_5.sqlite'); assert c.execute("SELECT name FROM threads").fetchone()[0]=='Build boot viewer with camera controls'; c.close()
    c=sqlite3.connect(data/'state.sqlite'); assert c.execute("SELECT activity FROM pending_activity WHERE thread_id='thread-1'").fetchone()[0]=='newer prompt during model call'; c.close()
print('rd lifecycle proof: PASS')
