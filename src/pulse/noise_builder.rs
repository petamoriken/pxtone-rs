// ノイズビルダー (pxtnPulse_NoiseBuilder)
// 波形テーブルを初期化し、Noise 設計から PCM を生成する

use crate::pulse::frequency::FrequencyTable;
use crate::pulse::noise::{Noise, NoiseOscillator, WaveType, WAVETYPE_NUM};
use crate::pulse::oscillator::{Oscillator, Point};
use crate::pulse::pcm::Pcm;

const BASIC_SPS      : f64 = 44100.0;
const BASIC_FREQUENCY: f64 = 100.0;
const SAMPLING_TOP   : i32 = 32767;
const SMP_NUM_RAND   : usize = 44100;
const SMP_NUM        : usize = (BASIC_SPS / BASIC_FREQUENCY) as usize; // 441

// ---- PRNG ----
struct Rand { buf: [u16; 2] }
impl Rand {
    fn new() -> Self { Self { buf: [0x4444, 0x8888] } }
    fn get(&mut self) -> i16 {
        let w1 = (self.buf[0] as i32).wrapping_add(self.buf[1] as i32);
        let b = w1.to_le_bytes();
        let w2 = i32::from_le_bytes([b[1], b[0], 0, 0]);
        self.buf[1] = self.buf[0];
        self.buf[0] = w2 as u16;
        w2 as i16
    }
}

// ---- 内部オシレーター状態 ----
#[derive(Clone)]
struct OscState {
    increment  : f64,
    offset     : f64,
    volume     : f64,
    wave_type  : WaveType,
    b_reverse  : bool,
    rdm_start  : i16,
    rdm_margin : i32,
    rdm_index  : usize,
}

impl OscState {
    fn from_design(osc: &NoiseOscillator, sps: i32, rand_tbl: &[i16]) -> Self {
        let ran = matches!(osc.wave_type, WaveType::Random | WaveType::Random2);
        let increment = (BASIC_SPS / sps as f64) * (osc.freq as f64 / BASIC_FREQUENCY);
        let offset = if ran {
            0.0
        } else {
            SMP_NUM as f64 * (osc.offset as f64 / 100.0)
        };
        let volume = osc.volume as f64 / 100.0;
        let rdm_index = ((SMP_NUM_RAND as f64 * osc.offset as f64 / 100.0) as usize).min(SMP_NUM_RAND - 1);
        let rdm_start = rand_tbl[rdm_index];
        let rdm_margin = rand_tbl[(rdm_index + 1).min(SMP_NUM_RAND - 1)] as i32 - rdm_start as i32;
        OscState {
            increment,
            offset,
            volume,
            wave_type: osc.wave_type,
            b_reverse: osc.b_rev,
            rdm_start,
            rdm_margin,
            rdm_index,
        }
    }

    fn get_sample(&self, tables: &[Vec<i16>; WAVETYPE_NUM]) -> f64 {
        let tbl = &tables[self.wave_type as usize];
        if tbl.is_empty() { return 0.0; }
        let offset = self.offset as usize;
        let work = match self.wave_type {
            WaveType::Random => {
                self.rdm_start as f64 + self.rdm_margin as f64 * offset as f64 / SMP_NUM as f64
            }
            WaveType::Random2 => self.rdm_start as f64,
            _ => {
                let idx = offset.min(tbl.len() - 1);
                tbl[idx] as f64
            }
        };
        let work = if self.b_reverse { -work } else { work };
        work * self.volume
    }

    fn increment(&mut self, inc: f64, rand_tbl: &[i16]) {
        self.offset += inc;
        if self.offset > SMP_NUM as f64 {
            self.offset -= SMP_NUM as f64;
            if self.offset >= SMP_NUM as f64 { self.offset = 0.0; }
            if matches!(self.wave_type, WaveType::Random | WaveType::Random2) {
                self.rdm_start  = rand_tbl[self.rdm_index];
                self.rdm_index  = (self.rdm_index + 1) % SMP_NUM_RAND;
                self.rdm_margin = rand_tbl[self.rdm_index] as i32 - self.rdm_start as i32;
            }
        }
    }
}

