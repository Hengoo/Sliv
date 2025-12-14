use std::{io::Write, mem::swap};

use anyhow::{Result, anyhow};
use crossterm::{
    ExecutableCommand, QueueableCommand, cursor,
    event::{DisableBracketedPaste, DisableMouseCapture, EnableBracketedPaste, EnableMouseCapture},
    style::{self, Stylize},
    terminal::{self, EnterAlternateScreen, LeaveAlternateScreen},
};

use crate::COLOR_UNUSED_DIGIT;

// not sure if i need to change this at some point.
pub type Writer = std::io::Stdout;

#[derive(Debug, Clone, Copy)]
pub enum CursorWriteMode {
    // Use | cursor shifted to right by one (just visually)
    Insert,
    // Use block cursor
    Replace,
}

/// Backend is mainly an abstraction layer over the rendering parts of crossterm.
/// It stores the entire screen so we can compare sections and higlight differences
/// Not a general implementation, only containts what SLIV needs
///
/// Additional benefits:
/// - simpler to gracefully handle terminals that are too narrow to render the entire app (currently not implemented)
/// - more efficent since we only push changed to crossterm
#[derive(Debug)]
pub struct Backend {
    writer: Writer,
    size: Size,
    cursor: Pos,
    cursor_write_mode: CursorWriteMode,

    buffer: Buffer,
    // Buffer from the previous frame
    buffer_last: Buffer,
    // write_cache is used during flush. Stored here to avoid repeated allocations
    write_cache: String,
}

impl Backend {
    pub fn new(width: u16, height: u16, cursor_write_mode: CursorWriteMode) -> Result<Self> {
        terminal::enable_raw_mode()?;
        let mut writer = std::io::stdout();
        writer
            .execute(EnterAlternateScreen)?
            .execute(EnableBracketedPaste)?
            .execute(EnableMouseCapture)?;
        let size = Size { width, height };
        Ok(Self {
            writer,
            size,
            cursor: Pos { x: 0, y: 0 },
            cursor_write_mode,
            buffer: Buffer::new(size),
            buffer_last: Buffer::new(size),
            write_cache: String::with_capacity(size.width as usize),
        })
    }

    /// Push the buffer changes to crossterm and advance to the next frame
    /// We expect the entire frame to be rendered from scratch every time, so this function also clears the buffer
    pub fn flush(&mut self, redraw: bool) -> Result<()> {
        // When queuing the Hide it seems to show a flickering cursor, but that is fixed when executing the Hide
        self.writer.execute(cursor::Hide)?;
        // Cursor before writing is the one we want to show in the terminal later
        let cursor_backup = self.cursor;

        if redraw {
            // rendering must be in one big queue, otherwise it seems we get flickering

            // Combine changes from adjacent characters into one write
            self.write_cache.clear();
            let mut first_pixel: Option<Pixel> = None;
            let mut first_pixel_y: Option<u16> = None;

            for (i, (p, p_last)) in self
                .buffer
                .pixels
                .iter()
                .zip(self.buffer_last.pixels.iter())
                .enumerate()
            {
                let x = (i % self.size.width as usize) as u16;
                let y = (i / self.size.width as usize) as u16;

                if first_pixel.is_some() {
                    let tmp = first_pixel.as_ref().unwrap();

                    if p == p_last
                        || first_pixel_y != Some(y)
                        || tmp.color != p.color
                        || tmp.background_color != p.background_color
                    {
                        // Cursor was already moved
                        self.writer.queue(style::PrintStyledContent(
                            self.write_cache
                                .as_str()
                                .with(tmp.color)
                                .on(tmp.background_color),
                        ))?;
                        self.write_cache.clear();
                        first_pixel = None;
                        first_pixel_y = None;
                    }
                }

                if p != p_last {
                    if first_pixel.is_none() {
                        first_pixel = Some(p.clone());
                        first_pixel_y = Some(y);
                        self.writer.queue(cursor::MoveTo(x, y))?;
                    }
                    self.write_cache.push(p.value);
                }
            }
            swap(&mut self.buffer, &mut self.buffer_last);
            self.clear();
        }

        self.cursor = cursor_backup;
        self.show_cursor_at(self.cursor.x, self.cursor.y)?;
        self.writer.flush()?;
        Ok(())
    }

    fn clear(&mut self) {
        for p in self.buffer.pixels.iter_mut() {
            *p = Pixel::default();
        }
    }

    // Writes text to backend buffers and advances the cursor accordingly
    // Does not handle newline
    // Does not wrap arround when going past the width
    pub fn print(&mut self, text: &str) -> Result<()> {
        self.print_with_color(text, style::Color::Reset)
    }

