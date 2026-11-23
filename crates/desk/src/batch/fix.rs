//! **One observation** — a vessel's position, and when the desk learned it.
//!
//! The desk records one fix per vessel and never a track (ADR-0009, decision 2). Every measured field is an
//! [`Option`]: a source that did not give a speed leaves it absent. Absence is `None`, never `0.0` — zero is
//! the claim that a ship is stopped, and a vessel whose speed nobody knows is not the same vessel.

use std::fmt;

/// The furthest a latitude can be from the equator. Our fence, not the contract's limit.
pub const LATITUDE_LIMIT_DEG: f64 = 90.0;
/// The furthest a longitude can be from the prime meridian. Our fence, not the contract's limit.
pub const LONGITUDE_LIMIT_DEG: f64 = 180.0;
/// A speed above this is not a vessel; it is a typo or a unit mistake.
pub const SPEED_LIMIT_KN: f64 = 100.0;
/// A course is a bearing, so it stops at a full turn — and the contract refuses `360` itself (`lt=360`: a full
/// turn is `0`, and a body carrying `360` comes back `400`). The fence is exclusive for the same reason.
pub const COURSE_LIMIT_DEG: f64 = 360.0;
/// A position claimed to be accurate to more than ten kilometres is not an observation worth reporting.
pub const ACCURACY_LIMIT_M: f64 = 10_000.0;

/// The field that failed [`Fix::check`], and the value that failed it.
///
/// A refusal names the field because the operator has one row to fix: "speed_kn" says where to look, while
/// "invalid fix" makes them re-read six numbers.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum FixError {
    Latitude { value: f64 },
    Longitude { value: f64 },
    SpeedKn { value: f64 },
    CourseDeg { value: f64 },
    AccuracyM { value: f64 },
}

impl FixError {
    /// The wire name of the refused field, so the row and a test speak the contract's vocabulary.
    pub fn field(self) -> &'static str {
        match self {
            Self::Latitude { .. } => "latitude",
            Self::Longitude { .. } => "longitude",
            Self::SpeedKn { .. } => "speed_kn",
            Self::CourseDeg { .. } => "course_deg",
            Self::AccuracyM { .. } => "accuracy_m",
        }
    }
}

impl fmt::Display for FixError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Latitude { value } => write!(
                f,
                "latitude {value} is outside -{LATITUDE_LIMIT_DEG}..={LATITUDE_LIMIT_DEG}"
            ),
            Self::Longitude { value } => write!(
                f,
                "longitude {value} is outside -{LONGITUDE_LIMIT_DEG}..={LONGITUDE_LIMIT_DEG}"
            ),
            Self::SpeedKn { value } => {
                write!(f, "speed_kn {value} is outside 0..={SPEED_LIMIT_KN}")
            }
            Self::CourseDeg { value } => {
                write!(f, "course_deg {value} is outside 0..<{COURSE_LIMIT_DEG}")
            }
            Self::AccuracyM { value } => {
                write!(f, "accuracy_m {value} is outside 0..={ACCURACY_LIMIT_M}")
            }
        }
    }
}

impl std::error::Error for FixError {}

/// A vessel's last observation, as the desk knows it.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Fix {
    /// Epoch milliseconds, and the offset that was in force where the operator read the clock. The pair is
    /// kept so the ISO string sent is unambiguous and the screen can say which clock it came from.
    pub recorded_at_ms: i64,
    pub offset_ms: i32,
    pub latitude: Option<f64>,
    pub longitude: Option<f64>,
    pub speed_kn: Option<f64>,
    pub course_deg: Option<f64>,
    pub accuracy_m: Option<f64>,
}

impl Fix {
    /// Refuse a fix no desk should put on a screen, before it can be put on the wire.
    ///
    /// These bounds are a **sanity fence for a desk**, not the contract's limits. The contract bounds the
    /// coordinates and leaves accuracy open; it has nothing to say about a speed sample that reads
    /// `speed_kn: 5666`. Five thousand knots is not a fact about a ship, and a desk that forwards it has
    /// turned a typo into a position the operations room will act on. The fence refuses it per vessel, so
    /// the nineteen honest rows in the batch still leave.
    ///
    /// An absent value is not checked: `None` is how the source says it does not know.
    pub fn check(&self) -> Result<(), FixError> {
        if let Some(value) = self.latitude
            && !(-LATITUDE_LIMIT_DEG..=LATITUDE_LIMIT_DEG).contains(&value)
        {
            return Err(FixError::Latitude { value });
        }
        if let Some(value) = self.longitude
            && !(-LONGITUDE_LIMIT_DEG..=LONGITUDE_LIMIT_DEG).contains(&value)
        {
            return Err(FixError::Longitude { value });
        }
        if let Some(value) = self.speed_kn
            && !(0.0..=SPEED_LIMIT_KN).contains(&value)
        {
            return Err(FixError::SpeedKn { value });
        }
        if let Some(value) = self.course_deg
            && !(0.0..COURSE_LIMIT_DEG).contains(&value)
        {
            return Err(FixError::CourseDeg { value });
        }
        if let Some(value) = self.accuracy_m
            && !(0.0..=ACCURACY_LIMIT_M).contains(&value)
        {
            return Err(FixError::AccuracyM { value });
        }
        Ok(())
    }
}
