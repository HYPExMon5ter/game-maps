//! Engine value types as they're laid out in memory (UE5, large world coordinates).

/// `FVector`: centimeters, Z up.
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct FVector {
    pub x: f64,
    pub y: f64,
    pub z: f64,
}

/// `FRotator`: degrees.
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct FRotator {
    pub pitch: f64,
    pub yaw: f64,
    pub roll: f64,
}

impl FRotator {
    /// The unit vector this rotation faces (`FRotator::Vector`).
    pub fn forward(self) -> FVector {
        let (sp, cp) = self.pitch.to_radians().sin_cos();
        let (sy, cy) = self.yaw.to_radians().sin_cos();
        FVector { x: cp * cy, y: cp * sy, z: sp }
    }

    /// The rotation's up vector (`FRotationMatrix(R).GetScaledAxis(EAxis::Z)`).
    pub fn up(self) -> FVector {
        let (sp, cp) = self.pitch.to_radians().sin_cos();
        let (sy, cy) = self.yaw.to_radians().sin_cos();
        let (sr, cr) = self.roll.to_radians().sin_cos();
        FVector { x: -(cr * sp * cy + sr * sy), y: cy * sr - cr * sp * sy, z: cr * cp }
    }
}

/// `TArray<T>`: data, count, capacity.
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct TArray<T> {
    pub data: *const T,
    pub num: i32,
    pub max: i32,
}

impl<T> Default for TArray<T> {
    fn default() -> Self {
        Self { data: std::ptr::null(), num: 0, max: 0 }
    }
}

impl<T: Copy> TArray<T> {
    /// A copy of the elements.
    ///
    /// # Safety
    /// The array must be live (owned by a live object, or just returned).
    pub unsafe fn to_vec(&self) -> Vec<T> {
        if self.data.is_null() || self.num <= 0 || self.num > self.max {
            return Vec::new();
        }
        std::slice::from_raw_parts(self.data, self.num as usize).to_vec()
    }
}

/// `FString`: UTF-16 with a terminating zero counted in `num`.
pub type FString = TArray<u16>;

impl FString {
    pub fn to_string_lossy(&self) -> String {
        let units = unsafe { self.to_vec() };
        let end = units.iter().position(|&u| u == 0).unwrap_or(units.len());
        String::from_utf16_lossy(&units[..end])
    }
}

/// An `FString` argument whose characters live in Rust: keep it alive for the call.
pub struct OwnedFString(Vec<u16>);

impl OwnedFString {
    pub fn new(text: &str) -> Self {
        Self(text.encode_utf16().chain(std::iter::once(0)).collect())
    }

    /// The `FString` to pass. The game must only read it.
    pub fn as_fstring(&self) -> FString {
        FString { data: self.0.as_ptr(), num: self.0.len() as i32, max: self.0.len() as i32 }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn close(a: FVector, b: FVector) -> bool {
        (a.x - b.x).abs() < 1e-9 && (a.y - b.y).abs() < 1e-9 && (a.z - b.z).abs() < 1e-9
    }

    #[test]
    fn rotators_face_where_unreal_says() {
        assert!(close(FRotator::default().forward(), FVector { x: 1.0, y: 0.0, z: 0.0 }));
        assert!(close(FRotator { yaw: 90.0, ..Default::default() }.forward(), FVector { x: 0.0, y: 1.0, z: 0.0 }));
        assert!(close(FRotator { pitch: 90.0, ..Default::default() }.forward(), FVector { x: 0.0, y: 0.0, z: 1.0 }));
        assert!(close(FRotator::default().up(), FVector { x: 0.0, y: 0.0, z: 1.0 }));
        assert!(close(FRotator { pitch: 90.0, ..Default::default() }.up(), FVector { x: -1.0, y: 0.0, z: 0.0 }));
    }

    #[test]
    fn strings_round_trip() {
        let owned = OwnedFString::new("Grounded");
        assert_eq!(owned.as_fstring().to_string_lossy(), "Grounded");
        assert_eq!(FString::default().to_string_lossy(), "");
    }
}
