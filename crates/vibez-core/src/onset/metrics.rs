//! Shared scoring for the transient corpus benchmark and detector tests.

#[derive(Debug, Default, PartialEq)]
pub struct Evaluation {
    pub correct: usize,
    pub false_positives: usize,
    pub false_negatives: usize,
    pub doubled: usize,
    pub signed_errors_ms: Vec<f64>,
}

impl Evaluation {
    pub fn precision(&self) -> f64 {
        self.correct as f64 / (self.correct + self.false_positives).max(1) as f64
    }

    pub fn recall(&self) -> f64 {
        self.correct as f64 / (self.correct + self.false_negatives).max(1) as f64
    }

    pub fn f1(&self) -> f64 {
        let precision = self.precision();
        let recall = self.recall();
        if precision + recall == 0.0 {
            0.0
        } else {
            2.0 * precision * recall / (precision + recall)
        }
    }

    pub fn merge(&mut self, other: Self) {
        self.correct += other.correct;
        self.false_positives += other.false_positives;
        self.false_negatives += other.false_negatives;
        self.doubled += other.doubled;
        self.signed_errors_ms.extend(other.signed_errors_ms);
    }

    pub fn median_absolute_error_ms(&self) -> f64 {
        percentile(
            self.signed_errors_ms
                .iter()
                .map(|error| error.abs())
                .collect(),
            0.5,
        )
    }

    pub fn p95_absolute_error_ms(&self) -> f64 {
        percentile(
            self.signed_errors_ms
                .iter()
                .map(|error| error.abs())
                .collect(),
            0.95,
        )
    }

    pub fn mean_signed_error_ms(&self) -> f64 {
        if self.signed_errors_ms.is_empty() {
            0.0
        } else {
            self.signed_errors_ms.iter().sum::<f64>() / self.signed_errors_ms.len() as f64
        }
    }
}

pub fn evaluate(
    detected: &[u64],
    expected: &[u64],
    tolerance_frames: u64,
    sample_rate: u32,
) -> Evaluation {
    let mut matched = vec![false; expected.len()];
    let mut evaluation = Evaluation::default();

    for &candidate in detected {
        let nearest = expected
            .iter()
            .enumerate()
            .filter(|(index, onset)| {
                !matched[*index] && candidate.abs_diff(**onset) <= tolerance_frames
            })
            .min_by_key(|(_, onset)| candidate.abs_diff(**onset));
        if let Some((index, onset)) = nearest {
            matched[index] = true;
            evaluation.correct += 1;
            evaluation
                .signed_errors_ms
                .push((candidate as f64 - *onset as f64) * 1_000.0 / f64::from(sample_rate));
        } else {
            evaluation.false_positives += 1;
            if expected
                .iter()
                .any(|onset| candidate.abs_diff(*onset) <= tolerance_frames)
            {
                evaluation.doubled += 1;
            }
        }
    }
    evaluation.false_negatives = matched.iter().filter(|matched| !**matched).count();
    evaluation
}

fn percentile(mut values: Vec<f64>, percentile: f64) -> f64 {
    if values.is_empty() {
        return 0.0;
    }
    values.sort_by(f64::total_cmp);
    let index = ((values.len() as f64 * percentile).ceil() as usize)
        .saturating_sub(1)
        .min(values.len() - 1);
    values[index]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn one_to_one_matching_counts_a_second_detection_as_doubled() {
        let evaluation = evaluate(&[100, 105, 200], &[100, 200], 10, 1_000);
        assert_eq!(evaluation.correct, 2);
        assert_eq!(evaluation.false_positives, 1);
        assert_eq!(evaluation.false_negatives, 0);
        assert_eq!(evaluation.doubled, 1);
        assert_eq!(evaluation.f1(), 0.8);
    }
}
