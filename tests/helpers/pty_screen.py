"""Tiny cell-buffer reader for the ANSI cursor updates emitted by Ratatui."""

import unicodedata


def screen_text(raw, columns=80, rows=24):
    screen = [[" "] * columns for _ in range(rows)]
    x = y = 0
    i = 0
    while i < len(raw):
        if raw[i : i + 1] == b"\x1b" and raw[i + 1 : i + 2] == b"[":
            j = i + 2
            while j < len(raw) and not 0x40 <= raw[j] <= 0x7E:
                j += 1
            if j == len(raw):
                break
            params = raw[i + 2 : j].decode(errors="ignore")
            final = chr(raw[j])
            clean = params.lstrip("?")
            if final in ("H", "f"):
                values = clean.split(";")
                y = max(0, (int(values[0]) if values[0] else 1) - 1)
                x = max(0, (int(values[1]) if len(values) > 1 and values[1] else 1) - 1)
            elif final == "J" and clean in ("2", "3"):
                screen = [[" "] * columns for _ in range(rows)]
            elif final == "K" and y < rows:
                for col in range(min(x, columns), columns):
                    screen[y][col] = " "
            elif final == "h" and "1049" in params:
                screen = [[" "] * columns for _ in range(rows)]
            i = j + 1
            continue
        byte = raw[i]
        if byte == 13:
            x = 0
            i += 1
            continue
        if byte == 10:
            y = min(rows - 1, y + 1)
            i += 1
            continue
        if byte < 32 or byte == 127:
            i += 1
            continue
        if byte >= 128:
            end = i + 1
            while end < len(raw) and raw[end] & 0xC0 == 0x80:
                end += 1
            char = raw[i:end].decode("utf-8", errors="replace")
            i = end
        else:
            char = chr(byte)
            i += 1
        if char and y < rows and x < columns:
            width = 0 if unicodedata.combining(char[0]) else (
                2 if unicodedata.east_asian_width(char[0]) in ("W", "F") else 1
            )
            if width:
                screen[y][x] = char[0]
                if width == 2 and x + 1 < columns:
                    screen[y][x + 1] = ""
            elif x > 0:
                screen[y][x - 1] += char[0]
            x += width
    return "\n".join("".join(line) for line in screen)
