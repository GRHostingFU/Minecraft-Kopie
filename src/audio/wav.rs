//! Minimal RIFF/WAVE writer (16-bit mono PCM) so synthesised effects can be handed to the audio
//! backend as an in-memory "file".

pub fn encode_mono16(samples: &[f32], sample_rate: u32) -> Vec<u8> {
    let data_len = (samples.len() * 2) as u32;
    let mut out = Vec::with_capacity(44 + data_len as usize);
    out.extend_from_slice(b"RIFF");
    out.extend_from_slice(&(36 + data_len).to_le_bytes());
    out.extend_from_slice(b"WAVE");
    out.extend_from_slice(b"fmt ");
    out.extend_from_slice(&16u32.to_le_bytes()); // fmt chunk size
    out.extend_from_slice(&1u16.to_le_bytes()); // PCM
    out.extend_from_slice(&1u16.to_le_bytes()); // mono
    out.extend_from_slice(&sample_rate.to_le_bytes());
    out.extend_from_slice(&(sample_rate * 2).to_le_bytes()); // byte rate
    out.extend_from_slice(&2u16.to_le_bytes()); // block align
    out.extend_from_slice(&16u16.to_le_bytes()); // bits per sample
    out.extend_from_slice(b"data");
    out.extend_from_slice(&data_len.to_le_bytes());
    for s in samples {
        let v = (s.clamp(-1.0, 1.0) * i16::MAX as f32).round() as i16;
        out.extend_from_slice(&v.to_le_bytes());
    }
    out
}

/// Reads back what [`encode_mono16`] wrote (used by the tests).
pub fn decode_mono16(bytes: &[u8]) -> Option<(u32, Vec<f32>)> {
    if bytes.len() < 44 || &bytes[0..4] != b"RIFF" || &bytes[8..12] != b"WAVE" || &bytes[36..40] != b"data" {
        return None;
    }
    let rate = u32::from_le_bytes(bytes[24..28].try_into().ok()?);
    let len = u32::from_le_bytes(bytes[40..44].try_into().ok()?) as usize;
    let data = bytes.get(44..44 + len)?;
    Some((rate, data.as_chunks::<2>().0.iter().map(|c| i16::from_le_bytes(*c) as f32 / i16::MAX as f32).collect()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trips_samples_and_clamps() {
        let input = [0.0, 0.5, -0.5, 1.0, -1.0, 2.0];
        let bytes = encode_mono16(&input, 44_100);
        assert_eq!(bytes.len(), 44 + input.len() * 2);
        let (rate, back) = decode_mono16(&bytes).unwrap();
        assert_eq!(rate, 44_100);
        for (a, b) in input.iter().zip(&back) {
            assert!((a.clamp(-1.0, 1.0) - b).abs() < 1e-3);
        }
    }

    #[test]
    fn header_is_well_formed() {
        let bytes = encode_mono16(&[0.0; 10], 22_050);
        assert_eq!(&bytes[0..4], b"RIFF");
        assert_eq!(u32::from_le_bytes(bytes[4..8].try_into().unwrap()) as usize, bytes.len() - 8);
        assert_eq!(&bytes[12..16], b"fmt ");
        assert_eq!(u16::from_le_bytes(bytes[20..22].try_into().unwrap()), 1, "PCM");
        assert!(decode_mono16(&bytes[..30]).is_none());
    }
}
