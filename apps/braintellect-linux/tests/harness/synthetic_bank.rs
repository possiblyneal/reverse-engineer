//! A synthetic instrument bank in the layout the original's `dll<id>.BT.dll`
//! embeds (`docs/reverse-engineering/braintellect2/audio-feedback.md`, "Wave
//! device and instrument 1002"): RIFF/WAVE blobs behind a big-endian length,
//! a Qt resource name table (UTF-16BE) and a tree of 14-byte file nodes in an
//! order unrelated to the note order. It holds invented samples, never vendor
//! bytes.

/// One 16-bit mono WAV file.
pub fn wav(rate_hz: u32, pcm: &[i16]) -> Vec<u8> {
    let data_len = u32::try_from(pcm.len() * 2).expect("size");
    let mut out = Vec::new();
    out.extend(b"RIFF");
    out.extend((36 + data_len).to_le_bytes());
    out.extend(b"WAVEfmt ");
    out.extend(16u32.to_le_bytes());
    out.extend(1u16.to_le_bytes());
    out.extend(1u16.to_le_bytes());
    out.extend(rate_hz.to_le_bytes());
    out.extend((rate_hz * 2).to_le_bytes());
    out.extend(2u16.to_le_bytes());
    out.extend(16u16.to_le_bytes());
    out.extend(b"data");
    out.extend(data_len.to_le_bytes());
    out.extend(pcm.iter().flat_map(|s| s.to_le_bytes()));
    out
}

/// A DLL-shaped byte string carrying `notes` as `Synth<note>` resources, each
/// with its WAV file. The blobs are stored in descending note order, so blob
/// order is not note order.
pub fn bank(notes: &[(u8, Vec<u8>)]) -> Vec<u8> {
    let mut out = vec![0xAA; 37];
    let data_base = out.len();
    let mut offsets = Vec::new();
    for (_, file) in notes.iter().rev() {
        offsets.push(out.len() - data_base);
        out.extend(u32::try_from(file.len()).expect("size").to_be_bytes());
        out.extend(file);
    }
    offsets.reverse();
    out.extend([0x55; 11]);
    let names_at = out.len();
    let mut name_offsets = Vec::new();
    for (note, _) in notes {
        name_offsets.push(out.len() - names_at);
        let name: Vec<u16> = format!("Synth{note}").encode_utf16().collect();
        out.extend(u16::try_from(name.len()).expect("len").to_be_bytes());
        out.extend(0u32.to_be_bytes());
        out.extend(name.iter().flat_map(|c| c.to_be_bytes()));
    }
    out.extend([0x33; 5]);
    // A directory node first, as the real tree has.
    out.extend(0u32.to_be_bytes());
    out.extend(2u16.to_be_bytes());
    out.extend([0u8; 8]);
    for (name_off, data_off) in name_offsets.iter().zip(&offsets) {
        out.extend(u32::try_from(*name_off).expect("off").to_be_bytes());
        out.extend([0u8; 6]);
        out.extend(u32::try_from(*data_off).expect("off").to_be_bytes());
    }
    out.extend([0x77; 9]);
    out
}
