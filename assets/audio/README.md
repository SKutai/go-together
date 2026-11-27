# Original marimba assets

The WAV files in this directory are original, project-specific assets generated
for Go Together. They contain no downloaded or third-party samples. The
generator and generated WAV files are distributed with the rest of the project
under the repository's [MIT License](../../LICENSE), Copyright (c) 2026 SKutai.

Regenerate all notes from the repository root:

```sh
python3 tools/generate_marimba.py
```

Verify that the committed files exactly match deterministic generator output
without changing them:

```sh
python3 tools/generate_marimba.py --check
```

The deterministic generator uses additive synthesis with inharmonic wooden-bar
modes, seeded noise transients, and click-free attack/release envelopes. The 11
marimba notes are 0.46 seconds each. `opening_duplicate_clack.wav` is a
0.18-second pair of close wooden impacts used only for a duplicate Round 0
selection. Every file is 44.1 kHz, 16-bit mono, and normalized to a 0.78 peak.
Cue gain is applied in code, with lower per-note gain for simultaneous
harmonies.
