use std::ops::Mul;

/// Used for viewing scale.
///
/// This replaces use of f32 so scales can be used as keys by HashMap.
#[derive(Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct Scale(u32);

impl From<f32> for Scale {
    fn from(value: f32) -> Self {
        let value = value * 1024.0;
        assert!(value.is_finite());
        Self(value as u32)
    }
}
impl From<Scale> for f32 {
    fn from(value: Scale) -> Self {
        (value.0 as f32) / 1024.0
    }
}

impl Mul<f32> for Scale {
    type Output = Scale;

    fn mul(self, rhs: f32) -> Self::Output {
        Scale::from(f32::from(self) * rhs)
    }
}
