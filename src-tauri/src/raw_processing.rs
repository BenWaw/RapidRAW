use crate::image_processing::apply_orientation;
use crate::olympus_metadata;
use anyhow::{Result, anyhow};
use image::{DynamicImage, ImageBuffer, Rgba};
use rawler::{
    decoders::{Orientation, RawDecodeParams},
    imgop::develop::{DemosaicAlgorithm, Intermediate, ProcessingStep, RawDevelop},
    rawimage::{RawImage, RawPhotometricInterpretation},
    rawsource::RawSource,
};
use std::sync::{
    Arc,
    atomic::{AtomicUsize, Ordering},
};

pub fn develop_raw_image(
    file_bytes: &[u8],
    fast_demosaic: bool,
    highlight_compression: f32,
    linear_mode: String,
    camera_profile: String,
    cancel_token: Option<(Arc<AtomicUsize>, usize)>,
) -> Result<DynamicImage> {
    let (developed_image, orientation) = develop_internal(
        file_bytes,
        fast_demosaic,
        highlight_compression,
        linear_mode,
        camera_profile,
        cancel_token,
    )?;
    Ok(apply_orientation(developed_image, orientation))
}

fn is_linear_raw_format(raw_image: &RawImage) -> bool {
    matches!(
        raw_image.photometric,
        RawPhotometricInterpretation::LinearRaw
    )
}

#[inline]
fn srgb_to_linear(value: f32) -> f32 {
    if value <= 0.04045 {
        value / 12.92
    } else {
        ((value + 0.055) / 1.055).powf(3.0)
    }
}

