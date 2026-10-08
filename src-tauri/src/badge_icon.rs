use tauri::{AppHandle, Manager, image::Image};

const BADGE_PURPLE: [u8; 4] = [139, 92, 246, 255];
const BADGE_BORDER: [u8; 4] = [11, 12, 16, 255];
const BADGE_TEXT: [u8; 4] = [255, 255, 255, 255];

pub fn update_notification_badges(app: &AppHandle, unread_count: usize) {
    let Some(base) = app.default_window_icon() else {
        return;
    };
    let icon = if unread_count > 0 {
        with_unread_badge(base, unread_count)
    } else {
        owned_copy(base)
    };

    if let Some(tray) = app.tray_by_id("main-tray") {
        let _ = tray.set_icon(Some(icon.clone()));
    }
    if let Some(window) = app.get_webview_window("main") {
        let _ = window.set_icon(owned_copy(base));
        #[cfg(target_os = "windows")]
        {
            let overlay = (unread_count > 0).then(|| unread_overlay(unread_count));
            let _ = window.set_overlay_icon(overlay);
        }
    }
}

fn with_unread_badge(base: &Image<'_>, unread_count: usize) -> Image<'static> {
    let width = base.width();
    let height = base.height();
    let expected_len = width as usize * height as usize * 4;
    if width == 0 || height == 0 || base.rgba().len() != expected_len {
        return owned_copy(base);
    }

    let mut rgba = base.rgba().to_vec();
    let shortest = width.min(height) as f32;
    // A radius of 28% makes the circle cover approximately one quarter of
    // the square icon area while leaving the underlying logo recognizable.
    let outer_radius = (shortest * 0.28).max(2.0);
    let inner_radius = (outer_radius - (shortest * 0.045).max(1.0)).max(1.0);
    let margin = (shortest * 0.02).max(1.0);
    let center_x = width as f32 - outer_radius - margin;
    let center_y = outer_radius + margin;

    draw_circle(
        &mut rgba,
        width,
        height,
        center_x,
        center_y,
        outer_radius,
        inner_radius,
    );
    draw_count(
        &mut rgba,
        width,
        height,
        center_x,
        center_y,
        inner_radius,
        unread_count,
    );

    Image::new_owned(rgba, width, height)
}

#[cfg(target_os = "windows")]
fn unread_overlay(unread_count: usize) -> Image<'static> {
    const SIZE: u32 = 32;
    let mut rgba = vec![0; (SIZE * SIZE * 4) as usize];
    draw_circle(&mut rgba, SIZE, SIZE, 16.0, 16.0, 15.0, 13.0);
    draw_count(&mut rgba, SIZE, SIZE, 16.0, 16.0, 13.0, unread_count);
    Image::new_owned(rgba, SIZE, SIZE)
}

fn draw_circle(
    rgba: &mut [u8],
    width: u32,
    height: u32,
    center_x: f32,
    center_y: f32,
    outer_radius: f32,
    inner_radius: f32,
) {
    for y in 0..height {
        for x in 0..width {
            let dx = x as f32 + 0.5 - center_x;
            let dy = y as f32 + 0.5 - center_y;
            let distance_squared = dx * dx + dy * dy;
            let color = if distance_squared <= inner_radius * inner_radius {
                Some(BADGE_PURPLE)
            } else if distance_squared <= outer_radius * outer_radius {
                Some(BADGE_BORDER)
            } else {
                None
            };
            if let Some(color) = color {
                set_pixel(rgba, width, x, y, color);
            }
        }
    }
}

