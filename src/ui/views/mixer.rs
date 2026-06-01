use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};
use ratatui::text::Span;
use ratatui::widgets::{Block, Borders, Paragraph, Widget};

use crate::model::Project;
use crate::ui::theme;

pub struct MixerView<'a> {
    pub project: &'a Project,
    pub cursor: usize,
    pub voice_count: u32,
}

impl<'a> Widget for MixerView<'a> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        let block = Block::default()
            .borders(Borders::ALL)
            .border_style(Style::default().fg(theme::BORDER))
            .title(Span::styled(
                " mixer ",
                Style::default().fg(theme::ACCENT).add_modifier(Modifier::BOLD),
            ));
        let inner = block.inner(area);
        block.render(area, buf);

        let info = format!(
            "channels: {}    master: {:.0} dB    voices: {}",
            self.project.channels.len(),
            volume_to_db(self.project.master_volume),
            self.voice_count
        );
        Paragraph::new(info)
            .style(Style::default().fg(theme::TEXT_DIM))
            .render(Rect::new(inner.x, inner.y, inner.width, 1), buf);

        let strip_w = 8u16;
        let strip_count = (self.project.channels.len() + 1) as u16; // +1 for master
        let total_w = strip_count * strip_w;
        if total_w > inner.width || inner.height < 16 {
            Paragraph::new("terminal too narrow for mixer strips")
                .style(Style::default().fg(theme::MUTED))
                .render(Rect::new(inner.x, inner.y + 2, inner.width, 1), buf);
            return;
        }

        let strips_y = inner.y + 2;
        let strips_h = inner.height.saturating_sub(3);

        for (i, channel) in self.project.channels.iter().enumerate() {
            let x = inner.x + i as u16 * strip_w;
            let focused = i == self.cursor;
            draw_strip(
                buf,
                x,
                strips_y,
                strip_w,
                strips_h,
                &channel.name,
                channel.volume,
                channel.pan,
                channel.mute,
                channel.solo,
                focused,
                false,
            );
        }

        // master strip
        let mx = inner.x + self.project.channels.len() as u16 * strip_w;
        draw_strip(
            buf,
            mx,
            strips_y,
            strip_w,
            strips_h,
            "master",
            self.project.master_volume,
            0.0,
            false,
            false,
            self.cursor == self.project.channels.len(),
            true,
        );

        let footer_y = inner.y + inner.height - 1;
        Paragraph::new("[hl] strip  [+/-] volume  [</>] pan  [m] mute  [o] solo")
            .style(Style::default().fg(theme::MUTED))
            .render(Rect::new(inner.x, footer_y, inner.width, 1), buf);
    }
}

#[allow(clippy::too_many_arguments)]
fn draw_strip(
    buf: &mut Buffer,
    x: u16,
    y: u16,
    w: u16,
    h: u16,
    name: &str,
    volume: f32,
    pan: f32,
    mute: bool,
    solo: bool,
    focused: bool,
    is_master: bool,
) {
    let bg = if focused { theme::SURFACE_HI } else { theme::SURFACE };

    // fill strip background
    for row in 0..h {
        for col in 0..w {
            write(buf, x + col, y + row, " ", Style::default().bg(bg));
        }
    }

    // name
    let header_color = if is_master {
        theme::ACCENT_HI
    } else if focused {
        theme::COOL
    } else {
        theme::TEXT
    };
    let name_text = truncate(name, w as usize - 1);
    let centered = format_centered(&name_text, w as usize - 1);
    write(
        buf,
        x + 1,
        y,
        &centered,
        Style::default().fg(header_color).bg(bg).add_modifier(Modifier::BOLD),
    );

    // pan indicator
    let pan_text = if pan.abs() < 0.05 {
        "──".to_string()
    } else if pan > 0.0 {
        format!("{:>2.0}R", (pan * 100.0).abs())
    } else {
        format!("{:>2.0}L", (pan * 100.0).abs())
    };
    let pan_centered = format_centered(&pan_text, w as usize - 1);
    write(
        buf,
        x + 1,
        y + 1,
        &pan_centered,
        Style::default().fg(theme::TEXT_DIM).bg(bg),
    );

    // meter vertical
    let meter_top = y + 3;
    let meter_h = h.saturating_sub(5);
    let level_rows = (volume * meter_h as f32).round() as u16;
    for row in 0..meter_h {
        let row_y = meter_top + (meter_h - 1 - row);
        let fill = row < level_rows;
        let glyph = if fill { "█" } else { "·" };
        let frac = row as f32 / meter_h as f32;
        let color = if !fill {
            theme::DIM
        } else if frac > 0.85 {
            theme::HOT
        } else if frac > 0.6 {
            theme::ACCENT
        } else {
            theme::GREEN
        };
        write(buf, x + 2, row_y, glyph, Style::default().fg(color).bg(bg));
        write(buf, x + 4, row_y, glyph, Style::default().fg(color).bg(bg));
    }

    // dB readout
    let db_y = y + h - 2;
    let db_text = if volume <= 0.0001 {
        "─∞ dB".to_string()
    } else {
        format!("{:>+4.0}", volume_to_db(volume))
    };
    let db_centered = format_centered(&db_text, w as usize - 1);
    write(
        buf,
        x + 1,
        db_y,
        &db_centered,
        Style::default().fg(theme::TEXT_DIM).bg(bg),
    );

    // mute / solo row
    let ms_y = y + h - 1;
    let m = if mute { "M" } else { "·" };
    let s = if solo { "S" } else { "·" };
    let m_color = if mute { theme::HOT } else { theme::DIM };
    let s_color = if solo { theme::COOL } else { theme::DIM };
    write(buf, x + 2, ms_y, m, Style::default().fg(m_color).bg(bg));
    write(buf, x + 4, ms_y, s, Style::default().fg(s_color).bg(bg));
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
