#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Appearance {
    pub light: bool,
    pub accent: Option<[u8; 3]>,
}

#[cfg(windows)]
pub fn read() -> Appearance {
    use winreg::{enums::HKEY_CURRENT_USER, RegKey};
    let light = RegKey::predef(HKEY_CURRENT_USER)
        .open_subkey(r"Software\Microsoft\Windows\CurrentVersion\Themes\Personalize")
        .and_then(|key| key.get_value::<u32, _>("AppsUseLightTheme"))
        .map(|v| v != 0)
        .unwrap_or(false);
    let mut color = 0u32;
    let mut opaque = 0i32;
    // Windows supplies 0xAARRGGBB. No registry color-format guessing.
    let accent = if unsafe { DwmGetColorizationColor(&mut color, &mut opaque) } == 0 {
        Some([
            ((color >> 16) & 255) as u8,
            ((color >> 8) & 255) as u8,
            (color & 255) as u8,
        ])
    } else {
        None
    };
    Appearance { light, accent }
}

#[cfg(windows)]
#[link(name = "dwmapi")]
extern "system" {
    fn DwmGetColorizationColor(color: *mut u32, opaque: *mut i32) -> i32;
}

#[cfg(not(windows))]
pub fn read() -> Appearance {
    Appearance {
        light: false,
        accent: None,
    }
}

pub fn contrast_text(rgb: [u8; 3]) -> [u8; 3] {
    let linear = |v: u8| {
        let v = f32::from(v) / 255.0;
        if v <= 0.04045 {
            v / 12.92
        } else {
            ((v + 0.055) / 1.055).powf(2.4)
        }
    };
    let lum = 0.2126 * linear(rgb[0]) + 0.7152 * linear(rgb[1]) + 0.0722 * linear(rgb[2]);
    if lum > 0.179 {
        [0, 0, 0]
    } else {
        [255, 255, 255]
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn extreme_custom_accents_stay_visible() {
        assert_ne!(super::readable_accent([0, 0, 0], true), [0, 0, 0]);
        assert_ne!(
            super::readable_accent([255, 255, 255], false),
            [255, 255, 255]
        );
        assert_eq!(
            super::readable_accent([255, 255, 255], true),
            [255, 255, 255]
        );
        assert_eq!(super::readable_accent([0, 0, 0], false), [0, 0, 0]);
    }
    #[test]
    fn readable_accent_labels() {
        assert_eq!(super::contrast_text([255, 255, 255]), [0, 0, 0]);
        assert_eq!(super::contrast_text([0, 0, 0]), [255, 255, 255]);
    }
}

pub fn readable_accent(rgb: [u8; 3], dark: bool) -> [u8; 3] {
    let target = if dark { 255.0 } else { 0.0 };
    let luminance = |c: [u8; 3]| {
        let linear = |v: u8| {
            let x = f32::from(v) / 255.0;
            if x <= 0.04045 {
                x / 12.92
            } else {
                ((x + 0.055) / 1.055).powf(2.4)
            }
        };
        0.2126 * linear(c[0]) + 0.7152 * linear(c[1]) + 0.0722 * linear(c[2])
    };
    let bg = luminance(if dark { [20, 23, 32] } else { [244, 245, 250] });
    for step in 0..=20 {
        let t = step as f32 / 20.0;
        let color = rgb.map(|v| (f32::from(v) * (1.0 - t) + target * t).round() as u8);
        let fg = luminance(color);
        if (fg.max(bg) + 0.05) / (fg.min(bg) + 0.05) >= 4.5 {
            return color;
        }
    }
    if dark {
        [255; 3]
    } else {
        [0; 3]
    }
}