fn draw_count(
    rgba: &mut [u8],
    width: u32,
    height: u32,
    center_x: f32,
    center_y: f32,
    radius: f32,
    unread_count: usize,
) {
    let label = if unread_count > 99 {
        "99+".to_string()
    } else {
        unread_count.to_string()
    };
    let glyph_count = label.chars().count() as u32;
    let units_wide = glyph_count * 3 + glyph_count.saturating_sub(1);
    let scale = (((radius * 1.7) as u32 / units_wide).min((radius * 1.2) as u32 / 5)).max(1);
    let text_width = units_wide * scale;
    let text_height = 5 * scale;
    let start_x = (center_x - text_width as f32 / 2.0).round() as i32;
    let start_y = (center_y - text_height as f32 / 2.0).round() as i32;

    for (glyph_index, character) in label.chars().enumerate() {
        let pattern = glyph_pattern(character);
        let glyph_x = start_x + glyph_index as i32 * 4 * scale as i32;
        for (row, bits) in pattern.into_iter().enumerate() {
            for column in 0..3 {
                if bits & (1 << (2 - column)) == 0 {
                    continue;
                }
                for offset_y in 0..scale {
                    for offset_x in 0..scale {
                        let x = glyph_x + column * scale as i32 + offset_x as i32;
                        let y = start_y + row as i32 * scale as i32 + offset_y as i32;
                        if x >= 0 && y >= 0 && x < width as i32 && y < height as i32 {
                            set_pixel(rgba, width, x as u32, y as u32, BADGE_TEXT);
                        }
                    }
                }
            }
        }
    }
}

fn glyph_pattern(character: char) -> [u8; 5] {
    match character {
        '0' => [0b111, 0b101, 0b101, 0b101, 0b111],
        '1' => [0b010, 0b110, 0b010, 0b010, 0b111],
        '2' => [0b111, 0b001, 0b111, 0b100, 0b111],
        '3' => [0b111, 0b001, 0b111, 0b001, 0b111],
        '4' => [0b101, 0b101, 0b111, 0b001, 0b001],
        '5' => [0b111, 0b100, 0b111, 0b001, 0b111],
        '6' => [0b111, 0b100, 0b111, 0b101, 0b111],
        '7' => [0b111, 0b001, 0b001, 0b001, 0b001],
        '8' => [0b111, 0b101, 0b111, 0b101, 0b111],
        '9' => [0b111, 0b101, 0b111, 0b001, 0b111],
        '+' => [0b000, 0b010, 0b111, 0b010, 0b000],
        _ => [0; 5],
    }
}

fn set_pixel(rgba: &mut [u8], width: u32, x: u32, y: u32, color: [u8; 4]) {
    let index = ((y * width + x) * 4) as usize;
    rgba[index..index + 4].copy_from_slice(&color);
}

fn owned_copy(image: &Image<'_>) -> Image<'static> {
    Image::new_owned(image.rgba().to_vec(), image.width(), image.height())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unread_badge_covers_about_one_quarter_and_draws_count() {
        let base = Image::new_owned(vec![0; 32 * 32 * 4], 32, 32);
        let badged = with_unread_badge(&base, 7);
        assert_eq!(badged.width(), 32);
        assert_eq!(badged.height(), 32);
        assert_eq!(&badged.rgba()[0..4], &[0, 0, 0, 0]);
        assert!(
            badged
                .rgba()
                .chunks_exact(4)
                .any(|pixel| pixel == BADGE_PURPLE)
        );
        assert!(
            badged
                .rgba()
                .chunks_exact(4)
                .any(|pixel| pixel == BADGE_BORDER)
        );
        assert!(
            badged
                .rgba()
                .chunks_exact(4)
                .any(|pixel| pixel == BADGE_TEXT)
        );
        let badge_pixels = badged
            .rgba()
            .chunks_exact(4)
            .filter(|pixel| pixel[3] != 0)
            .count();
        assert!((200..=320).contains(&badge_pixels));
    }

    #[cfg(target_os = "windows")]
    #[test]
    fn taskbar_overlay_contains_badge_and_count() {
        let overlay = unread_overlay(100);
        assert_eq!((overlay.width(), overlay.height()), (32, 32));
        assert!(
            overlay
                .rgba()
                .chunks_exact(4)
                .any(|pixel| pixel == BADGE_PURPLE)
        );
        assert!(
            overlay
                .rgba()
                .chunks_exact(4)
                .any(|pixel| pixel == BADGE_TEXT)
        );
    }
}
