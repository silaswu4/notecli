# noteCLI

fl-studio-style daw that lives in your terminal. channel rack, piano roll, playlist, mixer, sample browser. it synthesizes its own drums and synth voices in real time, loads your wav files, drives external vsts over virtual midi, and can hand a vibe to claude and get back a pattern.

ratatui for the ui, cpal for audio, written in rust. one binary, no electron, no node, no daw window. you run `cargo run --release`, press space, and it makes sound.

## why

i wanted to write a sequencer where the interesting part is real: a lock-free audio engine that renders procedural drums and a subtractive synth from scratch, never allocating or locking on the audio thread, while a full tui sits on top of it. it isn't a wrapper around someone else's audio backend. the oscillators, the adsr, the biquad filter, the schroeder reverb, the voice allocation, and the loop-accurate step timing are all in `src/`. the claude integration is a nice toy on top of an engine that stands on its own.

## what works

- channel rack with 16-step sequencer per channel (the fl signature)
- multiple patterns, instant switching
- piano roll editor for melodic channels
- playlist arrangement (chain patterns into a song)
- mixer with per-channel volume / pan / mute / solo and a master strip
- sample browser, navigates your filesystem and loads wav files into channels
- built-in drum synth: kick, snare, hat, clap, tom
- built-in subtractive synth: sine, triangle, saw, square, noise with adsr + lowpass
- sample player with linear-interpolation pitch shift
- midi input from the first connected device
- midi out via a virtual port called `noteCLI-out`, can drive vital or any vst host
- auto-launches a configured standalone synth (defaults to vital) on startup
- schroeder reverb on a global send
- claude agent for one-shot pattern generation (`g`) and 3-variation generation (`G`)
- mouse support: click steps to toggle, click tabs to switch views, scroll to navigate
- save / load projects as toml

## stack

cpal for audio, ratatui + crossterm for the tui, hound for wav, midir for midi, arc-swap + ringbuf for the audio↔ui channel, serde + toml for persistence, ureq for the claude api calls. one binary, no electron, no node.

## install + run

needs a rust toolchain (`rustup`) and, on linux, alsa dev headers (`libasound2-dev`) for cpal and midir. macos works out of the box.

run it straight from the repo:

```bash
cargo run --release
```

or build an optimized binary and install it on your path:

```bash
cargo build --release        # target/release/notecli
cargo install --path .       # then just run: notecli
```

release builds use `lto = "thin"` and a single codegen unit, since this is realtime audio and the engine should be as tight as the optimizer can make it.

first launch starts with a default project: 5 drum channels, 2 synth channels, one pattern with a basic four-on-the-floor on the kick + snare on 2 / 4 + hat on the off-eighths. press space and you should hear it. headphones recommended, the kick punches.

## keys

global:

| key | what it does |
|-----|--------------|
| `space` | play / pause |
| `S` | stop and reset playhead |
| `1` `2` `3` `4` `5` | switch view: channels / piano / playlist / mixer / browser |
| `q` | quit |
| `,` `.` | bpm down / up |
| `ctrl-s` | save project to `<name>.notecli.toml` |

channel rack:

| key | what it does |
|-----|--------------|
| `hjkl` | navigate cursor |
| `x` `enter` | toggle the step at the cursor |
| `c` | clear the focused channel's row |
| `m` | mute the focused channel |
| `o` | solo the focused channel |
| `+` `-` | channel volume up / down |
| `[` `]` | previous / next pattern |
| `n` | new pattern |
| `a` | append a new synth channel |
| `tab` | cycle the focused channel's kind (drum → synth → midi-out → drum) |
| `p` | preview the focused channel |
| `g` | open the agent prompt modal (generates a pattern) |
| `G` | open the variation modal (generates 3 variations) |

piano roll:

| key | what it does |
|-----|--------------|
| `hjkl` | navigate cursor (h/l = tick, j/k = pitch) |
| `a` | add a note at the cursor |
| `d` | delete the note under the cursor |

playlist:

| key | what it does |
|-----|--------------|
| `hjkl` | navigate bar |
| `enter` | place the currently active pattern at the cursor bar |
| `d` | clear the block at the cursor |

mixer:

| key | what it does |
|-----|--------------|
| `hl` | move strip cursor (last strip is master) |
| `+` `-` | volume |
| `<` `>` | pan |
| `m` `o` | mute / solo |

sample browser:

| key | what it does |
|-----|--------------|
| `hjkl` `enter` | navigate the filesystem, h goes up |
| `enter` | open a directory, or load a wav into the focused channel |
| `space` | preview message (audition routing tbd) |

modal input (`g` / `G` for agent):

| key | what it does |
|-----|--------------|
| `enter` | send |
| `esc` | cancel |
| `backspace` | delete last char |

## using your own samples

drop any wav into a directory you can find, open the browser with `5`, navigate to it, press `enter` on a `.wav` file. it loads into the focused channel as a sampler. step trigger plays the sample, and the pitch can be shifted in semitones with the sampler's `pitch_semitones` field (currently editable via project toml).

