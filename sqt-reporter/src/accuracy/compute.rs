use std::collections::HashMap;

use sqt_types::WorkloadType;

/// Computed accuracy metrics for ml.* constraints.
/// Keys match constraint metric names: `"ml.accuracy"`, `"ml.f1_score"`, etc.
pub type AccuracyMetrics = HashMap<String, f64>;

/// Compute accuracy metrics by comparing inference `outputs` against `reference`.
/// Returns a map of `metric_name` → value (empty map if not computable for this workload type).
pub fn compute(
    outputs: &[serde_json::Value],
    reference: &[serde_json::Value],
    workload: &WorkloadType,
) -> AccuracyMetrics {
    if outputs.is_empty() || reference.is_empty() || outputs.len() != reference.len() {
        return HashMap::new();
    }
    match workload {
        WorkloadType::ImageClassification | WorkloadType::Classification => {
            classification_metrics(outputs, reference)
        }
        WorkloadType::Regression | WorkloadType::TimeSeriesForecasting => {
            regression_metrics(outputs, reference)
        }
        WorkloadType::ObjectDetection | WorkloadType::PixelSegmentation => {
            // Basic placeholder: returns empty (complex mAP deferred)
            HashMap::new()
        }
    }
}

/// Precision comparison (SRS-FUNC-016): check whether outputs are within tolerance of reference.
///
/// Returns `("ml.output_within_tolerance", 1.0)` if all elements are within tolerance,
/// `("ml.output_within_tolerance", 0.0)` otherwise.
pub fn precision_compare(
    outputs: &[serde_json::Value],
    reference: &[serde_json::Value],
    tolerance: f64,
) -> AccuracyMetrics {
    let mut map = HashMap::new();
    if outputs.len() != reference.len() {
        map.insert("ml.output_within_tolerance".into(), 0.0);
        return map;
    }
    let all_within = outputs.iter().zip(reference.iter()).all(|(o, r)| {
        match (extract_f64(o), extract_f64(r)) {
            (Some(ov), Some(rv)) => (ov - rv).abs() <= tolerance,
            // For arrays: compare recursively element-wise
            (None, None) => {
                if let (Some(oa), Some(ra)) = (o.as_array(), r.as_array()) {
                    oa.len() == ra.len()
                        && oa.iter().zip(ra.iter()).all(|(x, y)| {
                            match (extract_f64(x), extract_f64(y)) {
                                (Some(xv), Some(yv)) => (xv - yv).abs() <= tolerance,
                                _ => false,
                            }
                        })
                } else {
                    false
                }
            }
            _ => false,
        }
    });
    map.insert(
        "ml.output_within_tolerance".into(),
        if all_within { 1.0 } else { 0.0 },
    );
    map
}

fn extract_f64(v: &serde_json::Value) -> Option<f64> {
    v.as_f64()
}

#[allow(clippy::cast_precision_loss)]
fn classification_metrics(
    outputs: &[serde_json::Value],
    reference: &[serde_json::Value],
) -> AccuracyMetrics {
    // Each output is a class index (i64) or a probability array (argmax → class).
    // Reference is the true class index.
    let mut correct = 0usize;
    let mut total = 0usize;
    let mut tp = 0usize;
    let mut fp = 0usize;
    let mut fn_ = 0usize;

    for (pred, truth) in outputs.iter().zip(reference.iter()) {
        let pred_class = predicted_class(pred);
        #[allow(clippy::cast_possible_truncation)]
        let true_class = truth.as_i64().or_else(|| truth.as_f64().map(|f| f as i64));
        if let (Some(p), Some(t)) = (pred_class, true_class) {
            total += 1;
            if p == t {
                correct += 1;
                tp += 1;
            } else {
                fp += 1;
                fn_ += 1;
            }
        }
    }
    if total == 0 {
        return HashMap::new();
    }

    let accuracy = correct as f64 / total as f64;
    let precision = if tp + fp > 0 {
        tp as f64 / (tp + fp) as f64
    } else {
        0.0
    };
    let recall = if tp + fn_ > 0 {
        tp as f64 / (tp + fn_) as f64
    } else {
        0.0
    };
    let f1 = if precision + recall > 0.0 {
        2.0 * precision * recall / (precision + recall)
    } else {
        0.0
    };

    let mut m = HashMap::new();
    m.insert("ml.accuracy".into(), accuracy);
    m.insert("ml.f1_score".into(), f1);
    m.insert("ml.precision".into(), precision);
    m.insert("ml.recall".into(), recall);
    m
}

