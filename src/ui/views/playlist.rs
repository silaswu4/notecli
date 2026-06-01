use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};
use ratatui::text::Span;
use ratatui::widgets::{Block, Borders, Paragraph, Widget};

use crate::model::Project;
use crate::ui::theme;

pub struct PlaylistView<'a> {
    pub project: &'a Project,
    pub cursor_bar: usize,
}

impl<'a> Widget for PlaylistView<'a> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        let block = Block::default()
            .borders(Borders::ALL)
            .border_style(Style::default().fg(theme::BORDER))
            .title(Span::styled(
                " playlist ",
                Style::default().fg(theme::ACCENT).add_modifier(Modifier::BOLD),
            ));
        let inner = block.inner(area);
        block.render(area, buf);

        let info = format!(
            "song: {}    bpm: {:.1}    {} bars in arrangement",
            self.project.name,
            self.project.bpm,
            self.project.playlist.len()
        );
        Paragraph::new(info)
            .style(Style::default().fg(theme::TEXT_DIM))
            .render(Rect::new(inner.x, inner.y, inner.width, 1), buf);

        let grid_y = inner.y + 2;
        let grid_w = inner.width.saturating_sub(8);
        let bar_w = (grid_w / 16).max(2);

        // bar number header
        for i in 0..16u16 {
            let x = inner.x + 8 + i * bar_w;
            let label = format!("{:>2}", i + 1);
            let style = if (i as usize) == self.cursor_bar {
                Style::default().fg(theme::ACCENT_HI).add_modifier(Modifier::BOLD)
            } else if i % 4 == 0 {
                Style::default().fg(theme::TEXT_DIM)
            } else {
                Style::default().fg(theme::MUTED)
            };
            write(buf, x, grid_y, &label, style);
        }

        // single "song" lane for v1
        let lane_y = grid_y + 2;
        write(
            buf,
            inner.x,
            lane_y,
            "song   ",
            Style::default().fg(theme::TEXT_DIM),
        );

        for (i, pat_id) in self.project.playlist.iter().enumerate().take(16) {
            let x = inner.x + 8 + i as u16 * bar_w;
            let name = self
                .project
                .patterns
                .get(*pat_id as usize)
                .map(|p| p.name.as_str())
                .unwrap_or("?");
            let label_w = (bar_w as usize).max(2);
            let block_text = format_centered(name, label_w);
            let style = Style::default()
                .fg(theme::TEXT)
                .bg(theme::ACCENT_DIM)
                .add_modifier(Modifier::BOLD);
            write(buf, x, lane_y, &block_text, style);
        }

        // hint
        let footer_y = inner.y + inner.height - 1;
        Paragraph::new("[hjkl] navigate  [enter] place active pattern  [d] delete block")
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
