"""Small VT screen decoder for the Crossterm sequences used by our PTY fixtures."""
import base64
import codecs
import re
import unicodedata


class Screen:
    def __init__(self, width, height):
        self.width, self.height = width, height
        self.cells = [[' '] * width for _ in range(height)]
        self.x = self.y = 0
        self.pending = ''
        self.clipboard = []
        self.decoder = codecs.getincrementaldecoder('utf-8')('replace')

    def feed(self, data):
        self.pending += self.decoder.decode(data)
        index = 0
        while index < len(self.pending):
            char = self.pending[index]
            if char == '\x1b':
                if index + 1 >= len(self.pending):
                    break
                if self.pending[index + 1] == ']':
                    ends = [(self.pending.find(end, index + 2), len(end)) for end in ('\x07', '\x1b\\')]
                    ends = [(position, size) for position, size in ends if position >= 0]
                    if not ends:
                        break
                    end, size = min(ends)
                    value = self.pending[index + 2:end]
                    if value.startswith('52;'):
                        encoded = value.split(';', 2)[2]
                        if encoded != '?':
                            self.clipboard.append(base64.b64decode(encoded, validate=True).decode('utf-8'))
                    index = end + size
                    continue
                if self.pending[index + 1] == '[':
                    match = re.match(r'\x1b\[([0-?]*)([ -/]*)([@-~])', self.pending[index:])
                    if match is None:
                        break
                    self.csi(match[1], match[3])
                    index += len(match[0])
                    continue
                if self.pending[index + 1] in '()':
                    if index + 2 >= len(self.pending):
                        break
                    index += 3
                else:
                    index += 2
                continue
            if char == '\r':
                self.x = 0
            elif char == '\n':
                self.y = min(self.height - 1, self.y + 1)
            elif char == '\b':
                self.x = max(0, self.x - 1)
            elif char >= ' ' and char != '\x7f':
                width = 0 if unicodedata.combining(char) else (2 if unicodedata.east_asian_width(char) in 'WF' else 1)
                if width == 0:
                    if self.x > 0:
                        self.cells[self.y][min(self.x - 1, self.width - 1)] += char
                else:
                    if self.x >= self.width:
                        self.x = 0
                        self.y = min(self.height - 1, self.y + 1)
                    self.cells[self.y][self.x] = char
                    if width == 2 and self.x + 1 < self.width:
                        self.cells[self.y][self.x + 1] = ''
                    self.x += width
            index += 1
        self.pending = self.pending[index:]

    def csi(self, parameters, command):
        if parameters.startswith('?'):
            if parameters == '?1049' and command == 'h':
                self.cells = [[' '] * self.width for _ in range(self.height)]
                self.x = self.y = 0
            return
        try:
            values = [int(value or '0') for value in parameters.split(';')]
        except ValueError:
            return
        amount = values[0] or 1
        if command in 'Hf':
            self.y = min(self.height - 1, max(0, (values[0] or 1) - 1))
            self.x = min(self.width - 1, max(0, ((values[1] if len(values) > 1 else 1) or 1) - 1))
        elif command == 'G': self.x = min(self.width - 1, amount - 1)
        elif command == 'A': self.y = max(0, self.y - amount)
        elif command == 'B': self.y = min(self.height - 1, self.y + amount)
        elif command == 'C': self.x = min(self.width - 1, self.x + amount)
        elif command == 'D': self.x = max(0, self.x - amount)
        elif command == 'J':
            if values[0] in (2, 3): self.cells = [[' '] * self.width for _ in range(self.height)]
            elif values[0] == 0:
                self.cells[self.y][self.x:] = [' '] * (self.width - self.x)
                for row in range(self.y + 1, self.height): self.cells[row] = [' '] * self.width
        elif command == 'K':
            start, end = (0, self.width) if values[0] == 2 else ((0, min(self.width, self.x + 1)) if values[0] == 1 else (self.x, self.width))
            self.cells[self.y][start:end] = [' '] * (end - start)

    def compact(self):
        return ''.join(''.join(''.join(row) for row in self.cells).split())