// ---- ユニット状態 ----
struct UnitState {
    enabled        : bool,
    pan            : [f64; 2],
    enves          : Vec<(i32, f64)>, // (smp, mag)
    enve_index     : usize,
    enve_mag_start : f64,
    enve_mag_margin: f64,
    enve_count     : i32,
    main: OscState,
    freq: OscState,
    volu: OscState,
}

// ---- NoiseBuilder ----

pub struct NoiseBuilder {
    freq  : FrequencyTable,
    tables: [Vec<i16>; WAVETYPE_NUM],
}

impl NoiseBuilder {
    pub fn new() -> Self {
        let mut tables: [Vec<i16>; WAVETYPE_NUM] = Default::default();
        let mut rng = Rand::new();
        let mut osci = Oscillator::new();

        // None (zero)
        tables[WaveType::None as usize] = vec![0; SMP_NUM];

        // Sine
        osci.ready_get_sample(vec![Point { x: 1, y: 128 }], 128, SMP_NUM as i32, 0);
        tables[WaveType::Sine as usize] = (0..SMP_NUM).map(|s| {
            (osci.get_one_sample_overtone(s as i32).clamp(-1.0, 1.0) * SAMPLING_TOP as f64) as i16
        }).collect();

        // Saw (down)
        let top2 = (SAMPLING_TOP * 2) as f64;
        tables[WaveType::Saw as usize] = (0..SMP_NUM).map(|s| {
            (SAMPLING_TOP as f64 - top2 * s as f64 / SMP_NUM as f64) as i16
        }).collect();

        // Rect
        let half = SMP_NUM / 2;
        tables[WaveType::Rect as usize] = (0..SMP_NUM).map(|s| {
            if s < half { SAMPLING_TOP as i16 } else { -(SAMPLING_TOP as i16) }
        }).collect();

        // Random
        tables[WaveType::Random as usize] = (0..SMP_NUM_RAND).map(|_| rng.get()).collect();

        // Saw2
        osci.ready_get_sample((1..=16).map(|i| Point { x: i, y: 128 }).collect(), 128, SMP_NUM as i32, 0);
        tables[WaveType::Saw2 as usize] = (0..SMP_NUM).map(|s| {
            (osci.get_one_sample_overtone(s as i32).clamp(-1.0, 1.0) * SAMPLING_TOP as f64) as i16
        }).collect();

        // Rect2
        osci.ready_get_sample((0..8).map(|i| Point { x: 1 + i * 2, y: 128 }).collect(), 128, SMP_NUM as i32, 0);
        tables[WaveType::Rect2 as usize] = (0..SMP_NUM).map(|s| {
            (osci.get_one_sample_overtone(s as i32).clamp(-1.0, 1.0) * SAMPLING_TOP as f64) as i16
        }).collect();

        // Tri
        let n = SMP_NUM as i32;
        osci.ready_get_sample(vec![
            Point { x: 0, y: 0 }, Point { x: n / 4, y: 128 },
            Point { x: n * 3 / 4, y: -128 }, Point { x: n, y: 0 },
        ], 128, SMP_NUM as i32, SMP_NUM as i32);
        tables[WaveType::Tri as usize] = (0..SMP_NUM).map(|s| {
            (osci.get_one_sample_coodinate(s as i32).clamp(-1.0, 1.0) * SAMPLING_TOP as f64) as i16
        }).collect();

        // Random2: C++ ではランダムテーブルと同じポインタを使う → None のまま（build 時に Random と同じ扱い）
        tables[WaveType::Random2 as usize] = Vec::new();

        // Rect3
        let t3 = SMP_NUM / 3;
        tables[WaveType::Rect3 as usize] = (0..SMP_NUM).map(|s| {
            if s < t3 { SAMPLING_TOP as i16 } else { -(SAMPLING_TOP as i16) }
        }).collect();

        // Rect4
        let t4 = SMP_NUM / 4;
        tables[WaveType::Rect4 as usize] = (0..SMP_NUM).map(|s| {
            if s < t4 { SAMPLING_TOP as i16 } else { -(SAMPLING_TOP as i16) }
        }).collect();

        // Rect8
        let t8 = SMP_NUM / 8;
        tables[WaveType::Rect8 as usize] = (0..SMP_NUM).map(|s| {
            if s < t8 { SAMPLING_TOP as i16 } else { -(SAMPLING_TOP as i16) }
        }).collect();

        // Rect16
        let t16 = SMP_NUM / 16;
        tables[WaveType::Rect16 as usize] = (0..SMP_NUM).map(|s| {
            if s < t16 { SAMPLING_TOP as i16 } else { -(SAMPLING_TOP as i16) }
        }).collect();

        // Saw3
        let t1 = SMP_NUM / 3; let t2 = SMP_NUM * 2 / 3;
        tables[WaveType::Saw3 as usize] = (0..SMP_NUM).map(|s| {
            if s < t1 { SAMPLING_TOP as i16 } else if s < t2 { 0 } else { -(SAMPLING_TOP as i16) }
        }).collect();

        // Saw4
        let a1 = SMP_NUM / 4; let a2 = SMP_NUM * 2 / 4; let a3 = SMP_NUM * 3 / 4;
        tables[WaveType::Saw4 as usize] = (0..SMP_NUM).map(|s| {
            if s < a1       { SAMPLING_TOP as i16 }
            else if s < a2  { (SAMPLING_TOP / 3) as i16 }
            else if s < a3  { -(SAMPLING_TOP / 3) as i16 }
            else            { -(SAMPLING_TOP as i16) }
        }).collect();

        // Saw6
        let seg6 = [
            SAMPLING_TOP as i16,
            (SAMPLING_TOP - SAMPLING_TOP * 2 / 5) as i16,
            (SAMPLING_TOP / 5) as i16,
            -(SAMPLING_TOP / 5) as i16,
            (-(SAMPLING_TOP as i32) + SAMPLING_TOP * 2 / 5) as i16,
            -(SAMPLING_TOP as i16),
        ];
        tables[WaveType::Saw6 as usize] = (0..SMP_NUM).map(|s| seg6[(s * 6 / SMP_NUM).min(5)]).collect();

        // Saw8
        let seg8 = [
            SAMPLING_TOP as i16,
            (SAMPLING_TOP - SAMPLING_TOP * 2 / 7) as i16,
            (SAMPLING_TOP - SAMPLING_TOP * 4 / 7) as i16,
            (SAMPLING_TOP / 7) as i16,
            -(SAMPLING_TOP / 7) as i16,
            (-(SAMPLING_TOP as i32) + SAMPLING_TOP * 4 / 7) as i16,
            (-(SAMPLING_TOP as i32) + SAMPLING_TOP * 2 / 7) as i16,
            -(SAMPLING_TOP as i16),
        ];
        tables[WaveType::Saw8 as usize] = (0..SMP_NUM).map(|s| seg8[(s * 8 / SMP_NUM).min(7)]).collect();

        Self { freq: FrequencyTable::new(), tables }
    }

