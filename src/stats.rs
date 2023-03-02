#[derive(Debug)]
pub struct Stats {
    pub min: u128,
    pub max: u128,
    pub mean: f64,
    pub std_dev: f64,
}

impl Stats {
    pub fn compute(data: &[u128]) -> Option<Stats> {
        if data.is_empty() {
            return None;
        }
        let min = *data.iter().min().expect("data length is nonzero");
        let max = *data.iter().max().expect("data length is nonzero");
        let sum: u128 = data.iter().sum();
        let mean = sum as f64 / data.len() as f64;

        let variance = data
            .iter()
            .map(|value| (mean - (*value as f64)).powf(2.))
            .sum::<f64>()
            / (data.len() as f64);

        Some(Stats {
            min,
            max,
            mean,
            std_dev: variance.sqrt(),
        })
    }
}
