use pxtone::{PxtoneService, VomitPreparation};
use std::fs::{self, File};
use std::io::BufReader;
use std::path::Path;

fn open_service(path: &Path) -> PxtoneService {
  let file = File::open(path).unwrap_or_else(|e| panic!("{}: {}", path.display(), e));
  let mut reader = BufReader::new(file);
  let mut service = PxtoneService::new();
  service
    .read(&mut reader)
    .unwrap_or_else(|e| panic!("{}: read failed: {:?}", path.display(), e));
  service
    .tones_ready()
    .unwrap_or_else(|e| panic!("{}: tones_ready failed: {:?}", path.display(), e));
  service
}

fn decode_to_wav(service: &mut PxtoneService) -> Vec<u8> {
  let prep = VomitPreparation::new(); // flags = 0 → no loop
  service
    .moo_preparation(Some(&prep))
    .expect("moo_preparation failed");

  let (ch, sps) = service.get_destination_quality();
  let bytes_per_frame = (ch * 2) as usize;
  let mut chunk = vec![0u8; bytes_per_frame * 4096];
  let mut pcm = Vec::new();

  while !service.is_end_vomit() {
    if !service.moo(&mut chunk) {
      break;
    }
    pcm.extend_from_slice(&chunk);
  }

  let data_len = pcm.len() as u32;
  let byte_rate = sps as u32 * ch as u32 * 2;
  let mut wav = Vec::with_capacity(44 + pcm.len());
  wav.extend_from_slice(b"RIFF");
  wav.extend_from_slice(&(36u32 + data_len).to_le_bytes());
  wav.extend_from_slice(b"WAVE");
  wav.extend_from_slice(b"fmt ");
  wav.extend_from_slice(&16u32.to_le_bytes());
  wav.extend_from_slice(&1u16.to_le_bytes()); // PCM
  wav.extend_from_slice(&(ch as u16).to_le_bytes());
  wav.extend_from_slice(&(sps as u32).to_le_bytes());
  wav.extend_from_slice(&byte_rate.to_le_bytes());
  wav.extend_from_slice(&(ch as u16 * 2).to_le_bytes());
  wav.extend_from_slice(&16u16.to_le_bytes());
  wav.extend_from_slice(b"data");
  wav.extend_from_slice(&data_len.to_le_bytes());
  wav.extend_from_slice(&pcm);
  wav
}

fn decode_to_metadata(service: &PxtoneService) -> String {
  let m = &service.master;
  let t = &service.text;
  format!(
    "name = {}\ncomment = {}\nbeat_clock = {}\nbeat_num = {}\nbeat_tempo = {}\nmeas_num = {}\nrepeat_meas = {}\nlast_meas = {}\n",
    toml_string(t.name().as_deref().unwrap_or("")),
    toml_string(t.comment().as_deref().unwrap_or("")),
    m.beat_clock(),
    m.beat_num(),
    m.beat_tempo(),
    m.meas_num(),
    m.repeat_meas(),
    m.last_meas(),
  )
}

fn toml_string(s: &str) -> String {
  format!("\"{}\"", s.replace('\\', "\\\\").replace('"', "\\\""))
}

#[test]
fn decoded_wav_matches_reference() {
  let update = std::env::var("UPDATE_SNAPSHOTS").is_ok();
  let sample_dir = Path::new("tests/sample");
  let snapshot_dir = Path::new("tests/snapshots");

  let mut entries: Vec<_> = fs::read_dir(sample_dir)
    .expect("tests/sample directory not found")
    .filter_map(|e| e.ok())
    .filter(|e| e.path().extension().map_or(false, |ext| ext == "ptcop"))
    .collect();
  entries.sort_by_key(|e| e.file_name());

  assert!(
    !entries.is_empty(),
    "no .ptcop files found in tests/sample/"
  );

  let mut failures = Vec::new();

  for entry in &entries {
    let ptcop_path = entry.path();
    let stem = ptcop_path.file_stem().unwrap().to_string_lossy();
    let wav_path = snapshot_dir.join(format!("{}.wav", stem));
    let txt_path = snapshot_dir.join(format!("{}.toml", stem));

    let mut service = open_service(&ptcop_path);
    let metadata = decode_to_metadata(&service);
    let wav = decode_to_wav(&mut service);

    if update {
      fs::write(&wav_path, &wav)
        .unwrap_or_else(|e| panic!("{}: failed to write snapshot: {}", wav_path.display(), e));
      fs::write(&txt_path, &metadata)
        .unwrap_or_else(|e| panic!("{}: failed to write snapshot: {}", txt_path.display(), e));
      continue;
    }

    // WAV comparison
    let expected_wav = fs::read(&wav_path)
      .unwrap_or_else(|e| panic!("{}: failed to read snapshot: {}", wav_path.display(), e));
    if wav != expected_wav {
      failures.push(wav_path.display().to_string());
    }

    // Metadata comparison
    let expected_txt = fs::read_to_string(&txt_path)
      .unwrap_or_else(|e| panic!("{}: failed to read snapshot: {}", txt_path.display(), e));
    if metadata != expected_txt {
      failures.push(txt_path.display().to_string());
    }
  }

  assert!(
    failures.is_empty(),
    "Decoded output does not match reference ({} file(s)):\n{}",
    failures.len(),
    failures.join("\n")
  );
}
