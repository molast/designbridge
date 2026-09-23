use super::*;

pub(crate) fn should_skip_slice(image: &DynamicImage) -> bool {
    if image.width() == 1 && image.height() == 1 {
        return true;
    }

    image.to_rgba8().pixels().all(|pixel| pixel[3] == 0)
}

pub(crate) fn export_target(platform: &str, scale: &str) -> Option<ExportTarget> {
    match (platform, scale) {
        ("android", "mdpi") => Some(ExportTarget {
            label: "mipmap-mdpi",
            directory: "mipmap-mdpi",
            suffix: "",
            factor: 1.0,
        }),
        ("android", "hdpi") => Some(ExportTarget {
            label: "mipmap-hdpi",
            directory: "mipmap-hdpi",
            suffix: "",
            factor: 1.5,
        }),
        ("android", "xhdpi") => Some(ExportTarget {
            label: "mipmap-xhdpi",
            directory: "mipmap-xhdpi",
            suffix: "",
            factor: 2.0,
        }),
        ("android", "xxhdpi") => Some(ExportTarget {
            label: "mipmap-xxhdpi",
            directory: "mipmap-xxhdpi",
            suffix: "",
            factor: 3.0,
        }),
        ("android", "xxxhdpi") => Some(ExportTarget {
            label: "mipmap-xxxhdpi",
            directory: "mipmap-xxxhdpi",
            suffix: "",
            factor: 4.0,
        }),
        ("ios", "1x") => Some(ExportTarget {
            label: "@1x",
            directory: "",
            suffix: "",
            factor: 1.0,
        }),
        ("ios", "2x") => Some(ExportTarget {
            label: "@2x",
            directory: "",
            suffix: "@2x",
            factor: 2.0,
        }),
        ("ios", "3x") => Some(ExportTarget {
            label: "@3x",
            directory: "",
            suffix: "@3x",
            factor: 3.0,
        }),
        _ => None,
    }
}

pub(crate) fn export_dimension(logical: f64, factor: f64) -> Result<u32, String> {
    let pixels = (logical * factor).round();
    if !pixels.is_finite() || !(1.0..=16_384.0).contains(&pixels) {
        return Err("切图目标尺寸无效".to_string());
    }
    Ok(pixels as u32)
}

pub(crate) fn encode_export_image(image: &DynamicImage, format: &str) -> Result<Vec<u8>, String> {
    let encoded_image = if format == "jpg" {
        let rgba = image.to_rgba8();
        let rgb = image::RgbImage::from_fn(rgba.width(), rgba.height(), |x, y| {
            let pixel = rgba.get_pixel(x, y);
            let alpha = u16::from(pixel[3]);
            image::Rgb([
                ((u16::from(pixel[0]) * alpha + 255 * (255 - alpha) + 127) / 255) as u8,
                ((u16::from(pixel[1]) * alpha + 255 * (255 - alpha) + 127) / 255) as u8,
                ((u16::from(pixel[2]) * alpha + 255 * (255 - alpha) + 127) / 255) as u8,
            ])
        });
        DynamicImage::ImageRgb8(rgb)
    } else {
        image.clone()
    };
    let image_format = match format {
        "png" => ImageFormat::Png,
        "jpg" => ImageFormat::Jpeg,
        "webp" => ImageFormat::WebP,
        _ => return Err("仅支持 PNG、JPG 和 WEBP 格式".to_string()),
    };
    let mut encoded = Cursor::new(Vec::new());
    encoded_image
        .write_to(&mut encoded, image_format)
        .map_err(|error| format!("无法编码切图：{error}"))?;
    Ok(encoded.into_inner())
}