#[allow(clippy::cast_possible_truncation, clippy::cast_possible_wrap)]
fn predicted_class(v: &serde_json::Value) -> Option<i64> {
    // If value is an integer, use it directly.
    if let Some(i) = v.as_i64() {
        return Some(i);
    }
    if let Some(f) = v.as_f64() {
        return Some(f as i64);
    }
    // If value is a probability array, take argmax.
    if let Some(arr) = v.as_array() {
        return arr
            .iter()
            .enumerate()
            .max_by(|(_, a), (_, b)| {
                a.as_f64()
                    .unwrap_or(f64::NEG_INFINITY)
                    .partial_cmp(&b.as_f64().unwrap_or(f64::NEG_INFINITY))
                    .unwrap_or(std::cmp::Ordering::Equal)
            })
            .map(|(i, _)| i as i64);
    }
    None
}

#[allow(clippy::cast_precision_loss)]
fn regression_metrics(
    outputs: &[serde_json::Value],
    reference: &[serde_json::Value],
) -> AccuracyMetrics {
    let pairs: Vec<(f64, f64)> = outputs
        .iter()
        .zip(reference.iter())
        .filter_map(|(o, r)| Some((o.as_f64()?, r.as_f64()?)))
        .collect();
    if pairs.is_empty() {
        return HashMap::new();
    }
    let n = pairs.len() as f64;
    let mse = pairs.iter().map(|(o, r)| (o - r).powi(2)).sum::<f64>() / n;
    let mae = pairs.iter().map(|(o, r)| (o - r).abs()).sum::<f64>() / n;
    let mut m = HashMap::new();
    m.insert("ml.mse".into(), mse);
    m.insert("ml.mae".into(), mae);
    m
}

