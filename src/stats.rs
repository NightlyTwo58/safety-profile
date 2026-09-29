/// Small descriptive-statistics helper for repeated timing samples.

#[derive(Debug, Clone, Copy, Default)]
pub struct Summary {
    pub n: usize,
    pub min: f64,
    pub median: f64,
    pub mean: f64,
    /// Sample standard deviation (n - 1); 0 when n < 2.
    pub stddev: f64,
}

impl Summary {
    /// Summarise microsecond samples.  Empty input yields all zeros.
    pub fn from_micros(samples: &[u128]) -> Self {
        if samples.is_empty() {
            return Self::default();
        }
        let mut v: Vec<f64> = samples.iter().map(|&s| s as f64).collect();
        v.sort_by(|a, b| a.total_cmp(b));
        let n = v.len();
        let median = if n % 2 == 1 {
            v[n / 2]
        } else {
            (v[n / 2 - 1] + v[n / 2]) / 2.0
        };
        let mean = v.iter().sum::<f64>() / n as f64;
        let stddev = if n > 1 {
            (v.iter().map(|x| (x - mean).powi(2)).sum::<f64>() / (n - 1) as f64).sqrt()
        } else {
            0.0
        };
        Self { n, min: v[0], median, mean, stddev }
    }
}