    /// ノイズ設計から PCM を生成する
    pub fn build_noise(&self, noise: &mut Noise, ch: usize, sps: i32, bps: i32) -> Option<Pcm> {
        noise.fix();
        let rand_tbl = &self.tables[WaveType::Random as usize];
        let smp_num = (noise.smp_num_44k as f64 / (44100.0 / sps as f64)) as usize;

        // ユニット状態を構築
        let mut units: Vec<UnitState> = noise.units.iter().map(|du| {
            let pan = if du.pan == 0 { [1.0, 1.0] }
            else if du.pan < 0 { [1.0, (100.0 + du.pan as f64) / 100.0] }
            else               { [(100.0 - du.pan as f64) / 100.0, 1.0] };

            let enves: Vec<(i32, f64)> = du.envelopes.iter()
                .map(|e| (sps * e.x / 1000, e.y as f64 / 100.0))
                .collect();

            let mut enve_index      = 0usize;
            let mut enve_mag_start  = 0.0f64;
            let mut enve_mag_margin = 0.0f64;
            while enve_index < enves.len() {
                enve_mag_margin = enves[enve_index].1 - enve_mag_start;
                if enves[enve_index].0 != 0 { break; }
                enve_mag_start = enves[enve_index].1;
                enve_index += 1;
            }

            UnitState {
                enabled: du.enabled,
                pan,
                enves,
                enve_index,
                enve_mag_start,
                enve_mag_margin,
                enve_count: 0,
                main: OscState::from_design(&du.main, sps, rand_tbl),
                freq: OscState::from_design(&du.freq, sps, rand_tbl),
                volu: OscState::from_design(&du.volu, sps, rand_tbl),
            }
        }).collect();

        let mut pcm = Pcm::create(ch as i32, sps, bps, smp_num as i32)?;
        let buf = pcm.samples_mut();
        let mut buf_pos = 0usize;

        for _ in 0..smp_num {
            for c in 0..ch {
                let mut store = 0.0f64;
                for u in units.iter() {
                    if !u.enabled { continue; }
                    let work = u.main.get_sample(&self.tables);
                    let vol  = u.volu.get_sample(&self.tables);
                    let mut work = work * (vol + SAMPLING_TOP as f64) / (SAMPLING_TOP as f64 * 2.0);
                    work *= u.pan[c];
                    if u.enve_index < u.enves.len() {
                        let smp = u.enves[u.enve_index].0;
                        if smp > 0 {
                            work *= u.enve_mag_start + u.enve_mag_margin * u.enve_count as f64 / smp as f64;
                        } else {
                            work *= u.enve_mag_start;
                        }
                    } else {
                        work *= u.enve_mag_start;
                    }
                    store += work;
                }
                let byte4 = (store as i32).clamp(-SAMPLING_TOP, SAMPLING_TOP);
                if bps == 8 {
                    buf[buf_pos] = ((byte4 >> 8) + 128) as u8;
                    buf_pos += 1;
                } else {
                    let bytes = (byte4 as i16).to_le_bytes();
                    buf[buf_pos]     = bytes[0];
                    buf[buf_pos + 1] = bytes[1];
                    buf_pos += 2;
                }
            }

            // increment all oscillators
            for u in units.iter_mut() {
                if !u.enabled { continue; }
                // freq → fre
                let fre = {
                    let po = &u.freq;
                    let raw = po.get_sample(&self.tables);
                    raw // already scaled by volume in get_sample
                };
                let main_inc = u.main.increment * self.freq.get(fre as i32) as f64;
                u.main.increment(main_inc, rand_tbl);
                let freq_inc = u.freq.increment;
                u.freq.increment(freq_inc, rand_tbl);
                let volu_inc = u.volu.increment;
                u.volu.increment(volu_inc, rand_tbl);

                // envelope
                if u.enve_index < u.enves.len() {
                    u.enve_count += 1;
                    let smp = u.enves[u.enve_index].0;
                    if u.enve_count >= smp {
                        u.enve_count       = 0;
                        u.enve_mag_start   = u.enves[u.enve_index].1;
                        u.enve_mag_margin  = 0.0;
                        u.enve_index      += 1;
                        while u.enve_index < u.enves.len() {
                            u.enve_mag_margin = u.enves[u.enve_index].1 - u.enve_mag_start;
                            if u.enves[u.enve_index].0 != 0 { break; }
                            u.enve_mag_start = u.enves[u.enve_index].1;
                            u.enve_index += 1;
                        }
                    }
                }
            }
        }

        Some(pcm)
    }
}