fn develop_internal(
    file_bytes: &[u8],
    fast_demosaic: bool,
    highlight_compression: f32,
    linear_mode: String,
    camera_profile: String,
    cancel_token: Option<(Arc<AtomicUsize>, usize)>,
) -> Result<(DynamicImage, Orientation)> {
    let check_cancel = || -> Result<()> {
        if let Some((tracker, generation)) = &cancel_token
            && tracker.load(Ordering::SeqCst) != *generation
        {
            return Err(anyhow!("Load cancelled"));
        }
        Ok(())
    };

    check_cancel()?;

    let source = RawSource::new_from_slice(file_bytes);
    let decoder = rawler::get_decoder(&source)?;

    check_cancel()?;
    let mut raw_image: RawImage = decoder.raw_image(&source, &RawDecodeParams::default(), false)?;

    let metadata = decoder.raw_metadata(&source, &RawDecodeParams::default())?;
    let orientation = metadata
        .exif
        .orientation
        .map(Orientation::from_u16)
        .unwrap_or(Orientation::Normal);
    let olympus_profile = resolve_olympus_profile(
        &camera_profile,
        &metadata.make,
        &metadata.model,
        olympus_metadata::picture_mode(file_bytes),
    );

    let is_linear_format = is_linear_raw_format(&raw_image);

    let (apply_ungamma, apply_calibration) = match linear_mode.as_str() {
        "gamma" => (true, true),
        "skip_calib" => (false, false),
        "gamma_skip_calib" => (true, false),
        _ => (false, true),
    };

    let original_white_level = raw_image
        .whitelevel
        .0
        .first()
        .cloned()
        .unwrap_or(u16::MAX as u32) as f32;
    let original_black_level = raw_image
        .blacklevel
        .levels
        .first()
        .map(|r| r.as_f32())
        .unwrap_or(0.0);

    for level in raw_image.whitelevel.0.iter_mut() {
        *level = u32::MAX;
    }

    let mut developer = RawDevelop::default();

    if is_linear_format {
        developer.steps.retain(|&step| {
            step != ProcessingStep::SRgb
                && step != ProcessingStep::Demosaic
                && (apply_calibration || step != ProcessingStep::Calibrate)
        });
    } else if fast_demosaic {
        developer.demosaic_algorithm = DemosaicAlgorithm::Speed;
        developer.steps.retain(|&step| step != ProcessingStep::SRgb);
    } else {
        developer.steps.retain(|&step| step != ProcessingStep::SRgb);
    }

    check_cancel()?;
    let mut developed_intermediate = developer.develop_intermediate(&raw_image)?;

    drop(raw_image);

    let denominator = (original_white_level - original_black_level).max(1.0);
    let rescale_factor = (u32::MAX as f32 - original_black_level) / denominator;

    let safe_highlight_compression = highlight_compression.max(1.01);

    let clamp_limit = if fast_demosaic {
        1.0
    } else {
        safe_highlight_compression
    };

    check_cancel()?;

    match &mut developed_intermediate {
        Intermediate::Monochrome(pixels) => {
            pixels.data.iter_mut().for_each(|p| {
                let mut linear_val = *p * rescale_factor;
                if is_linear_format && apply_ungamma {
                    linear_val = srgb_to_linear(linear_val.clamp(0.0, 1.0));
                }
                *p = linear_val.clamp(0.0, clamp_limit);
            });
        }
        Intermediate::ThreeColor(pixels) => {
            pixels.data.iter_mut().for_each(|p| {
                let mut r = (p[0] * rescale_factor).max(0.0);
                let mut g = (p[1] * rescale_factor).max(0.0);
                let mut b = (p[2] * rescale_factor).max(0.0);

                if is_linear_format && apply_ungamma {
                    r = srgb_to_linear(r.clamp(0.0, 1.0));
                    g = srgb_to_linear(g.clamp(0.0, 1.0));
                    b = srgb_to_linear(b.clamp(0.0, 1.0));
                }

                let max_c = r.max(g).max(b);

                let (final_r, final_g, final_b) = if max_c > 1.0 {
                    let min_c = r.min(g).min(b);
                    let compression_factor =
                        (1.0 - (max_c - 1.0) / (safe_highlight_compression - 1.0)).clamp(0.0, 1.0);
                    let compressed_r = min_c + (r - min_c) * compression_factor;
                    let compressed_g = min_c + (g - min_c) * compression_factor;
                    let compressed_b = min_c + (b - min_c) * compression_factor;
                    let compressed_max = compressed_r.max(compressed_g).max(compressed_b);

                    if compressed_max > 1e-6 {
                        let rescale = max_c / compressed_max;
                        (
                            compressed_r * rescale,
                            compressed_g * rescale,
                            compressed_b * rescale,
                        )
                    } else {
                        (max_c, max_c, max_c)
                    }
                } else {
                    (r, g, b)
                };

                p[0] = final_r.clamp(0.0, clamp_limit);
                p[1] = final_g.clamp(0.0, clamp_limit);
                p[2] = final_b.clamp(0.0, clamp_limit);
            });
        }
        Intermediate::FourColor(pixels) => {
            pixels.data.iter_mut().for_each(|p| {
                p.iter_mut().for_each(|c| {
                    let mut linear_val = *c * rescale_factor;
                    if is_linear_format && apply_ungamma {
                        linear_val = srgb_to_linear(linear_val.clamp(0.0, 1.0));
                    }
                    *c = linear_val.clamp(0.0, clamp_limit);
                });
            });
        }
    }

    let (width, height) = {
        let dim = developed_intermediate.dim();
        (dim.w as u32, dim.h as u32)
    };

    check_cancel()?;

    let dynamic_image = match developed_intermediate {
        Intermediate::ThreeColor(pixels) => {
            let buffer = ImageBuffer::<Rgba<f32>, _>::from_fn(width, height, |x, y| {
                let p = pixels.data[(y * width + x) as usize];
                Rgba([p[0], p[1], p[2], 1.0])
            });
            DynamicImage::ImageRgba32F(buffer)
        }
        Intermediate::Monochrome(pixels) => {
            let buffer = ImageBuffer::<Rgba<f32>, _>::from_fn(width, height, |x, y| {
                let p = pixels.data[(y * width + x) as usize];
                Rgba([p, p, p, 1.0])
            });
            DynamicImage::ImageRgba32F(buffer)
        }
        _ => {
            return Err(anyhow!("Unsupported intermediate format for conversion"));
        }
    };

    let dynamic_image = match olympus_profile {
        OlympusProfile::Vivid => {
            apply_olympus_color_profile(dynamic_image, 1.26, 1.022, 1.000, 0.978)
        }
        OlympusProfile::Natural => apply_olympus_natural_profile(dynamic_image),
        OlympusProfile::Muted => {
            apply_olympus_color_profile(dynamic_image, 0.90, 1.012, 1.000, 0.990)
        }
        OlympusProfile::Portrait => {
            apply_olympus_color_profile(dynamic_image, 1.05, 1.030, 1.000, 0.975)
        }
        OlympusProfile::IEnhance => {
            apply_olympus_color_profile(dynamic_image, 1.18, 1.020, 1.006, 0.980)
        }
        OlympusProfile::Monochrome => apply_olympus_monochrome_profile(dynamic_image),
        OlympusProfile::Sepia => apply_olympus_sepia_profile(dynamic_image),
        OlympusProfile::None => dynamic_image,
    };

    Ok((dynamic_image, orientation))
}

/// A gentle, camera-specific starting rendering, applied before all user edits.
/// It was tuned against JPEG+ORF pairs from an Olympus OM-D E-M5 Mark II.
fn apply_olympus_natural_profile(image: DynamicImage) -> DynamicImage {
    apply_olympus_color_profile(image, 1.14, 1.018, 1.003, 0.982)
}

