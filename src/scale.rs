use std::ops::Mul;

/// Used for viewing scale.
/// The scale uses fixed point representation.
///
/// This replaces use of float so scales can be used as keys by HashMap.
///
/// The upper bound of scales can be close to the u32 limit (536870912, as we can have 0.5 million
/// points per pixel column), so we prefer storing it as a u64 to be future proof.
#[derive(Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct Scale(u64);

impl From<f32> for Scale {
    fn from(value: f32) -> Self {
        let value = (value as f64) * 1024.0;
        assert!(value.is_finite());
        Self(value as u64)
    }
}

impl From<Scale> for f32 {
    fn from(value: Scale) -> Self {
        ((value.0 as f64) / 1024.0) as f32
    }
}

impl Mul<f32> for Scale {
    type Output = Scale;

    fn mul(self, rhs: f32) -> Self::Output {
        Scale::from(f32::from(self) * rhs)
    }
}
