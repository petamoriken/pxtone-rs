// Oscillator (pxtnPulse_Oscillator)
// Generates samples one at a time from a waveform table

#[derive(Clone, Debug)]
pub(crate) struct Point {
  pub(crate) x: i32,
  pub(crate) y: i32,
}

pub(crate) struct Oscillator {
  volume: u32,
  sample_num: u32,
  point_num: usize,
  point_reso: u32,
  points: Vec<Point>,
}

impl Oscillator {
  pub(crate) fn new() -> Self {
    Self {
      volume: 0,
      sample_num: 0,
      point_num: 0,
      point_reso: 0,
      points: Vec::new(),
    }
  }

  pub(crate) fn ready_get_sample(
    &mut self,
    points: Vec<Point>,
    volume: u32,
    sample_num: u32,
    point_reso: u32,
  ) {
    self.point_num = points.len();
    self.points = points;
    self.volume = volume;
    self.sample_num = sample_num;
    self.point_reso = point_reso;
  }

  /// Gets one sample via overtone synthesis
  pub(crate) fn get_one_sample_overtone(&self, index: u32) -> f64 {
    use std::f64::consts::PI;
    let mut work = 0.0f64;
    for p in &self.points {
      let sss = 2.0 * PI * p.x as f64 * index as f64 / self.sample_num as f64;
      work += sss.sin() * p.y as f64 / p.x as f64 / 128.0;
    }
    work * self.volume as f64 / 128.0
  }

  /// Gets one sample via coordinate interpolation
  pub(crate) fn get_one_sample_coordinate(&self, index: u32) -> f64 {
    let i = self.point_reso * index / self.sample_num;

    // Find the two surrounding points
    let c = self
      .points
      .iter()
      .position(|p| p.x > i as i32)
      .unwrap_or(self.point_num);

    let (x1, y1, x2, y2) = if c == self.point_num {
      // End of list
      let last = &self.points[c - 1];
      let first = &self.points[0];
      (last.x, last.y, self.point_reso as i32, first.y)
    } else if c > 0 {
      let prev = &self.points[c - 1];
      let cur = &self.points[c];
      (prev.x, prev.y, cur.x, cur.y)
    } else {
      let p0 = &self.points[0];
      (p0.x, p0.y, p0.x, p0.y)
    };

    let w = x2 - x1;
    let ii = i as i32 - x1;
    let h = y2 - y1;

    let work = if ii != 0 {
      y1 as f64 + h as f64 * ii as f64 / w as f64
    } else {
      y1 as f64
    };

    work * self.volume as f64 / 128.0 / 128.0
  }
}
