// オシレーター (pxtnPulse_Oscillator)
// 波形テーブルからサンプルを 1 個ずつ生成する

#[derive(Clone, Debug)]
pub struct Point {
    pub x: i32,
    pub y: i32,
}

pub struct Oscillator {
    volume    : i32,
    sample_num: i32,
    point_num : i32,
    point_reso: i32,
    points    : Vec<Point>,
}

impl Oscillator {
    pub fn new() -> Self {
        Self {
            volume    : 0,
            sample_num: 0,
            point_num : 0,
            point_reso: 0,
            points    : Vec::new(),
        }
    }

    pub fn ready_get_sample(&mut self, points: Vec<Point>, volume: i32, sample_num: i32, point_reso: i32) {
        self.point_num  = points.len() as i32;
        self.points     = points;
        self.volume     = volume;
        self.sample_num = sample_num;
        self.point_reso = point_reso;
    }

    /// 倍音合成でサンプルを 1 個取得
    pub fn get_one_sample_overtone(&self, index: i32) -> f64 {
        use std::f64::consts::PI;
        let mut work = 0.0f64;
        for p in &self.points {
            let sss = 2.0 * PI * p.x as f64 * index as f64 / self.sample_num as f64;
            work += sss.sin() * p.y as f64 / p.x as f64 / 128.0;
        }
        work * self.volume as f64 / 128.0
    }

    /// 座標補間でサンプルを 1 個取得
    pub fn get_one_sample_coodinate(&self, index: i32) -> f64 {
        let i = self.point_reso * index / self.sample_num;

        // 対象の 2 点を探す
        let c = self.points.iter().position(|p| p.x > i).unwrap_or(self.point_num as usize);

        let (x1, y1, x2, y2) = if c == self.point_num as usize {
            // 末端
            let last = &self.points[c - 1];
            let first = &self.points[0];
            (last.x, last.y, self.point_reso, first.y)
        } else if c > 0 {
            let prev = &self.points[c - 1];
            let cur  = &self.points[c    ];
            (prev.x, prev.y, cur.x, cur.y)
        } else {
            let p0 = &self.points[0];
            (p0.x, p0.y, p0.x, p0.y)
        };

        let w = x2 - x1;
        let ii = i - x1;
        let h = y2 - y1;

        let work = if ii != 0 {
            y1 as f64 + h as f64 * ii as f64 / w as f64
        } else {
            y1 as f64
        };

        work * self.volume as f64 / 128.0 / 128.0
    }
}