// ---------------------------------------------------------------------------
// Unit tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn classification_all_correct_returns_accuracy_one() {
        let outputs = vec![json!(0), json!(1), json!(2)];
        let reference = vec![json!(0), json!(1), json!(2)];
        let result = compute(&outputs, &reference, &WorkloadType::Classification);
        assert!((result["ml.accuracy"] - 1.0).abs() < f64::EPSILON);
        assert!((result["ml.f1_score"] - 1.0).abs() < f64::EPSILON);
    }

    #[test]
    fn classification_all_wrong_returns_accuracy_zero() {
        let outputs = vec![json!(1), json!(2), json!(0)];
        let reference = vec![json!(0), json!(1), json!(2)];
        let result = compute(&outputs, &reference, &WorkloadType::Classification);
        assert!((result["ml.accuracy"] - 0.0).abs() < f64::EPSILON);
    }

    #[test]
    fn classification_partial_correct() {
        // 2 out of 4 correct
        let outputs = vec![json!(0), json!(1), json!(0), json!(3)];
        let reference = vec![json!(0), json!(1), json!(2), json!(2)];
        let result = compute(&outputs, &reference, &WorkloadType::ImageClassification);
        let acc = result["ml.accuracy"];
        assert!((acc - 0.5).abs() < f64::EPSILON, "expected 0.5, got {acc}");
    }

    #[test]
    fn classification_with_argmax_probabilities() {
        // Probability arrays: [0.1, 0.9, 0.0] → class 1, [0.8, 0.1, 0.1] → class 0
        let outputs = vec![json!([0.1, 0.9, 0.0]), json!([0.8, 0.1, 0.1])];
        let reference = vec![json!(1), json!(0)];
        let result = compute(&outputs, &reference, &WorkloadType::Classification);
        assert!((result["ml.accuracy"] - 1.0).abs() < f64::EPSILON);
    }

    #[test]
    fn classification_argmax_picks_highest_prob() {
        // [0.3, 0.5, 0.2] → argmax = 1
        let outputs = vec![json!([0.3, 0.5, 0.2])];
        let reference = vec![json!(1)];
        let result = compute(&outputs, &reference, &WorkloadType::Classification);
        assert!((result["ml.accuracy"] - 1.0).abs() < f64::EPSILON);
    }

    #[test]
    fn regression_mse_computation() {
        // predictions: [2.0, 4.0], truth: [3.0, 3.0]
        // MSE = ((2-3)^2 + (4-3)^2) / 2 = (1 + 1) / 2 = 1.0
        let outputs = vec![json!(2.0), json!(4.0)];
        let reference = vec![json!(3.0), json!(3.0)];
        let result = compute(&outputs, &reference, &WorkloadType::Regression);
        assert!((result["ml.mse"] - 1.0).abs() < f64::EPSILON);
        // MAE = (|2-3| + |4-3|) / 2 = 1.0
        assert!((result["ml.mae"] - 1.0).abs() < f64::EPSILON);
    }

    #[test]
    fn regression_perfect_predictions_zero_mse() {
        let outputs = vec![json!(1.5), json!(2.5), json!(3.5)];
        let reference = vec![json!(1.5), json!(2.5), json!(3.5)];
        let result = compute(&outputs, &reference, &WorkloadType::TimeSeriesForecasting);
        assert!(result["ml.mse"].abs() < f64::EPSILON);
        assert!(result["ml.mae"].abs() < f64::EPSILON);
    }

    #[test]
    fn precision_compare_within_tolerance() {
        let outputs = vec![json!(1.0), json!(2.0)];
        let reference = vec![json!(1.01), json!(2.005)];
        let result = precision_compare(&outputs, &reference, 0.05);
        assert!((result["ml.output_within_tolerance"] - 1.0).abs() < f64::EPSILON);
    }

    #[test]
    fn precision_compare_outside_tolerance() {
        let outputs = vec![json!(1.0), json!(2.0)];
        let reference = vec![json!(1.1), json!(2.0)];
        let result = precision_compare(&outputs, &reference, 0.05);
        assert!((result["ml.output_within_tolerance"] - 0.0).abs() < f64::EPSILON);
    }

    #[test]
    fn precision_compare_array_elements_within_tolerance() {
        let outputs = vec![json!([1.0, 2.0]), json!([3.0, 4.0])];
        let reference = vec![json!([1.01, 2.01]), json!([3.01, 4.01])];
        let result = precision_compare(&outputs, &reference, 0.05);
        assert!((result["ml.output_within_tolerance"] - 1.0).abs() < f64::EPSILON);
    }

    #[test]
    fn precision_compare_array_elements_outside_tolerance() {
        let outputs = vec![json!([1.0, 2.0])];
        let reference = vec![json!([1.0, 2.5])];
        let result = precision_compare(&outputs, &reference, 0.1);
        assert!((result["ml.output_within_tolerance"] - 0.0).abs() < f64::EPSILON);
    }

    #[test]
    fn empty_outputs_returns_empty_map() {
        let result = compute(&[], &[], &WorkloadType::Classification);
        assert!(result.is_empty());
    }

    #[test]
    fn mismatched_lengths_returns_empty_map() {
        let outputs = vec![json!(0), json!(1)];
        let reference = vec![json!(0)];
        let result = compute(&outputs, &reference, &WorkloadType::Classification);
        assert!(result.is_empty());
    }

    #[test]
    fn precision_compare_mismatched_lengths_returns_zero() {
        let outputs = vec![json!(1.0), json!(2.0)];
        let reference = vec![json!(1.0)];
        let result = precision_compare(&outputs, &reference, 0.01);
        assert!((result["ml.output_within_tolerance"] - 0.0).abs() < f64::EPSILON);
    }

    #[test]
    fn object_detection_returns_empty_map() {
        let outputs = vec![json!(0)];
        let reference = vec![json!(0)];
        let result = compute(&outputs, &reference, &WorkloadType::ObjectDetection);
        assert!(result.is_empty());
    }

    #[test]
    fn pixel_segmentation_returns_empty_map() {
        let outputs = vec![json!(0)];
        let reference = vec![json!(0)];
        let result = compute(&outputs, &reference, &WorkloadType::PixelSegmentation);
        assert!(result.is_empty());
    }
}
