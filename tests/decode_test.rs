use encoding_rs::SHIFT_JIS;
use pxtone::{DestinationQuality, PxtoneService, VomitPreparation};
use std::fs::{self, File};
use std::io::BufReader;
use std::path::Path;
use toml::{Table, Value};

fn decode_shift_jis(raw: &[u8]) -> String {
  SHIFT_JIS.decode(raw).0.into_owned()
}

fn load_service(service: &mut PxtoneService, path: &Path) {
  let data = fs::read(path).unwrap_or_else(|e| panic!("{}: {}", path.display(), e));
  service
    .read(data)
    .unwrap_or_else(|e| panic!("{}: read failed: {:?}", path.display(), e));
  service
    .tones_ready()
    .unwrap_or_else(|e| panic!("{}: tones_ready failed: {:?}", path.display(), e));
}

fn pcm_to_wav(samples: &[u8], channels: u8, sample_rate: u32) -> Vec<u8> {
  let data_len = samples.len() as u32;
  let byte_rate = sample_rate * channels as u32 * 2;
  let mut wav = Vec::with_capacity(44 + samples.len());
  wav.extend_from_slice(b"RIFF");
  wav.extend_from_slice(&(36u32 + data_len).to_le_bytes());
  wav.extend_from_slice(b"WAVE");
  wav.extend_from_slice(b"fmt ");
  wav.extend_from_slice(&16u32.to_le_bytes());
  wav.extend_from_slice(&1u16.to_le_bytes()); // PCM
  wav.extend_from_slice(&(channels as u16).to_le_bytes());
  wav.extend_from_slice(&sample_rate.to_le_bytes());
  wav.extend_from_slice(&byte_rate.to_le_bytes());
  wav.extend_from_slice(&(channels as u16 * 2).to_le_bytes());
  wav.extend_from_slice(&16u16.to_le_bytes());
  wav.extend_from_slice(b"data");
  wav.extend_from_slice(&data_len.to_le_bytes());
  wav.extend_from_slice(samples);
  wav
}

fn decode_ptcop_to_wav(service: &mut PxtoneService) -> Vec<u8> {
  service
    .moo_preparation(VomitPreparation::default())
    .expect("moo_preparation failed");

  let q = service.get_destination_quality();
  let bytes_per_frame = (q.channels * 2) as usize;
  let mut chunk = vec![0u8; bytes_per_frame * 4096];
  let mut pcm = Vec::new();

  while !service.is_end_vomit() {
    if !service.moo(&mut chunk) {
      break;
    }
    pcm.extend_from_slice(&chunk);
  }

  pcm_to_wav(&pcm, q.channels, q.sample_rate)
}

fn decode_to_metadata(service: &PxtoneService) -> String {
  let m = &service.master;
  let t = &service.text;
  let mut table = Table::new();
  let mut text_table = Table::new();
  text_table.insert(
    "name".into(),
    Value::String(decode_shift_jis(t.name().unwrap_or_default())),
  );
  text_table.insert(
    "comment".into(),
    Value::String(decode_shift_jis(t.comment().unwrap_or_default())),
  );
  table.insert("text".into(), Value::Table(text_table));
  let mut master_table = Table::new();
  master_table.insert(
    "ticks_per_beat".into(),
    Value::Integer(m.ticks_per_beat() as i64),
  );
  master_table.insert(
    "beats_per_measure".into(),
    Value::Integer(m.beats_per_measure() as i64),
  );
  master_table.insert("beat_tempo".into(), Value::Float(m.beat_tempo() as f64));
  master_table.insert(
    "measure_count".into(),
    Value::Integer(m.measure_count() as i64),
  );
  master_table.insert(
    "repeat_measure".into(),
    Value::Integer(m.repeat_measure() as i64),
  );
  master_table.insert(
    "last_measure".into(),
    Value::Integer(m.last_measure() as i64),
  );
  table.insert("master".into(), Value::Table(master_table));

  let units_array = Value::Array(
    service
      .units
      .iter()
      .map(|u| {
        let mut t = Table::new();
        t.insert("name".into(), Value::String(decode_shift_jis(u.name())));
        t.insert("played".into(), Value::Boolean(u.played()));
        Value::Table(t)
      })
      .collect(),
  );
  table.insert("units".into(), units_array);

  let events_array = Value::Array(
    service
      .events
      .records()
      .iter()
      .map(|e| {
        let mut t = Table::new();
        t.insert("tick".into(), Value::Integer(e.tick() as i64));
        t.insert("unit_index".into(), Value::Integer(e.unit_index() as i64));
        t.insert("kind".into(), Value::Integer(e.kind() as i64));
        t.insert("value".into(), Value::Integer(e.value() as i64));
        Value::Table(t)
      })
      .collect(),
  );
  table.insert("events".into(), events_array);

  toml::to_string(&table).unwrap()
}

