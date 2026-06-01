use ratatui::buffer::Buffer;
use ratatui::layout::{Alignment, Rect};
use ratatui::style::{Modifier, Style};
use ratatui::text::Span;
use ratatui::widgets::{Block, Borders, Paragraph, Widget};

use crate::engine::current_step_index;
use crate::model::{Channel, ChannelKind, Pattern, Project};
use crate::ui::theme;

/// state the view reads to render. small + copyable on each frame.
pub struct ChannelsViewState<'a> {
    pub project: &'a Project,
    pub cursor_channel: usize,
    pub cursor_step: usize,
    pub playing: bool,
    pub sample_clock: u64,
    pub sample_rate: f32,
}

pub struct ChannelsView<'a> {
    pub state: ChannelsViewState<'a>,
}

impl<'a> Widget for ChannelsView<'a> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        let project = self.state.project;

        // outer block
        let block = Block::default()
            .borders(Borders::ALL)
            .border_style(Style::default().fg(theme::BORDER))
            .title(Span::styled(
                " channel rack ",
                Style::default().fg(theme::ACCENT).add_modifier(Modifier::BOLD),
            ));
        let inner = block.inner(area);
        block.render(area, buf);

        if project.channels.is_empty() {
            return;
        }

        let active_pattern = project.patterns.get(project.active_pattern as usize);
        let pattern_length = active_pattern.map(|p| p.length as usize).unwrap_or(16);
        let playhead = if self.state.playing {
            Some(current_step_index(
                self.state.sample_clock,
                project.bpm,
                self.state.sample_rate,
                pattern_length as u32,
            ))
        } else {
            None
        };

        let solo_any = project.channels.iter().any(|c| c.solo);

        // layout columns: [num 3] [name 12] [vol 5] [pan 3] [m/o 4] [steps fill]
        let num_w = 4;
        let name_w = 14;
        let vol_w = 5;
        let pan_w = 4;
        let ms_w = 5;
        let kind_w = 7;
        let meta_w = num_w + name_w + vol_w + pan_w + ms_w + kind_w;
        if inner.width as usize <= meta_w + 8 {
            // too tight, just draw a hint
            let p = Paragraph::new("terminal too narrow").style(Style::default().fg(theme::MUTED));
            p.render(inner, buf);
            return;
        }
        let steps_area_w = inner.width as usize - meta_w;
        // we always render the first 16 steps. distribute space across 16 + 3 gap chars (one per 4-group).
        let step_w = (steps_area_w.saturating_sub(3)) / 16;
        let step_w = step_w.clamp(2, 8);

        // header row inside inner area
        let mut x = inner.x;
        let y = inner.y;

        draw_header(buf, x, y, meta_w as u16, steps_area_w as u16, step_w as u16);

        let body_top = y + 2;
        let body_h = inner.height.saturating_sub(3);
        let n_channels = project.channels.len().max(1);
        // give each channel up to 3 rows of vertical space when the terminal
        // is tall, so fullscreen breathes instead of jamming everything at
        // the top.
        let row_h = ((body_h as usize) / n_channels).clamp(1, 3) as u16;
        let visible_count = (body_h / row_h.max(1)) as usize;
        let scroll = self.state.cursor_channel.saturating_sub(visible_count.saturating_sub(1));
        let visible = project
            .channels
            .iter()
            .enumerate()
            .skip(scroll)
            .take(visible_count);

        for (row_idx, (ch_idx, channel)) in visible.enumerate() {
            let row_y = body_top + (row_idx as u16) * row_h + (row_h.saturating_sub(1) / 2);
            let focused = ch_idx == self.state.cursor_channel;
            x = inner.x;
            draw_channel_row(
                buf,
                x,
                row_y,
                channel,
                ch_idx + 1,
                focused,
                num_w,
                name_w,
                vol_w,
                pan_w,
                ms_w,
                kind_w,
                solo_any,
            );

            // steps
            let steps_x_start = inner.x + meta_w as u16;
            if let Some(p) = active_pattern {
                draw_steps(
                    buf,
                    steps_x_start,
                    row_y,
                    step_w as u16,
                    ch_idx,
                    p,
                    focused,
                    self.state.cursor_step,
                    playhead,
                );
            }
        }

        // footer
        let footer_y = inner.y + inner.height - 1;
        let footer_text = format!(
            "pattern {} / {}  ·  {} channels  ·  cursor ch {} step {}{}",
            project.active_pattern + 1,
            project.patterns.len(),
            project.channels.len(),
            self.state.cursor_channel + 1,
            self.state.cursor_step + 1,
            if self.state.playing { "  ·  playing" } else { "" }
        );
        Paragraph::new(footer_text)
            .style(Style::default().fg(theme::MUTED))
            .alignment(Alignment::Left)
            .render(Rect::new(inner.x, footer_y, inner.width, 1), buf);

        // ignore _ vars
    }
}

