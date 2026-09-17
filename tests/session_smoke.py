"""Linux PTY integration check. Run after cargo build: python3 tests/session_smoke.py."""
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
    proc = subprocess.Popen([executable, "--file", str(history), "--clipboard", "terminal"], stdin=slave, stdout=slave, stderr=slave, env=dict(os.environ, TERM="xterm-256color"))

    screen = Screen(120, 36)

    def read_screen():
        until = time.monotonic() + 0.3
        while time.monotonic() < until:
            if select.select([master], [], [], 0.05)[0]:
                screen.feed(os.read(master, 65536))
                while screen.responses:
                    os.write(master, screen.responses.pop(0))
        return screen.compact()

    def wait_for(text, timeout=5):
        until = time.monotonic() + timeout
        while time.monotonic() < until:
            rendered = read_screen()
            if text in rendered:
                return rendered
        raise AssertionError(f"Missing {text}: {screen.compact()}")

    def press(key):
        os.write(master, key)
        return read_screen()

    try:
        assert "PROMPTHISTORY" in wait_for("Historyreloaded")
        assert "Sortorder" in press(b'O')
        press(b'\x1b[F'); press(b'\r')
        assert "smallestsessions" in screen.compact()
        press(b'O'); press(b'\x1b[H'); press(b'\r')  # restore default ordering
        assert "Choosesource" in press(b'T')
        press(b'\x1b[B'); press(b'\r')
        assert f'Source:{root.name}[t]' in screen.compact()
        press(b'z')
        assert 'Source:all[t]' in screen.compact()
        assert 'Matchingrecords:2/2loaded' in press(b'i')
        press(b'\x1b[F')
        assert "excludesonlythelist'sdatefilter." in screen.compact()
        press(b'\x1b')
        assert "Enterapply" in press(b'\x06')  # Ctrl+F
        press(b'\x1b[200~Hello\x1b[201~')
        assert "1/2records" in screen.compact()
        press(b'\x1b')  # cancel restores the original empty query
        assert "2/2records" in screen.compact()
        press(b'/HellX')
        assert "Keyboardshortcuts" in press(b'\x1bOP')  # help while editing
        press(b'\x1b')  # close help, keeping the draft and caret
        press(b'\x1b[D\x1b[3~o')  # left, Delete, insert o -> Hello
        press(b'\r')
        assert "Search:Hello[x]" in screen.compact()
        press(b'/\x15')  # clear the draft
        assert "Search:Hello[x]" in press(b'\x1b[A')  # recent query
        assert "2/2records" in press(b'\x1b[B')  # restore unfinished draft
        assert "Search:Hello[x]" in press(b'\x1b')  # cancel to confirmed query
        press(b'x')
        copied = len(screen.clipboard)
        press(b'\x1b[200~yYCq\x1b[201~')  # paste outside search invokes nothing
        assert proc.poll() is None and len(screen.clipboard) == copied
        assert "Keyboardshortcuts" in press(b'\x1bOP')  # F1
        assert "Searchhistorystaysinmemory." in press(b'\x1b[F')  # help End
        press(b'\x1b')
        press(b'y'); wait_for("Copyrequestsentforrecordcontent")
        assert screen.clipboard[-1] == "Hello session"
        press(b'Y'); wait_for("CopyrequestsentforsessionID")
        assert screen.clipboard[-1] == "demo"
        assert press(b'a')  # activity focus
        assert press(b'g')  # beginning of the default 30-day range
        assert 'Nomatchingprompts' in press(b'\r')  # fixture records are from 1970
        assert press(b'd')  # clear date filter, retaining all history
        assert press(b'w')  # all-time chart
        assert press(b'a')
        assert press(b'g')  # exact date of fixture records
        assert press(b'\r')  # filter that date
        assert press(b'd')
        assert press(b't')  # source filter for the single loaded source
        assert press(b'\x1b')  # clear source filter before opening the session
        press(b'\r')
        rendered = wait_for("SESSIONHISTORY")
        assert "Timeline" in rendered and "demo-model" in rendered and "ASSISTANT" in rendered and "Toolactivity" in rendered, rendered
        assert 'Matchingrecords:4/4loaded' in press(b'i')
        assert 'Toolcalls:2' in screen.compact() and 'Failed:1' in screen.compact()
        press(b'\x1b')
        press(b'/')
        assert "Search:Hello[x]" in press(b'\x1b[A')  # shared query history
        press(b'\x1b')
        assert press(b'\t')  # detail focus
        assert press(b'\x1b[6~')  # page down in the preview
        assert press(b'g')  # first preview line
        assert press(b'\t')  # list focus; selection is unchanged
        press(b'fPreview line')
        assert "1/40matches" in screen.compact()
        assert "4/4records" in screen.compact()  # detail find does not filter the timeline
        press(b'\r')
        assert "2/40matches" in press(b'n')
        assert "1/40matches" in press(b'N')
        assert "40/40matches" in press(b'N')  # wraps
        press(b'f\x15missing detail text')
        assert "0/0matches" in screen.compact()
        assert "40/40matches" in press(b'\x1b')  # cancel restores the prior find
        press(b'F')
        assert "Find:Previewline" not in screen.compact()
        press(b'\t')  # return to list focus
        assert "Nomatchingsessionentries" in press(b'/no-such-text')
        press(b'\x15')  # Ctrl+U clears session search
        press(b'\r')
        # Decode changed cells into a full screen before checking async updates.
        assert press(b'j')  # group
        assert press(b'\r')  # expand group
        press(b'Y'); wait_for('CopyrequestsentforsessionID')
        assert screen.clipboard[-1] == 'demo'  # group headers still have a real session ID
        assert press(b'j')  # first tool
        press(b'C'); wait_for('Copyrequestsentfortoolcommand')
        assert screen.clipboard[-1] == 'echo hello\nwhoami'
        assert press(b'\r')  # expand tool
        assert press(b' ')  # collapse tool
        assert press(b'c')  # collapse group
        assert 'Failedtool1/1' in press(b']')  # jump into the collapsed group
        press(b'[')  # wrap to the same failure
        press(b'r')
        wait_for("Sessionreloaded")
        assert "PROMPT" in press(b'\x1b')
        press(b'\r')
        wait_for("Sessionloadedfrommemorycache")
        press(b'\x1b')
        # A FIFO deliberately stalls background I/O without relying on file size
        # or CPU speed. The UI must still handle help and cancellation.
        original = history.read_bytes()
        history.unlink()
        os.mkfifo(history)
        press(b'r')
        wait_for("Openinghistoryfiles")
        assert "Keyboardshortcuts" in press(b'?')
        press(b'\x1b')  # dismiss help
        press(b'\x1b')  # cancel load while its OS read is blocked
        wait_for("Loadingcancelled")
        writer = os.open(history, os.O_WRONLY | os.O_NONBLOCK)
        try:
            os.write(writer, b'{"session_id":"stale","ts":99,"text":"stale payload"}\n')
        finally:
            os.close(writer)
        history.unlink()
        history.write_bytes(original)
        assert "stalepayload" not in read_screen()
        assert "Loadingcancelled" in screen.compact()
        press(b'r')
        wait_for("Historyreloaded")
        press(b'j')
        press(b'\r')
        wait_for("Nosessionlog")
        press(b'q')
        proc.wait(timeout=5)
        assert proc.returncode == 0
        assert termios.tcgetattr(slave) == before
        print("PASS: session navigation, cache hit, background I/O responsiveness, cancellation, stale-result rejection, and terminal restoration")
    finally:
        if proc.poll() is None:
            proc.kill()
            proc.wait()
        os.close(master)
        os.close(slave)