fn apply_olympus_color_profile(
    image: DynamicImage,
    saturation: f32,
    red_gain: f32,
    green_gain: f32,
    blue_gain: f32,
) -> DynamicImage {
    let mut pixels = image.to_rgba32f();

    for pixel in pixels.pixels_mut() {
        let r = pixel[0].max(0.0);
        let g = pixel[1].max(0.0);
        let b = pixel[2].max(0.0);
        let luma = 0.2126 * r + 0.7152 * g + 0.0722 * b;

        // Preserve neutral greys while bringing muted colours closer to the
        // Olympus Natural JPEG rendering. This is intentionally subtler than
        // a global saturation slider and leaves highlight data unclipped.
        let mut r = luma + (r - luma) * saturation;
        let mut g = luma + (g - luma) * saturation;
        let mut b = luma + (b - luma) * saturation;
        r *= red_gain;
        g *= green_gain;
        b *= blue_gain;

        pixel[0] = r.max(0.0);
        pixel[1] = g.max(0.0);
        pixel[2] = b.max(0.0);
    }

    DynamicImage::ImageRgba32F(pixels)
}

fn apply_olympus_monochrome_profile(image: DynamicImage) -> DynamicImage {
    let mut pixels = image.to_rgba32f();
    for pixel in pixels.pixels_mut() {
        let luma =
            0.2126 * pixel[0].max(0.0) + 0.7152 * pixel[1].max(0.0) + 0.0722 * pixel[2].max(0.0);
        pixel[0] = luma;
        pixel[1] = luma;
        pixel[2] = luma;
    }
    DynamicImage::ImageRgba32F(pixels)
}

fn apply_olympus_sepia_profile(image: DynamicImage) -> DynamicImage {
    let mut pixels = image.to_rgba32f();
    for pixel in pixels.pixels_mut() {
        let luma =
            0.2126 * pixel[0].max(0.0) + 0.7152 * pixel[1].max(0.0) + 0.0722 * pixel[2].max(0.0);
        pixel[0] = luma * 1.12;
        pixel[1] = luma * 1.00;
        pixel[2] = luma * 0.76;
    }
    DynamicImage::ImageRgba32F(pixels)
}

#[derive(Clone, Copy)]
enum OlympusProfile {
    None,
    Vivid,
    Natural,
    Muted,
    Portrait,
    IEnhance,
    Monochrome,
    Sepia,
}

fn resolve_olympus_profile(
    profile: &str,
    make: &str,
    model: &str,
    picture_mode: Option<u16>,
) -> OlympusProfile {
    if profile == "neutral" {
        return OlympusProfile::None;
    }

    let normalized_make = make.to_ascii_uppercase();
    let normalized_model: String = model
        .to_ascii_uppercase()
        .chars()
        .filter(|c| c.is_ascii_alphanumeric())
        .collect();
    let is_em5_mark_ii = normalized_make.contains("OLYMPUS")
        && (normalized_model.contains("EM5MARKII") || normalized_model.contains("EM5MKII"));

    if !is_em5_mark_ii {
        return OlympusProfile::None;
    }

    match profile {
        "auto" => match picture_mode {
            Some(1) => OlympusProfile::Vivid,
            Some(2) => OlympusProfile::Natural,
            Some(3) => OlympusProfile::Muted,
            Some(4) | Some(6) => OlympusProfile::Portrait,
            Some(5) => OlympusProfile::IEnhance,
            Some(12..=14) | Some(18) | Some(256) => OlympusProfile::Monochrome,
            Some(512) => OlympusProfile::Sepia,
            _ => OlympusProfile::Natural,
        },
        "olympus_natural" => OlympusProfile::Natural,
        "olympus_vivid" => OlympusProfile::Vivid,
        "olympus_muted" => OlympusProfile::Muted,
        "olympus_portrait" => OlympusProfile::Portrait,
        "olympus_i_enhance" => OlympusProfile::IEnhance,
        "olympus_monochrome" => OlympusProfile::Monochrome,
        "olympus_sepia" => OlympusProfile::Sepia,
        _ => OlympusProfile::None,
    }
}

pub fn get_fast_demosaic_scale_factor(
    file_bytes: &[u8],
    decoded_width: u32,
    decoded_height: u32,
) -> f32 {
    let source = RawSource::new_from_slice(file_bytes);
    if let Ok(decoder) = rawler::get_decoder(&source)
        && let Ok(raw_img) = decoder.raw_image(&source, &RawDecodeParams::default(), true)
    {
        let max_orig = (raw_img.width as f32).max(raw_img.height as f32);
        let max_comp = (decoded_width as f32).max(decoded_height as f32);
        if max_orig > 0.0 {
            let ratio = max_comp / max_orig;
            if ratio > 0.1 && ratio < 0.35 {
                return 0.25;
            } else if (0.35..0.75).contains(&ratio) {
                return 0.5;
            }
        }
    }
    1.0
}