    pub fn println(&mut self, text: &str) -> Result<()> {
        self.print_with_color(text, style::Color::Reset)?;
        self.cursor_move_to_next_line(1);
        Ok(())
    }

    // Writes text to backend buffers and advances the cursor accordingly
    // Does not handle newline
    // Does not wrap arround when going past the width
    pub fn print_with_color(&mut self, text: &str, color: style::Color) -> Result<()> {
        for c in text.chars() {
            let index = self.cursor.get_flat_index(self.size)?;
            self.buffer.pixels[index].value = c;
            self.buffer.pixels[index].color = color;
            self.cursor.x += 1;
        }
        Ok(())
    }

    // Compares two cells and sets the background color if they don't show the same character
    // Two background values are NOT different, even when they have differnt value
    pub fn set_background_color_if_different(
        &mut self,
        x0: u16,
        y0: u16,
        x1: u16,
        y1: u16,
        color: style::Color,
    ) -> Result<()> {
        let pos0 = Pos { x: x0, y: y0 };
        let pos1 = Pos { x: x1, y: y1 };
        let pix0 = self.buffer.pixels[pos0.get_flat_index(self.size)?].clone();
        let pix1 = self.buffer.pixels[pos1.get_flat_index(self.size)?].clone();

        if (pix0.color == COLOR_UNUSED_DIGIT && pix1.color == COLOR_UNUSED_DIGIT)
            || (pix0.color == COLOR_UNUSED_DIGIT && pix1.value == ' ')
            || (pix0.value == ' ' && pix1.color == COLOR_UNUSED_DIGIT)
        {
            return Ok(());
        }

        if pix0.value != pix1.value || pix0.color != pix1.color {
            self.buffer.pixels[pos0.get_flat_index(self.size)?].background_color = color;
            self.buffer.pixels[pos1.get_flat_index(self.size)?].background_color = color;
        }
        Ok(())
    }

    pub const fn cursor_set(&mut self, x: u16, y: u16) {
        self.cursor.x = x;
        self.cursor.y = y;
    }

    pub const fn cursor_move_to_next_line(&mut self, line_count: u16) {
        self.cursor.y += line_count;
        self.cursor.x = 0;
    }

    pub const fn set_cursor_write_mode(&mut self, cursor_write_mode: CursorWriteMode) {
        self.cursor_write_mode = cursor_write_mode;
    }

    pub fn show_cursor_at(&mut self, x: u16, y: u16) -> Result<()> {
        match self.cursor_write_mode {
            CursorWriteMode::Insert => {
                self.writer.queue(cursor::SetCursorStyle::BlinkingBar)?;
                self.writer.queue(cursor::MoveTo(x + 1, y))?;
            }
            CursorWriteMode::Replace => {
                self.writer.queue(cursor::SetCursorStyle::BlinkingBlock)?;
                self.writer.queue(cursor::MoveTo(x, y))?;
            }
        }
        self.writer.queue(cursor::Show)?;
        Ok(())
    }
}

impl Drop for Backend {
    fn drop(&mut self) {
        self.writer.flush().unwrap();
        self.writer
            .execute(LeaveAlternateScreen)
            .unwrap()
            .execute(DisableMouseCapture)
            .unwrap()
            .execute(DisableBracketedPaste)
            .unwrap();
        terminal::disable_raw_mode().unwrap();
    }
}

#[derive(Debug, Clone, Copy)]
struct Pos {
    x: u16,
    y: u16,
}

impl Pos {
    // Compute an array index from the position
    fn get_flat_index(self, size: Size) -> Result<usize> {
        if size.width >= self.x || size.height >= self.y {
            Ok(self.x as usize + self.y as usize * size.width as usize)
        } else {
            Err(anyhow!(
                "Accessing cell out of bounds of the backend buffer, at x {} and y {}",
                size.width,
                size.height
            ))
        }
    }
}

#[derive(Debug, Clone, Copy)]
struct Size {
    pub width: u16,
    pub height: u16,
}

#[derive(Debug)]
struct Buffer {
    pixels: Vec<Pixel>,
}

impl Buffer {
    fn new(size: Size) -> Self {
        Self {
            pixels: vec![Pixel::default(); size.width as usize * size.width as usize],
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
struct Pixel {
    // We know what we write, normal char is enough for SLIV
    value: char,
    color: style::Color,
    background_color: style::Color,
}

impl Pixel {
    const fn default() -> Self {
        Self {
            value: ' ',
            color: style::Color::Reset,
            background_color: style::Color::Reset,
        }
    }
}