#[test]
fn decoded_ptcop_matches_reference() {
  let update = std::env::var("UPDATE_SNAPSHOTS").is_ok();
  let sample_dir = Path::new("tests/sample/ptcop");
  let snapshot_dir = Path::new("tests/snapshots/ptcop");

  let mut entries: Vec<_> = fs::read_dir(sample_dir)
    .expect("tests/sample/ptcop directory not found")
    .filter_map(|e| e.ok())
    .filter(|e| e.path().extension().map_or(false, |ext| ext == "ptcop"))
    .collect();
  entries.sort_by_key(|e| e.file_name());

  assert!(
    !entries.is_empty(),
    "no .ptcop files found in tests/sample/ptcop/"
  );

  let mut service = PxtoneService::new(DestinationQuality::default());
  let mut failures = Vec::new();

  for entry in &entries {
    let ptcop_path = entry.path();
    let stem = ptcop_path.file_stem().unwrap().to_string_lossy();
    let wav_path = snapshot_dir.join(format!("{}.wav", stem));
    let toml_path = snapshot_dir.join(format!("{}.toml", stem));

    load_service(&mut service, &ptcop_path);
    let wav = decode_ptcop_to_wav(&mut service);
    let metadata = decode_to_metadata(&service);

    if update {
      fs::write(&wav_path, &wav)
        .unwrap_or_else(|e| panic!("{}: failed to write snapshot: {}", wav_path.display(), e));
      fs::write(&toml_path, &metadata)
        .unwrap_or_else(|e| panic!("{}: failed to write snapshot: {}", toml_path.display(), e));
      continue;
    }

    let expected_wav = fs::read(&wav_path)
      .unwrap_or_else(|e| panic!("{}: failed to read snapshot: {}", wav_path.display(), e));
    if wav != expected_wav {
      failures.push(wav_path.display().to_string());
    }

    let expected_txt = fs::read_to_string(&toml_path)
      .unwrap_or_else(|e| panic!("{}: failed to read snapshot: {}", toml_path.display(), e));
    let actual_toml: Table = metadata
      .parse()
      .expect("generated metadata is not valid TOML");
    let expected_toml: Table = expected_txt
      .parse()
      .unwrap_or_else(|e| panic!("{}: invalid TOML: {}", toml_path.display(), e));
    if actual_toml != expected_toml {
      failures.push(toml_path.display().to_string());
    }
  }

  assert!(
    failures.is_empty(),
    "Decoded output does not match reference ({} file(s)):\n{}",
    failures.len(),
    failures.join("\n")
  );
}

#[test]
fn decoded_ptnoise_matches_reference() {
  let update = std::env::var("UPDATE_SNAPSHOTS").is_ok();
  let sample_dir = Path::new("tests/sample/ptnoise");
  let snapshot_dir = Path::new("tests/snapshots/ptnoise");

  let mut entries: Vec<_> = fs::read_dir(sample_dir)
    .expect("tests/sample/ptnoise directory not found")
    .filter_map(|e| e.ok())
    .filter(|e| e.path().extension().map_or(false, |ext| ext == "ptnoise"))
    .collect();
  entries.sort_by_key(|e| e.file_name());

  assert!(
    !entries.is_empty(),
    "no .ptnoise files found in tests/sample/ptnoise/"
  );

  let mut service = PxtoneService::new(DestinationQuality::default());
  let mut failures = Vec::new();

  for entry in &entries {
    let ptnoise_path = entry.path();
    let stem = ptnoise_path.file_stem().unwrap().to_string_lossy();
    let wav_path = snapshot_dir.join(format!("{}.wav", stem));

    let file =
      File::open(&ptnoise_path).unwrap_or_else(|e| panic!("{}: {}", ptnoise_path.display(), e));
    let mut reader = BufReader::new(file);
    let noise_wave = service
      .render_noise(&mut reader)
      .unwrap_or_else(|e| panic!("{}: render_noise failed: {:?}", ptnoise_path.display(), e));

    let wav = pcm_to_wav(
      &noise_wave.samples,
      noise_wave.channels,
      noise_wave.sample_rate,
    );

    if update {
      fs::write(&wav_path, &wav)
        .unwrap_or_else(|e| panic!("{}: failed to write snapshot: {}", wav_path.display(), e));
      continue;
    }

    let expected_wav = fs::read(&wav_path)
      .unwrap_or_else(|e| panic!("{}: failed to read snapshot: {}", wav_path.display(), e));
    if wav != expected_wav {
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
