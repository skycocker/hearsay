use axum::Json;

use hearsay_audio::{InputDevice, list_input_devices};

use crate::error::ApiResult;

pub async fn list_devices() -> ApiResult<Json<Vec<InputDevice>>> {
    // cpal enumeration is blocking but very fast; no spawn_blocking needed.
    Ok(Json(list_input_devices()?))
}
