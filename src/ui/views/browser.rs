use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};
use ratatui::text::Span;
use ratatui::widgets::{Block, Borders, Paragraph, Widget};

use crate::ui::theme;
use std::path::PathBuf;

pub struct BrowserView<'a> {
    pub cwd: &'a PathBuf,
    pub entries: &'a [BrowserEntry],
    pub cursor: usize,
    pub message: Option<&'a str>,
}

#[derive(Clone, Debug)]
pub struct BrowserEntry {
    pub name: String,
    pub path: PathBuf,
    pub is_dir: bool,
    pub is_audio: bool,
}

impl<'a> Widget for BrowserView<'a> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        let block = Block::default()
            .borders(Borders::ALL)
            .border_style(Style::default().fg(theme::BORDER))
            .title(Span::styled(
                " sample browser ",
                Style::default().fg(theme::ACCENT).add_modifier(Modifier::BOLD),
            ));
        let inner = block.inner(area);
        block.render(area, buf);

        let path_text = self.cwd.display().to_string();
        Paragraph::new(format!(" {}", truncate(&path_text, inner.width as usize - 2)))
            .style(Style::default().fg(theme::COOL).add_modifier(Modifier::BOLD))
            .render(Rect::new(inner.x, inner.y, inner.width, 1), buf);

        if let Some(msg) = self.message {
            Paragraph::new(format!(" {msg}"))
                .style(Style::default().fg(theme::GREEN))
                .render(Rect::new(inner.x, inner.y + 1, inner.width, 1), buf);
        }

        let list_y = inner.y + 3;
        let list_h = inner.height.saturating_sub(5);
        let scroll = self.cursor.saturating_sub(list_h as usize - 1);
        let visible = self.entries.iter().enumerate().skip(scroll).take(list_h as usize);

        for (row, (i, entry)) in visible.enumerate() {
            let y = list_y + row as u16;
            let focused = i == self.cursor;
            let bg = if focused { theme::SURFACE_HI } else { theme::BG };

            let icon = if entry.is_dir {
                "▸"
            } else if entry.is_audio {
                "♪"
            } else {
                "·"
            };
            let fg = if entry.is_dir {
                theme::COOL
            } else if entry.is_audio {
                theme::ACCENT
            } else {
                theme::MUTED
            };

            // clear row
            for col in 0..inner.width {
                write(buf, inner.x + col, y, " ", Style::default().bg(bg));
            }

            let line = format!(" {} {}", icon, entry.name);
            write(
                buf,
                inner.x,
                y,
                &truncate(&line, inner.width as usize),
                Style::default().fg(fg).bg(bg),
            );
        }

        // hint footer
        let footer_y = inner.y + inner.height - 1;
        Paragraph::new("[hjkl] navigate  [enter] open / load into channel  [space] audition")
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

fn truncate(s: &str, w: usize) -> String {
    if s.chars().count() <= w {
        return s.to_string();
    }
    s.chars().take(w.saturating_sub(1)).chain(std::iter::once('…')).collect()
}