fn draw_header(
    buf: &mut Buffer,
    x_start: u16,
    y: u16,
    _meta_w: u16,
    _steps_area_w: u16,
    step_w: u16,
) {
    let header_style = Style::default()
        .fg(theme::TEXT_DIM)
        .add_modifier(Modifier::DIM);
    let mut x = x_start;
    write(buf, x, y, " #  ", header_style);
    x += 4;
    write(buf, x, y, "channel       ", header_style);
    x += 14;
    write(buf, x, y, " vol ", header_style);
    x += 5;
    write(buf, x, y, "pan ", header_style);
    x += 4;
    write(buf, x, y, " m/o ", header_style);
    x += 5;
    write(buf, x, y, " kind  ", header_style);
    x += 7;

    // step numbers grouped 1-4 / 5-8 / 9-c / d-g
    let labels = ["1", "2", "3", "4", "5", "6", "7", "8", "9", "a", "b", "c", "d", "e", "f", "g"];
    for i in 0..16 {
        let label = labels[i];
        let cell = format_centered(label, step_w as usize);
        let style = if i % 4 == 0 {
            Style::default().fg(theme::ACCENT_DIM).add_modifier(Modifier::DIM)
        } else {
            Style::default().fg(theme::MUTED)
        };
        write(buf, x, y, &cell, style);
        x += step_w;
        if i == 3 || i == 7 || i == 11 {
            write(buf, x, y, " ", Style::default().fg(theme::BORDER));
            x += 1;
        }
    }

    // thin divider line under header
    let line_y = y + 1;
    let line: String = std::iter::repeat('─').take(buf.area.width as usize).collect();
    write(buf, 0, line_y, &line, Style::default().fg(theme::BORDER));
}

#[allow(clippy::too_many_arguments)]
fn draw_channel_row(
    buf: &mut Buffer,
    x_start: u16,
    y: u16,
    channel: &Channel,
    number: usize,
    focused: bool,
    _num_w: usize,
    name_w: usize,
    _vol_w: usize,
    _pan_w: usize,
    _ms_w: usize,
    _kind_w: usize,
    solo_any: bool,
) {
    let row_bg = if focused { theme::SURFACE_HI } else { theme::SURFACE };
    let row_fg = if channel.mute || (solo_any && !channel.solo) {
        theme::DIM
    } else if focused {
        theme::TEXT
    } else {
        theme::TEXT
    };

    let mut x = x_start;

    let pointer = if focused { "▸" } else { " " };
    write(
        buf,
        x,
        y,
        &format!(" {} {:>1} ", pointer, number),
        Style::default().fg(if focused { theme::ACCENT } else { theme::MUTED }).bg(row_bg),
    );
    x += 4;

    // name
    let name = truncate(&channel.name, name_w - 1);
    write(
        buf,
        x,
        y,
        &format!(" {:<width$}", name, width = name_w - 1),
        Style::default().fg(row_fg).bg(row_bg),
    );
    x += name_w as u16;

    // volume as -dB
    let db = volume_to_db(channel.volume);
    let vol_text = if db <= -60.0 {
        "  ─∞".to_string()
    } else {
        format!("{:>4.0}", db)
    };
    write(buf, x, y, &format!(" {} ", vol_text), Style::default().fg(theme::TEXT_DIM).bg(row_bg));
    x += 5;

    // pan
    let pan_text = pan_label(channel.pan);
    write(buf, x, y, &format!(" {} ", pan_text), Style::default().fg(theme::TEXT_DIM).bg(row_bg));
    x += 4;

    // mute / solo
    let m = if channel.mute { "M" } else { "·" };
    let s = if channel.solo { "S" } else { "·" };
    let m_style = Style::default()
        .fg(if channel.mute { theme::HOT } else { theme::DIM })
        .bg(row_bg);
    let s_style = Style::default()
        .fg(if channel.solo { theme::COOL } else { theme::DIM })
        .bg(row_bg);
    write(buf, x, y, " ", Style::default().bg(row_bg));
    write(buf, x + 1, y, m, m_style);
    write(buf, x + 2, y, " ", Style::default().bg(row_bg));
    write(buf, x + 3, y, s, s_style);
    write(buf, x + 4, y, " ", Style::default().bg(row_bg));
    x += 5;

    // kind label
    let kind_text = match &channel.kind {
        ChannelKind::DrumSynth(k) => k.label(),
        ChannelKind::Synth(p) => p.osc.label(),
        ChannelKind::Sampler(_) => "sample",
        ChannelKind::MidiOut(_) => "midi",
    };
    write(
        buf,
        x,
        y,
        &format!(" {:<6}", truncate(kind_text, 6)),
        Style::default().fg(theme::MUTED).bg(row_bg),
    );
}

