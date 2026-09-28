#!/usr/bin/env python3
"""Run the real RecentlyDivorced binary against isolated stores and a model stub.

Failure cases: unknown-task invention, same-project title collisions, generic
prompt churn, manual-name overwrite on reuse/update/restore, stale model results,
child-agent contamination, uncertain-result churn, and lost legacy ownership.
No real Codex home, login, model request, or conversation is used.
"""
import json
import os
from pathlib import Path
import sqlite3
import subprocess
import tempfile
import unittest

ROOT = Path(__file__).resolve().parents[1]
RD = Path(os.environ.get("RD_BIN", ROOT / "plugins/recentlydivorced/runtime/target/debug/recentlydivorced")).resolve()

MODEL_STUB = r'''#!/usr/bin/env python3
import json, os, pathlib, sqlite3, subprocess, sys
home = pathlib.Path(os.environ['CODEX_HOME'])
text = sys.stdin.read()
with (home / 'calls.jsonl').open('a') as f:
    f.write(json.dumps({'prompt': text, 'args': sys.argv}) + '\n')
control = home / 'during-generation.json'
if control.exists():
    action = json.loads(control.read_text()); control.unlink()
    if action['kind'] == 'rename':
        with sqlite3.connect(home / 'state_5.sqlite') as db:
            db.execute('UPDATE threads SET name=? WHERE id=?', (action['name'], action['id']))
    elif action['kind'] == 'activity':
        env = dict(os.environ); env.pop('RECENTLYDIVORCED_INTERNAL', None)
        subprocess.run([env['RD_BIN'], '--activity'], input=json.dumps(action['event']),
                       text=True, env=env, check=True)
        if action.get('rollout'):
            with pathlib.Path(action['rollout']).open('a') as f:
                f.write(json.dumps({'type':'response_item','payload':{'type':'message',
                    'role':'user','content':[{'text':action['event']['prompt']}]}}) + '\n')
labels = []
for block in text.split('\nID ')[1:]:
    identity = block.splitlines()[0]
    body = block.split('\nCONVERSATION\n', 1)[1].lower()
    if (home / 'defer').exists():
        label = ''
    elif 'login expiry' in body:
        label = 'Shop login expiry investigation'
    elif 'checkout retries' in body:
        label = 'Shop checkout retries repair'
    elif 'api links' in body:
        label = 'Docs API links repair'
    else:
        label = 'Invented task from insufficient context'
    labels.append({'id':identity, 'label':label})
out = pathlib.Path(sys.argv[sys.argv.index('--output-last-message') + 1])
out.write_text(json.dumps({'labels':labels}))
'''


