"""Linux PTY test with a local stub only: never launches a real Codex session."""
import fcntl
import json
import os
import pty
import select
import struct
import subprocess
import tempfile
import termios
import time
from pathlib import Path
from terminal_screen import Screen

SESSION_ID = '019feeaf-b9a2-7773-8776-269af6ca57c4'
binary = Path(os.environ.get('HISTORY_BINARY', 'target/debug/codex-prompt-history')).resolve()
with tempfile.TemporaryDirectory() as directory:
    root = Path(directory)
    starting = root / 'starting directory'
    override = root / 'workspace & literal'
    starting.mkdir(); override.mkdir()
    receipt = root / 'receipt.jsonl'
    stub = root / 'fake codex'
    stub.write_text('''#!/usr/bin/env python3
import json, os, sys, termios
record = {"args":sys.argv[1:], "home":os.environ.get("CODEX_HOME"), "cwd":os.getcwd(), "tty":all(os.isatty(i) for i in [0,1,2]), "canonical":bool(termios.tcgetattr(0)[3] & termios.ICANON)}
with open(os.environ["RESUME_TEST_RECEIPT"], "a") as file:
    file.write(json.dumps(record)+"\\n")
print("STUB_READY", flush=True)
response=sys.stdin.readline().strip()
sys.exit(9 if response=="fail" else 0)
''')
    stub.chmod(0o755)
    histories = []
    projects = []
    for index, name in enumerate(['.codex', '.codex-beta', '.codex-gamma']):
        home = root / name
        (home / 'sessions').mkdir(parents=True)
        history = home / 'history.jsonl'; histories.append(history)
        profile = ['alpha', 'beta', 'gamma'][index]
        project = root / f'project {profile} & literal'
        project.mkdir(); projects.append(project)
        history.write_text(json.dumps({'session_id':SESSION_ID,'ts':index+1,'text':f'prompt {profile}'})+'\n')
        (home / 'sessions' / 'session.jsonl').write_text('\n'.join(json.dumps(value) for value in [
            {'type':'session_meta','payload':{'id':SESSION_ID, **({'cwd':str(project)} if profile != 'gamma' else {})}},
            {'type':'response_item','payload':{'type':'message','role':'assistant','content':[{'type':'output_text','text':profile}]}}
        ]))
    before = {str(path):path.read_bytes() for history in histories for path in history.parent.rglob('*') if path.is_file()}
    for index, profile in enumerate(['alpha', 'beta', 'gamma', 'missing', 'gone', 'nocwd']):
        master, slave = pty.openpty()
        fcntl.ioctl(slave, termios.TIOCSWINSZ, struct.pack('HHHH',36,120,0,0))
        original = termios.tcgetattr(slave)
        selected = 'alpha' if profile in ('missing', 'gone', 'nocwd') else profile
        alpha_session = histories[0].parent / 'sessions' / 'session.jsonl'
        saved_session = alpha_session.read_bytes()
        if profile in ('gone', 'nocwd'):
            lines = alpha_session.read_text().splitlines()
            metadata = json.loads(lines[0])
            if profile == 'gone': metadata['payload']['cwd'] = str(root / 'deleted-project')
            else: metadata['payload'].pop('cwd', None)
            lines[0] = json.dumps(metadata)
            alpha_session.write_text('\n'.join(lines))
        args = [str(binary), '--query', selected, '--clipboard', 'terminal', '--codex-bin', str(root/'missing-cli' if profile == 'missing' else stub)]
        for history in histories: args += ['--file', str(history)]
        if profile == 'gamma': args += ['--resume-cwd', str(override)]
        proc = subprocess.Popen(args, stdin=slave, stdout=slave, stderr=slave, cwd=starting,
            env=dict(os.environ, TERM='xterm-256color', CODEX_HOME=str(root/'parent-home'), RESUME_TEST_RECEIPT=str(receipt)))
        screen = Screen(120,36)
        def drain(seconds=0.2):
            until=time.monotonic()+seconds
            while time.monotonic()<until:
                if select.select([master],[],[],0.03)[0]:
                    screen.feed(os.read(master,65536))
                    while screen.responses: os.write(master, screen.responses.pop(0))
            return screen.compact()
        def wait_for(text):
            until=time.monotonic()+5
            while time.monotonic()<until:
                if text in drain(): return
            raise AssertionError(f'{text}: {screen.compact()}')
        def press(data): os.write(master,data); return drain()
        try:
            wait_for('Historyreloaded')
            if profile == 'beta':
                press(b'\r'); wait_for('SESSIONHISTORY')
            press(b'R')
            if profile in ('missing', 'gone', 'nocwd'):
                error = {'missing':'Codexexecutablenotfound', 'gone':'Cannotaccessprojectdirectory', 'nocwd':'Sessionhasnorecordedprojectdirectory'}[profile]
                wait_for('Resumeunavailable:' + error)
                assert len(receipt.read_text().splitlines()) == 3
                assert not (termios.tcgetattr(slave)[3] & termios.ICANON)
            else:
                until=time.monotonic()+5
                while time.monotonic()<until:
                    drain()
                    records = receipt.read_text().splitlines() if receipt.exists() else []
                    if len(records) == index+1: break
                assert len(records) == index+1
                record=json.loads(records[-1])
                assert record['home'] == str(histories[index].parent.resolve()), record
                assert record['tty'] and record['canonical'], record
                assert record['cwd'] == str(override if profile == 'gamma' else projects[index]), record
                assert record['args'] == ['resume','--cd','.','--',SESSION_ID], record
                press(b'fail\n' if profile == 'gamma' else b'exit\n')
                wait_for('Codex(.codex-gamma)exited' if profile=='gamma' else ('Sessionreloaded' if profile=='beta' else 'Historyreloaded'))
                assert not (termios.tcgetattr(slave)[3] & termios.ICANON)
            press(b'q'); proc.wait(timeout=5)
            assert proc.returncode == 0
            assert termios.tcgetattr(slave) == original
        finally:
            if proc.poll() is None: proc.kill(); proc.wait()
            os.close(master); os.close(slave)
            alpha_session.write_bytes(saved_session)
    after = {str(path):path.read_bytes() for history in histories for path in history.parent.rglob('*') if path.is_file()}
    assert before == after
print('PASS: alpha/beta/gamma resume routing, UUID args, recorded project cwd, explicit cwd override, terminal handoff/return, nonzero exit, missing executable/project/metadata; source files unchanged.')
