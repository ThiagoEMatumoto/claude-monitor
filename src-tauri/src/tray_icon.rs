use image::{Rgba, RgbaImage};

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum UsageLevel {
    Normal,
    Warning,
    Critical,
}

impl UsageLevel {
    pub fn from_pct(pct: f32) -> Self {
        if pct >= 75.0 {
            UsageLevel::Critical
        } else if pct >= 50.0 {
            UsageLevel::Warning
        } else {
            UsageLevel::Normal
        }
    }

    fn color(&self) -> Rgba<u8> {
        match self {
            UsageLevel::Normal => Rgba([0, 230, 118, 255]),   // green
            UsageLevel::Warning => Rgba([255, 171, 0, 255]),  // yellow
            UsageLevel::Critical => Rgba([255, 82, 82, 255]), // red
        }
    }
}

/// Generate a 32x32 RGBA PNG with a colored filled circle.
pub fn generate_icon(level: UsageLevel) -> Vec<u8> {
    let size: u32 = 32;
    let mut img = RgbaImage::new(size, size);
    let color = level.color();
    let center = (size as f32) / 2.0;
    let radius = center - 2.0;

    for y in 0..size {
        for x in 0..size {
            let dx = x as f32 - center;
            let dy = y as f32 - center;
            let dist = (dx * dx + dy * dy).sqrt();

            if dist <= radius {
                // Slight gradient: brighter center, darker edge
                let factor = 1.0 - (dist / radius) * 0.3;
                let r = (color[0] as f32 * factor).min(255.0) as u8;
                let g = (color[1] as f32 * factor).min(255.0) as u8;
                let b = (color[2] as f32 * factor).min(255.0) as u8;
                img.put_pixel(x, y, Rgba([r, g, b, 255]));
            } else if dist <= radius + 1.0 {
                // Anti-aliased edge
                let alpha = ((radius + 1.0 - dist) * 255.0) as u8;
                img.put_pixel(x, y, Rgba([color[0], color[1], color[2], alpha]));
            }
            // else transparent (default)
        }
    }

    let mut buf = std::io::Cursor::new(Vec::new());
    img.write_to(&mut buf, image::ImageFormat::Png)
        .expect("failed to encode icon");
    buf.into_inner()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_usage_level_from_pct() {
        assert_eq!(UsageLevel::from_pct(0.0), UsageLevel::Normal);
        assert_eq!(UsageLevel::from_pct(49.9), UsageLevel::Normal);
        assert_eq!(UsageLevel::from_pct(50.0), UsageLevel::Warning);
        assert_eq!(UsageLevel::from_pct(74.9), UsageLevel::Warning);
        assert_eq!(UsageLevel::from_pct(75.0), UsageLevel::Critical);
        assert_eq!(UsageLevel::from_pct(100.0), UsageLevel::Critical);
    }

    #[test]
    fn test_generate_icon_produces_valid_png() {
        for level in [UsageLevel::Normal, UsageLevel::Warning, UsageLevel::Critical] {
            let bytes = generate_icon(level);
            // PNG magic bytes
            assert_eq!(&bytes[0..4], &[0x89, 0x50, 0x4E, 0x47]);
            assert!(bytes.len() > 100, "icon too small: {} bytes", bytes.len());
        }
    }

    #[test]
    fn test_generate_icon_different_levels_differ() {
        let green = generate_icon(UsageLevel::Normal);
        let yellow = generate_icon(UsageLevel::Warning);
        let red = generate_icon(UsageLevel::Critical);
        assert_ne!(green, yellow);
        assert_ne!(yellow, red);
        assert_ne!(green, red);
    }
}
