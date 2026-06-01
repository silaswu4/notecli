use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};
use ratatui::text::Span;
use ratatui::widgets::{Block, Borders, Paragraph, Widget};

use crate::model::Project;
use crate::ui::theme;

pub struct PianoView<'a> {
    pub project: &'a Project,
    pub cursor_channel: usize,
    pub cursor_tick: u32,
    pub cursor_pitch: u8,
}

impl<'a> Widget for PianoView<'a> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        let block = Block::default()
            .borders(Borders::ALL)
            .border_style(Style::default().fg(theme::BORDER))
            .title(Span::styled(
                " piano roll ",
                Style::default().fg(theme::ACCENT).add_modifier(Modifier::BOLD),
            ));
        let inner = block.inner(area);
        block.render(area, buf);

        let channel = self.project.channels.get(self.cursor_channel);
        let pattern = self.project.patterns.get(self.project.active_pattern as usize);
        let (Some(channel), Some(pattern)) = (channel, pattern) else {
            return;
        };

        let info = format!(
            "channel: {}    pattern: {}    cursor: pitch {} tick {}",
            channel.name,
            pattern.name,
            note_name(self.cursor_pitch),
            self.cursor_tick,
        );
        Paragraph::new(info)
            .style(Style::default().fg(theme::TEXT_DIM))
            .render(Rect::new(inner.x, inner.y, inner.width, 1), buf);

        // grid area
        let grid = Rect::new(inner.x, inner.y + 2, inner.width, inner.height.saturating_sub(3));
        if grid.width < 12 || grid.height < 4 {
            return;
        }

        let key_w = 4u16;
        let body_w = grid.width.saturating_sub(key_w);
        let pitch_lo = (self.cursor_pitch as i32 - grid.height as i32 / 2).max(0) as u8;
        let pitches = (0..grid.height as u8).map(|i| {
            let p = pitch_lo as i32 + (grid.height as i32 - 1 - i as i32);
            p.clamp(0, 127) as u8
        });

        let length = pattern.length.max(1);
        let total_ticks = length * crate::model::TICKS_PER_STEP;
        let tick_per_col = (total_ticks as f32 / body_w as f32).max(1.0);

        let track = pattern.tracks.get(&(self.cursor_channel as u16));

        for (row, pitch) in pitches.enumerate() {
            let y = grid.y + row as u16;
            let is_black = matches!(pitch % 12, 1 | 3 | 6 | 8 | 10);
            let row_bg = if is_black { theme::SURFACE } else { theme::SURFACE_HI };

            // key column
            let label = note_name(pitch);
            write(
                buf,
                grid.x,
                y,
                &format!(" {:<3}", label),
                Style::default().fg(theme::TEXT_DIM).bg(row_bg),
            );
            // fill body
            for col in 0..body_w {
                write(buf, grid.x + key_w + col, y, " ", Style::default().bg(row_bg));
            }

            // draw notes for this pitch
            if let Some(t) = track {
                for note in &t.notes {
                    if note.pitch != pitch {
                        continue;
                    }
                    let start_col = (note.start as f32 / tick_per_col).round() as u16;
                    let end_col = ((note.start + note.length.max(1)) as f32 / tick_per_col)
                        .round() as u16;
                    let start_col = start_col.min(body_w);
                    let end_col = end_col.min(body_w).max(start_col + 1);
                    for c in start_col..end_col {
                        write(
                            buf,
                            grid.x + key_w + c,
                            y,
                            "█",
                            Style::default().fg(theme::COOL).bg(row_bg),
                        );
                    }
                }
            }
        }

        // cursor mark
        let cursor_col = (self.cursor_tick as f32 / tick_per_col).round() as u16;
        if cursor_col < body_w {
            for r in 0..grid.height {
                let y = grid.y + r;
                let pitch_row = pitch_lo as i32 + (grid.height as i32 - 1 - r as i32);
                let is_cursor_row = pitch_row == self.cursor_pitch as i32;
                let style = if is_cursor_row {
                    Style::default().fg(theme::ACCENT_HI).bg(theme::SURFACE_HI)
                } else {
                    Style::default().fg(theme::ACCENT_DIM)
                };
                write(buf, grid.x + key_w + cursor_col, y, "│", style);
            }
        }

        // hint footer
        let footer_y = inner.y + inner.height - 1;
        Paragraph::new("[a] add  [d] delete  [hjkl] navigate  [,/.] shorter / longer")
            .style(Style::default().fg(theme::MUTED))
            .render(Rect::new(inner.x, footer_y, inner.width, 1), buf);
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

fn note_name(pitch: u8) -> String {
    const NAMES: [&str; 12] = [
        "C", "C#", "D", "D#", "E", "F", "F#", "G", "G#", "A", "A#", "B",
    ];
    let octave = (pitch as i32 / 12) - 1;
    format!("{}{}", NAMES[(pitch % 12) as usize], octave)
}
