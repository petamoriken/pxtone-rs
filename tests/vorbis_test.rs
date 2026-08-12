//! Snapshot test for the OGG Vorbis decoder used by `.ptcop` OGGV materials.
//!
//! None of the `.ptcop` samples contains an OGGV material, so this test drives
//! the vendored lewton/ogg crates the same way `src/woice.rs` does, using
//! standalone `.ogg` fixtures.

mod common;

use common::{pcm_to_wav, wav_matches};
use lewton::inside_ogg::OggStreamReader;
use std::fs;
use std::io::Cursor;
use std::path::Path;

/// Mirrors `decode_ogg` in `src/woice.rs`.
fn decode_ogg_to_wav(data: &[u8], path: &Path) -> Vec<u8> {
  let mut reader = OggStreamReader::new(Cursor::new(data))
    .unwrap_or_else(|e| panic!("{}: reading headers failed: {:?}", path.display(), e));

  let channels = reader.ident_hdr.audio_channels;
  let sample_rate = reader.ident_hdr.audio_sample_rate;

  let mut pcm = Vec::new();
  while let Some(packet) = reader
    .read_dec_packet_itl()
    .unwrap_or_else(|e| panic!("{}: decoding failed: {:?}", path.display(), e))
  {
    pcm.extend(packet.iter().flat_map(|&s| s.to_le_bytes()));
  }

  pcm_to_wav(&pcm, channels, sample_rate)
}

#[test]
fn decoded_ogg_matches_reference() {
  let update = std::env::var("UPDATE_SNAPSHOTS").is_ok();
  let sample_dir = Path::new("tests/sample/ogg");
  let snapshot_dir = Path::new("tests/snapshots/ogg");

  let mut entries: Vec<_> = fs::read_dir(sample_dir)
    .expect("tests/sample/ogg directory not found")
    .filter_map(|e| e.ok())
    .filter(|e| e.path().extension().is_some_and(|ext| ext == "ogg"))
    .collect();
  entries.sort_by_key(|e| e.file_name());

  assert!(
    !entries.is_empty(),
    "no .ogg files found in tests/sample/ogg/"
  );

  let mut failures = Vec::new();

  for entry in &entries {
    let ogg_path = entry.path();
    let stem = ogg_path.file_stem().unwrap().to_string_lossy();
    let wav_path = snapshot_dir.join(format!("{}.wav", stem));

    let data = fs::read(&ogg_path).unwrap_or_else(|e| panic!("{}: {}", ogg_path.display(), e));
    let wav = decode_ogg_to_wav(&data, &ogg_path);

    if update {
      fs::create_dir_all(snapshot_dir).expect("failed to create tests/snapshots/ogg");
      fs::write(&wav_path, &wav)
        .unwrap_or_else(|e| panic!("{}: failed to write snapshot: {}", wav_path.display(), e));
      continue;
    }

    let expected_wav = fs::read(&wav_path)
      .unwrap_or_else(|e| panic!("{}: failed to read snapshot: {}", wav_path.display(), e));
    if !wav_matches(&wav, &expected_wav) {
      failures.push(wav_path.display().to_string());
    }
  }

  assert!(
    failures.is_empty(),
    "Decoded output does not match reference ({} file(s)):\n{}",
    failures.len(),
    failures.join("\n")
  );
}
