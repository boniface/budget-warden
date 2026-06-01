use chrono::{DateTime, Utc};

use crate::error::StoreError;
use crate::reservation::ReservationId;
use crate::store::StoreKey;
use crate::window::WindowBounds;

pub(crate) fn key(key: &StoreKey) -> String {
    key.compact()
}

pub(crate) fn window_start(window: WindowBounds) -> DateTime<Utc> {
    window.start()
}

pub(crate) fn window_end(window: WindowBounds) -> DateTime<Utc> {
    window.end()
}

pub(crate) fn to_i64(value: u64, name: &str) -> Result<i64, StoreError> {
    i64::try_from(value)
        .map_err(|_| StoreError::InvalidInput(format!("{name} is too large for backend integer")))
}

pub(crate) fn id_from_i64(value: i64) -> Result<ReservationId, StoreError> {
    let value = u64::try_from(value)
        .map_err(|_| StoreError::Other("backend returned negative reservation id".to_owned()))?;
    Ok(ReservationId::new(value))
}

pub(crate) fn usage_from_i64(
    committed: i64,
    reserved: i64,
) -> Result<crate::store::CounterUsage, StoreError> {
    let committed = u64::try_from(committed)
        .map_err(|_| StoreError::Other("backend returned negative committed usage".to_owned()))?;
    let reserved = u64::try_from(reserved)
        .map_err(|_| StoreError::Other("backend returned negative reserved usage".to_owned()))?;
    Ok(crate::store::CounterUsage::new(committed, reserved))
}