`hound` only reads wav for v1. mp3 / flac / ogg can be added by swapping the loader for `symphonia` later.

## driving serum / vital / any vst

noteCLI doesn't host vsts (vsts need a gui window handle that terminals can't provide). instead it opens a virtual midi port called `noteCLI-out` on launch, and on macos it'll also try to `open -a Vital` so the standalone vital app pops up alongside it.

routing:

1. with both apps running, in vital's settings find midi input and select `noteCLI-out`.
2. in noteCLI, cycle a channel to `midi` kind by pressing `tab` on it. each step on that channel sends a note on midi channel 1 to the virtual port. vital receives, plays its current patch.
3. if you'd rather use serum (or massive x, or anything else), edit the channel's `launch_app` field in the saved `.notecli.toml` and it'll launch that on next start.

the engine tracks the last note sent per channel and releases it before the next hit, so notes never stack up. transport stop also sends note-off to every ringing midi note.

## the claude agent

press `g` on the channel rack view. a modal opens. type a vibe (`"dusty boom-bap at 86"`, `"garage house with skip on hat"`, whatever) and hit enter. noteCLI sends the description plus the current channel list to claude sonnet 4.6 and applies the returned step grid to the active pattern. mentioning a tempo in the prompt updates the bpm.

press `G` (shift+g) for variations. an optional direction hint modal opens (or just hit enter to skip). the agent returns 3 musically related but distinct variations and appends them as new patterns named `agent 01`, `agent 02`, `agent 03`. flip through them with `[` / `]`.

requires `ANTHROPIC_API_KEY` in your env. the call takes ~3-5 seconds; the status bar shows `thinking…` while in flight and the ui keeps responding because the request runs on a worker thread.

## pattern wrap-cut

every time the playhead wraps from step 15 back to step 0, the engine kills every active voice and releases every midi note. nothing carries over between loop iterations, so synth tails, drum decays, and held midi notes all reset on the bar line. this prevents voices from stacking up over long loops.

## project file format

projects save as toml. a stripped-down example:

```toml
name = "untitled"
bpm = 120.0
master_volume = 0.8
active_pattern = 0
playlist = [0]

[[channels]]
name = "kick"
volume = 0.8
pan = 0.0
mute = false
solo = false

[channels.kind]
type = "DrumSynth"
content = "Kick"
```

## architecture

```
ui thread (main)            audio thread (cpal)            midi-out thread
─────────────────           ───────────────────            ───────────────
project mut ─┐                       │                            │
             ├──► ArcSwap<Project> ──┤                            │
             │                       │                            │
keypresses ──┘                       │                            │
             ┌─► ringbuf <Command> ──┤                            │
mouse ───────┤                       │                            │
midi input ──┘                       │                            │
                                     │                            │
                              engine renders:                     │
                              · pulls latest project              │
                              · drains commands                   │
                              · per frame: tick check,            │
                                step events, voices               │
                              · midi-out channels:                │
                                push to midi ringbuf ─────────────┤
                              · accumulator → reverb              │
                                send → soft clip                  │
                                                                  │
                                                          midir sends bytes
                                                          to noteCLI-out
                                                          virtual port
```

the audio thread never allocates, never locks. project mutations are arc-cloned and atomically swapped by the ui thread. immediate triggers (transport, audition, midi notes) cross via ringbuf. agent calls run on their own worker threads to keep the ui responsive.

## directory layout

```
src/
├── main.rs              entry, cpal setup, terminal lifecycle, app launching
├── transport.rs         shared atomic transport state
├── command.rs           ui → audio messages
├── model.rs             project / channel / pattern / step / note
├── sample.rs            wav loading via hound
├── voice.rs             enum-dispatched voice (drum / synth / sampler)
├── engine.rs            the audio engine itself
├── midi.rs              midi input
├── midi_out.rs          virtual midi output port + worker thread
├── agent.rs             claude api integration for pattern generation
├── dsp/
│   ├── osc.rs           polyblep oscillators + noise
│   ├── env.rs           adsr
│   ├── filter.rs        rbj biquad lowpass
│   ├── drums.rs         procedural kick / snare / hat / clap / tom
│   └── reverb.rs        schroeder reverb
└── ui/
    ├── app.rs           main app state + key dispatch + agent modal
    ├── theme.rs         palette
    └── views/
        ├── channels.rs  channel rack
        ├── piano.rs     piano roll
        ├── playlist.rs  arrangement
        ├── mixer.rs     mixer strips
        └── browser.rs   sample browser
```

## what's missing

- per-step velocity / pitch editing in the channel rack ui
- automation lanes
- audio file playback in the playlist (currently only patterns)
- multi-format sample loading (mp3 / flac / ogg)
- ui editor for midi-out channel / pitch (currently set via toml)
- vst / clap hosting (probably never; midi out is the real path)
