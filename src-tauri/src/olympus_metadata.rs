//! Minimal Olympus MakerNote reader for tags that affect RAW rendering.
//!
//! Olympus ORF MakerNotes use a small TIFF-like IFD. The offsets in this block
//! are relative to the `OLYMPUS\0` signature, not to the file start.

const OLYMPUS_MAKERNOTE_HEADER: &[u8] = b"OLYMPUS\0II\x03\0";
const CAMERA_SETTINGS_IFD: u16 = 0x2020;
const PICTURE_MODE: u16 = 0x0520;

fn read_u16_le(bytes: &[u8], offset: usize) -> Option<u16> {
    Some(u16::from_le_bytes(
        bytes.get(offset..offset + 2)?.try_into().ok()?,
    ))
}

fn read_u32_le(bytes: &[u8], offset: usize) -> Option<u32> {
    Some(u32::from_le_bytes(
        bytes.get(offset..offset + 4)?.try_into().ok()?,
    ))
}

fn find_ifd_entry_value(bytes: &[u8], ifd_offset: usize, wanted_tag: u16) -> Option<u32> {
    let entry_count = read_u16_le(bytes, ifd_offset)? as usize;
    let entries_start = ifd_offset.checked_add(2)?;
    let entries_length = entry_count.checked_mul(12)?;
    entries_start.checked_add(entries_length)?;

    for index in 0..entry_count {
        let entry_offset = entries_start.checked_add(index.checked_mul(12)?)?;
        if read_u16_le(bytes, entry_offset)? == wanted_tag {
            return read_u32_le(bytes, entry_offset + 8);
        }
    }
    None
}

/// Reads Olympus CameraSettings.PictureMode (tag 0x0520) from an ORF.
/// The first `u16` is the selected Picture Mode; the optional second value
/// describes an auto-override state and isn't relevant to the rendering choice.
pub fn picture_mode(file_bytes: &[u8]) -> Option<u16> {
    let maker_start = file_bytes
        .windows(OLYMPUS_MAKERNOTE_HEADER.len())
        .position(|window| window == OLYMPUS_MAKERNOTE_HEADER)?;
    let root_ifd_offset = maker_start.checked_add(OLYMPUS_MAKERNOTE_HEADER.len())?;
    let camera_settings_offset =
        find_ifd_entry_value(file_bytes, root_ifd_offset, CAMERA_SETTINGS_IFD)?;
    let camera_ifd_offset = maker_start.checked_add(camera_settings_offset as usize)?;
    let raw_mode = find_ifd_entry_value(file_bytes, camera_ifd_offset, PICTURE_MODE)?;
    Some((raw_mode & 0xffff) as u16)
}

pub fn picture_mode_name(mode: u16) -> Option<&'static str> {
    match mode {
        1 => Some("Vivid"),
        2 => Some("Natural"),
        3 => Some("Muted"),
        4 => Some("Portrait"),
        5 => Some("i-Enhance"),
        6 => Some("e-Portrait"),
        7 => Some("Color Creator"),
        8 => Some("Underwater"),
        9 => Some("Color Profile 1"),
        10 => Some("Color Profile 2"),
        11 => Some("Color Profile 3"),
        12 => Some("Monochrome Profile 1"),
        13 => Some("Monochrome Profile 2"),
        14 => Some("Monochrome Profile 3"),
        17 => Some("Art Mode"),
        18 => Some("Monochrome Profile 4"),
        256 => Some("Monotone"),
        512 => Some("Sepia"),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_picture_mode_from_minimal_olympus_makernote() {
        let mut bytes = OLYMPUS_MAKERNOTE_HEADER.to_vec();
        bytes.extend_from_slice(&1u16.to_le_bytes());
        bytes.extend_from_slice(&CAMERA_SETTINGS_IFD.to_le_bytes());
        bytes.extend_from_slice(&13u16.to_le_bytes());
        bytes.extend_from_slice(&1u32.to_le_bytes());
        bytes.extend_from_slice(&32u32.to_le_bytes());
        bytes.resize(32, 0);
        bytes.extend_from_slice(&1u16.to_le_bytes());
        bytes.extend_from_slice(&PICTURE_MODE.to_le_bytes());
        bytes.extend_from_slice(&3u16.to_le_bytes());
        bytes.extend_from_slice(&2u32.to_le_bytes());
        bytes.extend_from_slice(&(5u32 | (2u32 << 16)).to_le_bytes());

        assert_eq!(picture_mode(&bytes), Some(5));
        assert_eq!(picture_mode_name(256), Some("Monotone"));
    }
}
