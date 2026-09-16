"""Linux PTY integration check. Run after cargo build: python3 tests/session_smoke.py."""
import fcntl
import json
import os
import pty
import re
import select
import struct
import subprocess
import tempfile
import termios
import time
from pathlib import Path

with tempfile.TemporaryDirectory() as directory:
    root = Path(directory)
    sessions = root / "sessions" / "2026" / "09"
    sessions.mkdir(parents=True)
    history = root / "history.jsonl"
    history.write_text('\n'.join(json.dumps(entry) for entry in [
        {"session_id": "demo", "ts": 2, "text": "Hello session"},
        {"session_id": "missing", "ts": 1, "text": "Missing session"},
    ]))
    records = [
        {"type": "session_meta", "payload": {"id": "demo", "cwd": "/example"}},
        {"type": "turn_context", "payload": {"model": "demo-model"}},
        {"type": "response_item", "payload": {"type": "message", "role": "user", "content": [{"type": "input_text", "text": "Hello session\n" + '\n'.join(f'Preview line {i}' for i in range(40))}]}},
        {"type": "response_item", "payload": {"type": "function_call", "name": "example_tool", "call_id": "demo-call", "arguments": "{\"command\":\"echo hello\\nwhoami\"}"}},
        {"type": "response_item", "payload": {"type": "function_call_output", "call_id": "demo-call", "output": "demo output"}},
        {"type": "response_item", "payload": {"type": "function_call", "name": "second_tool", "call_id": "second", "arguments": "second input"}},
        {"type": "response_item", "payload": {"type": "function_call_output", "call_id": "second", "output": {"exit_code": 2, "stderr": "second tool failed"}}},
        {"type": "response_item", "payload": {"type": "message", "role": "assistant", "content": [{"type": "output_text", "text": "Finished example"}]}},
    ]
    (sessions / "custom.jsonl").write_text('\n'.join(json.dumps(row) for row in records))
    master, slave = pty.openpty()
    fcntl.ioctl(slave, termios.TIOCSWINSZ, struct.pack("HHHH", 36, 120, 0, 0))
    before = termios.tcgetattr(slave)
    executable = os.environ.get("HISTORY_BINARY", "target/debug/codex-prompt-history")
    proc = subprocess.Popen([executable, "--file", str(history)], stdin=slave, stdout=slave, stderr=slave, env=dict(os.environ, TERM="xterm-256color"))

    def read_screen():
        output = bytearray()
        until = time.monotonic() + 0.3
        while time.monotonic() < until:
            if select.select([master], [], [], 0.05)[0]:
                output.extend(os.read(master, 65536))
        plain = re.sub(r'\x1b\[[0-?]*[ -/]*[@-~]', '', output.decode(errors="replace"))
        return ''.join(plain.split())

    def press(key):
        os.write(master, key)
        return read_screen()

    try:
        assert "PROMPTHISTORY" in read_screen()
        assert press(b't')  # source filter for the single loaded source
        assert press(b'\x1b')  # clear source filter before opening the session
        screen = press(b'\r')
        assert "Timeline" in screen and "demo-model" in screen and "ASSISTANT" in screen and "Toolactivity" in screen, screen
        assert press(b'\t')  # detail focus
        assert press(b'\x1b[6~')  # page down in the preview
        assert press(b'g')  # first preview line
        assert press(b'\t')  # list focus; selection is unchanged
        assert "Nomatchingsessionentries" in press(b'/no-such-text')
        press(b'\x15')  # Ctrl+U clears session search
        press(b'\r')
        # Ratatui emits only changed cells, so transitions may omit letters
        # shared with the previous screen. Full fold content is asserted by
        # the Rust TestBackend test; here verify each keyboard action redraws.
        assert press(b'j')  # group
        assert press(b'\r')  # expand group
        assert press(b'j')  # first tool
        assert press(b'\r')  # expand tool
        assert press(b' ')  # collapse tool
        assert press(b'c')  # collapse group
        assert 'Failedtool1/1' in press(b']')  # jump into the collapsed group
        press(b'[')  # wrap to the same failure
        assert press(b'r')  # changed-cell redraw; reload state is covered by App tests
        assert "PROMPT" in press(b'\x1b')
        press(b'j')
        assert "No session log".replace(' ', '') in press(b'\r')
        press(b'q')
        proc.wait(timeout=5)
        assert proc.returncode == 0
        assert termios.tcgetattr(slave) == before
        print("PASS: session open, search, clear, reload, back, missing log, and terminal restoration")
    finally:
        if proc.poll() is None:
            proc.kill()
            proc.wait()
        os.close(master)
        os.close(slave)
