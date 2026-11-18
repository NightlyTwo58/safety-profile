//! Descriptive statistics for repeated timing samples.

/// Summary of a set of timing samples.
#[derive(Debug, Clone, Copy, Default)]
pub struct Summary {
    pub n: usize,
    pub min: f64,
    pub median: f64,
    pub mean: f64,
    /// Sample standard deviation (n - 1); 0 when `n < 2`.
    pub stddev: f64,
}

impl Summary {
    /// Summarize microsecond samples. Empty input yields all zeros.
    pub fn from_micros(samples: &[u128]) -> Self {
        if samples.is_empty() {
            return Self::default();
        }
        let mut v: Vec<f64> = samples.iter().map(|&s| s as f64).collect();
        v.sort_by(f64::total_cmp);
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
        Self {
            n,
            min: v[0],
            median,
            mean,
            stddev,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_is_zero() {
        assert_eq!(Summary::from_micros(&[]).n, 0);
    }

    #[test]
    fn odd_and_even_median() {
        assert_eq!(Summary::from_micros(&[3, 1, 2]).median, 2.0);
        assert_eq!(Summary::from_micros(&[4, 1, 2, 3]).median, 2.5);
    }

    #[test]
    fn single_sample_has_zero_stddev() {
        let s = Summary::from_micros(&[7]);
        assert_eq!((s.min, s.median, s.stddev), (7.0, 7.0, 0.0));
    }

    #[test]
    fn sample_stddev() {
        let s = Summary::from_micros(&[2, 4, 4, 4, 5, 5, 7, 9]);
        assert!((s.stddev - 2.138_089_935).abs() < 1e-6);
    }
}