#[allow(clippy::too_many_arguments)]
fn draw_steps(
    buf: &mut Buffer,
    x_start: u16,
    y: u16,
    step_w: u16,
    ch_idx: usize,
    pattern: &Pattern,
    focused: bool,
    cursor_step: usize,
    playhead: Option<usize>,
) {
    let track = pattern.tracks.get(&(ch_idx as u16));
    let mut x = x_start;
    // active glyph fills the cell width with 1 char of side padding so wide
    // cells at fullscreen don't look like single dots floating in space.
    let fill_w = step_w.saturating_sub(2).max(1) as usize;
    let active_glyph: String = std::iter::repeat('█').take(fill_w).collect();
    let inactive_glyph: String = std::iter::repeat('·').take(fill_w).collect();
    for i in 0..16 {
        let active = track.and_then(|t| t.steps.get(i)).map(|s| s.active).unwrap_or(false);
        let is_cursor = focused && i == cursor_step;
        let is_playing = playhead == Some(i);
        let glyph = if active { active_glyph.as_str() } else { inactive_glyph.as_str() };
        let cell = format_centered(glyph, step_w as usize);

        let beat_emphasis = i % 4 == 0;
        let mut fg = if active {
            if beat_emphasis {
                theme::ACCENT_HI
            } else {
                theme::ACCENT
            }
        } else if beat_emphasis {
            theme::TEXT_DIM
        } else {
            theme::DIM
        };
        let mut bg = theme::SURFACE;
        if is_playing {
            bg = if active { theme::ACCENT_DIM } else { theme::DIM };
            fg = theme::PLAYHEAD;
        }
        if is_cursor {
            bg = theme::SURFACE_HI;
            if !active {
                fg = theme::COOL;
            }
        }
        let style = Style::default().fg(fg).bg(bg);
        write(buf, x, y, &cell, style);
        x += step_w;
        if i == 3 || i == 7 || i == 11 {
            write(buf, x, y, " ", Style::default().fg(theme::BORDER).bg(theme::SURFACE));
            x += 1;
        }
    }
}

fn write(buf: &mut Buffer, x: u16, y: u16, s: &str, style: Style) {
    if y >= buf.area.bottom() {
        return;
    }
    let max_x = buf.area.right();
    let mut cx = x;
    for ch in s.chars() {
        if cx >= max_x {
            break;
        }
        buf.get_mut(cx, y).set_char(ch).set_style(style);
        cx += 1;
    }
}

fn format_centered(s: &str, w: usize) -> String {
    let len = s.chars().count();
    if len >= w {
        return s.chars().take(w).collect();
    }
    let pad = w - len;
    let left = pad / 2;
    let right = pad - left;
    let mut out = String::with_capacity(w);
    out.extend(std::iter::repeat(' ').take(left));
    out.push_str(s);
    out.extend(std::iter::repeat(' ').take(right));
    out
}

fn truncate(s: &str, w: usize) -> String {
    if s.chars().count() <= w {
        return s.to_string();
    }
    s.chars().take(w.saturating_sub(1)).chain(std::iter::once('…')).collect()
}

fn volume_to_db(v: f32) -> f32 {
    if v <= 0.0001 {
        -120.0
    } else {
        20.0 * v.log10()
    }
}

fn pan_label(p: f32) -> String {
    if p.abs() < 0.05 {
        "──".into()
    } else if p > 0.0 {
        format!("{:>2.0}R", (p * 100.0).abs())
    } else {
        format!("{:>2.0}L", (p * 100.0).abs())
    }
}
