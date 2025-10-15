/// List of enabled synchronization features.
#[derive(Copy, Clone)]
pub struct SyncFeatures {
    pub shift_x: bool,
    pub shift_y: bool,
    pub scale_x: bool,
    pub scale_y: bool,
}

impl SyncFeatures {
    /// Create a `SyncOptions` with all options enabled by default.
    pub fn new() -> Self {
        Self {
            shift_x: true,
            shift_y: true,
            scale_x: true,
            scale_y: true,
        }
    }

    /// Returns `true` if a least one option is enabled.
    pub fn any(&self) -> bool {
        self.shift_x || self.shift_y || self.scale_x || self.scale_y
    }

    /// Sets all options to `true` or `false`.
    pub fn set_all(&mut self, value: bool) {
        self.shift_x = value;
        self.shift_y = value;
        self.scale_x = value;
        self.scale_y = value;
    }
}

impl std::ops::Not for SyncFeatures {
    type Output = Self;

    fn not(self) -> Self::Output {
        Self {
            shift_x: !self.shift_x,
            shift_y: !self.shift_y,
            scale_x: !self.scale_x,
            scale_y: !self.scale_y,
        }
    }
}

impl std::ops::BitAnd for SyncFeatures {
    type Output = Self;

    fn bitand(self, rhs: Self) -> Self::Output {
        Self {
            shift_x: self.shift_x && rhs.shift_x,
            shift_y: self.shift_y && rhs.shift_y,
            scale_x: self.scale_x && rhs.scale_x,
            scale_y: self.scale_y && rhs.scale_y,
        }
    }
}
