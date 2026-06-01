# tek

fl-studio-style daw that lives in your terminal. ratatui ui, cpal audio, built-in drum synth + subtractive synth, wav sample loading.

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
- schroeder reverb on a global send
- save / load projects as toml

## stack

cpal for audio, ratatui + crossterm for the tui, hound for wav, midir for midi, arc-swap + ringbuf for the audio↔ui channel, serde + toml for persistence. one binary, no electron, no node.

## install + run

```bash
cargo run --release
```

first launch starts with a default project: 5 drum channels, 2 synth channels, a single pattern with a basic four-on-the-floor on the kick + snare on 2 / 4 + hat on the off-eighths. press space and you should hear it. headphones recommended, the kick punches.

## keys

global:

| key | what it does |
|-----|--------------|
| `space` | play / pause |
| `S` | stop and reset playhead |
| `1` `2` `3` `4` `5` | switch view: channels / piano / playlist / mixer / browser |
| `q` | quit |
| `,` `.` | bpm down / up |
| `ctrl-s` | save project to `<name>.tek.toml` |

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
| `tab` | cycle the focused channel's kind (drum / synth) |
| `p` | preview the focused channel |

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

## using your own samples

put any wav into a directory you can find, open the browser with `5`, navigate to it, press `enter` on a `.wav` file. it loads into the focused channel as a sampler. step trigger plays the sample, and the pitch can be shifted in semitones with the sampler's `pitch_semitones` field (currently editable via project toml).

`hound` only reads wav for v1. mp3 / flac / ogg can be added by swapping the loader for `symphonia` later.

## driving serum / vital / any vst

terminals can't host vst3 plugins (vsts need a gui window handle). the workable path is to drive your real daw or a standalone host from tek over midi:

1. on macos, open audio midi setup → midi studio → enable the iac driver and create a bus (name it whatever, "tek bus" works).
2. open logic / ableton / bitwig / studio one with serum or vital loaded on a track, set that track's midi input to the iac bus.
3. start tek. by default it grabs the first available midi input device, but for output to serum the plan is a `ChannelKind::MidiOut` variant that the engine sends notes to instead of triggering an internal voice. that variant lands in the next pass.

immediate workaround until midi out lands: use the built-in synth + drums for the rhythm scratch and route audio out of tek into your interface, then play serum manually over the top from your daw.

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
ui thread (main)            audio thread (cpal)
─────────────────           ───────────────────
project mut ─┐                       │
             ├──► ArcSwap<Project> ──┤
             │                       │
keypresses ──┘                       │
             ┌─► ringbuf <Command> ──┤
midi input ──┘                       │
                                     │
                              engine renders:
                              · pulls latest project snapshot
                              · drains commands
                              · per frame: check tick boundary,
                                fire step events, render voices
                              · stereo accumulator → reverb send
                                → soft clip → master out
```

the audio thread never allocates, never locks. project mutations are arc-cloned and atomically swapped by the ui thread. immediate triggers (transport, audition, midi notes) cross via the ringbuf.

## directory layout

```
src/
├── main.rs              entry, cpal setup, terminal lifecycle
├── transport.rs         shared atomic transport state
├── command.rs           ui → audio messages
├── model.rs             project / channel / pattern / step / note
├── sample.rs            wav loading via hound
├── voice.rs             enum-dispatched voice (drum / synth / sampler)
├── engine.rs            the audio engine itself
├── midi.rs              midi input
├── dsp/
│   ├── osc.rs           polyblep oscillators + noise
│   ├── env.rs           adsr
│   ├── filter.rs        rbj biquad lowpass
│   ├── drums.rs         procedural kick / snare / hat / clap / tom
│   └── reverb.rs        schroeder reverb
└── ui/
    ├── app.rs           main app state + key dispatch
    ├── theme.rs         palette
    └── views/
        ├── channels.rs  channel rack
        ├── piano.rs     piano roll
        ├── playlist.rs  arrangement
        ├── mixer.rs     mixer strips
        └── browser.rs   sample browser
```

## what's missing

- midi-out (planned, see above)
- pattern length other than 16 steps
- per-step velocity / pitch editing in the channel rack ui
- automation lanes
- audio file playback in the playlist (currently only patterns)
- multi-format sample loading (mp3 / flac / ogg)
- vst / clap hosting (probably never; midi out is the real path)
