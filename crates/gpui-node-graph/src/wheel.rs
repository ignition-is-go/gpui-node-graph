//! GPUI normalizes scrolling to positive-up deltas on native and web.
//! The browser backend already reverses DOM wheel deltas before dispatch.
pub(crate) fn zoom_factor(delta_y: f32, step: f32) -> Option<f32> {
    if !delta_y.is_finite() || !step.is_finite() || delta_y.abs() <= f32::EPSILON {
        return None;
    }
    Some((delta_y.signum() * step).exp())
}

#[cfg(test)]
mod tests {
    use super::zoom_factor;

    #[test]
    fn positive_gpui_delta_zooms_in_and_negative_zooms_out() {
        assert!(zoom_factor(1.0, 0.3).is_some_and(|factor| factor > 1.0));
        assert!(zoom_factor(-1.0, 0.3).is_some_and(|factor| factor < 1.0));
    }

    #[test]
    fn pixel_and_line_delta_magnitudes_keep_the_same_notch_step() {
        assert_eq!(zoom_factor(1.0, 0.3), zoom_factor(120.0, 0.3));
        assert_eq!(zoom_factor(-1.0, 0.3), zoom_factor(-120.0, 0.3));
    }

    #[test]
    fn opposite_notches_restore_scale() {
        let factors = [zoom_factor(120.0, 0.3), zoom_factor(-120.0, 0.3)];
        let factor: f32 = factors.into_iter().flatten().product();
        assert!((factor - 1.0).abs() < 1e-6);
    }

    #[test]
    fn nonfinite_and_zero_deltas_are_ignored() {
        for delta in [0.0, f32::NAN, f32::INFINITY, f32::NEG_INFINITY] {
            assert_eq!(zoom_factor(delta, 0.3), None);
        }
        assert_eq!(zoom_factor(1.0, f32::NAN), None);
    }
}
