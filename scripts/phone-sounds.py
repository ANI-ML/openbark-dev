"""Write the phone window's sounds (ADR-0023 section 4).

Usage: python scripts/phone-sounds.py

Twilio's Voice SDK plays a few short sounds: a chime when a call connects,
one when it ends, the standard keypad tones, and a ring for incoming calls.
By default it fetches them from sdk.twilio.com. OpenBark gives it these local
files instead (apps/desktop/src/phone/voice.ts), so the phone window contacts
no sound host. They are plain tones made here: 8 kHz, 16-bit, mono WAV.
"""
import math
import os
import struct
import wave

OUT = os.path.join(os.path.dirname(os.path.abspath(__file__)), "..", "apps", "desktop", "public", "phone-sounds")
RATE = 8000

# The standard keypad (DTMF) frequency pairs.
DTMF = {
    "1": (697, 1209), "2": (697, 1336), "3": (697, 1477),
    "4": (770, 1209), "5": (770, 1336), "6": (770, 1477),
    "7": (852, 1209), "8": (852, 1336), "9": (852, 1477),
    "star": (941, 1209), "0": (941, 1336), "hash": (941, 1477),
}


def tone(freqs, secs, volume=0.25):
    n = int(RATE * secs)
    fade = int(RATE * 0.01)
    out = []
    for i in range(n):
        v = sum(math.sin(2 * math.pi * f * i / RATE) for f in freqs) / len(freqs)
        edge = min(1.0, i / fade, (n - 1 - i) / fade) if fade else 1.0
        out.append(v * volume * max(0.0, edge))
    return out


def silence(secs):
    return [0.0] * int(RATE * secs)


def write(name, samples):
    os.makedirs(OUT, exist_ok=True)
    with wave.open(os.path.join(OUT, name), "wb") as w:
        w.setnchannels(1)
        w.setsampwidth(2)
        w.setframerate(RATE)
        w.writeframes(b"".join(struct.pack("<h", int(max(-1, min(1, s)) * 32767)) for s in samples))


def main():
    for key, freqs in DTMF.items():
        write(f"dtmf-{key}.wav", tone(freqs, 0.16))
    # Connected: two short rising notes.
    write("outgoing.wav", tone([660], 0.12) + silence(0.04) + tone([880], 0.16))
    # Ended: two short falling notes.
    write("disconnect.wav", tone([660], 0.12) + silence(0.04) + tone([440], 0.2))
    # Incoming (a later release): a soft North American ring, two seconds on, played in a loop.
    write("incoming.wav", tone([440, 480], 2.0, 0.2) + silence(1.0))


if __name__ == "__main__":
    main()