class Lifecycle(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.addCleanup(self.temp.cleanup)
        self.home = Path(self.temp.name)
        self.data = self.home / 'plugin-data'
        self.data.mkdir()
        self.env = dict(os.environ, HOME=str(self.home), CODEX_HOME=str(self.home),
                        PLUGIN_DATA=str(self.data), RD_BIN=str(RD))
        self.env.pop('RECENTLYDIVORCED_INTERNAL', None)
        fake = self.home / 'packages/standalone/current/bin/codex'
        fake.parent.mkdir(parents=True)
        fake.write_text(MODEL_STUB)
        fake.chmod(0o755)
        with self.state() as db:
            db.execute('CREATE TABLE threads (id TEXT PRIMARY KEY, rollout_path TEXT, '
                       'first_user_message TEXT, preview TEXT, name TEXT, source TEXT, '
                       'thread_source TEXT, agent_role TEXT, cwd TEXT)')

    def state(self):
        return sqlite3.connect(self.home / 'state_5.sqlite')

    def run_rd(self, *args, event=None):
        return subprocess.run([str(RD), *args], env=self.env,
                              input=None if event is None else json.dumps(event),
                              text=True, capture_output=True, check=True, timeout=30)

    def add(self, identity, prompt='continue', name=None, project='shop', role=None):
        path = self.home / f'{identity}.jsonl'
        path.touch()
        with self.state() as db:
            db.execute('INSERT INTO threads VALUES (?,?,?,?,?,?,?,?,?)',
                       (identity, str(path), prompt, 'Original transcript preview', name,
                        'cli', 'user', role, f'/work/{project}'))
        self.append(identity, 'user', prompt)
        return path

    def append(self, identity, role, text):
        with (self.home / f'{identity}.jsonl').open('a') as f:
            # Whitespace is intentional: JSON spacing must not hide real evidence.
            f.write(json.dumps({'type':'response_item','payload':{'type':'message',
                'role':role,'content':[{'text':text}]}}) + '\n')

    def name(self, identity):
        with self.state() as db:
            return db.execute('SELECT name FROM threads WHERE id=?', (identity,)).fetchone()[0]

    def rename(self, identity, name):
        with self.state() as db:
            db.execute('UPDATE threads SET name=? WHERE id=?', (name, identity))

    def activity(self, identity, prompt, **extra):
        self.run_rd('--activity', event=dict(session_id=identity, prompt=prompt, **extra))

    def calls(self):
        path = self.home / 'calls.jsonl'
        return [json.loads(x) for x in path.read_text().splitlines()] if path.exists() else []

    def prepare(self, identity='a'):
        self.add(identity, 'Repair checkout retries')
        self.run_rd('--catch-up')
        self.assertEqual(self.name(identity), 'Shop checkout retries repair')

    def test_generic_openings_wait_then_same_project_tasks_diverge(self):
        for identity, project in [('a','shop'), ('b','shop'), ('c','docs')]:
            self.add(identity, project=project)
        self.run_rd('--catch-up')
        self.assertEqual(self.calls(), [], 'Unknown tasks must not trigger model work')
        self.assertIsNone(self.name('a'))
        for identity, task in [('a','Repair checkout retries'), ('b','Investigate login expiry'), ('c','Fix API links')]:
            self.append(identity, 'assistant', task)
        self.run_rd('--catch-up')
        self.assertEqual(self.name('a'), 'Shop checkout retries repair')
        self.assertEqual(self.name('b'), 'Shop login expiry investigation')
        self.assertEqual(self.name('c'), 'Docs API links repair')
        prompt = self.calls()[-1]['prompt']
        self.assertIn('working directory: shop', prompt)
        self.assertIn('working directory: docs', prompt)
        for identity in ['a','b','c']:
            with self.state() as db:
                self.assertEqual(db.execute('SELECT preview FROM threads WHERE id=?', (identity,)).fetchone()[0],
                                 'Original transcript preview')
        n = len(self.calls())
        for short in ['continue!', 'Please continue.', 'yes, thanks', '/resume', 'go on']:
            self.activity('a', short)
            self.append('a', 'user', short)
            self.run_rd('--catch-up')
        self.assertEqual(len(self.calls()), n, 'Generic follow-ups must reuse useful evidence')

    def test_names_with_unknown_origin_require_explicit_auto(self):
        self.add('a', 'Repair checkout retries', name='My release brief')
        self.run_rd('--catch-up')
        self.assertEqual(self.name('a'), 'My release brief')
        self.assertEqual(self.calls(), [])
        self.run_rd('--auto', 'a')
        self.run_rd('--catch-up')
        self.assertEqual(self.name('a'), 'Shop checkout retries repair')
        self.run_rd('--pin', 'a')
        self.activity('a', 'Investigate login expiry')
        self.append('a', 'user', 'Investigate login expiry')
        n = len(self.calls()); self.run_rd('--catch-up')
        self.assertEqual(self.name('a'), 'Shop checkout retries repair')
        self.assertEqual(len(self.calls()), n)

    def test_manual_rename_survives_cache_reuse_updates_restart_and_restore(self):
        self.prepare()
        self.rename('a', 'Keep my exact title')
        n = len(self.calls()); self.run_rd('--catch-up')
        self.assertEqual(self.name('a'), 'Keep my exact title')
        self.activity('a', 'Investigate login expiry')
        self.append('a', 'user', 'Investigate login expiry')
        self.run_rd('--catch-up')
        self.assertEqual(self.name('a'), 'Keep my exact title')
        self.assertEqual(len(self.calls()), n)
        self.run_rd('--restore')
        self.assertEqual(self.name('a'), 'Keep my exact title')

    def test_manual_rename_during_model_is_not_overwritten(self):
        self.add('a', 'Repair checkout retries')
        (self.home / 'during-generation.json').write_text(json.dumps(
            dict(kind='rename', id='a', name='Manual while generating')))
        self.run_rd('--catch-up')
        self.assertEqual(self.name('a'), 'Manual while generating')
        self.run_rd('--catch-up')
        self.assertEqual(self.name('a'), 'Manual while generating')

    def test_newer_activity_rejects_old_model_result(self):
        path = self.add('a', 'Repair checkout retries')
        (self.home / 'during-generation.json').write_text(json.dumps(dict(kind='activity',
            event=dict(session_id='a', prompt='Investigate login expiry', turn_id='new-turn'), rollout=str(path))))
        self.run_rd('--catch-up')
        self.assertIsNone(self.name('a'), 'Old result must not write after new input')
        self.run_rd('--catch-up')
        self.assertEqual(self.name('a'), 'Shop login expiry investigation')

    def test_child_hook_cannot_queue_work_for_parent(self):
        self.prepare()
        n = len(self.calls())
        self.activity('a', 'Investigate login expiry', agent_id='child-1')
        self.run_rd('--catch-up')
        self.assertEqual(self.name('a'), 'Shop checkout retries repair')
        self.assertEqual(len(self.calls()), n)

    def test_model_can_defer_without_retries_or_blank_name(self):
        self.add('a', 'Review the task requirements')
        (self.home / 'defer').touch()
        self.run_rd('--catch-up')
        self.assertIsNone(self.name('a'))
        self.assertEqual(len(self.calls()), 1)
        self.run_rd('--catch-up')
        self.assertEqual(len(self.calls()), 1, 'Unchanged uncertain evidence must not retry')
        (self.home / 'defer').unlink()
        self.activity('a', 'Repair checkout retries')
        self.append('a', 'user', 'Repair checkout retries')
        self.run_rd('--catch-up')
        self.assertEqual(self.name('a'), 'Shop checkout retries repair')

    def test_legacy_plugin_labels_keep_ownership_on_upgrade(self):
        self.add('a', 'Repair checkout retries', name='Old plugin label')
        path = self.home / 'a.jsonl'; st = path.stat()
        with sqlite3.connect(self.data / 'state.sqlite') as db:
            db.executescript('CREATE TABLE summaries (thread_id TEXT PRIMARY KEY, rollout_path TEXT NOT NULL, '
                'dev INTEGER NOT NULL, inode INTEGER NOT NULL, processed_len INTEGER NOT NULL, '
                'label TEXT NOT NULL, activity TEXT NOT NULL DEFAULT ""); '
                'CREATE TABLE original_names (thread_id TEXT PRIMARY KEY, name TEXT); '
                'CREATE TABLE meta (key TEXT PRIMARY KEY, value TEXT NOT NULL);')
            db.execute('INSERT INTO summaries VALUES (?,?,?,?,?,?,?)',
                       ('a', str(path), st.st_dev, st.st_ino, st.st_size, 'Old plugin label', 'old context'))
            db.execute('INSERT INTO original_names VALUES (?,?)', ('a', None))
            db.execute('INSERT INTO meta VALUES (?,?)', ('capsule_generation','older-version'))
        self.run_rd('--catch-up')
        self.assertEqual(self.name('a'), 'Shop checkout retries repair')
        self.run_rd('--restore')
        self.assertIsNone(self.name('a'))

    def test_optional_graph_can_supply_task_without_a_substantive_opening(self):
        self.add('a')
        path = self.home / 'plugins/data/the-graphfather-the-graphfather'
        path.mkdir(parents=True)
        with sqlite3.connect(path / 'state.sqlite') as db:
            db.execute('CREATE TABLE sessions (id TEXT PRIMARY KEY, document TEXT)')
            db.execute('INSERT INTO sessions VALUES (?,?)', ('a',json.dumps(
                {'blueprint':{'objective':'Repair checkout retries'},'phase':'behavior'})))
        self.run_rd('--catch-up')
        self.assertEqual(self.name('a'), 'Shop checkout retries repair')


if __name__ == '__main__':
    unittest.main(verbosity=2)
